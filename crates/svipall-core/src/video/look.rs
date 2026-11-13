//! Looking at a video: which moments, how many, and what each picture costs to read.
//!
//! A video is looked at the way a person scrubs one: the whole of it first, small and cheap, then
//! a part of it closer. Everything here is the arithmetic of that and nothing else: a time as
//! someone writes it, how many moments a stretch deserves, where they fall, the order to capture
//! them in so a capture cut short is still spread over the stretch, and what a picture of a given
//! size costs a model to read.

/// A moment as a caller writes it: seconds (`90`, `90.5`), `m:ss` or `h:mm:ss`.
pub fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total = 0.0;
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    for (i, p) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        let v: f64 = p.trim().parse().ok()?;
        // Only the seconds may carry a fraction, and nothing may be negative.
        if !v.is_finite() || v < 0.0 || (!last && v.fract() != 0.0) {
            return None;
        }
        total = total * 60.0 + v;
    }
    Some(total)
}

/// The part of a video being looked at. `None` on a side is that end of the video.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Window {
    pub start: Option<f64>,
    pub end: Option<f64>,
}

impl Window {
    /// The window against a video of `duration` seconds, as `(from, to)` with `from < to`, or
    /// `None` when it lies outside the video or is empty.
    pub fn within(&self, duration: f64) -> Option<(f64, f64)> {
        let from = self.start.unwrap_or(0.0).max(0.0);
        let to = self.end.unwrap_or(duration).min(duration);
        (to > from).then_some((from, to))
    }

    pub fn is_whole(&self) -> bool {
        self.start.is_none() && self.end.is_none()
    }
}

/// Moments asked of one call, at most. Each is a seek and a capture on a live page, or a cell of
/// a sheet; past this a caller looks at a part instead.
pub const MAX_MOMENTS: usize = 120;

/// The closest two moments are placed when nobody asked for a number: four a second, which is
/// where motion, counting and order start to be readable from stills.
pub const DENSEST: f64 = 0.25;

/// How many moments a stretch of `span` seconds deserves when the caller named no number.
///
/// Generous on purpose: an agent that was shown too little answers anyway. A stretch nobody
/// speaks over gets twice as many, because the picture is then all there is. Never closer than
/// [`DENSEST`], never more than [`MAX_MOMENTS`].
pub fn budget(span: f64, spoken: bool) -> usize {
    let base = match span {
        s if s <= 30.0 => 16,
        s if s <= 600.0 => 32,
        s if s <= 3600.0 => 64,
        _ => 100,
    };
    let n = if spoken { base } else { base * 2 };
    let densest = (span / DENSEST).floor().max(1.0) as usize;
    n.min(densest).min(MAX_MOMENTS)
}

/// `n` moments spread evenly over `from..to`, each in the middle of its share.
pub fn spread(from: f64, to: f64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| from + (to - from) * (i as f64 + 0.5) / n as f64)
        .collect()
}

/// The order to capture `n` evenly spread moments in: the middle first, then the middles of what
/// is left, so that a capture stopped by the clock has still seen the whole stretch thinly rather
/// than its first part densely. Returns every index in `0..n` exactly once.
pub fn coarse_first(n: usize) -> Vec<usize> {
    let mut out = Vec::with_capacity(n);
    let mut spans = std::collections::VecDeque::from([(0usize, n)]);
    while let Some((lo, hi)) = spans.pop_front() {
        if lo >= hi {
            continue;
        }
        let mid = lo + (hi - lo) / 2;
        out.push(mid);
        spans.push_back((lo, mid));
        spans.push_back((mid + 1, hi));
    }
    out
}

/// What a picture costs a model that reads it in square patches of 28 px, the accounting the
/// widest-used vision models publish: one token a patch, and a picture over the limit is shrunk,
/// keeping its shape, to the largest size under both the edge and the token limit.
///
/// Returns the size it is read at and the tokens that costs. It is an estimate for the reader:
/// a model that counts otherwise is not far off, and one priced per picture ignores it.
pub fn patch_cost(width: u32, height: u32) -> ((u32, u32), u32) {
    const PATCH: u32 = 28;
    const MAX_EDGE: u32 = 1568;
    const MAX_TOKENS: u32 = 1568;
    let tokens = |w: u32, h: u32| w.div_ceil(PATCH) * h.div_ceil(PATCH);
    let (w, h) = (width.max(1), height.max(1));
    if w.max(h) <= MAX_EDGE && tokens(w, h) <= MAX_TOKENS {
        return ((w, h), tokens(w, h));
    }
    let (long, short) = (w.max(h), w.min(h));
    let at = |edge: u32| {
        let other = ((short as f64 * edge as f64 / long as f64).round() as u32).max(1);
        if w >= h {
            (edge, other)
        } else {
            (other, edge)
        }
    };
    // The largest long edge that fits: cost only grows with it, so it is found by halving.
    let (mut lo, mut hi) = (1u32, long.min(MAX_EDGE));
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        let (mw, mh) = at(mid);
        if tokens(mw, mh) <= MAX_TOKENS {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let (rw, rh) = at(lo);
    ((rw, rh), tokens(rw, rh))
}

/// How a sheet of `cells` pictures is laid out: columns and rows, read left to right and top to
/// bottom. Near square, because that is the shape models read best, and never more than four
/// across: past that a cell is too small to say what is in it.
pub fn grid(cells: usize) -> (u32, u32) {
    let cols = ((cells.max(1) as f64).sqrt().ceil() as u32).clamp(1, SHEET_COLS);
    let rows = (cells.max(1) as u32).div_ceil(cols);
    (cols, rows)
}

/// Columns of a full sheet, and the cells one holds.
pub const SHEET_COLS: u32 = 4;
pub const SHEET_CELLS: usize = 16;

/// The width a sheet is drawn at, whatever its columns: the widest a patch-reading model takes a
/// 16:9 picture without shrinking it much.
pub const SHEET_WIDTH: u32 = 1536;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_read_as_people_write_it() {
        assert_eq!(parse_time("90"), Some(90.0));
        assert_eq!(parse_time(" 1:30 "), Some(90.0));
        assert_eq!(parse_time("1:02:03.5"), Some(3723.5));
        assert_eq!(parse_time("0:07"), Some(7.0));
        for bad in ["", "abc", "-5", "1:2:3:4", "1.5:00", "1:-3", "NaN"] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_window_is_cut_to_the_video() {
        let w = Window {
            start: Some(50.0),
            end: Some(500.0),
        };
        assert_eq!(w.within(120.0), Some((50.0, 120.0)));
        assert_eq!(w.within(40.0), None, "it starts after the video ends");
        assert_eq!(Window::default().within(60.0), Some((0.0, 60.0)));
        assert!(Window::default().is_whole() && !w.is_whole());
    }

    #[test]
    fn a_longer_stretch_gets_more_moments_and_a_silent_one_twice_as_many() {
        assert_eq!(budget(20.0, true), 16);
        assert_eq!(budget(300.0, true), 32);
        assert_eq!(budget(1800.0, true), 64);
        assert_eq!(budget(7200.0, true), 100);
        assert_eq!(budget(300.0, false), 64);
        assert_eq!(budget(7200.0, false), MAX_MOMENTS, "never past the cap");
        assert_eq!(budget(1.0, true), 4, "never closer than four a second");
        assert_eq!(budget(0.1, true), 1);
    }

    #[test]
    fn moments_sit_in_the_middle_of_their_share_of_the_window() {
        assert_eq!(spread(0.0, 60.0, 3), vec![10.0, 30.0, 50.0]);
        assert_eq!(spread(100.0, 110.0, 2), vec![102.5, 107.5]);
    }

    #[test]
    fn a_capture_cut_short_has_still_seen_the_whole_stretch() {
        for n in [0usize, 1, 2, 7, 16, 100] {
            let order = coarse_first(n);
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..n).collect::<Vec<_>>(), "every moment once");
        }
        let order = coarse_first(16);
        assert_eq!(order[0], 8, "the middle first");
        let mut first_four = order[..4].to_vec();
        first_four.sort_unstable();
        // Four captures in, no stretch of the sixteen longer than half is unseen.
        let gaps = first_four.windows(2).map(|w| w[1] - w[0]).max().unwrap();
        assert!(gaps <= 8 && first_four[0] <= 4, "{first_four:?}");
    }

    #[test]
    fn a_picture_costs_its_patches_and_a_large_one_is_shrunk_first() {
        // The published examples: what each size is read at, and for how much.
        assert_eq!(patch_cost(512, 288), ((512, 288), 209));
        assert_eq!(patch_cost(768, 432), ((768, 432), 448));
        assert_eq!(patch_cost(1280, 720), ((1280, 720), 1196));
        assert_eq!(patch_cost(1920, 1080), ((1456, 819), 1560));
        assert_eq!(patch_cost(2000, 1500), ((1269, 952), 1564));
        assert_eq!(patch_cost(3840, 2160), ((1456, 819), 1560));
        // Under the edge limit and still over the token limit: shrunk all the same.
        assert_eq!(patch_cost(1075, 1520), ((924, 1307), 1551));
        let ((w, h), t) = patch_cost(1080, 1920);
        assert_eq!((w, h, t), (819, 1456, 1560), "a tall picture is the same");
    }

    #[test]
    fn a_sheet_is_near_square_and_at_most_four_across() {
        assert_eq!(grid(1), (1, 1));
        assert_eq!(grid(4), (2, 2));
        assert_eq!(grid(6), (3, 2));
        assert_eq!(grid(9), (3, 3));
        assert_eq!(grid(16), (4, 4));
        assert_eq!(grid(13), (4, 4));
        assert_eq!(grid(0), (1, 1));
    }
}

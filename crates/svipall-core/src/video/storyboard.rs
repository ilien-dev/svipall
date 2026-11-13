//! Storyboards: the sprite sheets of small frames a player shows while you drag the seek bar.
//!
//! They are the cheapest look at what a video shows — a few images for the whole of it, no
//! playback, no media request — and they are what scene changes are found in before any real frame
//! is captured. Every cell is described as a fraction 0..1 of its sheet, never in pixels.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Frame {
    /// Seconds into the video this cell shows.
    pub t: f64,
    pub sheet: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The pipe-separated spec: a URL template, then one `w#h#count#cols#rows#interval_ms#name#sig`
/// per level, smallest first. The largest level is read; `$L` is its index, `$N` its sheet name
/// with `$M` the sheet number, and the signature goes on as `sigh`.
pub fn pipe_spec(spec: &str, duration: Option<f64>) -> Vec<Frame> {
    let mut parts = spec.split('|');
    let Some(template) = parts.next() else {
        return Vec::new();
    };
    let levels: Vec<&str> = parts.collect();
    let Some((level, fields)) = levels.iter().enumerate().next_back() else {
        return Vec::new();
    };
    let f: Vec<&str> = fields.split('#').collect();
    if f.len() < 8 {
        return Vec::new();
    }
    let num = |i: usize| f[i].parse::<u32>().ok().filter(|n| *n > 0);
    let (Some(count), Some(cols), Some(rows)) = (num(2), num(3), num(4)) else {
        return Vec::new();
    };
    let interval = match f[5].parse::<f64>().ok().filter(|i| *i > 0.0) {
        Some(ms) => ms / 1000.0,
        // Level zero carries no interval: its frames are spread over the whole video.
        None => match duration {
            Some(d) => d / count as f64,
            None => return Vec::new(),
        },
    };
    let per_sheet = cols * rows;
    (0..count)
        .map(|i| {
            let sheet_no = i / per_sheet;
            let cell = i % per_sheet;
            let name = f[6].replace("$M", &sheet_no.to_string());
            let mut sheet = template
                .replace("$L", &level.to_string())
                .replace("$N", &name);
            if !f[7].is_empty() {
                sheet.push_str(if sheet.contains('?') { "&" } else { "?" });
                sheet.push_str("sigh=");
                sheet.push_str(f[7]);
            }
            Frame {
                t: i as f64 * interval,
                sheet,
                x: (cell % cols) as f32 / cols as f32,
                y: (cell / cols) as f32 / rows as f32,
                w: 1.0 / cols as f32,
                h: 1.0 / rows as f32,
            }
        })
        .collect()
}

/// A thumbnail as a WebVTT metadata track gives it: a picture, and the box of it to show, in the
/// picture's own pixels. The box is the track's wording, not ours: [`Thumb::on`] turns it into a
/// fraction as soon as the picture's size is known.
#[derive(Debug, Clone, PartialEq)]
pub struct Thumb {
    pub t: f64,
    pub sheet: String,
    /// `x, y, w, h`; `None` when the cue names a whole picture.
    pub px: Option<[u32; 4]>,
}

impl Thumb {
    /// The cell of a sheet `width` by `height` pixels this thumbnail is.
    pub fn on(&self, width: u32, height: u32) -> Frame {
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let [x, y, cw, ch] = self.px.unwrap_or([0, 0, width, height]);
        Frame {
            t: self.t,
            sheet: self.sheet.clone(),
            x: (x as f32 / w).min(1.0),
            y: (y as f32 / h).min(1.0),
            w: (cw as f32 / w).min(1.0),
            h: (ch as f32 / h).min(1.0),
        }
    }
}

/// The thumbnails of a WebVTT metadata track: every cue whose text is the address of a picture,
/// with or without a `#xywh=` box. A metadata track that carries something else has no such cue
/// and gives nothing. Addresses are resolved against the track's own.
pub fn vtt_thumbs(cues: &[super::Cue], track_url: &str) -> Vec<Thumb> {
    let base = url::Url::parse(track_url).ok();
    cues.iter()
        .filter_map(|c| {
            let text = c.text.trim();
            if text.contains(char::is_whitespace) {
                return None;
            }
            let (addr, frag) = text.split_once('#').unwrap_or((text, ""));
            let px = match frag.strip_prefix("xywh=") {
                Some(b) => {
                    let n: Vec<u32> = b
                        .trim_start_matches("pixel:")
                        .split(',')
                        .filter_map(|v| v.trim().parse().ok())
                        .collect();
                    Some(
                        <[u32; 4]>::try_from(n)
                            .ok()
                            .filter(|b| b[2] > 0 && b[3] > 0)?,
                    )
                }
                None => None,
            };
            let path = addr.split('?').next().unwrap_or(addr).to_ascii_lowercase();
            let picture = [".jpg", ".jpeg", ".png", ".webp"]
                .iter()
                .any(|e| path.ends_with(e));
            if !picture && px.is_none() {
                return None;
            }
            let sheet = match &base {
                Some(b) => b.join(addr).ok()?.to_string(),
                None => addr.to_string(),
            };
            Some(Thumb {
                t: c.start,
                sheet,
                px,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_metadata_track_of_sprites_is_a_storyboard() {
        let cue = |start: f64, text: &str| crate::video::Cue {
            start,
            end: start + 5.0,
            text: text.into(),
        };
        let cues = vec![
            cue(0.0, "sprite.jpg#xywh=0,0,160,90"),
            cue(5.0, "sprite.jpg#xywh=160,0,160,90"),
            cue(10.0, "/abs/one.png"),
            cue(15.0, "{\"some\": \"other metadata\"}"),
            cue(20.0, "a sentence, not an address"),
            cue(25.0, "sprite.jpg#xywh=0,0,0,90"),
        ];
        let t = vtt_thumbs(&cues, "https://v.example/media/thumbs.vtt");
        assert_eq!(t.len(), 3, "{t:?}");
        assert_eq!(t[1].sheet, "https://v.example/media/sprite.jpg");
        assert_eq!(t[1].px, Some([160, 0, 160, 90]));
        assert_eq!(t[2].sheet, "https://v.example/abs/one.png");
        let f = t[1].on(320, 180);
        assert_eq!((f.t, f.x, f.y, f.w, f.h), (5.0, 0.5, 0.0, 0.5, 0.5));
        let whole = t[2].on(640, 360);
        assert_eq!((whole.x, whole.y, whole.w, whole.h), (0.0, 0.0, 1.0, 1.0));
    }

    const SPEC: &str = "https://i.example/sb/ID/storyboard3_L$L/$N.jpg?sqp=abc|48#27#100#10#10#0#default#rs$A|80#45#108#10#10#2000#M$M#rs$X|160#90#108#5#5#2000#M$M#rs$B|320#180#108#3#3#2000#M$M#rs$C";

    #[test]
    fn the_largest_level_is_read() {
        let f = pipe_spec(SPEC, Some(213.0));
        assert_eq!(f.len(), 108);
        assert_eq!(
            f[0].sheet,
            "https://i.example/sb/ID/storyboard3_L3/M0.jpg?sqp=abc&sigh=rs$C"
        );
        assert_eq!(
            f[9].sheet,
            "https://i.example/sb/ID/storyboard3_L3/M1.jpg?sqp=abc&sigh=rs$C"
        );
        assert_eq!(f[10].t, 20.0);
    }

    #[test]
    fn cells_are_fractions_of_their_sheet() {
        let f = pipe_spec(SPEC, None);
        // Cell 4 of a 3x3 sheet is the centre.
        let c = &f[4];
        assert!((c.x - 1.0 / 3.0).abs() < 1e-6 && (c.y - 1.0 / 3.0).abs() < 1e-6);
        assert!((c.w - 1.0 / 3.0).abs() < 1e-6);
        assert!(f
            .iter()
            .all(|c| c.x + c.w <= 1.0 + 1e-6 && c.y + c.h <= 1.0 + 1e-6));
    }

    #[test]
    fn a_level_without_an_interval_is_spread_over_the_duration() {
        let f = pipe_spec(
            "https://i.example/$L/$N.jpg|48#27#100#10#10#0#default#s",
            Some(200.0),
        );
        assert_eq!(f.len(), 100);
        assert_eq!(f[50].t, 100.0);
        assert!(pipe_spec(
            "https://i.example/$L/$N.jpg|48#27#100#10#10#0#default#s",
            None
        )
        .is_empty());
    }

    #[test]
    fn nonsense_is_nothing() {
        assert!(pipe_spec("", None).is_empty());
        assert!(pipe_spec("https://x|1#2#3", None).is_empty());
    }
}

//! Pointer movement that looks like a hand rather than a teleport.
//!
//! `nudge()` used to be a `window.scrollBy`, which is what every anti-bot vendor looks for first:
//! a page that scrolls without a single `mousemove` beforehand, and clicks that land on the exact
//! pixel centre of their target with no approach and no travel time. What is generated here is a
//! cubic Bézier from wherever the pointer was to a point *near* the target, walked with an
//! ease-in-out velocity profile, and a press whose duration is a plausible human one.
//!
//! The geometry is deterministic given a seed, and the seed comes from the identity, for the same
//! reason canvas noise is deterministic: a path that is different on every load is itself a
//! signal, and a path that is identical on every load is a fingerprint. Per identity, stable.
//!
//! Everything in the first half of this file is pure and tested. Only the `dispatch` half needs a
//! browser.

use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use svipall_cdp::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventType, MouseButton,
};
use svipall_cdp::page::Page;

/// One pointer sample: where to be, and how long to wait before the next one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub x: f64,
    pub y: f64,
    pub delay_ms: u64,
}

/// Small deterministic PRNG. `rand` would work, but a fixed algorithm keeps a seeded path
/// reproducible across dependency updates, which is what the tests rely on.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Odd, non-zero: xorshift is degenerate at 0.
        Self(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// Uniform in [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Uniform in [-1, 1].
    fn signed(&mut self) -> f64 {
        self.unit() * 2.0 - 1.0
    }
}

fn bezier(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (
        a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
        a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
    )
}

/// Slow at both ends, fast in the middle — how a hand actually moves between two points.
fn ease(t: f64) -> f64 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

/// A pointer path from `from` to `to`.
///
/// The end point is the target *itself*: landing near it and clicking would miss. The jitter goes
/// into the curve and into where inside the element the caller aims, not into the last sample.
pub fn path(from: (f64, f64), to: (f64, f64), seed: u64) -> Vec<Step> {
    let mut rng = Rng::new(seed);
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < 1.0 {
        return vec![Step {
            x: to.0,
            y: to.1,
            delay_ms: 8,
        }];
    }

    // Control points pushed off the straight line, so the path bows the way an arm does. The
    // offset grows with distance but is capped: a long drag is not a semicircle.
    let bow = (distance * 0.18).min(90.0);
    let (nx, ny) = (-dy / distance, dx / distance);
    let side = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
    let c1 = (
        from.0 + dx * 0.3 + nx * bow * side * (0.5 + rng.unit() * 0.5),
        from.1 + dy * 0.3 + ny * bow * side * (0.5 + rng.unit() * 0.5),
    );
    let c2 = (
        from.0 + dx * 0.7 + nx * bow * side * (0.2 + rng.unit() * 0.5),
        from.1 + dy * 0.7 + ny * bow * side * (0.2 + rng.unit() * 0.5),
    );

    // Roughly one sample per 12 px, bounded so a tiny move is not one jump and a long one is not
    // a thousand CDP round trips.
    let samples = ((distance / 12.0).round() as usize).clamp(8, 48);
    let mut steps = Vec::with_capacity(samples);
    for i in 1..=samples {
        let t = ease(i as f64 / samples as f64);
        let (x, y) = bezier(from, c1, c2, to, t);
        // Hand tremor, a fraction of a pixel to about one pixel, and never on the last sample.
        let tremor = if i == samples { 0.0 } else { 1.0 };
        steps.push(Step {
            x: x + rng.signed() * tremor,
            y: y + rng.signed() * tremor,
            delay_ms: (6.0 + rng.unit() * 10.0) as u64,
        });
    }
    steps
}

/// Where inside an element's box to aim: near the middle, but not the exact centre, because the
/// exact centre is what a script hits and a person does not.
pub fn aim(x: f64, y: f64, width: f64, height: f64, seed: u64) -> (f64, f64) {
    let mut rng = Rng::new(seed ^ 0x9E37_79B9_7F4A_7C15);
    (
        x + width / 2.0 + rng.signed() * (width * 0.22),
        y + height / 2.0 + rng.signed() * (height * 0.22),
    )
}

/// How long a click holds the button down. Human presses cluster around 60-120 ms.
pub fn press_duration(seed: u64) -> Duration {
    let mut rng = Rng::new(seed ^ 0xD1B5_4A32_D192_ED03);
    Duration::from_millis(55 + (rng.unit() * 90.0) as u64)
}

/// One keystroke: which character, and how long after the previous one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub ch: char,
    pub delay_ms: u64,
}

/// Turn a string into the keystrokes a person would produce typing it.
///
/// Filling a field in one CDP call is instant, and instant is not a speed anyone types at. Three
/// things make the difference between a plausible cadence and a random one:
///
///   * **the same pair of letters costs the same time** — typing is muscle memory, so `th` is
///     always quicker than `qz` for the same typist;
///   * **a space or a punctuation mark is a pause**, because it is where a word ends and the next
///     is recalled;
///   * **a capital costs extra**, because the other hand has to reach for shift.
///
/// Seeded, so one identity types at one speed. A typist whose rhythm changes between two form
/// fills is a stranger tell than one who types instantly.
pub fn typing(text: &str, seed: u64) -> Vec<Key> {
    let mut rng = Rng::new(seed ^ 0x5DEE_CE66_D1B5_4A32);
    // A words-per-minute figure for this identity, in the range ordinary people actually manage.
    let wpm = 32.0 + rng.unit() * 48.0;
    let base = 60_000.0 / (wpm * 5.0); // ms per character

    let mut out = Vec::with_capacity(text.chars().count());
    let mut prev: Option<char> = None;
    for ch in text.chars() {
        let mut ms = base;
        // Deterministic in the digraph, not a fresh draw: muscle memory is repeatable.
        if let Some(p) = prev {
            let mut h = Rng::new(seed ^ ((p as u64) << 21) ^ (ch as u64));
            ms *= 0.65 + h.unit() * 0.7;
        }
        if ch == ' ' {
            ms *= 1.35;
        }
        if matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | '\n') {
            ms *= 1.9;
        }
        if ch.is_uppercase() {
            ms *= 1.4;
        }
        // A little jitter on top so two identical words are not two identical timings.
        ms *= 0.9 + rng.unit() * 0.2;
        out.push(Key {
            ch,
            delay_ms: ms.clamp(18.0, 900.0) as u64,
        });
        prev = Some(ch);
    }
    out
}

/// One turn of a mouse wheel: how far, and how long before the next one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scroll {
    pub delta_y: f64,
    pub delay_ms: u64,
}

/// Break a scroll into the discrete wheel events a mouse produces.
///
/// `window.scrollBy` moves the page without a single wheel event, which is trivially distinguished
/// from a person: the scroll position changes and nothing was scrolled. A real wheel fires in
/// notches, accelerates as the finger keeps going and eases off at the end.
pub fn scrolling(total: f64, seed: u64) -> Vec<Scroll> {
    if total == 0.0 {
        return Vec::new();
    }
    let mut rng = Rng::new(seed ^ 0x2545_F491_4F6C_DD1D);
    let dir = total.signum();
    let remaining = total.abs();
    // Chrome reports 100px per notch on most mice; people turn the wheel a few notches at a time.
    let notch = 100.0;
    let mut out = Vec::new();
    let mut done = 0.0;
    let mut n = 0usize;
    while done < remaining && n < 60 {
        // Ramp up over the first few notches and ease off near the target, the way a hand does.
        let progress = done / remaining;
        let speed = if progress < 0.25 {
            0.6 + progress * 1.6
        } else if progress > 0.8 {
            0.4 + (1.0 - progress) * 3.0
        } else {
            1.0
        };
        let step = (notch * speed * (0.85 + rng.unit() * 0.3)).min(remaining - done);
        done += step;
        out.push(Scroll {
            delta_y: dir * step,
            delay_ms: (28.0 + rng.unit() * 60.0) as u64,
        });
        n += 1;
    }
    out
}

/// How long to look at a page before asking for the next one.
///
/// Requesting the next page 40 ms after receiving this one is the cheapest signal a crawler emits:
/// nobody reads that fast, and no browser renders that fast either. The wait is proportional to how
/// much there was to read, with a floor for a page that is nearly empty and a ceiling so a long
/// article does not stall a crawl.
pub fn dwell(text_len: usize, seed: u64) -> Duration {
    let mut rng = Rng::new(seed ^ 0x9E37_79B9_7F4A_7C15);
    // Skim-reading runs at roughly 900 words a minute, and a word is about six characters.
    let words = text_len as f64 / 6.0;
    let ms = words / 900.0 * 60_000.0;
    let jittered = ms * (0.7 + rng.unit() * 0.6);
    Duration::from_millis(jittered.clamp(350.0, 9_000.0) as u64)
}

/// The pointer's position, kept per page so the next move starts where the last one ended.
///
/// Starting every movement from (0,0) would produce a path across the whole viewport before every
/// single click, which is both slow and obviously synthetic.
#[derive(Debug, Clone, Copy)]
pub struct Cursor {
    pub x: f64,
    pub y: f64,
}

impl Default for Cursor {
    fn default() -> Self {
        // Somewhere plausible for a pointer to have been left, not the origin.
        Self { x: 320.0, y: 380.0 }
    }
}

/// Where the pointer was left on each page.
///
/// The doc comment above has always said "kept per page"; it was not. Every call site built a
/// `Cursor::default()`, so the pointer teleported back to (320, 380) before every move, every
/// click and every wheel event — a hand that returns to the same spot between actions and never
/// drifts is a shape no arm makes.
static AT: std::sync::Mutex<Option<HashMap<String, (f64, f64)>>> = std::sync::Mutex::new(None);

impl Cursor {
    /// The pointer as this page last left it.
    pub fn at_page(page: &Page) -> Self {
        let key = page.target_id().inner();
        match AT.lock() {
            Ok(at) => at
                .as_ref()
                .and_then(|m| m.get(key))
                .map(|&(x, y)| Self { x, y })
                .unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    fn remember(&self, page: &Page) {
        if let Ok(mut at) = AT.lock() {
            at.get_or_insert_with(HashMap::new)
                .insert(page.target_id().inner().clone(), (self.x, self.y));
        }
    }

    /// Forget a page's pointer. Called when the page closes, so the map does not grow with every
    /// tab a long-running server opens.
    pub fn forget(page: &Page) {
        if let Ok(mut at) = AT.lock() {
            if let Some(m) = at.as_mut() {
                m.remove(page.target_id().inner());
            }
        }
    }
}

impl Cursor {
    /// Walk the pointer to `(x, y)`, dispatching real input events.
    pub async fn move_to(&mut self, page: &Page, x: f64, y: f64, seed: u64) -> anyhow::Result<()> {
        for step in path((self.x, self.y), (x, y), seed) {
            page.execute(
                DispatchMouseEventParams::builder()
                    .r#type(DispatchMouseEventType::MouseMoved)
                    .x(step.x)
                    .y(step.y)
                    .build()
                    .map_err(anyhow::Error::msg)?,
            )
            .await?;
            tokio::time::sleep(Duration::from_millis(step.delay_ms)).await;
        }
        self.x = x;
        self.y = y;
        self.remember(page);
        Ok(())
    }

    /// Move to a point and click it: press and release as separate events, held for a human
    /// interval. `Element::click` sends neither the approach nor the hold.
    pub async fn click_at(&mut self, page: &Page, x: f64, y: f64, seed: u64) -> anyhow::Result<()> {
        self.move_to(page, x, y, seed).await?;
        self.button(page, DispatchMouseEventType::MousePressed)
            .await?;
        tokio::time::sleep(press_duration(seed)).await;
        self.button(page, DispatchMouseEventType::MouseReleased)
            .await
    }

    /// Press, hold for `ms`, release — without moving. This is the whole answer to the
    /// "press and hold" challenges that `WallKind::Hold` classifies, which otherwise go straight
    /// to a human every time.
    pub async fn press_and_hold(
        &mut self,
        page: &Page,
        x: f64,
        y: f64,
        ms: u64,
        seed: u64,
    ) -> anyhow::Result<()> {
        self.move_to(page, x, y, seed).await?;
        self.button(page, DispatchMouseEventType::MousePressed)
            .await?;
        // A little over what was asked: these widgets measure the hold and reject a short one.
        tokio::time::sleep(Duration::from_millis(ms + 250)).await;
        self.button(page, DispatchMouseEventType::MouseReleased)
            .await
    }

    /// Press at the current point, carry the button along a curved path to `(x, y)`, release.
    ///
    /// The path is the same one a click approaches by, so the drag has the same hesitation and
    /// overshoot a hand has. What makes it a drag rather than a move is the button held through
    /// every intermediate event: a slider that sees the pointer arrive without it has seen a
    /// script, and the widgets that ask for this are the ones that look.
    pub async fn drag_to(&mut self, page: &Page, x: f64, y: f64, seed: u64) -> anyhow::Result<()> {
        self.button(page, DispatchMouseEventType::MousePressed)
            .await?;
        tokio::time::sleep(press_duration(seed)).await;
        for step in path((self.x, self.y), (x, y), seed) {
            page.execute(
                DispatchMouseEventParams::builder()
                    .r#type(DispatchMouseEventType::MouseMoved)
                    .x(step.x)
                    .y(step.y)
                    .button(MouseButton::Left)
                    .build()
                    .map_err(anyhow::Error::msg)?,
            )
            .await?;
            tokio::time::sleep(Duration::from_millis(step.delay_ms)).await;
        }
        self.x = x;
        self.y = y;
        // A hand settles before it lets go.
        tokio::time::sleep(press_duration(seed)).await;
        self.button(page, DispatchMouseEventType::MouseReleased)
            .await
    }

    async fn button(&self, page: &Page, kind: DispatchMouseEventType) -> anyhow::Result<()> {
        page.execute(
            DispatchMouseEventParams::builder()
                .r#type(kind)
                .x(self.x)
                .y(self.y)
                .button(MouseButton::Left)
                .click_count(1)
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await?;
        Ok(())
    }

    /// Turn the wheel at the pointer's current position, in notches.
    pub async fn wheel(&self, page: &Page, pixels: f64, seed: u64) -> anyhow::Result<()> {
        for step in scrolling(pixels, seed) {
            page.execute(
                DispatchMouseEventParams::builder()
                    .r#type(DispatchMouseEventType::MouseWheel)
                    .x(self.x)
                    .y(self.y)
                    .delta_x(0.0)
                    .delta_y(step.delta_y)
                    .build()
                    .map_err(anyhow::Error::msg)?,
            )
            .await?;
            tokio::time::sleep(Duration::from_millis(step.delay_ms)).await;
        }
        Ok(())
    }

    /// Idle activity while waiting for a challenge to clear: a pointer move first, then a scroll.
    /// The order matters — a scroll with no preceding pointer event is the cheap tell.
    ///
    /// The scroll is wheel input, not `window.scrollBy`. This module's own opening paragraph names
    /// `scrollBy` as the thing every vendor looks for first — a document that moves with no input
    /// event behind it — and this function was still calling it.
    pub async fn idle(&mut self, page: &Page, seed: u64) {
        let mut rng = Rng::new(seed);
        let (x, y) = (
            (self.x + rng.signed() * 260.0).clamp(20.0, 1200.0),
            (self.y + rng.signed() * 180.0).clamp(20.0, 700.0),
        );
        let _ = self.move_to(page, x, y, seed).await;
        let _ = self.wheel(page, 220.0 + rng.unit() * 300.0, seed).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        let _ = self.wheel(page, -120.0, seed).await;
    }
}

/// The bounding box of a selector in viewport coordinates, or `None` when it is not there.
pub async fn box_of(page: &Page, selector: &str) -> Option<(f64, f64, f64, f64)> {
    let js = format!(
        r#"(() => {{
            const el = document.querySelector({selector:?});
            if (!el) return null;
            const r = el.getBoundingClientRect();
            if (r.width < 1 || r.height < 1) return null;
            return {{x: r.x, y: r.y, w: r.width, h: r.height}};
        }})()"#
    );
    let v = page.evaluate(js).await.ok()?.into_value::<Value>().ok()?;
    Some((
        v.get("x")?.as_f64()?,
        v.get("y")?.as_f64()?,
        v.get("w")?.as_f64()?,
        v.get("h")?.as_f64()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn far() -> Vec<Step> {
        path((10.0, 10.0), (610.0, 410.0), 42)
    }

    #[test]
    fn typing_is_never_instant_and_never_glacial() {
        let keys = typing("hello world", 42);
        assert_eq!(keys.len(), 11);
        let total: u64 = keys.iter().map(|k| k.delay_ms).sum();
        // Eleven characters at any human speed is somewhere between a fifth of a second and
        // several seconds. Filling the field in one call is what this replaces.
        assert!(
            (200..6_000).contains(&total),
            "{total}ms to type eleven characters"
        );
    }

    #[test]
    fn the_same_pair_of_letters_always_costs_the_same() {
        // Typing is muscle memory. A typist whose rhythm changes between two fills of the same
        // form is a stranger signal than one who types instantly.
        let a = typing("the the the", 7);
        let b = typing("the the the", 7);
        assert_eq!(a, b);
    }

    #[test]
    fn punctuation_and_capitals_cost_more_than_plain_letters() {
        let plain = typing("aaaa", 3).iter().map(|k| k.delay_ms).sum::<u64>();
        let caps = typing("AAAA", 3).iter().map(|k| k.delay_ms).sum::<u64>();
        let stops = typing("a.a.", 3).iter().map(|k| k.delay_ms).sum::<u64>();
        assert!(caps > plain, "shift costs nothing: {caps} vs {plain}");
        assert!(stops > plain, "a full stop is a pause: {stops} vs {plain}");
    }

    #[test]
    fn two_identities_do_not_type_at_the_same_speed() {
        let a: u64 = typing("a sentence of some length", 1)
            .iter()
            .map(|k| k.delay_ms)
            .sum();
        let b: u64 = typing("a sentence of some length", 2)
            .iter()
            .map(|k| k.delay_ms)
            .sum();
        assert_ne!(a, b);
    }

    #[test]
    fn typing_nothing_produces_nothing() {
        assert!(typing("", 1).is_empty());
    }

    #[test]
    fn a_scroll_arrives_in_notches_that_add_up_to_the_distance() {
        // `window.scrollBy` moves the page with no wheel event at all: the position changes and
        // nothing was scrolled.
        let steps = scrolling(800.0, 11);
        assert!(steps.len() > 3, "only {} notches", steps.len());
        let total: f64 = steps.iter().map(|s| s.delta_y).sum();
        assert!((total - 800.0).abs() < 1.0, "moved {total} instead of 800");
        assert!(steps.iter().all(|s| s.delta_y > 0.0));
    }

    #[test]
    fn scrolling_upward_keeps_its_direction() {
        let steps = scrolling(-500.0, 11);
        let total: f64 = steps.iter().map(|s| s.delta_y).sum();
        assert!((total + 500.0).abs() < 1.0, "{total}");
        assert!(steps.iter().all(|s| s.delta_y < 0.0));
    }

    #[test]
    fn a_scroll_ramps_up_and_eases_off() {
        // A constant delta every notch is a script; a hand accelerates and then slows.
        let steps = scrolling(2000.0, 5);
        let first = steps.first().unwrap().delta_y;
        let middle = steps[steps.len() / 2].delta_y;
        assert!(middle > first, "no acceleration: {first} then {middle}");
    }

    #[test]
    fn scrolling_nowhere_does_nothing() {
        assert!(scrolling(0.0, 1).is_empty());
    }

    #[test]
    fn a_long_page_is_read_for_longer_than_a_short_one() {
        let short = dwell(400, 5);
        let long = dwell(40_000, 5);
        assert!(long > short, "{long:?} should exceed {short:?}");
    }

    #[test]
    fn nobody_reads_a_page_in_forty_milliseconds_or_takes_a_minute() {
        // The floor is the signal this exists to remove; the ceiling keeps a long article from
        // stalling a crawl.
        for len in [0, 10, 200, 5_000, 500_000] {
            let d = dwell(len, 9).as_millis();
            assert!((350..=9_000).contains(&d), "{d}ms for {len} chars");
        }
    }

    #[test]
    fn a_path_ends_exactly_on_its_target() {
        let steps = far();
        let last = steps.last().expect("at least one step");
        // Approaching "near" the target and clicking there is how a click misses. The jitter is in
        // the curve, never in the landing.
        assert!((last.x - 610.0).abs() < 1e-9, "{last:?}");
        assert!((last.y - 410.0).abs() < 1e-9, "{last:?}");
    }

    #[test]
    fn a_path_is_not_the_straight_line_between_its_endpoints() {
        let steps = far();
        // Distance from the straight line, for the sample halfway along.
        let mid = steps[steps.len() / 2];
        let (x0, y0, x1, y1) = (10.0, 10.0, 610.0, 410.0);
        let num = ((y1 - y0) * mid.x - (x1 - x0) * mid.y + x1 * y0 - y1 * x0).abs();
        let den = ((y1 - y0).powi(2) + (x1 - x0).powi(2)).sqrt();
        assert!(
            num / den > 3.0,
            "a perfectly straight path is the thing being avoided: {mid:?}"
        );
    }

    #[test]
    fn a_path_stays_in_a_sane_corridor() {
        // Bowed, but not a detour: no sample further from the line than the cap on the bow.
        let (x0, y0, x1, y1) = (10.0f64, 10.0f64, 610.0f64, 410.0f64);
        let den = ((y1 - y0).powi(2) + (x1 - x0).powi(2)).sqrt();
        for s in far() {
            let num = ((y1 - y0) * s.x - (x1 - x0) * s.y + x1 * y0 - y1 * x0).abs();
            assert!(num / den < 95.0, "{s:?} strays too far");
        }
    }

    #[test]
    fn two_identities_do_not_draw_the_same_path() {
        let a = path((0.0, 0.0), (400.0, 300.0), 1);
        let b = path((0.0, 0.0), (400.0, 300.0), 2);
        assert_ne!(a[a.len() / 2], b[b.len() / 2]);
    }

    #[test]
    fn one_identity_draws_the_same_path_twice() {
        // Deterministic for the same reason canvas noise is: a path that changes on every load is
        // itself a signal.
        assert_eq!(
            path((0.0, 0.0), (400.0, 300.0), 7),
            path((0.0, 0.0), (400.0, 300.0), 7)
        );
    }

    #[test]
    fn sample_count_follows_the_distance() {
        let short = path((0.0, 0.0), (30.0, 0.0), 5);
        let long = path((0.0, 0.0), (900.0, 0.0), 5);
        assert!(short.len() < long.len());
        assert!((8..=48).contains(&short.len()));
        assert!((8..=48).contains(&long.len()));
    }

    #[test]
    fn a_move_of_no_distance_is_a_single_step() {
        let steps = path((100.0, 100.0), (100.0, 100.0), 3);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].x, 100.0);
    }

    #[test]
    fn travel_time_is_human() {
        let total: u64 = far().iter().map(|s| s.delay_ms).sum();
        assert!(
            (80..=800).contains(&total),
            "{total}ms to cross the viewport is not a hand"
        );
    }

    #[test]
    fn a_click_never_lands_on_the_exact_centre() {
        let (x, y) = aim(100.0, 200.0, 80.0, 40.0, 11);
        assert!((x - 140.0).abs() > 0.01 || (y - 220.0).abs() > 0.01);
        // But it does land inside the element.
        assert!((100.0..180.0).contains(&x), "{x}");
        assert!((200.0..240.0).contains(&y), "{y}");
    }

    #[test]
    fn a_press_is_held_for_a_plausible_time() {
        for seed in 0..50u64 {
            let ms = press_duration(seed).as_millis();
            assert!((55..=145).contains(&ms), "{ms}ms for seed {seed}");
        }
    }
}

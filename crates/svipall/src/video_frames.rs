//! Keyframes: which moments of a video to look at, and the script that shows each one.
//!
//! A frame is worth returning where the picture changes. Changes are found by comparing colour
//! histograms of consecutive candidates: a storyboard's cells when the player publishes one (a few
//! small images for the whole video, no playback), otherwise frames captured at even intervals and
//! compared after the fact. The chosen moments are then captured from the page's own `<video>`,
//! seeked in place, so what comes back is what a person watching would have seen.

use image::{DynamicImage, GenericImageView, RgbImage};
use std::path::{Path, PathBuf};

/// Frames asked for, at most. Each is a seek and a capture on a live page.
pub const MAX_FRAMES: u32 = 24;

/// Candidates per frame wanted when there is no storyboard to choose from.
pub const CANDIDATES_PER_FRAME: usize = 3;

const BINS: usize = 16;

/// A colour histogram, each channel normalised to sum to one.
pub fn histogram(img: &RgbImage) -> Vec<f32> {
    let mut h = vec![0f32; BINS * 3];
    for p in img.pixels() {
        for c in 0..3 {
            h[c * BINS + (p[c] as usize * BINS / 256)] += 1.0;
        }
    }
    let n = (img.width() * img.height()).max(1) as f32;
    h.iter_mut().for_each(|v| *v /= n);
    h
}

/// How different two pictures are, 0 (same colours) to 1 (nothing in common).
pub fn distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / 6.0
}

/// A cell of a sheet, given as fractions of it.
pub fn crop(sheet: &DynamicImage, x: f32, y: f32, w: f32, h: f32) -> RgbImage {
    let (sw, sh) = sheet.dimensions();
    let px = |f: f32, full: u32| ((f * full as f32).round() as u32).min(full.saturating_sub(1));
    let (x0, y0) = (px(x, sw), px(y, sh));
    let cw = ((w * sw as f32).round() as u32).clamp(1, sw - x0);
    let ch = ((h * sh as f32).round() as u32).clamp(1, sh - y0);
    sheet.crop_imm(x0, y0, cw, ch).to_rgb8()
}

/// The moments to capture: the first one, then wherever the picture changed most, no two closer
/// than `min_gap` seconds. Returned in time order, as indices into `times`.
pub fn pick(times: &[f64], hists: &[Vec<f32>], n: usize, min_gap: f64) -> Vec<usize> {
    if times.is_empty() || n == 0 {
        return Vec::new();
    }
    let mut change: Vec<(usize, f32)> = (1..times.len().min(hists.len()))
        .map(|i| (i, distance(&hists[i - 1], &hists[i])))
        .collect();
    change.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut chosen = vec![0usize];
    for (i, _) in change {
        if chosen.len() >= n {
            break;
        }
        if chosen
            .iter()
            .all(|&c| (times[c] - times[i]).abs() >= min_gap)
        {
            chosen.push(i);
        }
    }
    chosen.sort_by(|a, b| times[*a].total_cmp(&times[*b]));
    chosen
}

/// `count` moments spread evenly over `duration`, each in the middle of its share.
pub fn uniform(duration: f64, count: usize) -> Vec<f64> {
    (0..count)
        .map(|i| duration * (i as f64 + 0.5) / count as f64)
        .collect()
}

/// Where a frame is written: one directory per video, one file per moment, named so a listing
/// sorts in time order.
pub fn frame_path(root: &Path, video: &str, t: f64) -> PathBuf {
    let safe: String = video
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    let s = t.max(0.0).round() as u64;
    root.join(safe).join(format!(
        "{:02}-{:02}-{:02}.png",
        s / 3600,
        (s / 60) % 60,
        s % 60
    ))
}

/// Whether a `<video>` is where a person sees it: laid out, not hidden, and nearly all of it inside
/// the document. A player parks the element it has not started, and the one it preloads behind an
/// advert, above the page (measured: `top: -1050px`, 68 px of it still showing).
const SHOWN: &str = "(v => { const r = v.getBoundingClientRect(); \
    const d = document.documentElement; \
    const W = Math.max(d.scrollWidth, innerWidth), H = Math.max(d.scrollHeight, innerHeight); \
    const w = Math.min(r.right + scrollX, W) - Math.max(r.left + scrollX, 0); \
    const h = Math.min(r.bottom + scrollY, H) - Math.max(r.top + scrollY, 0); \
    return r.width > 1 && r.height > 1 && Math.max(w, 0) * Math.max(h, 0) >= 0.9 * r.width * r.height \
        && (!v.checkVisibility || v.checkVisibility({opacityProperty: true, visibilityProperty: true})); })";

/// The page's videos, largest first. Players keep tiny ones for previews.
const VIDEOS: &str = "[...document.querySelectorAll('video')].sort((a, b) => \
    b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)";

/// The largest video a person can see, as a script expression.
fn main_js() -> String {
    format!("{VIDEOS}.filter({SHOWN})[0]")
}

/// What the page's player is showing, as the probe saw it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Playing {
    /// The video itself is on screen. `picture` is false for a stream the browser plays the sound
    /// of and cannot decode the picture of.
    Main {
        length: f64,
        picture: bool,
    },
    /// Something of another length: an advert, with the video waiting behind it. `parked` when
    /// the player has not started it either, which headless it will not do by itself.
    Advert {
        parked: bool,
    },
    /// The video is parked off the page, or on it with no frame loaded: the player has not
    /// started it.
    Cued,
    Nothing,
}

/// Read a [`probe_js`] answer. `want` is the video's length as the page declared it; without it
/// an advert cannot be told from the video, and whatever is on screen is taken.
pub fn playing(probe: &serde_json::Value, want: Option<f64>) -> Playing {
    let length = |k: &str| {
        probe
            .get(k)
            .and_then(|v| v.get("d"))
            .and_then(|d| d.as_f64())
            .filter(|d| *d > 0.0)
    };
    if let Some(d) = length("shown") {
        if want.is_some_and(|w| (d - w).abs() >= 3.0) {
            return Playing::Advert { parked: false };
        }
        // No frame yet is a video still loading (measured: a player sets the length before a
        // byte arrives), not one without a picture.
        let num = |k: &str| probe["shown"].get(k).and_then(|v| v.as_f64());
        if num("ready").is_some_and(|r| r < 2.0) {
            return Playing::Cued;
        }
        return Playing::Main {
            length: d,
            picture: num("w") != Some(0.0),
        };
    }
    match length("hidden") {
        Some(d) if want.is_none_or(|w| (d - w).abs() < 3.0) => Playing::Cued,
        Some(_) => Playing::Advert { parked: true },
        None => Playing::Nothing,
    }
}

/// The video on screen and the largest one off it, each with its length and picture width, and
/// `idle`: where a video sits in the viewport that has loaded nothing and is not playing, which
/// a player that fetches on its own play button leaves until someone presses it.
pub fn probe_js() -> String {
    format!(
        "(() => {{ const shown = {SHOWN}; const all = {VIDEOS}; \
         const say = v => v ? {{d: Number.isFinite(v.duration) ? v.duration : null, \
         ready: v.readyState, w: v.videoWidth}} : null; \
         const idle = all.find(v => {{ const r = v.getBoundingClientRect(); \
           const w = Math.min(r.right, innerWidth) - Math.max(r.left, 0); \
           const h = Math.min(r.bottom, innerHeight) - Math.max(r.top, 0); \
           return v.readyState === 0 && v.paused && r.width > 40 && r.height > 40 \
             && Math.max(w, 0) * Math.max(h, 0) >= 0.5 * r.width * r.height; }}); \
         const box = idle && idle.getBoundingClientRect(); \
         return {{shown: say(all.filter(shown)[0]), hidden: say(all.filter(v => !shown(v))[0]), \
           idle: box ? {{x: box.left, y: box.top, w: box.width, h: box.height}} : null}}; }})()"
    )
}

/// Where to press play for a player that loads nothing until asked: the `idle` box of a probe, in
/// viewport pixels.
pub fn idle_box(probe: &serde_json::Value) -> Option<(f64, f64, f64, f64)> {
    let b = probe.get("idle")?;
    let n = |k: &str| b.get(k).and_then(|v| v.as_f64());
    Some((n("x")?, n("y")?, n("w")?, n("h")?))
}

/// Start a video the player has parked, or left on the page without loading: muted, as any page
/// may. The player moves it into place
/// once it plays; `seek_js` pauses it again.
pub fn wake_js() -> String {
    format!(
        "(() => {{ const shown = {SHOWN}; \
         const v = {VIDEOS}.filter(v => !shown(v) || v.readyState < 2)[0]; \
         if (!v) return false; v.muted = true; v.play().catch(() => {{}}); return true; }})()"
    )
}

/// Run every video of another length than `want` through, muted and at the fastest rate the
/// element takes: the player's own logic then moves on to the video when the advert ends, in
/// seconds instead of minutes (measured: a 150 s advert, against a 90 s budget for the call).
pub fn hurry_js(want: f64) -> String {
    format!(
        "(() => {{ let n = 0; for (const v of document.querySelectorAll('video')) {{ \
         if (!Number.isFinite(v.duration) || Math.abs(v.duration - {want}) < 3) continue; \
         v.muted = true; v.playbackRate = 16; v.play().catch(() => {{}}); n++; }} return n; }})()"
    )
}

/// Pause the main video on `t` and resolve once that frame is painted, with where it is on the
/// page (document coordinates, CSS pixels) for the capture to clip to.
///
/// Self-contained like every script this crate evaluates: it reads the element and sets the
/// state a person's seek would set, and leaves nothing on `window` or in the DOM.
pub fn seek_js(t: f64) -> String {
    let main = main_js();
    format!(
        r#"(async () => {{
            const v = {main};
            if (!v) return {{ok: false}};
            v.muted = true;
            v.pause();
            const t = Math.max(0, Math.min({t}, (Number.isFinite(v.duration) ? v.duration : {t}) - 0.1));
            if (Math.abs(v.currentTime - t) > 0.05) {{
                await new Promise(done => {{
                    const on = () => {{ v.removeEventListener('seeked', on); done(); }};
                    v.addEventListener('seeked', on);
                    v.currentTime = t;
                    setTimeout(done, 8000);
                }});
            }}
            await new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
            const r = v.getBoundingClientRect();
            return {{ok: v.readyState >= 2 && v.videoWidth > 0 && ({SHOWN})(v), t: v.currentTime,
                x: r.left + scrollX, y: r.top + scrollY, w: r.width, h: r.height}};
        }})()"#
    )
}

/// The main video's current picture, read from the element itself, as a base64 PNG at most
/// `max_w` wide: no controls, no captions, nothing the player draws over it.
///
/// `{ok: false}` when the picture cannot be read: a cross-origin file served without CORS taints
/// the canvas, and a protected stream draws nothing. The caller then clips a screenshot instead.
/// The canvas is never attached to the document.
pub fn grab_js(max_w: u32) -> String {
    let main = main_js();
    format!(
        r#"(async () => {{
            const v = {main};
            if (!v || !v.videoWidth) return {{ok: false}};
            const k = Math.min(1, {max_w} / v.videoWidth);
            const w = Math.round(v.videoWidth * k), h = Math.round(v.videoHeight * k);
            try {{
                const c = new OffscreenCanvas(w, h);
                c.getContext('2d').drawImage(v, 0, 0, w, h);
                const bytes = new Uint8Array(await (await c.convertToBlob({{type: 'image/png'}})).arrayBuffer());
                let s = '';
                for (let i = 0; i < bytes.length; i += 32768) s += String.fromCharCode(...bytes.subarray(i, i + 32768));
                return {{ok: true, png: btoa(s)}};
            }} catch (_) {{ return {{ok: false}}; }}
        }})()"#
    )
}

/// Widest frame read from the element. A 4K source would otherwise be 10 MB a frame.
pub const GRAB_MAX_WIDTH: u32 = 1280;

/// A picture with nothing in it: one flat colour, the way an element that painted nothing
/// captures. Measured on luma, 0..255.
pub fn blank(img: &RgbImage) -> bool {
    let n = (img.width() * img.height()).max(1) as f64;
    let (mut s, mut s2) = (0f64, 0f64);
    for p in img.pixels() {
        let l = 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64;
        s += l;
        s2 += l * l;
    }
    let mean = s / n;
    (s2 / n - mean * mean).max(0.0).sqrt() < 3.0
}

/// Which captures are worth returning: not blank, and not the same picture as one already kept.
/// Returns the indices kept, then how many were blank and how many repeated.
pub fn informative(thumbs: &[RgbImage]) -> (Vec<usize>, usize, usize) {
    let (mut keep, mut blanks, mut repeats) = (Vec::new(), 0, 0);
    let mut seen: Vec<Vec<f32>> = Vec::new();
    for (i, t) in thumbs.iter().enumerate() {
        if blank(t) {
            blanks += 1;
            continue;
        }
        let h = histogram(t);
        if seen.iter().any(|s| distance(s, &h) < 0.002) {
            repeats += 1;
            continue;
        }
        seen.push(h);
        keep.push(i);
    }
    (keep, blanks, repeats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn solid(r: u8, g: u8, b: u8) -> RgbImage {
        RgbImage::from_pixel(16, 9, Rgb([r, g, b]))
    }

    #[test]
    fn the_same_picture_is_no_change_and_opposite_colours_are_all_change() {
        let a = histogram(&solid(10, 10, 10));
        assert_eq!(distance(&a, &a), 0.0);
        let b = histogram(&solid(250, 250, 250));
        assert!((distance(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn frames_land_on_the_cuts_and_not_on_each_other() {
        // Three scenes: dark 0-20 s, red 20-40 s, white 40-60 s, a candidate every 4 s.
        let times: Vec<f64> = (0..15).map(|i| i as f64 * 4.0).collect();
        let hists: Vec<Vec<f32>> = times
            .iter()
            .map(|t| {
                histogram(&match *t as u32 {
                    0..=19 => solid(5, 5, 5),
                    20..=39 => solid(200, 20, 20),
                    _ => solid(250, 250, 250),
                })
            })
            .collect();
        let got: Vec<f64> = pick(&times, &hists, 3, 8.0)
            .into_iter()
            .map(|i| times[i])
            .collect();
        assert_eq!(got, vec![0.0, 20.0, 40.0]);
        // Asked for more than there are scenes: the rest are spaced, never duplicates.
        let more = pick(&times, &hists, 6, 8.0);
        assert!(more.windows(2).all(|w| times[w[1]] - times[w[0]] >= 8.0));
    }

    #[test]
    fn a_cell_is_cut_from_fractions_of_its_sheet() {
        let mut sheet = RgbImage::from_pixel(30, 30, Rgb([0, 0, 0]));
        for y in 10..20 {
            for x in 10..20 {
                sheet.put_pixel(x, y, Rgb([255, 0, 0]));
            }
        }
        let third = 1.0 / 3.0;
        let centre = crop(&DynamicImage::ImageRgb8(sheet), third, third, third, third);
        assert_eq!(centre.dimensions(), (10, 10));
        assert!(centre.pixels().all(|p| p[0] == 255));
    }

    #[test]
    fn even_moments_sit_in_the_middle_of_their_share() {
        assert_eq!(uniform(60.0, 3), vec![10.0, 30.0, 50.0]);
    }

    #[test]
    fn frame_files_sort_in_time_order_and_stay_in_their_directory() {
        let root = Path::new("/out/video");
        let p = frame_path(root, "../../etc", 3725.4);
        assert!(p.starts_with(root.join("______etc")), "{p:?}");
        assert!(p.ends_with("01-02-05.png"));
    }

    #[test]
    fn the_scripts_leave_nothing_behind() {
        for js in [
            seek_js(12.5),
            probe_js(),
            wake_js(),
            grab_js(640),
            hurry_js(60.0),
        ] {
            let low = js.to_ascii_lowercase();
            assert!(!low.contains("svipall"));
            assert!(!low.contains("window.") && !low.contains("globalthis"));
            assert!(!low.contains("setattribute") && !low.contains("dataset"));
            assert!(!low.contains("dispatchevent"), "no forged events");
        }
        assert!(seek_js(12.5).contains("Math.min(12.5,"));
        assert!(hurry_js(1721.5).contains("v.duration - 1721.5"));
        assert!(
            !grab_js(640).contains("appendChild"),
            "the canvas never joins the page"
        );
    }

    fn probe(shown: Option<(f64, f64)>, hidden: Option<f64>) -> serde_json::Value {
        serde_json::json!({
            "shown": shown.map(|(d, w)| serde_json::json!({"d": d, "w": w, "ready": 4})),
            "hidden": hidden.map(|d| serde_json::json!({"d": d, "w": 1920, "ready": 4})),
        })
    }

    #[test]
    fn an_advert_on_screen_is_waited_out_not_captured_behind() {
        // Measured: a 27 s advert on screen, the 1721 s video preloaded above the page.
        let p = probe(Some((27.0, 1280.0)), Some(1721.3));
        assert_eq!(playing(&p, Some(1721.0)), Playing::Advert { parked: false });
        assert_eq!(
            playing(&probe(Some((1721.3, 1920.0)), None), Some(1721.0)),
            Playing::Main {
                length: 1721.3,
                picture: true
            }
        );
    }

    #[test]
    fn a_video_parked_off_the_page_is_cued_not_shown() {
        assert_eq!(
            playing(&probe(None, Some(1721.3)), Some(1721.0)),
            Playing::Cued
        );
        assert_eq!(playing(&probe(None, Some(1721.3)), None), Playing::Cued);
        assert_eq!(
            playing(&probe(None, Some(27.0)), Some(1721.0)),
            Playing::Advert { parked: true },
            "measured: on a second visit the advert waits off the page too, and nothing plays"
        );
        assert_eq!(playing(&probe(None, None), None), Playing::Nothing);
    }

    #[test]
    fn with_no_length_to_compare_what_is_on_screen_is_the_video() {
        assert_eq!(
            playing(&probe(Some((30.0, 0.0)), None), None),
            Playing::Main {
                length: 30.0,
                picture: false
            }
        );
    }

    #[test]
    fn a_video_on_screen_with_no_frame_yet_is_loading_not_pictureless() {
        // Measured: length set, nothing loaded, width 0, and it was reported as a lost codec.
        let p = serde_json::json!({"shown": {"d": 61.853, "ready": 0, "w": 0}, "hidden": null});
        assert_eq!(playing(&p, Some(62.0)), Playing::Cued);
    }

    #[test]
    fn a_player_waiting_for_its_play_button_says_where_it_is() {
        let p = serde_json::json!({"shown": null, "hidden": {"d": null, "ready": 0, "w": 0},
            "idle": {"x": 0.0, "y": 10.0, "w": 640.0, "h": 360.0}});
        assert_eq!(playing(&p, None), Playing::Nothing);
        assert_eq!(idle_box(&p), Some((0.0, 10.0, 640.0, 360.0)));
        assert_eq!(idle_box(&probe(Some((30.0, 640.0)), None)), None);
    }

    fn textured(seed: u8) -> RgbImage {
        RgbImage::from_fn(32, 18, |x, y| {
            Rgb([
                (x * 8) as u8 ^ seed,
                (y * 14) as u8,
                ((x + y) * 5) as u8 ^ seed,
            ])
        })
    }

    #[test]
    fn a_flat_capture_is_blank_and_a_picture_is_not() {
        assert!(blank(&solid(255, 255, 255)));
        assert!(blank(&solid(0, 0, 0)));
        assert!(!blank(&textured(0)));
    }

    #[test]
    fn blank_and_repeated_captures_are_not_returned_and_are_counted() {
        let shots = vec![
            solid(255, 255, 255),
            textured(0),
            textured(0),
            solid(255, 255, 255),
            textured(99),
        ];
        assert_eq!(informative(&shots), (vec![1, 4], 2, 1));
    }
}

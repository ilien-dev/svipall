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

/// The largest `<video>` on the page, as a script expression. Players keep tiny ones for previews.
const LARGEST: &str = "[...document.querySelectorAll('video')].sort((a, b) => \
    b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)[0]";

/// How long the page's main video is, and whether it has anything to show yet.
pub fn probe_js() -> String {
    format!(
        "(() => {{ const v = {LARGEST}; \
         return v ? {{d: Number.isFinite(v.duration) ? v.duration : null, ready: v.readyState, \
         w: v.videoWidth}} : null; }})()"
    )
}

/// Pause the main video on `t` and resolve once that frame is painted, with where it is on the
/// page (document coordinates, CSS pixels) for the capture to clip to.
///
/// Self-contained like every script this crate evaluates: it reads the element and sets the
/// state a person's seek would set, and leaves nothing on `window` or in the DOM.
pub fn seek_js(t: f64) -> String {
    format!(
        r#"(async () => {{
            const v = {LARGEST};
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
            return {{ok: v.readyState >= 2 && v.videoWidth > 0 && r.width > 0, t: v.currentTime,
                x: r.left + scrollX, y: r.top + scrollY, w: r.width, h: r.height}};
        }})()"#
    )
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
        for js in [seek_js(12.5), probe_js()] {
            let low = js.to_ascii_lowercase();
            assert!(!low.contains("svipall"));
            assert!(!low.contains("window.") && !low.contains("globalthis"));
            assert!(!low.contains("setattribute") && !low.contains("dataset"));
            assert!(!low.contains("dispatchevent"), "no forged events");
        }
        assert!(seek_js(12.5).contains("Math.min(12.5,"));
    }
}

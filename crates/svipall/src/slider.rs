//! Reading a slider challenge off a screenshot, with no access to its DOM.
//!
//! The fingerprinting vendor's slider lives in a frame in another process, where this session can
//! evaluate nothing. What it can do is take a picture of the frame and dispatch pointer events at
//! page coordinates — both of which cross the process boundary — so the whole challenge has to be
//! read from pixels: where the puzzle image is, where its notch is, where the handle sits.
//!
//! Everything here is pure geometry over a grayscale image, so it is tested against pictures drawn
//! in the test itself. `vision::notch_column` finds the notch; this file finds everything around it.

use crate::vision::{notch_column, Offset};
use image::GrayImage;

/// A rectangle in image pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn right(&self) -> u32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }
}

/// Where the parts of the widget are on the screenshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The puzzle picture.
    pub image: Rect,
    /// The centre of the handle, in screenshot pixels.
    pub handle: (u32, u32),
}

/// The gesture that should complete the challenge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub from: (f64, f64),
    pub to: (f64, f64),
    /// From the notch finder. Below the caller's threshold this is a guess.
    pub confidence: f32,
}

/// The value most pixels have. On these widgets that is the page behind the picture.
fn background(img: &GrayImage) -> u8 {
    let mut hist = [0u32; 256];
    for p in img.pixels() {
        hist[p.0[0] as usize] += 1;
    }
    hist.iter()
        .enumerate()
        .max_by_key(|(_, n)| **n)
        .map(|(v, _)| v as u8)
        .unwrap_or(255)
}

/// Fraction of each row (or column) that is not background.
fn profile(img: &GrayImage, bg: u8, rows: bool) -> Vec<f32> {
    let (w, h) = img.dimensions();
    let (outer, inner) = if rows { (h, w) } else { (w, h) };
    (0..outer)
        .map(|o| {
            let hits = (0..inner)
                .filter(|&i| {
                    let p = if rows {
                        img.get_pixel(i, o)
                    } else {
                        img.get_pixel(o, i)
                    };
                    (p.0[0] as i32 - bg as i32).unsigned_abs() > 24
                })
                .count();
            hits as f32 / inner.max(1) as f32
        })
        .collect()
}

/// The longest run of indices whose value is at least `min`.
fn longest_run(v: &[f32], min: f32) -> Option<(u32, u32)> {
    let mut best: Option<(u32, u32)> = None;
    let mut start: Option<usize> = None;
    for (i, x) in v.iter().enumerate() {
        match (start, *x >= min) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                let len = i - s;
                if best.is_none_or(|(_, l)| len as u32 > l) {
                    best = Some((s as u32, len as u32));
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        let len = v.len() - s;
        if best.is_none_or(|(_, l)| len as u32 > l) {
            best = Some((s as u32, len as u32));
        }
    }
    best
}

/// Find the picture and the handle.
///
/// The picture is the largest dense block of non-background rows; the handle is the first
/// non-background blob in the strip beneath it, which is where every one of these widgets puts
/// its slider. Nothing here depends on colour, so a dark theme and a light one read the same.
pub fn locate(img: &GrayImage) -> Option<Layout> {
    let (w, h) = img.dimensions();
    if w < 64 || h < 64 {
        return None;
    }
    let bg = background(img);
    let rows = profile(img, bg, true);
    // The picture fills most of its rows; text and controls do not.
    let (y, ih) = longest_run(&rows, 0.5)?;
    if ih < 24 {
        return None;
    }
    // Its columns, within those rows.
    let band = image::imageops::crop_imm(img, 0, y, w, ih).to_image();
    let cols = profile(&band, bg, false);
    let (x, iw) = longest_run(&cols, 0.5)?;
    if iw < 48 {
        return None;
    }
    let image = Rect { x, y, w: iw, h: ih };

    // The handle: in the strip below the picture, the first dense blob from the left, within the
    // picture's own columns. Bounded so a footer or a logo further down is never mistaken for it.
    let strip_top = image.bottom();
    let strip_h = (h - strip_top).min(image.h.max(40));
    if strip_h < 8 {
        return None;
    }
    let strip = image::imageops::crop_imm(img, image.x, strip_top, image.w, strip_h).to_image();
    let strip_rows = profile(&strip, bg, true);
    let (hy, hh) = longest_run(&strip_rows, 0.02)?;
    let track = image::imageops::crop_imm(&strip, 0, hy, image.w, hh).to_image();
    let track_cols = profile(&track, bg, false);
    // The handle is the densest thing on the track: a filled circle against a thin bar.
    let peak = track_cols.iter().cloned().fold(0f32, f32::max);
    if peak <= 0.0 {
        return None;
    }
    let (hx, hw) = longest_run(&track_cols, peak * 0.6)?;
    Some(Layout {
        image,
        handle: (image.x + hx + hw / 2, strip_top + hy + hh / 2),
    })
}

/// Plan the drag: from the handle, right by the distance between the piece and the notch.
///
/// The piece sits at the left edge of the picture and the handle moves it one-for-one, which is
/// the arrangement every widget of this kind uses; the distance is therefore the notch's own x.
/// `None` when the notch cannot be found with any confidence — a drag to a guessed position is
/// an attempt spent, and the site counts it.
pub fn plan(img: &GrayImage, min_confidence: f32) -> Option<Drag> {
    let layout = locate(img)?;
    let picture = image::imageops::crop_imm(
        img,
        layout.image.x,
        layout.image.y,
        layout.image.w,
        layout.image.h,
    )
    .to_image();
    let Offset { x, confidence } = notch_column(&picture)?;
    if confidence < min_confidence {
        return None;
    }
    let (hx, hy) = layout.handle;
    Some(Drag {
        from: (hx as f64, hy as f64),
        to: (hx as f64 + x as f64, hy as f64),
        confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    /// A widget drawn the way the real one is laid out: a textured picture with a notch, a thin
    /// track under it, and a filled handle at the track's left end. White page around it.
    fn widget(notch_x: u32) -> (GrayImage, Rect, (u32, u32)) {
        let mut img = GrayImage::from_pixel(400, 300, Luma([255]));
        let pic = Rect {
            x: 50,
            y: 40,
            w: 300,
            h: 150,
        };
        for y in pic.y..pic.bottom() {
            for x in pic.x..pic.right() {
                // Vertical stripes: enough texture for edge energy to mean something.
                let v = if ((x - pic.x) / 6).is_multiple_of(2) {
                    120
                } else {
                    160
                };
                img.put_pixel(x, y, Luma([v]));
            }
        }
        // The notch: a hole in the picture, page-coloured, at the given column.
        for y in pic.y + 50..pic.y + 100 {
            for x in pic.x + notch_x..pic.x + notch_x + 40 {
                img.put_pixel(x, y, Luma([252]));
            }
        }
        // The track and the handle beneath.
        let ty = pic.bottom() + 30;
        for x in pic.x..pic.right() {
            img.put_pixel(x, ty, Luma([200]));
        }
        let handle = (pic.x + 20, ty);
        for dy in 0..20u32 {
            for dx in 0..20u32 {
                let (cx, cy) = (dx as i32 - 10, dy as i32 - 10);
                if cx * cx + cy * cy <= 100 {
                    img.put_pixel(handle.0 + dx - 10, handle.1 + dy - 10, Luma([30]));
                }
            }
        }
        (img, pic, handle)
    }

    #[test]
    fn the_picture_and_the_handle_are_found_where_they_were_drawn() {
        let (img, pic, handle) = widget(180);
        let layout = locate(&img).expect("a widget");
        assert!(
            (layout.image.x as i32 - pic.x as i32).abs() <= 2
                && (layout.image.y as i32 - pic.y as i32).abs() <= 2,
            "{layout:?} vs {pic:?}"
        );
        assert!(
            (layout.handle.0 as i32 - handle.0 as i32).abs() <= 3
                && (layout.handle.1 as i32 - handle.1 as i32).abs() <= 3,
            "{:?} vs {handle:?}",
            layout.handle
        );
    }

    #[test]
    fn the_drag_goes_from_the_handle_to_the_notch() {
        // The whole point: one gesture, from where the handle is, by exactly the notch's offset.
        let (img, _, handle) = widget(180);
        let drag = plan(&img, 0.2).expect("a plan");
        assert!((drag.from.0 - handle.0 as f64).abs() <= 3.0, "{drag:?}");
        assert!(
            (drag.to.0 - drag.from.0 - 180.0).abs() <= 6.0,
            "drag distance should be the notch offset: {drag:?}"
        );
        assert_eq!(drag.from.1, drag.to.1, "a slider moves sideways only");
    }

    #[test]
    fn a_blank_page_is_not_a_widget() {
        // A drag to a guessed position is an attempt spent, and the site counts it.
        let blank = GrayImage::from_pixel(400, 300, Luma([255]));
        assert!(locate(&blank).is_none());
        assert!(plan(&blank, 0.2).is_none());
    }

    #[test]
    fn a_picture_with_no_notch_yields_no_plan() {
        let (mut img, pic, _) = widget(180);
        // Fill the notch back in.
        for y in pic.y + 50..pic.y + 100 {
            for x in pic.x + 180..pic.x + 220 {
                let v = if ((x - pic.x) / 6).is_multiple_of(2) {
                    120
                } else {
                    160
                };
                img.put_pixel(x, y, Luma([v]));
            }
        }
        assert!(
            plan(&img, 0.3).is_none(),
            "no notch, no confidence, no gesture"
        );
    }

    #[test]
    fn the_longest_run_is_the_longest_not_the_first() {
        assert_eq!(
            longest_run(&[1.0, 0.0, 1.0, 1.0, 1.0, 0.0], 0.5),
            Some((2, 3))
        );
        assert_eq!(longest_run(&[0.0, 0.0], 0.5), None);
        assert_eq!(longest_run(&[1.0, 1.0], 0.5), Some((0, 2)));
    }
}

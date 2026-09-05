//! Captchas that are geometry, not recognition.
//!
//! A slider asks where a notch is. A rotation asks which way is up. Neither needs to know what the
//! picture is *of*, which is what makes them solvable here without a model, without a download, and
//! without anyone's attention: they are image processing from before machine learning existed.
//!
//! Every function returns a confidence alongside its answer and `None` below a threshold. That is
//! the same contract the image-grid classifier holds to, and for the same reason: clicking on a
//! guess spends the attempt and tells the widget what it is dealing with, so not answering is
//! strictly better than answering badly.
//!
//! The tests build their own images — a background, a notch cut at a known column, a piece pasted
//! in — so the geometry is checked against a known answer rather than against a fixture nobody can
//! verify by eye.

use image::GrayImage;

/// Where the slider's piece has to go, and how sure we are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Offset {
    /// Horizontal distance to drag, in pixels of the image.
    pub x: u32,
    /// 0..1. Below the caller's threshold, this is a guess and should not be acted on.
    pub confidence: f32,
}

/// Find the notch in a slider background by looking for the column that is least like its
/// neighbours.
///
/// The notch is a hole punched into the picture, so it interrupts whatever texture was there. That
/// shows up as a column of unusually strong vertical edges — no template needed, which matters
/// because the piece is often rendered separately and never available on its own.
pub fn notch_column(bg: &GrayImage) -> Option<Offset> {
    let (w, h) = bg.dimensions();
    if w < 24 || h < 8 {
        return None;
    }
    // Edge energy per column: how much this column differs from the one before it.
    let mut energy = vec![0f32; w as usize];
    for x in 1..w {
        let mut sum = 0f32;
        for y in 0..h {
            let a = bg.get_pixel(x, y).0[0] as f32;
            let b = bg.get_pixel(x - 1, y).0[0] as f32;
            sum += (a - b).abs();
        }
        energy[x as usize] = sum / h as f32;
    }
    // The leftmost stretch is the piece itself in most of these widgets, and it is always the
    // strongest edge on the image. Ignoring it is the difference between finding the notch and
    // finding the piece.
    let skip = (w / 8).max(4) as usize;
    let (mut best_x, mut best) = (0usize, 0f32);
    let mut total = 0f32;
    for (x, e) in energy.iter().enumerate().skip(skip) {
        total += e;
        if *e > best {
            best = *e;
            best_x = x;
        }
    }
    let mean = total / (w as usize - skip).max(1) as f32;
    if mean <= f32::EPSILON {
        return None;
    }
    // A notch has two edges of equal strength — where the picture stops and where it resumes — and
    // the strongest column is as likely to be the far one. The piece slides to the *near* edge, so
    // among the columns that are essentially as strong as the winner, take the leftmost. Without
    // this the drag overshoots by exactly the width of the notch, every time.
    if let Some((x, _)) = energy
        .iter()
        .enumerate()
        .skip(skip)
        .find(|(_, e)| **e >= best * 0.85)
    {
        best_x = x;
    }
    // How far the winner stands out from the average column. A flat image has no notch, and saying
    // so beats returning its noisiest column.
    let ratio = best / mean;
    // A notch is unique: two edges, where the picture stops and where it resumes. A picture whose
    // texture repeats — stripes, tiles, a brick wall — has strong edges everywhere, and the average
    // says nothing about that. Every column as strong as the winner beyond that pair is evidence
    // the winner is texture, not a hole.
    let rivals = energy
        .iter()
        .skip(skip)
        .filter(|e| **e >= best * 0.85)
        .count()
        .saturating_sub(2) as f32;
    Some(Offset {
        x: best_x as u32,
        confidence: (((ratio - 1.0) / 4.0) / (1.0 + rivals)).clamp(0.0, 1.0),
    })
}

/// Locate a piece inside a background by normalised cross-correlation.
///
/// Used when the widget does hand over the piece separately, which gives a much stronger answer
/// than edge energy alone.
pub fn match_template(bg: &GrayImage, piece: &GrayImage) -> Option<Offset> {
    let (bw, bh) = bg.dimensions();
    let (pw, ph) = piece.dimensions();
    if pw == 0 || ph == 0 || pw > bw || ph > bh {
        return None;
    }
    let p_mean = mean_of(piece);
    let mut best = (0u32, f32::MIN);
    let mut second = f32::MIN;
    // Only the horizontal axis matters: these pieces slide, they do not rise.
    let y0 = (bh - ph) / 2;
    for x in 0..=(bw - pw) {
        let mut num = 0f32;
        let mut den_b = 0f32;
        let mut den_p = 0f32;
        let b_mean = window_mean(bg, x, y0, pw, ph);
        for py in 0..ph {
            for px in 0..pw {
                let b = bg.get_pixel(x + px, y0 + py).0[0] as f32 - b_mean;
                let p = piece.get_pixel(px, py).0[0] as f32 - p_mean;
                num += b * p;
                den_b += b * b;
                den_p += p * p;
            }
        }
        let denom = (den_b * den_p).sqrt();
        let score = if denom > f32::EPSILON {
            num / denom
        } else {
            0.0
        };
        if score > best.1 {
            second = best.1;
            best = (x, score);
        } else if score > second {
            second = score;
        }
    }
    if best.1 <= 0.0 {
        return None;
    }
    // Confidence is how much the winner beats the runner-up. A picture that matches equally well
    // in twenty places has not been located, however high the raw score is.
    let margin = (best.1 - second.max(0.0)).clamp(0.0, 1.0);
    Some(Offset {
        x: best.0,
        confidence: (best.1 * margin.max(0.05)).clamp(0.0, 1.0),
    })
}

fn mean_of(img: &GrayImage) -> f32 {
    let n = (img.width() * img.height()) as f32;
    if n == 0.0 {
        return 0.0;
    }
    img.pixels().map(|p| p.0[0] as f32).sum::<f32>() / n
}

fn window_mean(img: &GrayImage, x: u32, y: u32, w: u32, h: u32) -> f32 {
    let mut sum = 0f32;
    for yy in 0..h {
        for xx in 0..w {
            sum += img.get_pixel(x + xx, y + yy).0[0] as f32;
        }
    }
    sum / (w * h) as f32
}

/// How far an image has been rotated, in degrees, and how sure we are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    pub degrees: f32,
    pub confidence: f32,
}

/// Find the rotation that puts an image back upright.
///
/// Photographs are full of horizontal and vertical lines — horizons, buildings, tables — and those
/// lines produce the strongest gradients when they are axis-aligned. So the upright angle is the
/// one that maximises axis-aligned edge energy, found by trying each candidate.
pub fn upright_angle(img: &GrayImage, step_degrees: f32) -> Option<Rotation> {
    let step = if step_degrees <= 0.0 {
        6.0
    } else {
        step_degrees
    };
    let mut best = (0f32, f32::MIN);
    let mut total = 0f32;
    let mut n = 0f32;
    let mut angle = 0f32;
    while angle < 360.0 {
        let score = axis_energy(img, angle);
        total += score;
        n += 1.0;
        if score > best.1 {
            best = (angle, score);
        }
        angle += step;
    }
    if n == 0.0 || best.1 <= 0.0 {
        return None;
    }
    let mean = total / n;
    let ratio = if mean > f32::EPSILON {
        best.1 / mean
    } else {
        1.0
    };
    Some(Rotation {
        // The answer is how far to turn it *back*.
        degrees: (360.0 - best.0) % 360.0,
        confidence: ((ratio - 1.0) * 3.0).clamp(0.0, 1.0),
    })
}

/// Axis-aligned edge energy after rotating by `angle`, sampled rather than fully rendered: the
/// answer only needs to compare candidates, not to look at anything.
fn axis_energy(img: &GrayImage, angle: f32) -> f32 {
    let (w, h) = img.dimensions();
    if w < 4 || h < 4 {
        return 0.0;
    }
    let rad = -angle.to_radians();
    let (sin, cos) = rad.sin_cos();
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    // Stay inside the inscribed circle so no candidate is scored on the corners it lost to
    // rotation, which would favour the angles that keep the most image.
    let r = (w.min(h) as f32 / 2.0) - 2.0;
    let mut energy = 0f32;
    let mut count = 0f32;
    let stride = 2;
    let mut y = 1;
    while y < h - 1 {
        let mut x = 1;
        while x < w - 1 {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            if dx * dx + dy * dy <= r * r {
                let sx = cx + dx * cos - dy * sin;
                let sy = cy + dx * sin + dy * cos;
                if sx >= 1.0 && sy >= 1.0 && sx < w as f32 - 1.0 && sy < h as f32 - 1.0 {
                    let (px, py) = (sx as u32, sy as u32);
                    let c = img.get_pixel(px, py).0[0] as f32;
                    let right = img.get_pixel(px + 1, py).0[0] as f32;
                    let down = img.get_pixel(px, py + 1).0[0] as f32;
                    energy += (c - right).abs() + (c - down).abs();
                    count += 1.0;
                }
            }
            x += stride;
        }
        y += stride;
    }
    if count == 0.0 {
        0.0
    } else {
        energy / count
    }
}

/// Decode PNG or JPEG bytes to grayscale.
pub fn to_gray(bytes: &[u8]) -> Option<GrayImage> {
    image::load_from_memory(bytes).ok().map(|i| i.to_luma8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    /// A background with vertical texture and a notch cut at a known column.
    fn slider_bg(width: u32, notch_x: u32) -> GrayImage {
        let mut img = GrayImage::from_pixel(width, 60, Luma([140]));
        for x in 0..width {
            for y in 0..60 {
                // Gentle texture, so the notch is not the only thing in the picture.
                let v = 120 + ((x * 7 + y * 3) % 40) as u8;
                img.put_pixel(x, y, Luma([v]));
            }
        }
        // The notch: a dark square, which is what a hole punched in the image looks like.
        for x in notch_x..(notch_x + 20).min(width) {
            for y in 20..44 {
                img.put_pixel(x, y, Luma([20]));
            }
        }
        img
    }

    #[test]
    fn the_notch_is_found_where_it_was_cut() {
        let img = slider_bg(300, 180);
        let o = notch_column(&img).expect("a notch");
        assert!(
            (o.x as i64 - 180).abs() <= 3,
            "found {} instead of 180",
            o.x
        );
        assert!(o.confidence > 0.2, "confidence {}", o.confidence);
    }

    #[test]
    fn the_piece_on_the_left_is_not_mistaken_for_the_notch() {
        // The piece is always the strongest edge on the image, and it always sits at the far left.
        // Returning its position would drag the slider nowhere.
        let mut img = slider_bg(300, 200);
        for x in 0..24 {
            for y in 18..46 {
                img.put_pixel(x, y, Luma([255]));
            }
        }
        let o = notch_column(&img).expect("a notch");
        assert!(o.x > 100, "locked onto the piece at {}", o.x);
    }

    #[test]
    fn a_flat_image_has_no_notch_to_find() {
        // Returning the noisiest column of a blank picture is a guess, and a guess spends the
        // attempt.
        let flat = GrayImage::from_pixel(200, 60, Luma([128]));
        let found = notch_column(&flat);
        assert!(
            found.is_none() || found.unwrap().confidence < 0.2,
            "claimed to find a notch in a blank image"
        );
    }

    #[test]
    fn an_image_too_small_to_hold_a_slider_is_refused() {
        assert!(notch_column(&GrayImage::from_pixel(10, 4, Luma([0]))).is_none());
    }

    #[test]
    fn a_template_is_located_where_it_was_pasted() {
        let mut bg = slider_bg(240, 0);
        // Paste a distinctive block at a known column.
        let mut piece = GrayImage::from_pixel(20, 20, Luma([0]));
        for i in 0..20 {
            piece.put_pixel(i, i, Luma([255]));
        }
        for x in 0..20 {
            for y in 0..20 {
                bg.put_pixel(150 + x, 20 + y, *piece.get_pixel(x, y));
            }
        }
        let o = match_template(&bg, &piece).expect("located");
        assert!(
            (o.x as i64 - 150).abs() <= 4,
            "found {} instead of 150",
            o.x
        );
    }

    #[test]
    fn a_template_bigger_than_its_background_is_refused_rather_than_panicking() {
        let bg = GrayImage::from_pixel(10, 10, Luma([0]));
        let piece = GrayImage::from_pixel(20, 20, Luma([0]));
        assert!(match_template(&bg, &piece).is_none());
    }

    #[test]
    fn an_upright_image_needs_no_rotation() {
        // Strong horizontal lines: the answer is zero, and anything else means the search is
        // finding structure that is not there.
        let mut img = GrayImage::from_pixel(80, 80, Luma([200]));
        for y in (10..70).step_by(10) {
            for x in 5..75 {
                img.put_pixel(x, y, Luma([10]));
            }
        }
        let r = upright_angle(&img, 15.0).expect("an angle");
        let off = r.degrees.min(360.0 - r.degrees);
        assert!(
            off <= 20.0,
            "said {} degrees for an upright image",
            r.degrees
        );
    }

    #[test]
    fn a_featureless_image_reports_low_confidence() {
        // Nothing to align, so any angle is as good as any other, and the caller must be told.
        let flat = GrayImage::from_pixel(80, 80, Luma([128]));
        match upright_angle(&flat, 30.0) {
            None => {}
            Some(r) => assert!(r.confidence < 0.3, "confident about noise: {r:?}"),
        }
    }

    #[test]
    fn an_angle_is_always_reported_as_a_turn_back_within_one_revolution() {
        let mut img = GrayImage::from_pixel(60, 60, Luma([200]));
        for y in (8..52).step_by(8) {
            for x in 4..56 {
                img.put_pixel(x, y, Luma([0]));
            }
        }
        let r = upright_angle(&img, 30.0).expect("an angle");
        assert!((0.0..360.0).contains(&r.degrees), "{}", r.degrees);
    }

    #[test]
    fn unreadable_bytes_are_none_rather_than_a_panic() {
        assert!(to_gray(b"not an image at all").is_none());
        assert!(to_gray(&[]).is_none());
    }
}

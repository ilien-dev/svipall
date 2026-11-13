//! Contact sheets: many moments of a video in one picture, each with its time written on it.
//!
//! A sheet is what a model looks at first. Sixteen moments on one cost about what one full frame
//! does, so the whole of a video can be seen for the price of a few frames, and the time in the
//! corner of each cell is what lets the reader say *where* to look closer. A sheet is for finding
//! the moment, never for reading it: text in a cell is a few pixels tall.
//!
//! The digits are drawn from a table in this file. A sheet needs eleven glyphs, and a font file
//! is a licence and a megabyte to carry for them.

use image::{imageops::FilterType, Rgb, RgbImage};
use svipall_core::video::{align::clock, look};

/// One moment on its way to a sheet.
pub struct Cell {
    pub t: f64,
    pub img: RgbImage,
}

/// A sheet drawn, with what it shows.
pub struct Sheet {
    pub image: RgbImage,
    /// The first and last moment on it.
    pub from: f64,
    pub to: f64,
    pub cols: u32,
    pub rows: u32,
    pub cells: usize,
}

/// The narrowest a cell is drawn. A storyboard's own cell is often smaller; under this a model's
/// patches each cover too much of the picture to tell what is in it.
const MIN_CELL: u32 = 256;

/// `0`-`9` and `:`, five columns by seven rows, a bit a pixel, top row first.
const GLYPHS: [[u8; 7]; 11] = [
    [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
    [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
    [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
    [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
    [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
    [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
    [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
    [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
    [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
    [0x00, 0x04, 0x04, 0x00, 0x04, 0x04, 0x00],
];

fn glyph(c: char) -> Option<&'static [u8; 7]> {
    match c {
        '0'..='9' => Some(&GLYPHS[c as usize - '0' as usize]),
        ':' => Some(&GLYPHS[10]),
        _ => None,
    }
}

/// Write `text` on a dark box with its corner at `(x, y)`: white on black, whatever is under it.
fn label(img: &mut RgbImage, x: u32, y: u32, text: &str, scale: u32) {
    let chars: Vec<&[u8; 7]> = text.chars().filter_map(glyph).collect();
    let (w, h) = (
        chars.len() as u32 * 6 * scale + scale,
        7 * scale + 2 * scale,
    );
    for dy in 0..h {
        for dx in 0..w {
            if x + dx < img.width() && y + dy < img.height() {
                img.put_pixel(x + dx, y + dy, Rgb([0, 0, 0]));
            }
        }
    }
    for (i, g) in chars.iter().enumerate() {
        for (row, bits) in g.iter().enumerate() {
            for col in 0..5u32 {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                let (px, py) = (
                    x + scale + (i as u32 * 6 + col) * scale,
                    y + scale + row as u32 * scale,
                );
                for dy in 0..scale {
                    for dx in 0..scale {
                        if px + dx < img.width() && py + dy < img.height() {
                            img.put_pixel(px + dx, py + dy, Rgb([255, 255, 255]));
                        }
                    }
                }
            }
        }
    }
}

/// The moments on sheets of sixteen, in time order, each cell with its time in the corner.
///
/// Every cell of a sheet has one size, taken from the first picture's shape. A cell is never
/// drawn wider than its picture is (above [`MIN_CELL`]): enlarging a thumbnail costs the reader
/// tokens and shows nothing more.
pub fn compose(cells: &[Cell]) -> Vec<Sheet> {
    cells
        .chunks(look::SHEET_CELLS)
        .filter_map(|chunk| {
            let first = chunk.first()?;
            let (sw, sh) = (first.img.width().max(1), first.img.height().max(1));
            let (cols, rows) = look::grid(chunk.len());
            let cw = sw.clamp(MIN_CELL, (look::SHEET_WIDTH / cols).max(MIN_CELL));
            let ch = ((cw as f64 * sh as f64 / sw as f64).round() as u32).max(1);
            let mut image = RgbImage::from_pixel(cols * cw, rows * ch, Rgb([0, 0, 0]));
            let scale = (cw / 128).max(2);
            for (i, cell) in chunk.iter().enumerate() {
                let (x, y) = ((i as u32 % cols) * cw, (i as u32 / cols) * ch);
                let fitted = if cell.img.dimensions() == (cw, ch) {
                    cell.img.clone()
                } else {
                    image::imageops::resize(&cell.img, cw, ch, FilterType::Triangle)
                };
                image::imageops::replace(&mut image, &fitted, x as i64, y as i64);
                label(&mut image, x, y, &clock(cell.t), scale);
            }
            Some(Sheet {
                image,
                from: first.t,
                to: chunk.last().map_or(first.t, |c| c.t),
                cols,
                rows,
                cells: chunk.len(),
            })
        })
        .collect()
}

/// A picture as JPEG. Sheets and video frames are photographs as far as a codec is concerned,
/// and a PNG of one is five times the bytes for the same pixels a model reads.
pub fn jpeg(img: &RgbImage, quality: u8) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(out)
}

/// The part of a picture a caller asked to see closer, given as fractions of it: `x, y, w, h`.
/// `None` for a box that is not inside the picture.
pub fn parse_crop(s: &str) -> Option<[f32; 4]> {
    let n: Vec<f32> = s
        .split(',')
        .map(|v| v.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .ok()?;
    let b = <[f32; 4]>::try_from(n).ok()?;
    let ok = b.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        && b[2] > 0.0
        && b[3] > 0.0
        && b[0] + b[2] <= 1.0 + 1e-6
        && b[1] + b[3] <= 1.0 + 1e-6;
    ok.then_some(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(t: f64, w: u32, h: u32, v: u8) -> Cell {
        Cell {
            t,
            img: RgbImage::from_pixel(w, h, Rgb([v, v, v])),
        }
    }

    #[test]
    fn twenty_moments_are_a_full_sheet_and_a_small_one() {
        let cells: Vec<Cell> = (0..20)
            .map(|i| cell(i as f64 * 30.0, 320, 180, 128))
            .collect();
        let sheets = compose(&cells);
        assert_eq!(sheets.len(), 2);
        let s = &sheets[0];
        assert_eq!((s.cols, s.rows, s.cells), (4, 4, 16));
        assert_eq!(
            s.image.dimensions(),
            (1280, 720),
            "a cell is not drawn wider than its picture"
        );
        assert_eq!((s.from, s.to), (0.0, 450.0));
        let t = &sheets[1];
        assert_eq!((t.cols, t.rows, t.cells), (2, 2, 4));
        assert_eq!((t.from, t.to), (480.0, 570.0));
    }

    #[test]
    fn a_tiny_thumbnail_is_drawn_large_enough_to_read_and_no_larger() {
        let cells: Vec<Cell> = (0..16).map(|i| cell(i as f64, 160, 90, 90)).collect();
        let s = &compose(&cells)[0];
        assert_eq!(s.image.dimensions(), (1024, 576));
        let big: Vec<Cell> = (0..16).map(|i| cell(i as f64, 1280, 720, 90)).collect();
        assert_eq!(compose(&big)[0].image.dimensions(), (1536, 864));
    }

    #[test]
    fn every_cell_carries_its_time_in_its_corner() {
        let cells: Vec<Cell> = (0..4)
            .map(|i| cell(i as f64 * 61.0, 320, 180, 128))
            .collect();
        let s = &compose(&cells)[0];
        let (cw, ch) = (s.image.width() / 2, s.image.height() / 2);
        for i in 0..4u32 {
            let (x0, y0) = ((i % 2) * cw, (i / 2) * ch);
            let (mut white, mut black) = (0, 0);
            for y in y0..y0 + 30 {
                for x in x0..x0 + 60 {
                    match s.image.get_pixel(x, y).0 {
                        [255, 255, 255] => white += 1,
                        [0, 0, 0] => black += 1,
                        _ => {}
                    }
                }
            }
            assert!(white > 20 && black > 100, "cell {i}: {white} {black}");
            // The middle of the cell is still the picture.
            assert_eq!(s.image.get_pixel(x0 + cw / 2, y0 + ch / 2).0, [128; 3]);
        }
        // Different times draw different labels.
        let a: Vec<_> = (0..60).map(|x| s.image.get_pixel(x, 8).0).collect();
        let b: Vec<_> = (0..60).map(|x| s.image.get_pixel(cw + x, 8).0).collect();
        assert_ne!(a, b);
    }

    #[test]
    fn a_sheet_is_a_jpeg_a_fraction_of_its_pixels() {
        let cells: Vec<Cell> = (0..16).map(|i| cell(i as f64, 320, 180, 60)).collect();
        let bytes = jpeg(&compose(&cells)[0].image, 80).unwrap();
        assert!(bytes.starts_with(&[0xFF, 0xD8]));
        assert!(bytes.len() < 200_000, "{}", bytes.len());
        let back = image::load_from_memory(&bytes).unwrap();
        assert_eq!((back.width(), back.height()), (1280, 720));
    }

    #[test]
    fn a_crop_is_a_box_inside_the_picture_in_fractions() {
        assert_eq!(parse_crop("0.5, 0.25,0.5,0.5"), Some([0.5, 0.25, 0.5, 0.5]));
        assert_eq!(parse_crop("0,0,1,1"), Some([0.0, 0.0, 1.0, 1.0]));
        for bad in [
            "",
            "0.5,0.5,0.6,0.2",
            "0,0,0,1",
            "10,20,300,400",
            "a,b,c,d",
            "0,0,1",
        ] {
            assert_eq!(parse_crop(bad), None, "{bad}");
        }
    }

    #[test]
    fn nothing_to_show_is_no_sheet() {
        assert!(compose(&[]).is_empty());
    }
}

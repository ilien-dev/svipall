//! The picture on each page of a scanned PDF, for the text reader to read.
//!
//! A scanner writes each page as one image and nothing else. This takes the largest image on each
//! page in the form it is stored: a JPEG as its bytes (a JPEG stream in a PDF is a JPEG file), and
//! deflated raw pixels as pixels. Encodings a scanner uses for black and white (JBIG2, CCITT fax)
//! are not decoded here; they are named instead, so the caller can say which pages were skipped
//! and why.

use lopdf::{Document, Object};

/// One page's picture.
#[derive(Debug, Clone, PartialEq)]
pub enum PageImage {
    /// A complete image file (JPEG, or JPEG 2000), as stored.
    Encoded(Vec<u8>),
    /// Raw 8-bit pixels, `channels` per pixel (1 grey, 3 RGB), row by row.
    Pixels {
        width: u32,
        height: u32,
        channels: u8,
        data: Vec<u8>,
    },
    /// An image in an encoding not decoded here, by its filter's name.
    Unsupported(String),
}

/// The largest image on each of the first `max_pages` pages, by page number. A page with no image
/// is left out.
pub fn page_images(bytes: &[u8], max_pages: usize) -> anyhow::Result<Vec<(u32, PageImage)>> {
    let doc = Document::load_mem(bytes)?;
    let mut out = Vec::new();
    for (number, page) in doc.get_pages().into_iter().take(max_pages) {
        let Ok(images) = doc.get_page_images(page) else {
            continue;
        };
        let Some(img) = images
            .iter()
            .max_by_key(|i| i.width.max(0) as u64 * i.height.max(0) as u64)
        else {
            continue;
        };
        let filters = img.filters.clone().unwrap_or_default();
        let picture = match filters.last().map(String::as_str) {
            Some("DCTDecode") | Some("JPXDecode") if filters.len() == 1 => {
                PageImage::Encoded(img.content.to_vec())
            }
            None | Some("FlateDecode") => raw_pixels(&doc, img),
            Some(other) => PageImage::Unsupported(other.to_string()),
        };
        out.push((number, picture));
    }
    Ok(out)
}

fn raw_pixels(doc: &Document, img: &lopdf::xobject::PdfImage<'_>) -> PageImage {
    let channels = match img.color_space.as_deref() {
        Some("DeviceGray") | Some("CalGray") => 1u8,
        Some("DeviceRGB") | Some("CalRGB") => 3,
        other => {
            return PageImage::Unsupported(format!("colour space {}", other.unwrap_or("unnamed")))
        }
    };
    if img.bits_per_component != Some(8) {
        return PageImage::Unsupported(format!(
            "{} bits per component",
            img.bits_per_component.unwrap_or(0)
        ));
    }
    let data = match doc.get_object(img.id) {
        Ok(Object::Stream(s)) => s
            .decompressed_content()
            .unwrap_or_else(|_| s.content.clone()),
        _ => return PageImage::Unsupported("unreadable image stream".into()),
    };
    let (width, height) = (img.width.max(0) as u32, img.height.max(0) as u32);
    if data.len() < width as usize * height as usize * channels as usize {
        return PageImage::Unsupported("image stream shorter than its size says".into());
    }
    PageImage::Pixels {
        width,
        height,
        channels,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scanned_page_gives_up_its_jpeg() {
        let pages = page_images(include_bytes!("../fixtures/pdf/scanned.pdf"), 10).unwrap();
        assert_eq!(pages.len(), 1);
        let (number, PageImage::Encoded(jpeg)) = &pages[0] else {
            panic!("{:?}", pages[0].1);
        };
        assert_eq!(*number, 1);
        assert!(jpeg.starts_with(&[0xff, 0xd8]), "a JPEG file");
    }
}

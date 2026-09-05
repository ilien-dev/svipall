//! PDF text extraction.
//!
//! `web_fetch` on a PDF used to run the bytes through `r.text()` and hand back mojibake. Plenty of
//! documentation, specifications and papers are only published as PDF, so that was a whole class of
//! page the tool simply could not read.
//!
//! `pdf-extract` is the choice among the pure-Rust options because it resolves CMap and ToUnicode
//! tables and Type1/TrueType encodings. That is what separates readable text from noise on any file
//! using subset fonts, which is most of them. `lopdf` alone returns the raw bytes of glyph indices;
//! `pdf_oxide` is young and its API is still moving.
//!
//! It is also known to panic on malformed input, so every call is wrapped — see `extract`.

use std::fmt::Write as _;

#[derive(Debug, Clone)]
pub struct PdfLimits {
    pub max_bytes: usize,
    pub max_pages: usize,
}

impl Default for PdfLimits {
    fn default() -> Self {
        Self {
            max_bytes: 50 * 1024 * 1024,
            max_pages: 100,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct PdfDoc {
    pub text: String,
    pub pages: usize,
    pub truncated: bool,
}

pub fn looks_like_pdf(bytes: &[u8], content_type: &str) -> bool {
    content_type.contains("application/pdf") || bytes.starts_with(b"%PDF-")
}

/// Extract text, refusing to crash on a bad file.
///
/// The `catch_unwind` is not defensive dressing: the underlying parser panics on some real-world
/// documents, and a panic here would take down whichever task is holding the fetch.
pub fn extract(bytes: &[u8], limits: &PdfLimits) -> anyhow::Result<PdfDoc> {
    if bytes.len() > limits.max_bytes {
        anyhow::bail!(
            "PDF is {} bytes, over the {} byte limit",
            bytes.len(),
            limits.max_bytes
        );
    }
    if !bytes.starts_with(b"%PDF-") {
        anyhow::bail!("not a PDF: missing the %PDF- header");
    }
    #[cfg(feature = "pdf")]
    {
        let owned = bytes.to_vec();
        let raw = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            pdf_extract::extract_text_from_mem(&owned)
        }))
        .map_err(|_| anyhow::anyhow!("the PDF parser could not read this file"))?
        .map_err(|e| anyhow::anyhow!("PDF extraction failed: {e}"))?;
        Ok(finish(&raw, limits))
    }
    #[cfg(not(feature = "pdf"))]
    {
        let _ = limits;
        anyhow::bail!("PDF support is not compiled in (build with --features pdf)")
    }
}

/// Tidy up what the extractor produces: rejoin hyphenated line breaks and drop the header and
/// footer that repeat on every page, which are pure noise once the text is out of its layout.
fn finish(raw: &str, limits: &PdfLimits) -> PdfDoc {
    let pages: Vec<&str> = raw.split('\u{c}').collect();
    let truncated = pages.len() > limits.max_pages;
    let kept = &pages[..pages.len().min(limits.max_pages)];

    // A line present on most pages is furniture, not content.
    let mut freq: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    if kept.len() >= 3 {
        for p in kept {
            let mut once = std::collections::HashSet::new();
            for l in p.lines().map(str::trim).filter(|l| !l.is_empty()) {
                if once.insert(l) {
                    *freq.entry(l).or_insert(0) += 1;
                }
            }
        }
    }
    let floor = ((kept.len() as f32) * 0.6).ceil() as usize;

    let mut out = String::new();
    for (i, page) in kept.iter().enumerate() {
        if i > 0 && kept.len() > 1 {
            let _ = write!(out, "\n\n*page {}*\n\n", i + 1);
        }
        let mut pending: Option<String> = None;
        for line in page.lines() {
            let line = line.trim_end();
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Page numbers and running heads.
            let furniture = kept.len() >= 3
                && freq.get(trimmed).copied().unwrap_or(0) >= floor
                && trimmed.len() < 120;
            if furniture {
                // A pending stem must still be written, or dropping the line that continued it
                // would silently swallow the half-word before it.
                if let Some(prev) = pending.take() {
                    out.push_str(&prev);
                    out.push('\n');
                }
                continue;
            }
            // A word split across a line break: `inter-` + `esting` is one word.
            let line_text = match pending.take() {
                Some(prev) => format!("{prev}{trimmed}"),
                None => trimmed.to_string(),
            };
            if let Some(stem) = line_text.strip_suffix('-') {
                pending = Some(stem.to_string());
            } else {
                out.push_str(&line_text);
                out.push('\n');
            }
        }
        // Anything still pending at the end of a page belongs to the output too.
        if let Some(prev) = pending.take() {
            out.push_str(&prev);
            out.push('\n');
        }
    }
    PdfDoc {
        text: out.trim().to_string(),
        pages: kept.len(),
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn something_that_is_not_a_pdf_is_refused() {
        let e = extract(b"<html>not a pdf</html>", &PdfLimits::default()).unwrap_err();
        assert!(e.to_string().contains("not a PDF"), "{e}");
    }

    #[test]
    fn an_oversized_file_is_refused_before_parsing() {
        let limits = PdfLimits {
            max_bytes: 10,
            ..Default::default()
        };
        let e = extract(b"%PDF-1.7 and then some more bytes", &limits).unwrap_err();
        assert!(e.to_string().contains("over the"), "{e}");
    }

    #[test]
    fn a_truncated_pdf_is_an_error_not_a_crash() {
        // Enough of a header to get past the sniff, then nothing usable.
        let bytes = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n";
        let result = extract(bytes, &PdfLimits::default());
        assert!(result.is_err(), "a truncated PDF should error, not panic");
    }

    #[test]
    fn content_type_or_magic_bytes_both_identify_a_pdf() {
        assert!(looks_like_pdf(b"%PDF-1.4 ...", "application/octet-stream"));
        assert!(looks_like_pdf(
            b"anything",
            "application/pdf; charset=binary"
        ));
        assert!(!looks_like_pdf(b"<html>", "text/html"));
    }

    #[test]
    fn repeated_running_heads_are_dropped_and_hyphens_rejoined() {
        // Four pages sharing a header and a footer, with one unique line each.
        let raw = (1..=4)
            .map(|i| {
                format!(
                    "ACME Corporation Report\nUnique body line {i} about inter-\nesting things {i}.\nPage footer text here"
                )
            })
            .collect::<Vec<_>>()
            .join("\u{c}");
        let doc = finish(&raw, &PdfLimits::default());
        assert_eq!(doc.pages, 4);
        assert!(
            !doc.text.contains("ACME Corporation Report"),
            "a header on every page should be dropped: {}",
            doc.text
        );
        assert!(
            !doc.text.contains("Page footer text here"),
            "a footer on every page should be dropped: {}",
            doc.text
        );
        assert!(doc.text.contains("Unique body line 1"), "{}", doc.text);
        assert!(
            doc.text.contains("interesting things 1."),
            "a hyphenated line break should rejoin: {}",
            doc.text
        );
    }

    #[test]
    fn the_page_limit_is_reported() {
        let raw = (0..10)
            .map(|i| format!("page {i}"))
            .collect::<Vec<_>>()
            .join("\u{c}");
        let doc = finish(
            &raw,
            &PdfLimits {
                max_pages: 3,
                ..Default::default()
            },
        );
        assert_eq!(doc.pages, 3);
        assert!(doc.truncated);
    }

    #[test]
    fn a_single_page_gets_no_page_markers() {
        let doc = finish("just one page of text", &PdfLimits::default());
        assert!(!doc.text.contains("*page"), "{}", doc.text);
        assert_eq!(doc.pages, 1);
    }
}

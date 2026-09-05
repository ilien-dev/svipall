//! Office documents, OpenDocument, RTF, EPUB and CSV as markdown, the way PDFs already are.
//!
//! An agent that can read a page and not the report linked from it has half a window. The
//! conversion is a pure-Rust library that never makes a network call; it sits behind the `docs`
//! feature (on by default) the way PDF sits behind `pdf`, so a build without it still says why.
//!
//! Same discipline as `pdf.rs`: a size ceiling before anything is parsed, `catch_unwind` around
//! the parser because a malformed file must not take the server with it, and a failure that
//! becomes content rather than an error — the caller asked for the document and gets told what
//! happened to it.

/// What a document turned out to be, by the name the page or the file gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Docx,
    Xlsx,
    Pptx,
    Odt,
    Ods,
    Odp,
    Epub,
    Rtf,
    Doc,
    Csv,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Docx => "docx",
            Kind::Xlsx => "xlsx",
            Kind::Pptx => "pptx",
            Kind::Odt => "odt",
            Kind::Ods => "ods",
            Kind::Odp => "odp",
            Kind::Epub => "epub",
            Kind::Rtf => "rtf",
            Kind::Doc => "doc",
            Kind::Csv => "csv",
        }
    }

    fn from_extension(ext: &str) -> Option<Self> {
        Some(match ext {
            "docx" | "docm" | "dotx" => Kind::Docx,
            "xlsx" | "xlsm" | "xltx" => Kind::Xlsx,
            "pptx" | "pptm" | "potx" => Kind::Pptx,
            "odt" => Kind::Odt,
            "ods" => Kind::Ods,
            "odp" => Kind::Odp,
            "epub" => Kind::Epub,
            "rtf" => Kind::Rtf,
            "doc" | "dot" => Kind::Doc,
            "csv" | "tsv" => Kind::Csv,
            _ => return None,
        })
    }

    fn from_content_type(ct: &str) -> Option<Self> {
        let ct = ct.split(';').next().unwrap_or("").trim();
        Some(match ct {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Kind::Docx,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Kind::Xlsx,
            "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
                Kind::Pptx
            }
            "application/vnd.oasis.opendocument.text" => Kind::Odt,
            "application/vnd.oasis.opendocument.spreadsheet" => Kind::Ods,
            "application/vnd.oasis.opendocument.presentation" => Kind::Odp,
            "application/epub+zip" => Kind::Epub,
            "application/rtf" | "text/rtf" => Kind::Rtf,
            "application/msword" => Kind::Doc,
            "text/csv" | "text/tab-separated-values" => Kind::Csv,
            _ => return None,
        })
    }
}

fn extension_of(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.')?.1;
    Some(ext.to_ascii_lowercase())
}

/// Is this body a document this module reads? Decided from the content type, the bytes, and
/// the URL's extension, in that order of trust. A zip container with no name is not guessed at:
/// a `.zip` is a `.zip`.
pub fn looks_like_document(bytes: &[u8], content_type: &str, url: &str) -> Option<Kind> {
    if let Some(k) = Kind::from_content_type(content_type) {
        return Some(k);
    }
    let by_name = extension_of(url).and_then(|e| Kind::from_extension(&e));
    if bytes.starts_with(b"{\\rtf") {
        return Some(Kind::Rtf);
    }
    // OLE compound file: the legacy Word format, and nothing else this tool is asked to read.
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return Some(by_name.unwrap_or(Kind::Doc));
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return by_name.filter(|k| {
            matches!(
                k,
                Kind::Docx
                    | Kind::Xlsx
                    | Kind::Pptx
                    | Kind::Odt
                    | Kind::Ods
                    | Kind::Odp
                    | Kind::Epub
            )
        });
    }
    by_name.filter(|k| matches!(k, Kind::Csv | Kind::Rtf))
}

#[derive(Debug, Clone)]
pub struct DocLimits {
    pub max_bytes: usize,
}

impl Default for DocLimits {
    fn default() -> Self {
        Self {
            max_bytes: 50 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Doc {
    pub markdown: String,
    pub kind: Kind,
}

/// Convert, within limits. The parser runs under `catch_unwind`; the workspace deliberately does
/// not build with `panic = "abort"` so that this can hold.
pub fn extract(bytes: &[u8], kind: Kind, limits: &DocLimits) -> anyhow::Result<Doc> {
    if bytes.len() > limits.max_bytes {
        anyhow::bail!(
            "document is {} bytes, over the {} byte limit",
            bytes.len(),
            limits.max_bytes
        );
    }
    if bytes.is_empty() {
        anyhow::bail!("document is empty");
    }
    convert(bytes, kind)
}

#[cfg(feature = "docs")]
fn convert(bytes: &[u8], kind: Kind) -> anyhow::Result<Doc> {
    let format = match kind {
        Kind::Csv => Some(anydoc::Format::Csv),
        // Everything else carries a signature the converter reads itself.
        _ => None,
    };
    let owned = bytes.to_vec();
    let markdown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        anydoc::to_markdown_bytes(&owned, format)
    }))
    .map_err(|_| anyhow::anyhow!("the {} parser could not read this file", kind.name()))?
    .map_err(|e| anyhow::anyhow!("{} conversion failed: {e}", kind.name()))?;
    Ok(Doc { markdown, kind })
}

#[cfg(not(feature = "docs"))]
fn convert(_bytes: &[u8], kind: Kind) -> anyhow::Result<Doc> {
    anyhow::bail!(
        "{} documents need the `docs` feature (build without --no-default-features)",
        kind.name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_is_recognised_by_type_then_bytes_then_name() {
        let ct = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
        assert_eq!(
            looks_like_document(b"PK\x03\x04", ct, "https://x/y"),
            Some(Kind::Docx)
        );
        assert_eq!(
            looks_like_document(
                b"PK\x03\x04",
                "application/octet-stream",
                "https://x/report.XLSX?dl=1"
            ),
            Some(Kind::Xlsx)
        );
        assert_eq!(
            looks_like_document(b"{\\rtf1", "text/plain", "https://x/a"),
            Some(Kind::Rtf)
        );
        assert_eq!(
            looks_like_document(b"a,b\n1,2", "text/csv", "https://x/a"),
            Some(Kind::Csv)
        );
        assert_eq!(
            looks_like_document(b"a,b\n1,2", "text/plain", "https://x/rows.csv"),
            Some(Kind::Csv)
        );
        assert_eq!(
            looks_like_document(b"PK\x03\x04", "application/zip", "https://x/bundle.zip"),
            None,
            "a zip is a zip"
        );
        assert_eq!(
            looks_like_document(b"<html>", "text/html", "https://x/page.docx"),
            None,
            "markup named like a document is markup"
        );
    }

    #[cfg(feature = "docs")]
    #[test]
    fn rtf_and_csv_become_markdown() {
        let rtf = b"{\\rtf1\\ansi Hello {\\b bold} world\\par}";
        let doc = extract(rtf, Kind::Rtf, &DocLimits::default()).unwrap();
        assert!(doc.markdown.contains("Hello"), "{}", doc.markdown);
        assert!(doc.markdown.contains("**bold**"), "{}", doc.markdown);
        let csv = b"name,price\nCup,3\nPot,9\n";
        let doc = extract(csv, Kind::Csv, &DocLimits::default()).unwrap();
        assert!(doc.markdown.contains("| Cup | 3 |"), "{}", doc.markdown);
    }

    #[cfg(feature = "docs")]
    #[test]
    fn a_document_that_is_not_one_fails_without_taking_the_process_down() {
        let junk = b"PK\x03\x04\x00garbage that is not a package at all";
        let r = extract(junk, Kind::Docx, &DocLimits::default());
        assert!(r.is_err());
    }

    #[test]
    fn an_oversized_document_is_refused_before_it_is_parsed() {
        let big = vec![b'a'; 10];
        let r = extract(&big, Kind::Csv, &DocLimits { max_bytes: 5 });
        assert!(r.unwrap_err().to_string().contains("over the"));
        assert!(extract(&[], Kind::Csv, &DocLimits::default()).is_err());
    }
}

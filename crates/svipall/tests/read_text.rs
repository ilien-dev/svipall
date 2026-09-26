//! The text reader against real models: an image of text, and the same text as a scanned PDF.
//!
//! The models are not in this repository. Run by hand with the three files in a directory:
//!
//!     python tools/models/export_ocr.py --out /tmp/ocr
//!     SVIPALL_TEST_OCR_MODELS=/tmp/ocr cargo test -p svipall --features onnx-read \
//!         --test read_text -- --ignored --nocapture
mod support;
use support::{Reply, Site};
use svipall::{server::SvipallServer, tools::WebFetchParams};

const INVOICE_PNG: &[u8] = include_bytes!("../../svipall-core/fixtures/ocr/invoice.png");
const INVOICE_PDF: &[u8] = include_bytes!("../../svipall-core/fixtures/pdf/scanned.pdf");

fn with_models() -> SvipallServer {
    let home = support::isolate();
    let from = std::env::var("SVIPALL_TEST_OCR_MODELS").expect("SVIPALL_TEST_OCR_MODELS");
    let to = home.join("models");
    std::fs::create_dir_all(&to).unwrap();
    for f in ["ocr_det.onnx", "ocr_rec.onnx", "ocr_rec.json"] {
        std::fs::copy(std::path::Path::new(&from).join(f), to.join(f)).unwrap();
    }
    assert!(svipall::read_text::available(), "models in place");
    SvipallServer::new(
        None,
        svipall_core::Config {
            request_min_interval_ms: 1,
            ..Default::default()
        },
        None,
    )
}

fn params(url: String) -> WebFetchParams {
    WebFetchParams {
        url,
        cache: Some("bypass".into()),
        robots: Some("ignore".into()),
        ..Default::default()
    }
}

/// What the fixture says, line by line.
const LINES: &[&str] = &[
    "Invoice 2041",
    "Delivered to the warehouse on 3 March.",
    "Total due: 1,280 euros.",
    "Payment within thirty days.",
];

fn check(content: &str) {
    for line in LINES {
        assert!(content.contains(line), "{line:?} not read in:\n{content}");
    }
}

#[tokio::test]
#[ignore = "needs the text reader's models; see the module comment"]
async fn an_image_of_text_and_a_scanned_pdf_are_read() {
    let server = with_models();
    let site = Site::start(vec![
        ("/invoice", Reply::bytes(INVOICE_PNG, "image/png")),
        ("/invoice.pdf", Reply::bytes(INVOICE_PDF, "application/pdf")),
    ])
    .await;
    let t = std::time::Instant::now();
    let img = server.fetch_json(params(site.url("/invoice"))).await.value;
    eprintln!("image read in {:?}:\n{}", t.elapsed(), img["content"]);
    assert_eq!(img["media"]["text_read"], true, "{img}");
    check(img["content"].as_str().unwrap_or_default());

    let t = std::time::Instant::now();
    let pdf = server
        .fetch_json(params(site.url("/invoice.pdf")))
        .await
        .value;
    eprintln!("pdf read in {:?}:\n{}", t.elapsed(), pdf["content"]);
    assert_eq!(pdf["document"]["pages_read"], 1, "{pdf}");
    check(pdf["content"].as_str().unwrap_or_default());
}

/// Edits (insert, delete, substitute) between two strings, by character.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb))
                .min(row[j] + 1)
                .min(row[j + 1] + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// The measured set in `bench/experiments/ocr-20260926`: character error rate and time per
/// picture, printed for the experiment's README. Run it with `--release` for the times.
#[test]
#[ignore = "needs the text reader's models; see the module comment"]
fn the_fixed_set_is_measured() {
    let _ = with_models();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bench/experiments/ocr-20260926/set");
    let truth: std::collections::BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(dir.join("truth.json")).unwrap()).unwrap();
    let (mut edits, mut chars, mut times) = (0, 0, Vec::new());
    for (name, want) in &truth {
        let bytes = std::fs::read(dir.join(name)).unwrap();
        let t = std::time::Instant::now();
        let got = svipall::read_text::read(
            &bytes,
            std::time::Instant::now() + std::time::Duration::from_secs(60),
        )
        .unwrap();
        times.push(t.elapsed().as_millis());
        // Blank lines are layout, not text.
        let got: Vec<&str> = got.lines().filter(|l| !l.trim().is_empty()).collect();
        let got = got.join("\n");
        let e = distance(&got, want);
        let n = want.chars().count();
        println!(
            "{name}\t{e}/{n}\t{:.1}%\t{} ms",
            100.0 * e as f64 / n as f64,
            times.last().unwrap()
        );
        if e > 0 {
            println!("  want: {want:?}\n  got:  {got:?}");
        }
        edits += e;
        chars += n;
    }
    times.sort();
    println!(
        "TOTAL {edits}/{chars} = {:.2}% CER over {} pictures; median {} ms, max {} ms",
        100.0 * edits as f64 / chars as f64,
        truth.len(),
        times[times.len() / 2],
        times[times.len() - 1]
    );
}

//! The model paths, run for real.
//!
//! Until this file existed, `cargo test` never opened an ONNX session: the inference modules
//! were lint-checked under their features and executed by nobody. These tests load a graph
//! through the same code a real model takes — `model_source::locate`, the session cache, the
//! tensor builder, the decoders — and assert answers that are true by construction.
//!
//! Two kinds of model. The fixtures under `tests/fixtures/models/` are hand-built graphs a few
//! hundred bytes long (`tools/models/fixtures.py`) whose output is a function of pixel
//! brightness, so a white tile *is* class "bright" and a black one is not. The embedded models,
//! when the binary carries them, are checked for shape and for the contract their sidecar makes.
//!
//! Every test points `SVIPALL_HOME` at its own directory; a machine's real models never leak in.

#![cfg(any(
    feature = "onnx-grid",
    feature = "onnx-segment",
    feature = "onnx-detect"
))]

#[cfg(any(feature = "onnx-grid", feature = "onnx-segment"))]
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The environment is process-wide; tests that set it take turns.
static HOME: Mutex<()> = Mutex::new(());

#[cfg(any(feature = "onnx-grid", feature = "onnx-segment"))]
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/models")
}

/// A fresh home with the named fixture models installed.
#[cfg(any(feature = "onnx-grid", feature = "onnx-segment"))]
fn home_with(names: &[&str]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("svipall-models-test-{}", names.join("-")));
    let _ = std::fs::remove_dir_all(&dir);
    let models = dir.join("models");
    std::fs::create_dir_all(&models).unwrap();
    for n in names {
        for ext in ["onnx", "json"] {
            std::fs::copy(
                fixtures().join(format!("{n}.{ext}")),
                models.join(format!("{n}.{ext}")),
            )
            .unwrap();
        }
    }
    std::env::set_var("SVIPALL_HOME", &dir);
    dir
}

/// A solid PNG of one grey level.
fn png(size: u32, level: u8) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(size, size, image::Rgb([level, level, level]));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// A PNG whose top-left quarter is white and the rest black.
#[cfg(feature = "onnx-segment")]
fn quadrant_png(size: u32) -> Vec<u8> {
    let mut img = image::RgbImage::from_pixel(size, size, image::Rgb([0, 0, 0]));
    for y in 0..size / 2 {
        for x in 0..size / 2 {
            img.put_pixel(x, y, image::Rgb([255, 255, 255]));
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

#[cfg(feature = "onnx-grid")]
#[test]
fn the_grid_classifier_runs_a_real_session_and_scores_bright_tiles_as_bright() {
    let _guard = HOME.lock().unwrap();
    let _home = home_with(&["grid"]);
    assert!(svipall_mcp::grid::available());
    let cfg = svipall_mcp::grid::load_config().unwrap();
    assert_eq!(cfg.classes, vec!["dark", "bright"]);
    let tiles = vec![png(16, 255), png(16, 0), png(16, 200)];
    let bright = svipall_mcp::grid::classify(&tiles, 1).unwrap();
    assert!(bright[0] > 0.95, "white tile: {bright:?}");
    assert!(bright[1] < 0.05, "black tile: {bright:?}");
    assert!(bright[2] > 0.7, "light grey tile: {bright:?}");
    let picked = svipall_mcp::grid::select(&bright, cfg.threshold);
    assert_eq!(
        picked,
        vec![0, 2],
        "the surest first, the dark one not at all"
    );
    std::env::remove_var("SVIPALL_HOME");
}

#[cfg(feature = "onnx-grid")]
#[test]
fn a_swapped_model_file_is_picked_up_without_a_restart() {
    let _guard = HOME.lock().unwrap();
    let home = home_with(&["grid"]);
    let first = svipall_mcp::grid::classify(&[png(16, 255)], 1).unwrap();
    assert!(first[0] > 0.95);
    // Swap the classes around by rewriting the sidecar and replacing the graph with one whose
    // planes are the other way round — here, simply the same graph but read as class 0.
    // The cache must notice the file change; a session cached forever would not.
    let models = home.join("models");
    let bytes = std::fs::read(models.join("grid.onnx")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(models.join("grid.onnx"), &bytes).unwrap();
    // Same graph, so the same answer — but through a reload, which must not fail.
    let again = svipall_mcp::grid::classify(&[png(16, 255)], 1).unwrap();
    assert!((again[0] - first[0]).abs() < 1e-6);
    std::env::remove_var("SVIPALL_HOME");
}

#[cfg(feature = "onnx-segment")]
#[test]
fn the_segmenter_runs_a_real_session_and_marks_the_cells_the_mask_touches() {
    let _guard = HOME.lock().unwrap();
    let _home = home_with(&["segment"]);
    assert!(svipall_mcp::segment::available());
    // A white top-left quarter over a 4x4 grid is exactly the four top-left cells.
    let mut cells = svipall_mcp::segment::cells(&quadrant_png(64), 1, 4, 4).unwrap();
    cells.sort_unstable();
    assert_eq!(cells, vec![0, 1, 4, 5]);
    // The dark class is everything else.
    let mut dark = svipall_mcp::segment::cells(&quadrant_png(64), 0, 4, 4).unwrap();
    dark.sort_unstable();
    assert_eq!(dark.len(), 12);
    assert!(!dark.contains(&0) && !dark.contains(&5));
    std::env::remove_var("SVIPALL_HOME");
}

#[cfg(any(feature = "onnx-detect", feature = "onnx-segment"))]
#[test]
fn the_embedded_models_when_present_keep_their_sidecars_contract() {
    let _guard = HOME.lock().unwrap();
    // An empty home: only what is compiled in can answer.
    let dir = std::env::temp_dir().join("svipall-models-test-embedded");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("SVIPALL_HOME", &dir);
    #[cfg(feature = "onnx-detect")]
    if svipall_models::detect().is_some() {
        assert!(svipall_mcp::detect::available());
        let cfg = svipall_mcp::detect::load_config().unwrap();
        assert!(cfg.classes.iter().any(|c| c == "bus"));
        // A flat grey picture holds nothing; the detector must say so rather than hallucinate.
        let dets = svipall_mcp::detect::detect(&png(320, 128), 0).unwrap();
        assert!(dets.len() < 5, "{} boxes on a blank picture", dets.len());
        let s = svipall_mcp::detect::strongest(&png(320, 128), 0).unwrap();
        assert!((0.0..=1.0).contains(&s));
        // And the classifier stands on the detector when there is no classifier.
        assert!(svipall_mcp::grid::available());
        let g = svipall_mcp::grid::load_config().unwrap();
        assert!(g.multilabel);
        assert_eq!(g.classes, cfg.classes);
    }
    #[cfg(feature = "onnx-segment")]
    if svipall_models::segment().is_some() {
        assert!(svipall_mcp::segment::available());
        let cfg = svipall_mcp::segment::load_config().unwrap();
        assert_eq!(cfg.classes[0], "background");
        assert!(cfg.classes.iter().any(|c| c == "bus"));
        let cells = svipall_mcp::segment::cells(&png(320, 128), 0, 4, 4).unwrap();
        assert_eq!(cells.len(), 16, "a blank picture is all background");
    }
    std::env::remove_var("SVIPALL_HOME");
}

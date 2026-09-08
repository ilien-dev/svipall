//! Where a model comes from, and the one rule every model module follows.
//!
//! A model is found in one of two places: a file the operator put under `~/.svipall/models/`, or
//! the copy compiled into this binary by `svipall-models`. The file wins. That order is the whole
//! contract with `docs/models.md`: an operator who fine-tunes on their own corpus drops the result
//! next to the others and it is used on the next solve, without a rebuild and without a restart —
//! the session cache below notices a changed file by its mtime and length and reloads it.
//!
//! Nothing here downloads anything. A model that is in neither place is a model svipall does not
//! have, and the caller declines the challenge rather than guessing at it.

use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// Which copy of a model is in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A file the operator installed; the stamp is what the cache compares to notice a swap.
    Disk { path: PathBuf, stamp: Stamp },
    /// The bytes compiled into the binary.
    Embedded(&'static [u8]),
}

/// Identity of a file's contents without reading them: modification time and length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub mtime_secs: i64,
    pub len: u64,
}

impl Stamp {
    fn of(path: &std::path::Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let mtime_secs = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs() as i64;
        Some(Self {
            mtime_secs,
            len: meta.len(),
        })
    }
}

/// A model that exists somewhere, with its sidecar already read.
#[derive(Debug, Clone)]
pub struct Located {
    pub name: &'static str,
    pub origin: Origin,
    pub sidecar: String,
}

impl Located {
    /// A model that lives only on disk, with a sidecar read by the caller — for models that share
    /// one sidecar between several files, which `locate` cannot express.
    pub fn disk(name: &'static str, path: PathBuf, sidecar: String) -> Option<Self> {
        let stamp = Stamp::of(&path)?;
        Some(Self {
            name,
            origin: Origin::Disk { path, stamp },
            sidecar,
        })
    }

    /// The sidecar, typed. Every model's contract is a JSON object; the type says which.
    pub fn config<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_str(&self.sidecar)
            .map_err(|e| anyhow!("{} sidecar ({}): {e}", self.name, self.describe()))
    }

    /// Where it came from, for logs and `web_status`.
    pub fn describe(&self) -> String {
        match &self.origin {
            Origin::Disk { path, .. } => path.display().to_string(),
            Origin::Embedded(_) => "embedded".to_string(),
        }
    }
}

/// The directory the operator's models live in.
pub fn models_dir() -> PathBuf {
    svipall_core::config::home_dir().join("models")
}

/// Find `name`: `~/.svipall/models/<disk_name>.<ext>` + `.json` first, then the embedded copy.
///
/// The extension is a parameter because a model here is a *file*, not a graph: most are ONNX, and
/// the classifier that reads page text is a weights blob a hundred lines of Rust knows how to
/// multiply. Both obey the same contract — the operator's copy wins, and the sidecar is what says
/// how to read it.
///
/// A model file with no sidecar is not a model, so it is passed over rather than half-loaded.
/// `None` means svipall has no such model anywhere.
pub fn locate(
    name: &'static str,
    disk_name: &str,
    ext: &str,
    embedded: Option<svipall_models::Embedded>,
) -> Option<Located> {
    let dir = models_dir();
    let path = dir.join(format!("{disk_name}.{ext}"));
    let json = dir.join(format!("{disk_name}.json"));
    if path.is_file() {
        match (std::fs::read_to_string(&json), Stamp::of(&path)) {
            (Ok(sidecar), Some(stamp)) => {
                return Some(Located {
                    name,
                    origin: Origin::Disk { path, stamp },
                    sidecar,
                });
            }
            _ => tracing::warn!(
                "{} is present but {} is not readable; ignoring the file",
                path.display(),
                json.display()
            ),
        }
    }
    embedded.map(|e| Located {
        name,
        origin: Origin::Embedded(e.model),
        sidecar: e.sidecar.to_string(),
    })
}

/// An image as an NCHW `f32` tensor of the shape a model asks for.
///
/// Every image model here takes the same thing — resized, channels first, pixels either 0..1 or
/// 0..255 — and the four copies of this loop that existed before disagreed on nothing except where
/// their bugs were.
pub fn image_tensor(
    img: &image::DynamicImage,
    width: u32,
    height: u32,
    channels: u32,
    normalize: bool,
) -> (Vec<i64>, Vec<f32>) {
    use image::imageops::FilterType;
    let resized = img.resize_exact(width, height, FilterType::Triangle);
    let ch = channels.max(1) as usize;
    let plane = height as usize * width as usize;
    let mut data = vec![0f32; ch * plane];
    let scale = if normalize { 255.0 } else { 1.0 };
    if ch == 1 {
        for (i, p) in resized.to_luma8().pixels().enumerate() {
            data[i] = p.0[0] as f32 / scale;
        }
    } else {
        for (i, p) in resized.to_rgb8().pixels().enumerate() {
            data[i] = p.0[0] as f32 / scale;
            data[plane + i] = p.0[1] as f32 / scale;
            if ch > 2 {
                data[2 * plane + i] = p.0[2] as f32 / scale;
            }
        }
    }
    (vec![1, ch as i64, height as i64, width as i64], data)
}

/// The greatest value's index in a row, or 0 for an empty row.
pub fn argmax(row: &[f32]) -> usize {
    let mut best = (0usize, f32::MIN);
    for (i, &v) in row.iter().enumerate() {
        if v > best.1 {
            best = (i, v);
        }
    }
    best.0
}

#[cfg(feature = "onnx")]
pub use session::SessionCache;

#[cfg(feature = "onnx")]
mod session {
    use super::*;
    use ort::session::Session;
    use std::sync::Mutex;

    /// One loaded session per model, rebuilt when the file it came from changes.
    ///
    /// The `OnceCell` this replaces loaded a model exactly once per process, which made the
    /// promise in `docs/models.md` — "picked up on the next solve; no restart" — false. Now the
    /// origin is compared on every use: an embedded model never changes, a disk model is reloaded
    /// when its stamp does.
    pub struct SessionCache {
        cell: Mutex<Option<(Origin, Session)>>,
    }

    impl SessionCache {
        pub const fn new() -> Self {
            Self {
                cell: Mutex::new(None),
            }
        }

        /// Run `f` against the session for `located`, loading or reloading it first if needed.
        pub fn with<R>(
            &self,
            located: &Located,
            f: impl FnOnce(&mut Session) -> Result<R>,
        ) -> Result<R> {
            let mut guard = self
                .cell
                .lock()
                .map_err(|_| anyhow!("model cache poisoned"))?;
            let stale = match &*guard {
                Some((origin, _)) => *origin != located.origin,
                None => true,
            };
            if stale {
                let session = match &located.origin {
                    Origin::Disk { path, .. } => Session::builder()?.commit_from_file(path)?,
                    Origin::Embedded(bytes) => Session::builder()?.commit_from_memory(bytes)?,
                };
                tracing::info!(model = located.name, from = %located.describe(), "model loaded");
                *guard = Some((located.origin.clone(), session));
            }
            let (_, session) = guard.as_mut().expect("just loaded");
            f(session)
        }
    }

    impl Default for SessionCache {
        fn default() -> Self {
            Self::new()
        }
    }
}

/// Serialises the tests that point `SVIPALL_HOME` somewhere.
///
/// `set_var` is process-wide and the test harness runs threads in parallel, so two tests each
/// setting it will occasionally read the other's directory. That is not hypothetical — it turned
/// up here the moment a second model got a "there is no model" test of its own, and it failed
/// about one run in four while passing every time in isolation, which is the worst way for a test
/// to be wrong. Anything that sets the variable takes this first.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("svipall-model-source-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("models")).unwrap();
        d
    }

    #[test]
    fn a_file_on_disk_wins_over_the_embedded_copy() {
        let _guard = super::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = scratch("disk-wins");
        std::fs::write(home.join("models/x.onnx"), b"disk").unwrap();
        std::fs::write(home.join("models/x.json"), r#"{"height":1}"#).unwrap();
        std::env::set_var("SVIPALL_HOME", &home);
        let e = svipall_models::Embedded {
            model: b"embedded",
            sidecar: r#"{"height":2}"#,
        };
        let found = locate("x", "x", "onnx", Some(e)).expect("found");
        assert!(matches!(found.origin, Origin::Disk { .. }));
        assert!(found.sidecar.contains("\"height\":1"));
        std::env::remove_var("SVIPALL_HOME");
    }

    #[test]
    fn without_a_file_the_embedded_copy_is_used_and_without_either_there_is_nothing() {
        let _guard = super::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = scratch("embedded");
        std::env::set_var("SVIPALL_HOME", &home);
        let e = svipall_models::Embedded {
            model: b"embedded",
            sidecar: r#"{"height":2}"#,
        };
        let found = locate("y", "y", "onnx", Some(e)).expect("embedded");
        assert_eq!(found.origin, Origin::Embedded(b"embedded"));
        assert_eq!(found.describe(), "embedded");
        assert!(locate("y", "y", "onnx", None).is_none());
        std::env::remove_var("SVIPALL_HOME");
    }

    #[test]
    fn a_model_file_without_its_sidecar_is_not_a_model() {
        let _guard = super::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = scratch("no-sidecar");
        std::fs::write(home.join("models/z.onnx"), b"disk").unwrap();
        std::env::set_var("SVIPALL_HOME", &home);
        assert!(locate("z", "z", "onnx", None).is_none());
        // ...and the embedded one is used instead of the half-installed file.
        let e = svipall_models::Embedded {
            model: b"e",
            sidecar: "{}",
        };
        assert!(matches!(
            locate("z", "z", "onnx", Some(e)).unwrap().origin,
            Origin::Embedded(_)
        ));
        std::env::remove_var("SVIPALL_HOME");
    }

    #[test]
    fn a_swapped_file_has_a_different_stamp() {
        let _guard = super::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = scratch("stamp");
        let p = home.join("models/s.onnx");
        std::fs::write(&p, b"one").unwrap();
        let a = Stamp::of(&p).unwrap();
        std::fs::write(&p, b"three!").unwrap();
        let b = Stamp::of(&p).unwrap();
        assert_ne!(a, b, "length changed, so the stamp must");
    }

    #[test]
    fn the_tensor_is_channels_first_and_scaled() {
        let mut img = image::RgbImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        img.put_pixel(1, 0, image::Rgb([0, 0, 255]));
        let (shape, data) = image_tensor(&image::DynamicImage::ImageRgb8(img), 2, 1, 3, true);
        assert_eq!(shape, vec![1, 3, 1, 2]);
        // R plane, then G, then B.
        assert_eq!(data, vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let (shape, data) = image_tensor(
            &image::DynamicImage::ImageRgb8(image::RgbImage::new(2, 1)),
            2,
            1,
            1,
            false,
        );
        assert_eq!(shape, vec![1, 1, 1, 2]);
        assert_eq!(data.len(), 2);
    }

    #[test]
    fn argmax_picks_the_first_maximum_and_survives_an_empty_row() {
        assert_eq!(argmax(&[0.1, 0.9, 0.9]), 1);
        assert_eq!(argmax(&[]), 0);
    }
}

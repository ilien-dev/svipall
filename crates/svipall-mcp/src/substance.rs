//! Finding and running the page-substance classifier.
//!
//! The model itself — the format, the arithmetic, the training — lives in
//! `svipall_core::quality::substance`, because it is pure and belongs where it can be tested
//! without a server. This is only the part that has to know about disks: where the file is, and
//! keeping one loaded copy rather than re-reading two megabytes per page.
//!
//! There is no model embedded in the binary today, and that is the honest state: nobody has
//! trained one yet, and shipping weights fitted on nothing would be worse than shipping none. The
//! contract is the same one `docs/models.md` already states for `grid`, `ocr` and `audio` — put a
//! file in `~/.svipall/models/` and it is used; leave the directory empty and the field is simply
//! absent from the response, which is not the same as a page scoring badly.

use crate::model_source::{self, Located, Origin};
use std::sync::{Mutex, OnceLock};
use svipall_core::quality::substance::{Model, Substance};

/// Where the classifier is, if it is anywhere.
pub fn locate() -> Option<Located> {
    model_source::locate("substance", "substance", "bin", svipall_models::substance())
}

pub fn available() -> bool {
    locate().is_some()
}

/// One loaded copy, reloaded when the operator swaps the file.
///
/// The same rule the ONNX models get: identity is the path plus mtime and length, so replacing the
/// file takes effect on the next page rather than on the next restart.
static LOADED: OnceLock<Mutex<Option<(Origin, Model)>>> = OnceLock::new();

/// What the model makes of this page, or `None` when there is no model to ask.
///
/// Never an error the caller has to handle: a missing or unreadable model is a signal svipall does
/// not have, and a fetch is not worth failing over one.
pub fn assess(text: &str) -> Option<Substance> {
    let located = locate()?;
    let cell = LOADED.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().ok()?;

    let stale = match &*guard {
        Some((origin, _)) => *origin != located.origin,
        None => true,
    };
    if stale {
        let bytes = match &located.origin {
            Origin::Disk { path, .. } => std::fs::read(path).ok()?,
            Origin::Embedded(b) => b.to_vec(),
        };
        match Model::load(&bytes, &located.sidecar) {
            Ok(m) => {
                tracing::info!(from = %located.describe(), "substance model loaded");
                *guard = Some((located.origin.clone(), m));
            }
            Err(e) => {
                // Said once per swap, not once per page: a broken file is an operator problem and
                // repeating it on every fetch would bury the rest of the log.
                tracing::warn!("substance model at {} is unusable: {e}", located.describe());
                return None;
            }
        }
    }
    guard.as_ref().map(|(_, m)| m.predict(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_model_means_no_answer_rather_than_a_bad_one() {
        // The distinction the whole design rests on: "svipall cannot say" and "this page is junk"
        // must never look the same to a caller.
        let _guard = crate::model_source::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("svipall-no-substance-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("SVIPALL_HOME", &dir);
        if svipall_models::substance().is_none() {
            assert!(assess("any text at all").is_none());
            assert!(!available());
        }
    }
}

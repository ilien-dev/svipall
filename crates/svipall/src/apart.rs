//! Reading a page from where the page cannot watch.
//!
//! A script evaluated the ordinary way runs in the page's own world, and in that world the page
//! owns everything a script touches. `document.querySelector` is whatever the page last assigned
//! to it. Measured on the interstitial that reads "Performing security verification": it wraps
//! the document's lookups, and a single question about its answer field
//! (`input[name="cf-turnstile-response"]`) made from that world fails the verification. The page
//! reloads into the same challenge, the next poll asks again, and the wall never clears. A browser
//! with nothing attached passed the same page in six seconds; one polled for that field every
//! 1.2 s never passed, and the same poll made from here passed again.
//!
//! The isolated world is the same DOM seen through bindings of its own: the page's replacements
//! are not there, a property the page redefined reads as the browser's original, and nothing run
//! in it leaves a name the page can find. The protocol layer already creates one per document.
//!
//! So everything svipall asks a page on its own account goes through here. A script the *caller*
//! supplied still runs in the page's world, because the page's own variables are what it is for.

use std::time::Duration;

use anyhow::{bail, Result};
use serde_json::Value;
use svipall_cdp::cdp::js_protocol::runtime::EvaluateParams;
use svipall_cdp::page::Page;

/// Evaluate `js` in the isolated world and return its value. `Null` when it returned nothing.
///
/// Never falls back to the page's world: a read that cannot be made unseen is reported as a
/// failure, and the caller decides what a missing answer means.
pub async fn eval(page: &Page, js: &str) -> Result<Value> {
    let mut last = None;
    // A navigation replaces the world, so the id fetched a moment ago can already be gone.
    for _ in 0..3 {
        let Ok(Some(world)) = page.secondary_execution_context().await else {
            tokio::time::sleep(Duration::from_millis(120)).await;
            continue;
        };
        let params = EvaluateParams::builder()
            .expression(js)
            .context_id(world)
            .return_by_value(true)
            .await_promise(true)
            .build()
            .map_err(anyhow::Error::msg)?;
        match page.evaluate(params).await {
            Ok(r) => return Ok(r.value().cloned().unwrap_or(Value::Null)),
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(120)).await;
            }
        }
    }
    match last {
        Some(e) => Err(anyhow::Error::new(e).context("evaluating in the isolated world")),
        None => bail!("the page has no isolated world to evaluate in"),
    }
}

/// [`eval`] for a script that answers with a string. Empty when it did not.
pub async fn text(page: &Page, js: &str) -> String {
    eval(page, js)
        .await
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// [`eval`] for a script that answers yes or no. `false` when it did not answer.
pub async fn flag(page: &Page, js: &str) -> bool {
    eval(page, js)
        .await
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

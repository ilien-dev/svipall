//! What arrived alongside the document: the response headers of the page itself.
//!
//! The http tier has always had them. The browser tiers never did, and that was not a gap in
//! reporting — it was a gap in *detection*. A vendor whose only give-away is a header or the cookie
//! it sets was invisible at exactly the tiers that hold a live browser, so its wall came back
//! labelled "near-empty body" and the strategy written to answer it never ran.
//!
//! Two facts make the design smaller than it looks.
//!
//! **Redirects need no handling.** CDP does not emit `Network.responseReceived` for a redirect hop;
//! those arrive as `redirectResponse` on `Network.requestWillBeSent`. So the document responses
//! this module sees are final ones, and keeping the latest is right — including after the warm
//! loop re-navigates to re-earn a clearance.
//!
//! **Filtering by frame is not optional.** The vendored handler marks the frame's request for
//! *every* request carrying that frame's id, scripts and XHR included, which is why the status
//! Chromium reports can belong to a sub-resource. Building this on the navigation response would
//! inherit that bug and hand the classifier an image's headers.
//!
//! The filtering is pure and tested here. Only the subscription needs a browser.

use serde_json::Value;

/// Is this response *the page*, as opposed to something the page asked for?
///
/// A document in the main frame and nothing else. `main_frame` being unknown is treated as "cannot
/// tell", and the answer is no: a wrong header map is worse than none, because it would be believed.
pub fn is_the_document(
    resource_type: &str,
    frame_id: Option<&str>,
    main_frame: Option<&str>,
) -> bool {
    if !resource_type.eq_ignore_ascii_case("document") {
        return false;
    }
    match (frame_id, main_frame) {
        (Some(f), Some(m)) => f == m,
        _ => false,
    }
}

/// CDP's `Network.Headers` is a free-form JSON object, and the values are not reliably strings:
/// a numeric `content-length` arrives as a number. Anything that is not an object is no headers.
pub fn headers_from_cdp(v: &Value) -> Vec<(String, String)> {
    let Some(map) = v.as_object() else {
        return Vec::new();
    };
    map.iter()
        .map(|(k, val)| {
            let s = match val {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.clone(), s)
        })
        .collect()
}

/// The document's response headers, kept current while a page loads.
///
/// Deliberately not a spawned task. `capture_json` spawns because it needs a buffer it can abort;
/// here only the latest headers matter, so the stream is drained without blocking at the points
/// that already read the page. That leaves nothing to abort, nothing to lock, and nothing to leak
/// when an early `?` unwinds — and dropping the watch ends the subscription on its own.
///
/// Draining matters. An undrained subscription buffers every response a heavy page makes for the
/// whole of a warm wait; this keeps one header map, replaced.
pub struct DocumentWatch {
    events: svipall_cdp::listeners::EventStream<
        svipall_cdp::cdp::browser_protocol::network::EventResponseReceived,
    >,
    main_frame: Option<String>,
    headers: Vec<(String, String)>,
}

impl DocumentWatch {
    /// Subscribe. Must be called before navigation: the document's response is the first thing that
    /// comes back, and a listener started afterwards has already missed it.
    pub async fn start(page: &svipall_cdp::Page) -> anyhow::Result<Self> {
        use svipall_cdp::cdp::browser_protocol::network::EventResponseReceived;
        // `Network.enable` is already sent for every page by the handler's init commands, so this
        // adds a listener and nothing else.
        let events = page.event_listener::<EventResponseReceived>().await?;
        let main_frame = page
            .mainframe()
            .await
            .ok()
            .flatten()
            .map(|f| f.inner().clone());
        Ok(Self {
            events,
            main_frame,
            headers: Vec::new(),
        })
    }

    /// Take whatever has arrived since the last call. Never waits.
    pub fn drain(&mut self) {
        use futures::{FutureExt, StreamExt};
        while let Some(Some(ev)) = self.events.next().now_or_never() {
            let frame = ev.frame_id.as_ref().map(|f| f.inner().clone());
            if is_the_document(
                &format!("{:?}", ev.r#type),
                frame.as_deref(),
                self.main_frame.as_deref(),
            ) {
                self.headers = headers_from_cdp(ev.response.headers.inner());
            }
        }
    }

    /// The latest document response's headers.
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_the_main_frames_document_response_is_the_document() {
        assert!(is_the_document("Document", Some("F1"), Some("F1")));
        assert!(is_the_document("document", Some("F1"), Some("F1")));
    }

    #[test]
    fn a_sub_resource_in_the_main_frame_is_not_the_document() {
        // The hazard this exists for: the frame carries every request it made, so a script or an
        // image in the main frame looks exactly like the page unless the type is checked.
        assert!(!is_the_document("Script", Some("F1"), Some("F1")));
        assert!(!is_the_document("Image", Some("F1"), Some("F1")));
        assert!(!is_the_document("XHR", Some("F1"), Some("F1")));
        // A document in an iframe is that iframe's page, not ours. An ad frame must never supply
        // the headers a wall is judged on.
        assert!(!is_the_document("Document", Some("F2"), Some("F1")));
        // Unknown frame: no answer rather than a guess.
        assert!(!is_the_document("Document", None, Some("F1")));
        assert!(!is_the_document("Document", Some("F1"), None));
    }

    #[test]
    fn headers_arrive_as_pairs_whatever_json_shape_they_came_in() {
        let v = json!({"x-kpsdk-ct": "2|abc", "content-length": 1234});
        let mut got = headers_from_cdp(&v);
        got.sort();
        assert_eq!(
            got,
            vec![
                ("content-length".to_string(), "1234".to_string()),
                ("x-kpsdk-ct".to_string(), "2|abc".to_string()),
            ]
        );
        assert!(headers_from_cdp(&json!(null)).is_empty());
        assert!(headers_from_cdp(&json!("not an object")).is_empty());
    }
}

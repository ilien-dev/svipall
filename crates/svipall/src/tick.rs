//! Finding the box a widget is waiting on.
//!
//! A widget that wants a click draws a box and stops. On the interstitial that box is inside a
//! frame from another origin, and the frame is inside a shadow root the document cannot open, so
//! nothing evaluated in the page can find either. The protocol can: the frame is a target of its
//! own with a URL, the element that owns it has a box, and the row in `WIDGETS` says where inside
//! that box the widget draws its own. Three calls, none of them visible to the page.
//!
//! A widget that lives in the host document is simpler, and the row names its box by selector.
//!
//! Nothing here clicks. The click is `behavior`'s, like every other one.

use serde_json::Value;
use svipall_cdp::cdp::browser_protocol::dom::{GetBoxModelParams, GetFrameOwnerParams};
use svipall_cdp::cdp::browser_protocol::target::GetTargetsParams;
use svipall_cdp::page::Page;
use svipall_core::widget::{self, Modality, Tick, WIDGETS};

/// How long an interstitial is left to pass on its own before its box is ticked.
///
/// Measured: the one that needs nothing clears five to six seconds after the document starts, and
/// the one that wants a click is still showing its box at thirty. A tick while it is still
/// working does nothing, so the cost of being early is a pointer move.
const FRAME_DUE_MS: f64 = 6_000.0;

/// A box in the page is ticked after the time it takes to read the line beside it.
const ELEMENT_DUE_MS: f64 = 1_200.0;

/// A widget's box, in the page's own coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// The row it came from.
    pub widget: &'static str,
    pub modality: Modality,
    /// Centre of the box.
    pub x: f64,
    pub y: f64,
    /// How far from the centre a click may land and still be on it.
    pub reach: f64,
    /// Whether the widget has had the time it needs to finish by itself.
    pub due: bool,
}

/// Where in a frame's box its tick box is. Fractions in, page coordinates out.
pub fn in_frame(frame: (f64, f64, f64, f64), at: (f64, f64)) -> Option<(f64, f64, f64)> {
    let (x, y, w, h) = frame;
    // A widget that renders nothing visible is not one anybody ticks.
    if w < 60.0 || h < 30.0 {
        return None;
    }
    Some((x + at.0 * w, y + at.1 * h, (h * 0.1).clamp(3.0, 8.0)))
}

/// The rows whose box is an element of the page, as the script below wants them.
fn selector_rows() -> String {
    let rows: Vec<Value> = WIDGETS
        .iter()
        .filter_map(|w| match w.tick {
            Some(Tick::Selector(s)) => Some(serde_json::json!([w.id, s, w.containers.join(", ")])),
            _ => None,
        })
        .collect();
    Value::Array(rows).to_string()
}

fn element_js() -> String {
    format!(
        r#"(() => {{
            for (const [id, box] of {rows}) {{
                for (const el of document.querySelectorAll(box)) {{
                    if (el.checked || el.disabled) continue;
                    const r = el.getBoundingClientRect();
                    const st = getComputedStyle(el);
                    if (r.width < 6 || r.height < 6 || st.display === 'none' || st.visibility === 'hidden') continue;
                    if (r.bottom < 0 || r.top > innerHeight) continue;
                    return {{id: id, x: r.x + r.width / 2, y: r.y + r.height / 2,
                             reach: Math.min(r.width, r.height) * 0.3, age: performance.now()}};
                }}
            }}
            return null;
        }})()"#,
        rows = selector_rows()
    )
}

/// Is a widget with a box still on the page, and has its answer field been filled?
fn present_js() -> String {
    let fields: Vec<&str> = widget::response_fields().collect();
    format!(
        r#"(() => {{
            let present = false;
            for (const [, , containers] of {rows}) if (document.querySelector(containers)) present = true;
            let filled = false;
            for (const n of {fields}) {{
                const el = document.querySelector('input[name="' + n + '"], textarea[name="' + n + '"]');
                if (el && el.value && el.value.length > 20) filled = true;
            }}
            return {{present: present, filled: filled}};
        }})()"#,
        rows = selector_rows(),
        fields = serde_json::to_string(&fields).unwrap_or_else(|_| "[]".into()),
    )
}

/// The box of the frame a widget of this page lives in, with the row it belongs to.
async fn frame(page: &Page) -> Option<(&'static widget::WidgetSpec, (f64, f64, f64, f64))> {
    let targets = page.execute(GetTargetsParams::default()).await.ok()?;
    for t in &targets.result.target_infos {
        if t.r#type != "iframe" {
            continue;
        }
        let Some(spec) = widget::from_url(&t.url) else {
            continue;
        };
        // Every tab's frames are listed. Asking this page who owns the frame is what tells its
        // own from another tab's: for a stranger the answer is an error.
        let Ok(owner) = page
            .execute(GetFrameOwnerParams::new(t.target_id.inner().clone()))
            .await
        else {
            continue;
        };
        let Ok(model) = page
            .execute(
                GetBoxModelParams::builder()
                    .backend_node_id(owner.result.backend_node_id)
                    .build(),
            )
            .await
        else {
            continue;
        };
        let m = &model.result.model;
        let quad = m.content.inner();
        if quad.len() < 2 {
            continue;
        }
        return Some((spec, (quad[0], quad[1], m.width as f64, m.height as f64)));
    }
    None
}

/// The box a widget on this page is waiting on, if one is.
pub async fn target(page: &Page) -> Option<Target> {
    if let Ok(v) = crate::apart::eval(page, &element_js()).await {
        if let Some(spec) = v.get("id").and_then(Value::as_str).and_then(widget::spec) {
            return Some(Target {
                widget: spec.id,
                modality: spec.default_modality,
                x: v.get("x")?.as_f64()?,
                y: v.get("y")?.as_f64()?,
                reach: v.get("reach")?.as_f64()?.clamp(2.0, 8.0),
                due: v.get("age").and_then(Value::as_f64).unwrap_or(0.0) >= ELEMENT_DUE_MS,
            });
        }
    }
    let (spec, rect) = frame(page).await?;
    let Some(Tick::InFrame { x, y }) = spec.tick else {
        return None;
    };
    let (x, y, reach) = in_frame(rect, (x, y))?;
    let age = document_age_ms(page).await;
    Some(Target {
        widget: spec.id,
        modality: spec.default_modality,
        x,
        y,
        reach,
        due: age >= FRAME_DUE_MS,
    })
}

/// What a widget that was acted on is doing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asking {
    /// Still on the page with nothing in its answer field.
    Yes,
    /// Its answer field holds an answer: what a widget embedded in a form does when satisfied.
    Answered,
    /// Not on the page. What a wall does once it lets the visitor through, and also what one
    /// does for a moment while it reloads into itself, so the caller looks at the page too.
    Gone,
}

/// Is a widget that was acted on still asking?
pub async fn asking(page: &Page) -> Asking {
    let v = crate::apart::eval(page, &present_js())
        .await
        .unwrap_or(Value::Null);
    if v.get("filled").and_then(Value::as_bool).unwrap_or(false) {
        return Asking::Answered;
    }
    if v.get("present").and_then(Value::as_bool).unwrap_or(false) || frame(page).await.is_some() {
        return Asking::Yes;
    }
    Asking::Gone
}

/// How long the document has been there, in milliseconds. Zero when it cannot say.
pub async fn document_age_ms(page: &Page) -> f64 {
    crate::apart::eval(page, "performance.now()")
        .await
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

/// When the document was created, on the page's own clock, in milliseconds since the epoch.
/// Zero when it cannot say. A reload or a navigation gives the tab a new one.
pub async fn document_born(page: &Page) -> f64 {
    crate::apart::eval(page, "performance.timeOrigin")
        .await
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

/// Is the document read `now` another one than the document read `then`? An answer that could
/// not be read is not evidence of one.
pub fn another_document(then: f64, now: f64) -> bool {
    then > 0.0 && now > 0.0 && (now - then).abs() > 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_is_another_one_only_when_both_readings_say_so() {
        assert!(another_document(1_794_471_751_000.0, 1_794_471_757_300.5));
        assert!(!another_document(1_794_471_751_000.0, 1_794_471_751_000.2));
        // A page that could not answer, before or after, has not been shown to have changed.
        assert!(!another_document(0.0, 1_794_471_757_300.5));
        assert!(!another_document(1_794_471_751_000.0, 0.0));
    }

    #[test]
    fn the_box_is_placed_by_fraction_of_the_frame_wherever_the_frame_is() {
        // The frame measured on the interstitial: 300 by 65 at (176.5, 304), box 28 px in.
        let spec = widget::spec("challenges.cloudflare.com").unwrap();
        let Some(Tick::InFrame { x, y }) = spec.tick else {
            panic!("the interstitial's row names a box in its frame");
        };
        let (cx, cy, reach) = in_frame((176.5, 304.0, 300.0, 65.0), (x, y)).unwrap();
        assert!((cx - 204.4).abs() < 1.0, "{cx}");
        assert!((cy - 336.5).abs() < 1.0, "{cy}");
        // Anywhere within reach is still on a 24 px box.
        assert!(reach <= 8.0);
        // The same widget drawn elsewhere and larger keeps its box in the same place within it.
        let (cx, _, _) = in_frame((0.0, 0.0, 600.0, 130.0), (x, y)).unwrap();
        assert!((cx - 55.8).abs() < 1.0, "{cx}");
    }

    #[test]
    fn a_frame_with_nothing_to_see_has_no_box() {
        assert_eq!(in_frame((0.0, 0.0, 0.0, 0.0), (0.1, 0.5)), None);
        assert_eq!(in_frame((10.0, 10.0, 300.0, 1.0), (0.1, 0.5)), None);
    }

    #[test]
    fn the_scripts_are_built_from_the_table_and_ask_nothing_of_the_page() {
        let js = element_js();
        assert!(js.contains("altcha.org"), "{js}");
        // A box named by selector is in the script; one placed by fraction is not a selector.
        assert!(!js.contains("challenges.cloudflare.com"));
        let js = present_js();
        for field in widget::response_fields() {
            assert!(js.contains(field), "{field} is not asked about");
        }
    }
}

//! A live challenge the models could not answer, handed to a person and finished on the page.
//!
//! Before this, a person could only answer *queued* jobs: a challenge on a page the strategy loop
//! had given up on was reported as a wall, the page was closed, and whoever came to the dashboard
//! found nothing to do. Now the page is parked, what it shows is posted as a job with its picture,
//! and the answer — in fractions of that picture, never pixels — is replayed on the page by the
//! same behaviour layer the models use. The page still decides.
//!
//! What is captured is the geometry an answer needs to land: the tile boxes of a grid, the box of
//! a picture, the frame a slider lives in, the control to hold. That is read once, before the
//! job is posted, because by the time the answer arrives the loop's own probe may be stale.

use std::time::{Duration, Instant};

use serde_json::Value;
use svipall_cdp::page::Page;
use svipall_core::answer::{Answer, Point};
use svipall_core::widget::Modality;

use crate::solve_loop::Surface;
use crate::solver_engine::{
    eval_in_frame, grid_context, hold_target, shot, PageSurface, SolveEngine, AUDIO_JS, DRAG_JS,
    GRID_JS, PICTURE_JS, ROTATE_JS,
};

/// Where on the page an answer lands, in CSS pixels of the top document.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Scene {
    /// A grid of tiles, each with its box, and the button that submits the choice.
    Grid {
        boxes: Vec<(f64, f64, f64, f64)>,
        verify: Option<(f64, f64, f64, f64)>,
    },
    /// One picture, and the button that submits clicks on it.
    Picture {
        rect: (f64, f64, f64, f64),
        submit: Option<String>,
    },
    /// The frame a slider is drawn in; a fraction of its width is how far to drag.
    Frame { rect: (f64, f64, f64, f64) },
    /// A control to press and hold.
    Hold { selector: String },
    /// A slider whose travel is one full turn.
    Rotate {
        handle: (f64, f64, f64, f64),
        track: (f64, f64, f64, f64),
    },
    /// An area a piece is dragged within.
    Drag { rect: (f64, f64, f64, f64) },
    /// A text field an answer is typed into.
    Field { selector: String },
}

/// What was captured for a person: where the answer goes, the pictures they need, and what to
/// tell them.
pub(crate) struct Capture {
    pub scene: Scene,
    /// `(kind, index, png)` — `tile` per square of a grid, else one `image`.
    pub assets: Vec<(&'static str, i64, Vec<u8>)>,
    pub payload: Value,
}

/// Turn an answer's fractions into page pixels for a rectangle.
pub(crate) fn to_page(rect: (f64, f64, f64, f64), p: Point) -> (f64, f64) {
    let (x, y, w, h) = rect;
    (x + w * p.x.clamp(0.0, 1.0), y + h * p.y.clamp(0.0, 1.0))
}

/// The answer a solved job carries, from the two columns the dashboard writes.
///
/// A token lands in `token`; text and nonces land in `text` as they are; every geometric answer
/// lands in `text` as the JSON the panel sent. So: JSON with a `kind` is an `Answer`, anything
/// else in `text` is `Text`, and a token is a `Token`.
pub(crate) fn answer_of(token: Option<&str>, text: Option<&str>) -> Option<Answer> {
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        return Some(Answer::Token {
            value: t.to_string(),
        });
    }
    let t = text.filter(|t| !t.trim().is_empty())?;
    match serde_json::from_str::<Answer>(t) {
        Ok(a) => Some(a),
        Err(_) => Some(Answer::Text {
            value: t.to_string(),
        }),
    }
}

impl SolveEngine {
    /// Read what the page is showing for `modality`, in enough detail to replay an answer.
    async fn capture(&self, page: &Page, modality: Modality) -> Option<Capture> {
        let num = |v: &Value, k: &str| v[k].as_f64().unwrap_or(0.0);
        match modality {
            Modality::Tiles => {
                let ctx = grid_context(page).await?;
                let (ox, oy, _, _) =
                    crate::behavior::box_of(page, "iframe[src*=\"bframe\"]").await?;
                let info = eval_in_frame(page, ctx, GRID_JS).await.ok()?;
                let (rows, cols) = (
                    info["rows"].as_u64().unwrap_or(0) as usize,
                    info["cols"].as_u64().unwrap_or(0) as usize,
                );
                if rows == 0 || cols == 0 {
                    return None;
                }
                let boxes = crate::grid::tile_boxes(
                    ox + num(&info, "x"),
                    oy + num(&info, "y"),
                    num(&info, "w"),
                    num(&info, "h"),
                    rows,
                    cols,
                );
                let mut assets = Vec::with_capacity(boxes.len());
                for (i, (x, y, w, h)) in boxes.iter().enumerate() {
                    assets.push(("tile", i as i64, shot(page, *x, *y, *w, *h).await.ok()?));
                }
                let v = &info["verify"];
                let verify = v["w"]
                    .as_f64()
                    .map(|w| (ox + num(v, "x"), oy + num(v, "y"), w, num(v, "h")));
                Some(Capture {
                    scene: Scene::Grid { boxes, verify },
                    assets,
                    payload: serde_json::json!({
                        "prompt": info["prompt"].as_str().unwrap_or(""), "rows": rows, "cols": cols
                    }),
                })
            }
            Modality::Points | Modality::Polygon => {
                let info = page.evaluate(PICTURE_JS).await.ok()?.value().cloned()?;
                if info.is_null() {
                    return None;
                }
                let rect = (
                    num(&info, "x"),
                    num(&info, "y"),
                    num(&info, "w"),
                    num(&info, "h"),
                );
                let png = shot(page, rect.0, rect.1, rect.2, rect.3).await.ok()?;
                Some(Capture {
                    scene: Scene::Picture {
                        rect,
                        submit: info["submit"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_string),
                    },
                    assets: vec![("image", 0, png)],
                    payload: serde_json::json!({
                        "prompt": info["prompt"].as_str().unwrap_or(""), "w": rect.2, "h": rect.3
                    }),
                })
            }
            Modality::Slide => {
                let rect =
                    crate::behavior::box_of(page, "iframe[src*=\"captcha-delivery\"]").await?;
                let png = shot(page, rect.0, rect.1, rect.2, rect.3).await.ok()?;
                Some(Capture {
                    scene: Scene::Frame { rect },
                    assets: vec![("image", 0, png)],
                    payload: serde_json::json!({"w": rect.2, "h": rect.3}),
                })
            }
            Modality::Hold => {
                let selector = hold_target(page).await?;
                let rect = crate::behavior::box_of(page, &selector).await?;
                let png = shot(page, rect.0, rect.1, rect.2, rect.3).await.ok()?;
                Some(Capture {
                    scene: Scene::Hold { selector },
                    assets: vec![("image", 0, png)],
                    payload: serde_json::json!({"prompt": "press and hold"}),
                })
            }
            Modality::Rotate => {
                let info = page.evaluate(ROTATE_JS).await.ok()?.value().cloned()?;
                if info.is_null() {
                    return None;
                }
                let rect = (
                    num(&info, "x"),
                    num(&info, "y"),
                    num(&info, "w"),
                    num(&info, "h"),
                );
                let png = shot(page, rect.0, rect.1, rect.2, rect.3).await.ok()?;
                let b = |v: &Value| (num(v, "x"), num(v, "y"), num(v, "w"), num(v, "h"));
                Some(Capture {
                    scene: Scene::Rotate {
                        handle: b(&info["handle"]),
                        track: b(&info["track"]),
                    },
                    assets: vec![("image", 0, png)],
                    payload: serde_json::json!({"prompt": info["prompt"].as_str().unwrap_or("")}),
                })
            }
            Modality::Drag => {
                let info = page.evaluate(DRAG_JS).await.ok()?.value().cloned()?;
                if info.is_null() {
                    return None;
                }
                let a = &info["area"];
                let rect = (num(a, "x"), num(a, "y"), num(a, "w"), num(a, "h"));
                let png = shot(page, rect.0, rect.1, rect.2, rect.3).await.ok()?;
                Some(Capture {
                    scene: Scene::Drag { rect },
                    assets: vec![("image", 0, png)],
                    payload: serde_json::json!({"prompt": info["prompt"].as_str().unwrap_or("")}),
                })
            }
            Modality::Audio | Modality::Text => {
                let info = page.evaluate(AUDIO_JS).await.ok()?.value().cloned()?;
                let field = info["field"]
                    .as_str()
                    .filter(|f| !f.is_empty())?
                    .to_string();
                let src = info["src"].as_str().unwrap_or("").to_string();
                // The clip itself is fetched by the page's own session, as the model does.
                let b64 = page
                    .evaluate(crate::solver_engine::fetch_clip_js(&src).as_str())
                    .await
                    .ok()
                    .and_then(|r| r.value().and_then(Value::as_str).map(str::to_string))
                    .unwrap_or_default();
                use base64::{engine::general_purpose::STANDARD, Engine};
                let bytes = STANDARD.decode(b64).unwrap_or_default();
                let assets = if bytes.is_empty() {
                    Vec::new()
                } else {
                    vec![("audio", 0, bytes)]
                };
                Some(Capture {
                    scene: Scene::Field { selector: field },
                    assets,
                    payload: serde_json::json!({"prompt": "type what you hear"}),
                })
            }
            // Nothing on a live page is waiting on any of these. A token and a nonce are the
            // widget's own business, and a page rating is a job created directly rather than
            // captured from a challenge — there is nothing to photograph and nothing to replay.
            Modality::Token | Modality::Nonce | Modality::Rate => None,
        }
    }

    /// Replay a person's answer on the page. `Ok(true)` when the input was delivered; whether it
    /// was *right* is the page's to say, and the caller asks it.
    async fn replay(&self, page: &Page, scene: &Scene, answer: &Answer) -> anyhow::Result<bool> {
        let seed = self.pool.identity().noise_seed;
        let mut cursor = crate::behavior::Cursor::at_page(page);
        let click_all = |pts: Vec<(f64, f64)>| async move {
            let mut cursor = crate::behavior::Cursor::at_page(page);
            for (n, (x, y)) in pts.into_iter().enumerate() {
                let s = seed ^ (n as u64).wrapping_mul(0x9E37_79B9);
                let (tx, ty) = crate::behavior::aim(x - 6.0, y - 6.0, 12.0, 12.0, s);
                cursor.click_at(page, tx, ty, s).await?;
                tokio::time::sleep(Duration::from_millis(160 + (s % 220))).await;
            }
            Ok::<_, anyhow::Error>(())
        };
        match (scene, answer) {
            (Scene::Grid { boxes, verify }, Answer::Tiles { indices }) => {
                for (n, i) in indices.iter().enumerate() {
                    let Some((x, y, w, h)) = boxes.get(*i as usize) else {
                        continue;
                    };
                    let s = seed ^ (n as u64).wrapping_mul(0x9E37_79B9);
                    let (tx, ty) = crate::behavior::aim(*x, *y, *w, *h, s);
                    cursor.click_at(page, tx, ty, s).await?;
                    tokio::time::sleep(Duration::from_millis(180 + (s % 240))).await;
                }
                if let Some((x, y, w, h)) = verify {
                    let (tx, ty) = crate::behavior::aim(*x, *y, *w, *h, seed);
                    cursor.click_at(page, tx, ty, seed).await?;
                }
                Ok(true)
            }
            (Scene::Picture { rect, submit }, Answer::Points { points })
            | (Scene::Picture { rect, submit }, Answer::Polygon { points }) => {
                click_all(points.iter().map(|p| to_page(*rect, *p)).collect()).await?;
                if let Some(sel) = submit {
                    let _ = self.pool.human_click(page, sel).await;
                }
                Ok(true)
            }
            (Scene::Frame { rect }, Answer::Slide { fraction }) => {
                // The handle sits at the left edge of the frame's lower band, as these widgets
                // draw it; the person said how far along the frame it goes.
                let (x, y, w, h) = *rect;
                let from = (x + w * 0.08, y + h * 0.82);
                let to = (x + w * fraction.clamp(0.0, 1.0), from.1);
                cursor.move_to(page, from.0, from.1, seed).await?;
                cursor.drag_to(page, to.0, to.1, seed).await?;
                Ok(true)
            }
            (Scene::Hold { selector }, Answer::Hold { ms }) => {
                self.pool
                    .press_and_hold(page, selector, u64::from(*ms))
                    .await?;
                Ok(true)
            }
            (Scene::Rotate { handle, track }, Answer::Rotate { degrees }) => {
                let (hx, hy, hw, hh) = *handle;
                let (tx, _, tw, _) = *track;
                let travel = (tw - hw).max(1.0);
                let dx = travel * (degrees.clamp(0.0, 360.0) / 360.0);
                let from = (hx + hw / 2.0, hy + hh / 2.0);
                let to = ((tx + hw / 2.0 + dx).min(tx + tw - hw / 2.0), from.1);
                cursor.move_to(page, from.0, from.1, seed).await?;
                cursor.drag_to(page, to.0, to.1, seed).await?;
                Ok(true)
            }
            (Scene::Drag { rect }, Answer::Drag { from, to }) => {
                let (fx, fy) = to_page(*rect, *from);
                let (tx, ty) = to_page(*rect, *to);
                cursor.move_to(page, fx, fy, seed).await?;
                cursor.drag_to(page, tx, ty, seed).await?;
                Ok(true)
            }
            (Scene::Field { selector }, Answer::Text { value })
            | (Scene::Field { selector }, Answer::Nonce { value }) => {
                self.pool.human_type(page, selector, value).await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Post what the page is showing to the dashboard, wait for a person, and replay their answer.
    ///
    /// `Ok(false)` covers every way this can come to nothing — no dashboard, nothing capturable,
    /// nobody answered in time, an answer the page rejected — because to the caller they are the
    /// same thing: the wall is still there.
    pub(crate) async fn hand_to_dashboard(
        &self,
        page: &Page,
        modality: Modality,
        wait: Duration,
    ) -> anyhow::Result<bool> {
        let Some(st) = &self.state else {
            return Ok(false);
        };
        let Some(capture) = self.capture(page, modality).await else {
            tracing::info!(?modality, "nothing on the page a person could be shown");
            return Ok(false);
        };
        let surface = PageSurface::new(self, page);
        let widget = surface
            .probe()
            .await
            .and(surface.widget())
            .unwrap_or("unknown");
        let modality_name = format!("{modality:?}").to_lowercase();
        let url = page.url().await.ok().flatten();
        let (job_id, task_id) = {
            let db = st.db_pool.read().await;
            let job = db.create_local_job(
                url.as_deref(),
                widget,
                &modality_name,
                Some(&capture.payload.to_string()),
            )?;
            for (kind, idx, bytes) in &capture.assets {
                let mime = if *kind == "audio" {
                    "audio/mpeg"
                } else {
                    "image/png"
                };
                db.put_asset(&job.id, kind, *idx, mime, bytes);
            }
            db.set_needs_human(
                &job.task_id,
                "the models could not answer this one; a person can, at the dashboard",
            )?;
            (job.id, job.task_id)
        };
        tracing::info!(task = %task_id, ?modality, "challenge handed to the dashboard");
        let _ = job_id;

        let deadline = Instant::now() + wait;
        let answer = loop {
            let rec = st.db_pool.read().await.get_by_task_id(&task_id)?;
            match rec {
                Some(r) if r.status == "solved" => {
                    break answer_of(r.token.as_deref(), r.text.as_deref());
                }
                Some(r) if r.status == "failed" => break None,
                _ => {}
            }
            if Instant::now() >= deadline {
                tracing::info!(task = %task_id, "nobody answered in time");
                break None;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        };
        let Some(answer) = answer else {
            return Ok(false);
        };
        let delivered = self.replay(page, &capture.scene, &answer).await?;
        if !delivered {
            tracing::info!(task = %task_id, "the answer does not fit what the page is showing");
            return Ok(false);
        }
        self.pool.settle(page, Duration::from_millis(2_000)).await;
        surface.remember(Some(modality));
        let ok = surface.settled().await;
        let _ = st.db_pool.read().await.set_ok(&task_id, ok);
        Ok(ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_wins_and_text_is_read_as_json_when_it_is_json() {
        assert!(matches!(
            answer_of(Some("tok"), Some("ignored")),
            Some(Answer::Token { value }) if value == "tok"
        ));
        let a = answer_of(None, Some(r#"{"kind":"tiles","indices":[0,4]}"#)).unwrap();
        assert!(matches!(a, Answer::Tiles { indices } if indices == vec![0, 4]));
        let a = answer_of(None, Some("four seven")).unwrap();
        assert!(matches!(a, Answer::Text { value } if value == "four seven"));
        assert!(answer_of(None, Some("   ")).is_none());
        assert!(answer_of(Some(""), None).is_none());
    }

    #[test]
    fn fractions_land_inside_the_rectangle_they_describe() {
        let r = (100.0, 200.0, 50.0, 10.0);
        assert_eq!(to_page(r, Point { x: 0.0, y: 0.0 }), (100.0, 200.0));
        assert_eq!(to_page(r, Point { x: 1.0, y: 1.0 }), (150.0, 210.0));
        // Out of range is clamped, never extrapolated to a click somewhere else on the page.
        assert_eq!(to_page(r, Point { x: 2.0, y: -1.0 }), (150.0, 200.0));
    }

    #[test]
    fn every_modality_a_person_can_answer_has_a_scene_or_is_named_as_not_handed_over() {
        // Token and nonce are the two a person cannot help with on a live page: one is what the
        // widget produces by itself, the other is arithmetic. Everything else must map to a
        // scene, else the dashboard renders it and the answer has nowhere to land.
        let handed: &[Modality] = &[
            Modality::Tiles,
            Modality::Points,
            Modality::Polygon,
            Modality::Slide,
            Modality::Hold,
            Modality::Rotate,
            Modality::Drag,
            Modality::Audio,
            Modality::Text,
        ];
        let kept: &[Modality] = &[Modality::Token, Modality::Nonce, Modality::Rate];
        for m in Modality::ALL {
            assert!(
                handed.contains(m) || kept.contains(m),
                "{m:?} is neither handed to a person nor named as kept"
            );
        }
    }
}

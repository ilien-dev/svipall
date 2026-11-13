//! `web_video` looking at the picture: thumbnails, captures, sheets, text on screen, search.
//!
//! What to look at is decided in `crate::video` and `svipall_core::video::look`, where it can be
//! tested without a browser. This is the part that fetches and captures: the player's own
//! thumbnails over plain HTTP, frames from the page's `<video>` seeked in place, and the record
//! of both kept beside the frames so no call repeats another's work.

use super::*;
use crate::video::{self as vid, Look, Moments, Seen, SheetRef};
use crate::video_frames as vf;
use crate::video_index::{self as vix, Index};
use crate::video_sheet::{self as sheet, Cell};
use svipall_core::video::{align::FrameRef, look, storyboard, Cue, VideoInfo};

/// Sheets of thumbnails fetched for one call, at most. A sheet holds nine to a hundred cells.
const MAX_BOARD_SHEETS: usize = 160;

/// Thumbnails compared with the words of a search, at most: twice what an overview shows, since
/// a search is for the moment an overview stepped over.
const SEARCH_CELLS: usize = look::MAX_MOMENTS * 2;

/// Moments a search returns for each way of finding one.
const HITS: usize = 8;

/// How different two pictures in a row must be to be marked a cut, on [`vf::distance`]'s scale.
const CUT: f32 = 0.2;

/// Widest frame read from the element when a part of it was asked for: the part is what is
/// returned, so the whole can be as large as the video is.
const CROP_GRAB_WIDTH: u32 = 3840;

/// The player's thumbnails: where each cell is, and the sheets fetched so far.
#[derive(Default)]
pub(super) struct Board {
    frames: Vec<storyboard::Frame>,
    sheets: HashMap<String, Option<image::DynamicImage>>,
}

impl Board {
    pub(super) fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    fn span(&self) -> f64 {
        self.frames.last().map_or(0.0, |f| f.t)
    }

    /// The cells nearest `times`, each once, in time order.
    fn nearest(&self, times: &[f64]) -> Vec<usize> {
        let mut out: Vec<usize> = times
            .iter()
            .filter_map(|t| {
                let i = self.frames.partition_point(|f| f.t < *t);
                [i.checked_sub(1), (i < self.frames.len()).then_some(i)]
                    .into_iter()
                    .flatten()
                    .min_by(|a, b| {
                        (self.frames[*a].t - t)
                            .abs()
                            .total_cmp(&(self.frames[*b].t - t).abs())
                    })
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// A frame captured from the player.
pub(super) struct Shot {
    t: f64,
    png: Vec<u8>,
    /// Asked for by its time, not placed by the spread: never dropped for repeating another.
    exact: bool,
}

/// What a live capture brought back.
pub(super) struct Captured {
    shots: Vec<Shot>,
    moments: Moments,
    unreached: usize,
}

/// What looking produced, for the answer.
#[derive(Default)]
pub(super) struct Looked {
    pub frames: Vec<FrameRef>,
    pub sheets: Vec<SheetRef>,
    pub seen: Option<Seen>,
    pub found: Option<(String, Vec<vix::Hit>)>,
}

/// The time a call has: longer when it looks at the video itself.
pub(super) fn limit(look: &Look, wait: Option<u64>) -> Duration {
    let base = if look.live() || look.query.is_some() {
        180
    } else {
        90
    };
    Duration::from_secs(base + wait.map_or(0, |w| w.min(crate::steer::MAX_WAIT_SECS)))
}

fn sheet_refs(dir: &std::path::Path, cells: &[Cell], tag: &str) -> anyhow::Result<Vec<SheetRef>> {
    let mut out = Vec::new();
    for s in sheet::compose(cells) {
        let name = vf::frame_path(dir, "", s.from)
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let path = dir.join(format!("sheet-{tag}-{name}.jpg"));
        std::fs::create_dir_all(dir)?;
        std::fs::write(&path, sheet::jpeg(&s.image, 80)?)?;
        out.push(SheetRef {
            path: path.to_string_lossy().into_owned(),
            from: s.from,
            to: s.to,
            cells: s.cells,
            grid: format!("{}x{}", s.cols, s.rows),
            tokens: look::patch_cost(s.image.width(), s.image.height()).1,
        });
    }
    Ok(out)
}

/// How near a frame captured earlier must be to stand in for a moment asked for now.
fn near(m: &Moments, exact: bool) -> f64 {
    if exact {
        return 0.3;
    }
    m.every().map_or(0.3, |e| (e / 4.0).clamp(0.1, 2.0))
}

/// The frames an earlier call captured for every moment of `m`, or `None` when one is missing.
fn all_cached(index: &Index, m: &Moments) -> Option<Vec<vix::Seen>> {
    let mut out: Vec<vix::Seen> = Vec::new();
    for (i, t) in m.order().into_iter().enumerate() {
        let f = index.frame_near(t, near(m, i < m.at.len()))?;
        if !out.iter().any(|o| o.path == f.path) {
            out.push(f.clone());
        }
    }
    out.sort_by(|a, b| a.t.total_cmp(&b.t));
    Some(out)
}

impl SvipallServer {
    /// Where a video's frames, sheets and record are kept: one directory a video.
    pub(super) fn video_dir(info: &VideoInfo, final_url: &str) -> PathBuf {
        let root = svipall_core::config::home_dir().join("out").join("video");
        let name = match &info.id {
            Some(id) => id.clone(),
            None => {
                // A site has many videos: the address tells them apart where no id does.
                let digest = format!("{:x}", md5::compute(final_url.as_bytes()));
                format!("{}-{}", domain_from_url(final_url), &digest[..8])
            }
        };
        vf::frame_path(&root, &name, 0.0)
            .parent()
            .map(PathBuf::from)
            .unwrap_or(root)
    }

    /// The player's thumbnails, from its boot data or from a WebVTT metadata track.
    pub(super) async fn video_board(&self, info: &VideoInfo, wait: Option<u64>) -> Board {
        let mut board = Board {
            frames: info.storyboard.clone(),
            sheets: HashMap::new(),
        };
        if !board.is_empty() {
            return board;
        }
        for track in info.thumb_tracks.iter().take(2) {
            let Ok(body) = self.video_text(track, wait).await else {
                continue;
            };
            let Some(cues) = svipall_core::video::cues::parse(&body, false) else {
                continue;
            };
            let thumbs = storyboard::vtt_thumbs(&cues, track);
            if thumbs.is_empty() {
                continue;
            }
            // A box in a sheet's pixels means nothing until the sheet's size is known.
            let mut urls: Vec<String> = thumbs.iter().map(|t| t.sheet.clone()).collect();
            urls.sort();
            urls.dedup();
            urls.truncate(MAX_BOARD_SHEETS);
            self.board_fetch(&mut board, urls).await;
            board.frames = thumbs
                .iter()
                .filter_map(|t| {
                    let img = board.sheets.get(&t.sheet)?.as_ref()?;
                    Some(t.on(img.width(), img.height()))
                })
                .collect();
            board.frames.sort_by(|a, b| a.t.total_cmp(&b.t));
            if !board.is_empty() {
                break;
            }
        }
        board
    }

    async fn board_fetch(&self, board: &mut Board, urls: Vec<String>) {
        let missing: Vec<String> = urls
            .into_iter()
            .filter(|u| !board.sheets.contains_key(u))
            .collect();
        let Some(first) = missing.first() else {
            return;
        };
        let fetcher = self.fetcher_for(self.exit_for(&domain_from_url(first)).as_deref());
        let fetched: Vec<(String, Option<image::DynamicImage>)> =
            futures::stream::iter(missing.into_iter().map(|u| {
                let fetcher = fetcher.clone();
                async move {
                    let img = match fetcher.send(HttpRequest::get(u.clone())).await {
                        Ok(r) if (200..300).contains(&r.status) => {
                            image::load_from_memory(&r.body).ok()
                        }
                        _ => None,
                    };
                    (u, img)
                }
            }))
            .buffer_unordered(6)
            .collect()
            .await;
        board.sheets.extend(fetched);
    }

    /// The cells at `idx`, cut from their sheets, fetching the sheets not fetched yet.
    async fn board_cells(&self, board: &mut Board, idx: &[usize]) -> Vec<Cell> {
        let mut urls: Vec<String> = idx.iter().map(|i| board.frames[*i].sheet.clone()).collect();
        urls.sort();
        urls.dedup();
        urls.truncate(MAX_BOARD_SHEETS);
        self.board_fetch(board, urls).await;
        idx.iter()
            .filter_map(|i| {
                let f = &board.frames[*i];
                let img = board.sheets.get(&f.sheet)?.as_ref()?;
                Some(Cell {
                    t: f.t,
                    img: vf::crop(img, f.x, f.y, f.w, f.h),
                })
            })
            .collect()
    }

    /// Whether a look can be answered from frames an earlier call captured, and with which.
    pub(super) fn video_cached(
        index: &Index,
        want: &Look,
        duration: Option<f64>,
        spoken: bool,
    ) -> Result<Option<(Moments, Vec<vix::Seen>)>, String> {
        let Some(d) = duration else {
            return Ok(None);
        };
        let m = vid::moments(want, d, spoken)?;
        // A part of the frame is cut from a larger capture than the one kept.
        if want.crop.is_some() || m.is_empty() {
            return Ok(None);
        }
        Ok(all_cached(index, &m).map(|f| (m, f)))
    }

    /// Seek the page's main video to each moment of the look and capture it, until `until`.
    pub(super) async fn video_capture(
        &self,
        page: &svipall_cdp::Page,
        info: &VideoInfo,
        want: &Look,
        spoken: bool,
        until: Instant,
        notes: &mut Vec<String>,
    ) -> anyhow::Result<Option<Captured>> {
        use svipall_cdp::cdp::browser_protocol::page::CaptureScreenshotFormat;

        // An advert plays first, and a player that has not started keeps the video off the page.
        let state = watch_player(
            page,
            info.duration,
            Duration::from_secs(45),
            |p| matches!(p, vf::Playing::Main { .. }),
            self.pool.identity().noise_seed,
            notes,
        )
        .await?;
        let length = match state {
            vf::Playing::Main { picture: false, .. } => {
                // A picture of no size is a stream this browser cannot decode (a codec it
                // dropped), playing its sound and nothing else.
                notes.push(
                    "the browser decodes no picture for this video (its codec), so no frames"
                        .into(),
                );
                return Ok(None);
            }
            vf::Playing::Main { length, .. } => length,
            vf::Playing::Advert { .. } => {
                notes.push(
                    "an advert was still on screen when the wait ran out, so no frames".into(),
                );
                return Ok(None);
            }
            _ => {
                notes.push("the player never showed the video itself, so no frames".into());
                return Ok(None);
            }
        };
        let moments = match vid::moments(want, info.duration.unwrap_or(length), spoken) {
            Ok(m) => m,
            Err(e) => {
                notes.push(e);
                return Ok(None);
            }
        };
        let max_w = if want.crop.is_some() {
            CROP_GRAB_WIDTH
        } else {
            vf::GRAB_MAX_WIDTH
        };
        // Whatever the wait for the player cost, the capture gets a fair run at the video.
        let until = until.max(Instant::now() + Duration::from_secs(15));

        let order = moments.order();
        let mut shots: Vec<Shot> = Vec::new();
        let (mut clipped, mut unreached) = (0usize, 0usize);
        for (i, t) in order.iter().copied().enumerate() {
            if Instant::now() >= until {
                unreached = order.len() - i;
                break;
            }
            let Some(v) = quiet_eval(page, vf::seek_js(t)).await? else {
                continue;
            };
            if v.get("ok").and_then(|o| o.as_bool()) != Some(true) {
                continue;
            }
            // Where the player landed, which is a frame's own time and not always the one asked.
            let at = v.get("t").and_then(|x| x.as_f64()).unwrap_or(t);
            let exact = i < moments.at.len();
            // The element's own picture first: no controls, no captions drawn over it.
            let grabbed = quiet_eval(page, vf::grab_js(max_w))
                .await?
                .and_then(|g| g.get("png").and_then(|p| p.as_str()).map(str::to_string))
                .and_then(|b| {
                    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b).ok()
                });
            if let Some(png) = grabbed {
                shots.push(Shot { t: at, png, exact });
                continue;
            }
            let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
            // Where the player is, as the page itself measures it on this seek: a layout that
            // moved (a banner, a theatre mode) moves the clip with it.
            let params = svipall_cdp::page::ScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .clip(svipall_cdp::cdp::browser_protocol::page::Viewport {
                    x: num("x"),
                    y: num("y"),
                    width: num("w"),
                    height: num("h"),
                    scale: 1.0,
                })
                .capture_beyond_viewport(true)
                .build();
            // One capture sometimes waits seconds on the compositor; a frame is not worth that.
            let shot = tokio::time::timeout(Duration::from_secs(4), page.screenshot(params)).await;
            if let Ok(Ok(png)) = shot {
                clipped += 1;
                shots.push(Shot { t: at, png, exact });
            }
        }
        if clipped > 0 {
            notes.push(format!(
                "{clipped} frame(s) could not be read from the player itself (a cross-origin or \
                 protected stream), so they are screenshots and can show its controls"
            ));
        }
        Ok(Some(Captured {
            shots,
            moments,
            unreached,
        }))
    }

    /// Everything after the video has been captured and its words are known: keep what shows
    /// something, write it down, read the text on it, draw the sheets, and search.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn video_looked(
        &self,
        info: &VideoInfo,
        want: &Look,
        dir: &std::path::Path,
        mut index: Index,
        mut board: Board,
        captured: Option<Captured>,
        reused: Option<(Moments, Vec<vix::Seen>)>,
        cues: &[Cue],
        spoken: bool,
        started: Instant,
        limit: Duration,
        notes: &mut Vec<String>,
    ) -> Looked {
        let mut out = Looked::default();
        let mut seen = Seen::default();
        let mut pictures: Vec<Cell> = Vec::new();

        if let Some((m, frames)) = reused {
            seen = Seen {
                from: m.from,
                to: m.to,
                every: m.every(),
                source: "player",
                reused: frames.len(),
                ..Seen::default()
            };
            for f in frames {
                let Some(path) = f.path.clone() else { continue };
                if let Ok(img) = image::open(&path) {
                    pictures.push(Cell {
                        t: f.t,
                        img: img.to_rgb8(),
                    });
                }
                out.frames.push(FrameRef {
                    t: f.t,
                    path,
                    tokens: f.size.map(|(w, h)| look::patch_cost(w, h).1),
                    shown: f.shown.clone().filter(|s| !s.is_empty()),
                    cut: false,
                });
            }
        } else if let Some(c) = captured {
            seen = Seen {
                from: c.moments.from,
                to: c.moments.to,
                every: c.moments.every(),
                source: "player",
                unreached: c.unreached,
                ..Seen::default()
            };
            let mut shots = c.shots;
            shots.sort_by(|a, b| a.t.total_cmp(&b.t));
            let decoded: Vec<Option<image::DynamicImage>> = shots
                .iter()
                .map(|s| image::load_from_memory(&s.png).ok())
                .collect();
            // A capture of nothing is not a frame, and the same picture twice is one frame:
            // both are counted for the caller rather than handed over.
            let mut kept: Vec<Vec<f32>> = Vec::new();
            let mut kept_paths: Vec<String> = Vec::new();
            for (shot, img) in shots.iter().zip(decoded) {
                let Some(img) = img else {
                    seen.blank += 1;
                    continue;
                };
                let small = img.thumbnail(64, 64).to_rgb8();
                if vf::blank(&small) {
                    seen.blank += 1;
                    continue;
                }
                let hist = vf::histogram(&small);
                let same = kept.iter().position(|k| vf::distance(k, &hist) < 0.002);
                if let Some(k) = same.filter(|_| !shot.exact) {
                    seen.repeated += 1;
                    if want.crop.is_none() {
                        index.repeats.push((shot.t, kept_paths[k].clone()));
                    }
                    continue;
                }
                let cut = kept.last().is_some_and(|k| vf::distance(k, &hist) >= CUT);
                let whole = vf::frame_path(dir, "", shot.t);
                let (path, picture, png) = match want.crop {
                    Some([x, y, w, h]) => {
                        let part = vf::crop(&img, x, y, w, h);
                        let mut bytes = Vec::new();
                        let ok = image::DynamicImage::ImageRgb8(part.clone())
                            .write_to(
                                &mut std::io::Cursor::new(&mut bytes),
                                image::ImageFormat::Png,
                            )
                            .is_ok();
                        if !ok {
                            continue;
                        }
                        let stem = whole
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let pm = |v: f32| (v * 1000.0).round() as u32;
                        let name =
                            format!("{stem}-crop-{}-{}-{}-{}.png", pm(x), pm(y), pm(w), pm(h));
                        (whole.with_file_name(name), part, bytes)
                    }
                    None => (whole, img.to_rgb8(), shot.png.clone()),
                };
                if let Err(e) =
                    std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, png))
                {
                    notes.push(format!("a frame could not be written: {e}"));
                    continue;
                }
                let path = path.to_string_lossy().into_owned();
                kept.push(hist);
                kept_paths.push(path.clone());
                let size = picture.dimensions();
                if want.crop.is_none() {
                    index.put_frame(vix::Seen {
                        t: shot.t,
                        path: Some(path.clone()),
                        size: Some(size),
                        ..Default::default()
                    });
                }
                out.frames.push(FrameRef {
                    t: shot.t,
                    path,
                    tokens: Some(look::patch_cost(size.0, size.1).1),
                    shown: None,
                    cut,
                });
                pictures.push(Cell {
                    t: shot.t,
                    img: picture,
                });
            }
        }

        // The text on each frame, read once and kept. A frame an earlier call read is not read
        // again; one the clock did not allow is left for the next call.
        let read_until = started + limit.mul_f32(0.8);
        if crate::read_text::available() && !pictures.is_empty() {
            let todo: Vec<(usize, image::RgbImage)> = out
                .frames
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    f.shown.is_none()
                        && !index
                            .frames
                            .iter()
                            .any(|s| s.path.as_deref() == Some(&f.path) && s.shown.is_some())
                })
                .map(|(i, _)| (i, pictures[i].img.clone()))
                .collect();
            let asked = todo.len();
            let read = tokio::task::spawn_blocking(move || {
                let mut done: Vec<(usize, String)> = Vec::new();
                for (i, img) in todo {
                    if Instant::now() >= read_until {
                        break;
                    }
                    let per_frame = (Instant::now() + Duration::from_secs(6)).min(read_until);
                    let text = crate::read_text::read_image(
                        &image::DynamicImage::ImageRgb8(img),
                        per_frame,
                    )
                    .unwrap_or_default();
                    done.push((i, text.trim().to_string()));
                }
                done
            })
            .await
            .unwrap_or_default();
            if read.len() < asked {
                notes.push(format!(
                    "the text on {} of {asked} frame(s) was not read before the time ran out; \
                     the same call again reads the rest",
                    asked - read.len()
                ));
            }
            for (i, text) in read {
                let f = &mut out.frames[i];
                if let Some(s) = index
                    .frames
                    .iter_mut()
                    .find(|s| s.path.as_deref() == Some(&f.path))
                {
                    s.shown = Some(text.clone());
                }
                f.shown = Some(text).filter(|t| !t.is_empty());
            }
        }
        // The same slide over ten frames is said once.
        let mut last: Option<String> = None;
        for f in &mut out.frames {
            if f.shown.is_some() && f.shown == last {
                f.shown = Some("(the same text as the frame before)".into());
            } else if f.shown.is_some() {
                last = f.shown.clone();
            }
        }

        if !pictures.is_empty() {
            seen.moments = pictures.len();
            // A handful of frames is read one by one; more than that is found on a sheet first.
            // Unless the frames are so small that the sheet would cost more than they do.
            if pictures.len() >= 4 && want.crop.is_none() {
                match sheet_refs(dir, &pictures, "frames") {
                    Ok(s) => {
                        let frames: u32 = out.frames.iter().filter_map(|f| f.tokens).sum();
                        if s.iter().map(|s| s.tokens).sum::<u32>() < frames {
                            out.sheets = s;
                        } else {
                            s.iter().for_each(|s| drop(std::fs::remove_file(&s.path)));
                        }
                    }
                    Err(e) => notes.push(format!("no sheet could be written: {e}")),
                }
            }
            out.seen = Some(seen);
        } else if !board.is_empty() && !want.off() && !want.live() && want.query.is_none() {
            // Nothing captured and nothing asked of the player: the whole video on sheets of
            // its own thumbnails, which costs a few requests and no playback.
            let duration = info.duration.unwrap_or_else(|| board.span());
            let over = Look {
                window: want.window,
                ..Look::default()
            };
            match vid::moments(&over, duration.max(0.1), spoken) {
                Ok(m) => {
                    let idx = board.nearest(&m.spread);
                    let cells = self.board_cells(&mut board, &idx).await;
                    if cells.len() < idx.len() {
                        notes.push(format!(
                            "{} of {} thumbnail(s) could not be fetched",
                            idx.len() - cells.len(),
                            idx.len()
                        ));
                    }
                    if !cells.is_empty() {
                        match sheet_refs(dir, &cells, "thumbs") {
                            Ok(s) => out.sheets = s,
                            Err(e) => notes.push(format!("no sheet could be written: {e}")),
                        }
                        out.seen = Some(Seen {
                            from: m.from,
                            to: m.to,
                            moments: cells.len(),
                            every: (cells.len() > 1).then(|| (m.to - m.from) / cells.len() as f64),
                            source: "thumbnails",
                            ..Seen::default()
                        });
                    }
                }
                Err(e) => notes.push(e),
            }
        } else if out.frames.is_empty() && !want.off() && !want.live() && want.query.is_none() {
            notes.push(
                "this player publishes no thumbnails, so nothing was shown: frames=N, or \
                 start/end, captures the video itself"
                    .into(),
            );
        }

        if let Some(query) = &want.query {
            let hits = self
                .video_search(
                    info, want, query, dir, &mut index, &mut board, cues, started, limit, notes,
                )
                .await;
            out.found = Some((query.clone(), hits));
        }
        if let Err(e) = index.save(dir) {
            notes.push(format!("what was seen could not be recorded: {e}"));
        }
        out
    }

    /// Where in the video something is: in what was said, in the text on the frames captured so
    /// far, and in the pictures themselves when the image-text model is installed.
    #[allow(clippy::too_many_arguments)]
    async fn video_search(
        &self,
        info: &VideoInfo,
        want: &Look,
        query: &str,
        dir: &std::path::Path,
        index: &mut Index,
        board: &mut Board,
        cues: &[Cue],
        started: Instant,
        limit: Duration,
        notes: &mut Vec<String>,
    ) -> Vec<vix::Hit> {
        let range = (want.window.start, want.window.end);
        let inside = |t: f64| t >= range.0.unwrap_or(0.0) && t < range.1.unwrap_or(f64::INFINITY);
        let passages =
            svipall_core::video::align::timeline(cues, &[], &[], vid::WINDOW_SECS, range);
        let mut hits = vix::text_hits(
            query,
            passages.iter().map(|s| (s.start, s.text.as_str(), None)),
            "said",
        );
        hits.truncate(HITS);
        let mut shown = vix::text_hits(
            query,
            index
                .frames
                .iter()
                .filter(|f| inside(f.t))
                .filter_map(|f| Some((f.t, f.shown.as_deref()?, f.path.as_deref()))),
            "shown",
        );
        shown.truncate(HITS);
        hits.extend(shown);

        if !crate::zeroshot::available() {
            notes.push(
                "searched what was said and the text on the frames captured so far; searching \
                 the pictures themselves needs the image-text model pair (docs/models.md)"
                    .into(),
            );
            return hits;
        }
        // The thumbnails the record has no vector for yet, spread over the video.
        let duration = info.duration.unwrap_or_else(|| board.span()).max(0.1);
        let idx = board.nearest(&look::spread(0.0, duration, SEARCH_CELLS));
        let missing: Vec<usize> = idx
            .into_iter()
            .filter(|i| {
                let t = board.frames[*i].t;
                !index
                    .cells
                    .iter()
                    .any(|c| (c.t - t).abs() < 0.01 && c.vec.is_some())
            })
            .collect();
        let cells = self.board_cells(board, &missing).await;
        let frames: Vec<(String, f64)> = index
            .frames
            .iter()
            .filter(|f| f.vec.is_none())
            .filter_map(|f| Some((f.path.clone()?, f.t)))
            .collect();
        let until = started + limit.mul_f32(0.92);
        let text = query.to_string();
        let (todo_cells, todo_frames) = (cells.len(), frames.len());
        let embedded = tokio::task::spawn_blocking(move || {
            let query = crate::zeroshot::embed_text(&text)?;
            let mut of_cells: Vec<(f64, Vec<f32>)> = Vec::new();
            let mut of_frames: Vec<(String, Vec<f32>)> = Vec::new();
            for c in cells {
                if Instant::now() >= until {
                    break;
                }
                let img = image::DynamicImage::ImageRgb8(c.img);
                of_cells.push((c.t, crate::zeroshot::embed_picture(&img)?));
            }
            for (path, _) in frames {
                if Instant::now() >= until {
                    break;
                }
                if let Ok(img) = image::open(&path) {
                    of_frames.push((path, crate::zeroshot::embed_picture(&img)?));
                }
            }
            anyhow::Ok((query, of_cells, of_frames))
        })
        .await;
        let (query_vec, of_cells, of_frames) = match embedded {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                notes.push(format!("the pictures could not be searched: {e:#}"));
                return hits;
            }
            Err(e) => {
                notes.push(format!("the pictures could not be searched: {e}"));
                return hits;
            }
        };
        let left = (todo_cells - of_cells.len()) + (todo_frames - of_frames.len());
        if left > 0 {
            notes.push(format!(
                "{left} picture(s) were not compared before the time ran out; the same search \
                 again compares the rest"
            ));
        }
        for (t, v) in of_cells {
            index.cells.push(vix::Seen {
                t,
                vec: Some(vix::pack(&v)),
                ..Default::default()
            });
        }
        for (path, v) in of_frames {
            if let Some(f) = index
                .frames
                .iter_mut()
                .find(|f| f.path.as_deref() == Some(&path))
            {
                f.vec = Some(vix::pack(&v));
            }
        }
        let pool: Vec<(f64, Vec<f32>, Option<String>)> = index
            .frames
            .iter()
            .chain(index.cells.iter())
            .filter(|s| inside(s.t))
            .filter_map(|s| Some((s.t, vix::unpack(s.vec.as_deref()?)?, s.path.clone())))
            .collect();
        if pool.is_empty() {
            notes.push(
                "there is no picture of this video to search yet: frames=N captures some".into(),
            );
            return hits;
        }
        let span = range.1.unwrap_or(duration) - range.0.unwrap_or(0.0);
        let (seen, clear) = vix::visual_hits(&query_vec, &pool, HITS, (span / 40.0).max(2.0));
        if !clear {
            notes.push(
                "no picture stands out for these words: the moments marked `seen` are the \
                 model's order and little more. It reads English, and scenes better than details"
                    .into(),
            );
        }
        // The found pictures on one sheet, so they can be judged at a glance.
        let found_idx = board.nearest(&seen.iter().map(|h| h.t).collect::<Vec<_>>());
        let found_cells = self.board_cells(board, &found_idx).await;
        if found_cells.len() >= 2 {
            match sheet_refs(dir, &found_cells, "found") {
                Ok(s) => notes.push(format!(
                    "the pictures found are on {}",
                    s.iter()
                        .map(|s| s.path.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                Err(e) => notes.push(format!("no sheet could be written: {e}")),
            }
        }
        hits.extend(seen);
        hits
    }
}

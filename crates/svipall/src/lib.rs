//! svipall library re-exports.

/// Reading a page from the isolated world, where the page cannot watch.
pub mod apart;
pub mod asr;
pub mod audio;
pub mod behavior;
pub mod blocklists;
/// A response body kept as bytes: in memory to be read, or streamed to the file asked for.
pub mod body;
pub mod browser;
pub mod capture;
pub mod detect;
/// What this build is, and whether it will work on this machine.
pub mod doctor;
pub mod grid;
/// A live challenge handed to a person at the dashboard and finished on the page.
pub mod handoff;
/// Answers for an AI harness's tool-call hooks.
pub mod hooks;
/// Work that outlives the request that asked for it.
pub mod jobs;
/// `json_path`: the fields of a JSON answer the caller asked for.
pub mod json_select;
/// Where a model comes from: the operator's file first, the embedded copy second.
pub mod model_install;
pub mod model_source;
pub mod ocr;
/// Tool arguments read against the tool's schema: lenient types, unknown keys named.
pub mod params;
/// What a tab found where a challenge was: the page, or the site's own refusal.
pub mod passage;
pub mod profiles;
/// How a long job says how it is going, to whoever is listening.
pub mod progress;
pub mod provision;
pub mod quality_cli;
pub mod read_text;
/// The same server over HTTP: one endpoint per tool, behind a bearer key.
pub mod rest;
pub mod search;
pub mod secrets;
/// The 4x4 single-picture grid: one segmentation, every cell the mask touches.
pub mod segment;
pub mod server;
pub mod settings;
pub mod slider;
pub mod snapshot;
pub mod solve_loop;
pub mod solver_engine;
pub mod steer;
pub mod substance;
pub mod surface;
/// The box a widget is waiting on: where it is, and whether the widget is still asking.
pub mod tick;
pub mod tools;
pub mod tunnels;
/// Explicit release checks and user-confirmed updates of the shared installation.
pub mod update;
pub mod video;
pub mod video_frames;
pub mod video_index;
pub mod video_sheet;
/// Slider and rotation captchas: geometry, no model.
pub mod vision;
pub mod wire;
pub mod zeroshot;

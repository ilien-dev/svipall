//! svipall-mcp library re-exports.

pub mod audio;
pub mod behavior;
pub mod blocklists;
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
/// Where a model comes from: the operator's file first, the embedded copy second.
pub mod model_source;
pub mod ocr;
pub mod profiles;
/// How a long job says how it is going, to whoever is listening.
pub mod progress;
pub mod provision;
pub mod quality_cli;
/// The same server over HTTP: one endpoint per tool, behind a bearer key.
pub mod rest;
pub mod search;
pub mod secrets;
/// The 4x4 single-picture grid: one segmentation, every cell the mask touches.
pub mod segment;
pub mod server;
pub mod slider;
pub mod snapshot;
pub mod solve_loop;
pub mod solver_engine;
pub mod substance;
pub mod tools;
/// Slider and rotation captchas: geometry, no model.
pub mod vision;
pub mod wire;
pub mod zeroshot;

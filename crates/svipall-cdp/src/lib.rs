//! The Chrome DevTools Protocol client svipall drives its browser tiers with.
//!
//! This is `chromiumoxide` 0.7.0, vendored. It is not a general-purpose fork and it is not
//! published: it exists so the automation residue the upstream client leaves in the page can be
//! removed, which is impossible from outside the crate. `UPSTREAM.md` records where the copy came
//! from and how to refresh it; `PATCHES.md` records every deviation and the test that covers it.
//!
//! What is *not* patched is worth stating, because it was the original reason to fork. Between
//! roughly 2022 and 2025 a page could detect any CDP client by planting a getter on a thrown
//! `Error`'s `stack` and passing it to `console.debug`: with the `Runtime` domain enabled, the
//! browser serialised the argument for the debugging client and the getter fired. Chrome changed
//! that serialisation path during 2025 and the probe went dark â measured against this browser
//! with the domain explicitly enabled, and guarded by a check in `svipall-bench fingerprint`.
//! Removing `Runtime.enable` would therefore buy nothing today while breaking execution-context
//! discovery, so it stays. If a future Chrome reopens the path, the bench is where it shows up.
//!
//! The generated protocol types still come from the published `chromiumoxide_cdp`, which ships
//! them pre-generated. Vendoring 99,000 lines of machine-written code to change none of it would
//! be all cost.

#![warn(missing_debug_implementations, rust_2018_idioms)]

use crate::handler::http::HttpRequest;
use std::sync::Arc;

/// reexport the generated cdp types
pub use chromiumoxide_cdp::cdp;
pub use chromiumoxide_types::{self as types, Binary, Command, Method, MethodType};

pub use crate::browser::{Browser, BrowserConfig};
pub use crate::conn::Connection;
pub use crate::element::Element;
pub use crate::error::Result;
pub use crate::handler::Handler;
pub use crate::page::Page;

pub mod async_process;
pub mod auth;
pub mod browser;
pub(crate) mod cmd;
pub mod conn;
pub mod detection;
pub mod element;
pub mod error;
pub mod handler;
pub mod js;
pub mod keys;
pub mod layout;
pub mod listeners;
pub mod page;
pub(crate) mod utils;
/// PATCH: the identity script a freshly attached worker is given before it runs.
pub mod worker;
/// PATCH: opaque, per-process names for the isolated world and its script URL.
pub mod world;

pub type ArcHttpRequest = Option<Arc<HttpRequest>>;

//! Choosing what on a page is the content.
//!
//! Everything here works on the DOM the fetch already parsed and returns node ids, never new
//! markup. That is the constraint the whole module is shaped by: `Html::parse_document` dominates
//! the cost of extraction, one parse per response is asserted by tests and by the perf budgets, and
//! every extraction crate that could be dropped in instead takes HTML and hands back HTML.

pub mod blocks;
pub mod candidates;
pub mod forum;
pub mod profile;
pub mod stats;
pub mod vote;

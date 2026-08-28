//! C1 render orchestrator (`render_rs`): mirror of the `render_only.py` flow
//! that reverses control — Rust orchestrates, Python only supplies the
//! prepare/page-specs "render bundle". Mode dispatch: `typst`/`typst_visual`
//! run the native background -> typst emit/compile -> save chain; every other
//! mode is rejected (C3 progressively takes over the remaining modes).

pub mod bundle;
pub mod delegate;
pub mod run;
pub mod spec;
pub mod stages;
pub mod summary;

pub use run::{run, RenderOutcome};

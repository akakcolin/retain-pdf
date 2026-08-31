//! C1 render orchestrator (`render_rs`): mirror of the `render_only.py` flow
//! that reverses control — Rust orchestrates, Python only supplies the
//! prepare/page-specs "render bundle". Mode dispatch: `typst`/`typst_visual`
//! run the native background -> typst emit/compile -> save chain; every other
//! mode is rejected (C3 progressively takes over the remaining modes).

pub mod bundle;
pub mod bundle_builder;
pub mod events;
pub mod extract_text_layer;
pub mod native_stats;
pub mod normalize;
pub mod protected_pages;
pub mod run;
pub mod spec;
pub mod stages;
pub mod summary;
pub mod translations;

pub use run::{run, RenderOutcome};

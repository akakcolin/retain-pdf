//! `render.bundle.v1` schema — the delegation hand-off produced by
//! `entrypoints/run_render_delegate.py` and consumed by the native stage chain.
//!
//! Two shape constraints (C1 plan):
//! - `translated_pages` are the ORIGINAL pre-page-spec enriched items — Rust
//!   `page_specs::apply_page_specs_and_fills` does the replacement and derives
//!   `formula_source_pages` from the un-replaced originals.
//! - `page_specs` are the FULL emitter dataclass serialization (mirror of
//!   `output/typst/_native.py::_page_spec_to_dict`), because the emitter-side
//!   `rendering_output::dto::RenderPageSpec` is the shape the typst source
//!   builder consumes. Each stage re-deserializes the raw `Value`s into its own
//!   DTO (`page_index`+`blocks` for the redaction chain, the full shape for the
//!   emitter); serde ignores the extra keys on the redaction side.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use rendering_output::dto::RenderBlock;
use rendering_writer::background::redaction::RedactionItem;
use serde::Deserialize;

pub const RENDER_BUNDLE_SCHEMA_VERSION: &str = "render.bundle.v1";

#[derive(Debug, Clone, Deserialize)]
pub struct RenderBundle {
    pub schema_version: String,
    pub mode: String,
    pub source_pdf: PathBuf,
    pub output_pdf: PathBuf,
    pub work_dir: PathBuf,
    pub font_family: String,
    #[serde(default)]
    pub redaction_strategy: Option<String>,
    #[serde(default)]
    pub precleaned_page_indices: Vec<i32>,
    #[serde(default)]
    pub visual_profile_fill_map: HashMap<String, [f64; 3]>,
    pub page_map: PageMap,
    pub translated_pages: BTreeMap<i32, Vec<RedactionItem>>,
    pub page_specs: Vec<serde_json::Value>,
    #[serde(default)]
    pub start_page: i32,
    #[serde(default)]
    pub end_page: i32,
    /// Overlay/dual bundle view: per-page geometry + RenderBlock DTO dicts
    /// (serialized by `output/typst/_native.py::_render_block_to_dict`), sizes
    /// from the original source. `None` for typst/typst_visual bundles.
    #[serde(default)]
    pub overlay_page_specs: Option<Vec<OverlayPageSpec>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OverlayPageSpec {
    pub page_index: i32,
    pub page_width_pt: f64,
    pub page_height_pt: f64,
    pub blocks: Vec<RenderBlock>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PageMap {
    #[serde(default)]
    pub source_page_indices: Vec<i32>,
}

impl RenderBundle {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let bundle: RenderBundle = serde_json::from_str(&text)?;
        if bundle.schema_version != RENDER_BUNDLE_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported render bundle schema_version: {} (expected {RENDER_BUNDLE_SCHEMA_VERSION})",
                bundle.schema_version
            );
        }
        Ok(bundle)
    }

    /// Redaction-chain view (`page_index` + `blocks` only).
    pub fn redaction_page_specs(&self) -> anyhow::Result<Vec<rendering_writer::background::redaction::page_specs::RenderPageSpec>> {
        let raw = serde_json::to_value(&self.page_specs)?;
        Ok(serde_json::from_value(raw)?)
    }

    /// Emitter view (full `rendering_output::dto::RenderPageSpec`).
    pub fn emitter_page_specs(&self) -> anyhow::Result<Vec<rendering_output::dto::RenderPageSpec>> {
        let raw = serde_json::to_value(&self.page_specs)?;
        Ok(serde_json::from_value(raw)?)
    }
}

// Port of `provider_adapters/paddle/context.py` — the per-page working context
// threaded through block/page building.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::block_labels::BlockKind;

/// `PaddlePageContext` — everything `build_block_spec` needs for one page.
pub struct PaddlePageContext {
    pub page_index: i64,
    pub page_payload: Value,
    pub page_meta: Value,
    pub preprocessed_image: String,
    pub pruned: Value,
    pub parsing_res_list: Vec<Value>,
    pub layout_box_lookup: Vec<(Vec<f64>, Value)>,
    pub markdown_text: String,
    pub markdown_images: Map<String, Value>,
    pub classified_kinds: Vec<BlockKind>,
    pub column_signals: Value,
    pub repair_metadata: BTreeMap<i64, Map<String, Value>>,
    pub repair_summary: Value,
}

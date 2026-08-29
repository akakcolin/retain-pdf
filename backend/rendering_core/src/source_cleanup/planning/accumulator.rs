//! Port of `planning/accumulator.py` — per-page plans → candidates.

use std::collections::BTreeMap;
use std::collections::HashSet;

use serde::Serialize;

use crate::source_cleanup::planning::rect_ops::rect_tuple;
use crate::source_cleanup::planning::PageCleanupFeatures;
use crate::source_cleanup::planning::BBoxTextStripPagePlan;
use crate::source_cleanup::planning::SkipReason;

/// Serialized form of `types.py::BBoxTextStripCandidates` (the fields the
/// accumulator produces).
#[derive(Debug, Clone, Serialize)]
pub struct BBoxTextStripCandidates {
    pub page_rects: BTreeMap<i64, Vec<[f64; 4]>>,
    pub page_protected_rects: BTreeMap<i64, Vec<[f64; 4]>>,
    pub uncovered_unsafe_vector_item_ids: Vec<String>,
    pub pages_skipped_complex: usize,
    pub pages_skipped_no_text_overlap: usize,
    pub pages_skipped_visual_background: usize,
    pub skipped_complex_page_indices: Vec<i64>,
    pub skipped_no_text_overlap_page_indices: Vec<i64>,
    pub skipped_visual_background_page_indices: Vec<i64>,
    pub page_features: BTreeMap<i64, PageCleanupFeatures>,
}

#[derive(Debug, Default)]
pub struct BBoxTextStripCandidateAccumulator {
    page_rects: BTreeMap<i64, Vec<[f64; 4]>>,
    page_protected_rects: BTreeMap<i64, Vec<[f64; 4]>>,
    skipped_complex_page_indices: HashSet<i64>,
    skipped_no_text_overlap_page_indices: HashSet<i64>,
    skipped_visual_background_page_indices: HashSet<i64>,
    uncovered_unsafe_vector_item_ids: HashSet<String>,
    page_features: BTreeMap<i64, PageCleanupFeatures>,
}

impl BBoxTextStripCandidateAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_page_features(&mut self, page_idx: i64, features: PageCleanupFeatures) {
        self.page_features.insert(page_idx, features);
    }

    pub fn add_page_plan(&mut self, page_idx: i64, page_plan: &BBoxTextStripPagePlan) {
        for item_id in &page_plan.uncovered_unsafe_vector_item_ids {
            self.uncovered_unsafe_vector_item_ids.insert(item_id.clone());
        }
        match page_plan.skip_reason {
            SkipReason::Complex => {
                self.skipped_complex_page_indices.insert(page_idx);
                return;
            }
            SkipReason::NoTextOverlap => {
                self.skipped_no_text_overlap_page_indices.insert(page_idx);
                return;
            }
            SkipReason::VisualBackground => {
                self.skipped_visual_background_page_indices.insert(page_idx);
                return;
            }
            SkipReason::None => {}
        }
        if page_plan.strip_rects.is_empty() {
            return;
        }
        self.page_rects.insert(
            page_idx,
            page_plan.strip_rects.iter().map(rect_tuple).collect(),
        );
        if !page_plan.protected_rects.is_empty() {
            self.page_protected_rects.insert(
                page_idx,
                page_plan.protected_rects.iter().map(rect_tuple).collect(),
            );
        }
    }

    pub fn build(&self) -> BBoxTextStripCandidates {
        let mut skipped_complex: Vec<i64> = self.skipped_complex_page_indices.iter().copied().collect();
        let mut skipped_no_text_overlap: Vec<i64> =
            self.skipped_no_text_overlap_page_indices.iter().copied().collect();
        let mut skipped_visual_background: Vec<i64> =
            self.skipped_visual_background_page_indices.iter().copied().collect();
        skipped_complex.sort_unstable();
        skipped_no_text_overlap.sort_unstable();
        skipped_visual_background.sort_unstable();
        let mut uncovered: Vec<String> = self.uncovered_unsafe_vector_item_ids.iter().cloned().collect();
        uncovered.sort_unstable();
        BBoxTextStripCandidates {
            page_rects: self.page_rects.clone(),
            page_protected_rects: self.page_protected_rects.clone(),
            uncovered_unsafe_vector_item_ids: uncovered,
            pages_skipped_complex: skipped_complex.len(),
            pages_skipped_no_text_overlap: skipped_no_text_overlap.len(),
            pages_skipped_visual_background: skipped_visual_background.len(),
            skipped_complex_page_indices: skipped_complex,
            skipped_no_text_overlap_page_indices: skipped_no_text_overlap,
            skipped_visual_background_page_indices: skipped_visual_background,
            page_features: self.page_features.clone(),
        }
    }
}

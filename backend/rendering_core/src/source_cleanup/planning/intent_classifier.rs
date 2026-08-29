//! Port of `planning/intent_classifier.py` — the ordered intent rules that
//! decide strip-vs-protect for a translated item.

use serde_json::Value;

use crate::source_cleanup::planning::evidence::SourceCleanupEvidence;
use crate::source_cleanup::planning::evidence::evidence_has_text_overlay;
use crate::source_cleanup::planning::formula_classifier::formula_text_has_latin_words;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceCleanupIntent {
    pub should_strip_text: bool,
    pub should_protect_source: bool,
}

const STRIP_TEXT: &str = "strip_text";
const PROTECT_SOURCE: &str = "protect_source";

/// `classify_source_cleanup_evidence` — first matching rule wins; the default
/// noop rule terminates otherwise.
pub fn classify_source_cleanup_evidence(evidence: &SourceCleanupEvidence) -> SourceCleanupIntent {
    let is_text = evidence.block_kind == "text";
    let has_overlay = evidence_has_text_overlay(evidence);
    let has_latin = formula_text_has_latin_words(&evidence.item);

    if is_text && has_overlay && evidence.is_force_strip_text {
        return intent(STRIP_TEXT);
    }
    if is_text && has_overlay && evidence.has_unresolved_embedded_formula {
        return intent(PROTECT_SOURCE);
    }
    if is_text && has_overlay {
        return intent(STRIP_TEXT);
    }
    if is_text {
        return intent("noop");
    }
    if evidence.has_formula_region && has_latin && has_overlay {
        return intent(STRIP_TEXT);
    }
    if evidence.has_formula_region && has_latin {
        return intent(PROTECT_SOURCE);
    }
    if evidence.has_formula_region {
        return intent(PROTECT_SOURCE);
    }
    intent("noop")
}

fn intent(cleanup_action: &str) -> SourceCleanupIntent {
    match cleanup_action {
        STRIP_TEXT => SourceCleanupIntent { should_strip_text: true, should_protect_source: false },
        PROTECT_SOURCE => SourceCleanupIntent { should_strip_text: false, should_protect_source: true },
        _ => SourceCleanupIntent { should_strip_text: false, should_protect_source: false },
    }
}

/// `classify_source_cleanup_intent(item)` — evidence from the raw item dict.
pub fn classify_source_cleanup_intent(item: &Value) -> SourceCleanupIntent {
    let evidence = crate::source_cleanup::planning::evidence::build_source_cleanup_evidence(item);
    classify_source_cleanup_evidence(&evidence)
}

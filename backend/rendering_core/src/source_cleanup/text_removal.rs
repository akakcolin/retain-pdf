//! Port of backend/scripts/services/rendering/source_cleanup/pdf/text_removal.py.

use super::hit_test::{is_protected_text_op, RectIndex, RectTuple};
use super::pdf_math::{Operand, PdfMatrix};
use super::text_ops::{
    estimated_user_text_geometry, text_operand_metrics, TextOperandMetrics, TextState,
};

#[derive(Debug, Clone, PartialEq)]
pub struct TextShowRewriteDecision {
    pub text_metrics: TextOperandMetrics,
    pub user_point: (f64, f64),
    pub text_rect: RectTuple,
    pub remove: bool,
}

/// `decide_text_show_rewrite(operands, ctm, text_matrix, text_state,
/// strip_index, protected_index)`.
pub fn decide_text_show_rewrite(
    operands: &[Operand],
    ctm: &PdfMatrix,
    text_matrix: &PdfMatrix,
    text_state: &TextState,
    strip_index: &RectIndex,
    protected_index: &RectIndex,
) -> TextShowRewriteDecision {
    let text_metrics = text_operand_metrics(operands);
    let (user_point, text_rect) =
        estimated_user_text_geometry(ctm, text_matrix, text_state, text_metrics.chars);
    let remove = strip_index.matches_text_for_removal(user_point.0, user_point.1, &text_rect)
        && !is_protected_text_op(user_point, &text_rect, None, Some(protected_index));
    TextShowRewriteDecision {
        text_metrics,
        user_point,
        text_rect,
        remove,
    }
}

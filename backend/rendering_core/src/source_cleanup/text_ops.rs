//! Port of backend/scripts/services/rendering/source_cleanup/pdf/text_ops.py.

use super::hit_test::RectTuple;
use super::pdf_math::{matrix_point, to_float, Operand, PdfMatrix};

pub const TEXT_SHOW_OPERATORS: [&str; 4] = ["Tj", "TJ", "'", "\""];
pub const DEFAULT_TEXT_ADVANCE_PT: f64 = 18.0;
pub const MIN_TEXT_BOX_HEIGHT_PT: f64 = 2.0;
pub const TEXT_DEFAULT_RENDER_MODE: i64 = 0;
pub const DEFAULT_GLYPH_WIDTH_EM: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextState {
    pub font_size: f64,
    pub char_spacing: f64,
    pub word_spacing: f64,
    pub horizontal_scaling: f64,
    pub rise: f64,
    pub render_mode: i64,
}

impl Default for TextState {
    fn default() -> Self {
        TextState {
            font_size: 12.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scaling: 1.0,
            rise: 0.0,
            render_mode: TEXT_DEFAULT_RENDER_MODE,
        }
    }
}

impl TextState {
    /// `TextState.copy()`.
    pub fn copy(&self) -> TextState {
        *self
    }

    /// `set_font_size` — clamp to >= 0.1pt.
    pub fn set_font_size(&mut self, font_size: f64) {
        self.font_size = font_size.max(0.1);
    }

    pub fn set_char_spacing(&mut self, char_spacing: f64) {
        self.char_spacing = char_spacing;
    }

    pub fn set_word_spacing(&mut self, word_spacing: f64) {
        self.word_spacing = word_spacing;
    }

    /// `set_horizontal_scaling` — percent to ratio, clamped to >= 0.01.
    pub fn set_horizontal_scaling(&mut self, percent: f64) {
        self.horizontal_scaling = (percent / 100.0).max(0.01);
    }

    pub fn set_rise(&mut self, rise: f64) {
        self.rise = rise;
    }

    pub fn set_render_mode(&mut self, render_mode: i64) {
        self.render_mode = render_mode;
    }
}

/// `TextOperandMetrics = (chars, spaces, adjustment)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextOperandMetrics {
    pub chars: i64,
    pub spaces: i64,
    pub adjustment: f64,
}

/// `text_operand_length(operands)`.
pub fn text_operand_length(operands: &[Operand]) -> i64 {
    text_operand_metrics(operands).chars
}

/// `text_operand_profile(operands)`.
pub fn text_operand_profile(operands: &[Operand]) -> TextOperandMetrics {
    text_operand_metrics(operands)
}

/// `text_operand_metrics(operands)` — metrics of the last operand.
pub fn text_operand_metrics(operands: &[Operand]) -> TextOperandMetrics {
    if operands.is_empty() {
        return TextOperandMetrics {
            chars: 0,
            spaces: 0,
            adjustment: 0.0,
        };
    }
    let value = if operands.len() > 1 {
        &operands[operands.len() - 1]
    } else {
        &operands[0]
    };
    value_text_metrics(value)
}

/// `text_advance_tx(text_matrix, operands, text_length, text_metrics,
/// text_state)` — estimated advance in text space, clamped to >= 1pt.
pub fn text_advance_tx(
    text_matrix: &PdfMatrix,
    operands: &[Operand],
    text_length: Option<i64>,
    text_metrics: Option<&TextOperandMetrics>,
    text_state: Option<&TextState>,
) -> f64 {
    let state = match text_state {
        Some(state) => *state,
        None => TextState {
            font_size: text_matrix.idx(0).abs().max(1.0),
            ..TextState::default()
        },
    };
    let metrics = match text_metrics {
        Some(metrics) => *metrics,
        None => text_operand_metrics(operands),
    };
    let text_length = match text_length {
        Some(n) => n,
        None => metrics.chars,
    };
    let glyph_advance = text_length as f64 * state.font_size * DEFAULT_GLYPH_WIDTH_EM;
    let spacing_advance =
        text_length as f64 * state.char_spacing + metrics.spaces as f64 * state.word_spacing;
    let adjustment_advance = -metrics.adjustment * state.font_size / 1000.0;
    let tx = (glyph_advance + spacing_advance + adjustment_advance) * state.horizontal_scaling;
    tx.max(0.0).max(1.0)
}

/// `estimated_text_rect(matrix, text_length, text_state)` — text-space bbox.
pub fn estimated_text_rect(
    matrix: &PdfMatrix,
    text_length: i64,
    text_state: Option<&TextState>,
) -> RectTuple {
    let (x, y) = matrix_point(matrix);
    let state = match text_state {
        Some(state) => *state,
        None => TextState {
            font_size: matrix
                .idx(3)
                .abs()
                .max(matrix.idx(1).abs())
                .max(MIN_TEXT_BOX_HEIGHT_PT),
            ..TextState::default()
        },
    };
    let font_height = matrix
        .idx(3)
        .abs()
        .max(matrix.idx(1).abs())
        .max(state.font_size)
        .max(MIN_TEXT_BOX_HEIGHT_PT);
    let char_width =
        (state.font_size * state.horizontal_scaling * DEFAULT_GLYPH_WIDTH_EM).max(1.0);
    let width = char_width.max(char_width * text_length.max(1) as f64);
    [x, y - font_height * 0.35, x + width, y + font_height * 1.05]
}

/// `estimated_user_text_geometry(ctm, text_matrix, text_state, text_length)` —
/// returns the user-space origin and estimated text bbox.
pub fn estimated_user_text_geometry(
    ctm: &PdfMatrix,
    text_matrix: &PdfMatrix,
    text_state: &TextState,
    text_length: i64,
) -> ((f64, f64), RectTuple) {
    let [a, b, c, d, e, f] = text_matrix.0;
    let font_width = text_state.font_size * text_state.horizontal_scaling;
    let font_height = text_state.font_size;
    let rise = text_state.rise;
    let user_a = ctm.idx(0) * (a * font_width) + ctm.idx(2) * (b * font_width);
    let user_b = ctm.idx(1) * (a * font_width) + ctm.idx(3) * (b * font_width);
    let user_c = ctm.idx(0) * (c * font_height) + ctm.idx(2) * (d * font_height);
    let user_d = ctm.idx(1) * (c * font_height) + ctm.idx(3) * (d * font_height);
    let user_x = ctm.idx(0) * (c * rise + e) + ctm.idx(2) * (d * rise + f) + ctm.idx(4);
    let user_y = ctm.idx(1) * (c * rise + e) + ctm.idx(3) * (d * rise + f) + ctm.idx(5);
    let rect = estimated_text_rect(
        &PdfMatrix([user_a, user_b, user_c, user_d, user_x, user_y]),
        text_length,
        Some(text_state),
    );
    ((user_x, user_y), rect)
}

/// `_value_text_metrics(value)` — metrics of a single text-show operand.
///
/// Python's `str(pikepdf.String(b))` is a latin-1 decode, so `len` is the byte
/// count and spaces count `0x20` bytes. `Bytes` counts raw bytes to match;
/// `Str` counts chars (identical for ASCII, which the corpus exercises).
fn value_text_metrics(value: &Operand) -> TextOperandMetrics {
    match value {
        Operand::Str(text) => TextOperandMetrics {
            chars: text.chars().count() as i64,
            spaces: text.matches(' ').count() as i64,
            adjustment: 0.0,
        },
        Operand::Bytes(bytes) => TextOperandMetrics {
            chars: bytes.len() as i64,
            spaces: bytes.iter().filter(|&&b| b == b' ').count() as i64,
            adjustment: 0.0,
        },
        Operand::Array(items) => {
            let mut chars = 0;
            let mut spaces = 0;
            let mut adjustment = 0.0;
            for item in items {
                match item {
                    Operand::Str(text) => {
                        chars += text.chars().count() as i64;
                        spaces += text.matches(' ').count() as i64;
                    }
                    Operand::Bytes(bytes) => {
                        chars += bytes.len() as i64;
                        spaces += bytes.iter().filter(|&&b| b == b' ').count() as i64;
                    }
                    _ => adjustment += to_float(item, 0.0),
                }
            }
            TextOperandMetrics {
                chars,
                spaces,
                adjustment,
            }
        }
        _ => TextOperandMetrics {
            chars: 1,
            spaces: 0,
            adjustment: 0.0,
        },
    }
}

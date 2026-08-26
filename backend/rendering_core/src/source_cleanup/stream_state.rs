//! Port of backend/scripts/services/rendering/source_cleanup/pdf/stream_state.py.
//!
//! The `_STATE_HANDLERS` table (15 ops) becomes a `match` in
//! `apply_state_operator`; q/Q/cm/BT/Tm/Td/TD/TL/Tf/Tc/Tw/Tz/Ts/Tr/T*.

use super::pdf_math::{matrix_from_operands, mul_matrix, to_float, Operand, PdfMatrix, IDENTITY_MATRIX};
use super::text_ops::{text_advance_tx, TextOperandMetrics, TextState, TEXT_DEFAULT_RENDER_MODE};

/// Mutable PDF content-stream graphics/text state tracker.
#[derive(Debug, Clone)]
pub struct ContentStreamState {
    pub ctm: PdfMatrix,
    pub text_matrix: PdfMatrix,
    pub line_matrix: PdfMatrix,
    pub leading: f64,
    pub text_state: TextState,
    ctm_stack: Vec<PdfMatrix>,
    text_state_stack: Vec<TextState>,
}

impl Default for ContentStreamState {
    fn default() -> Self {
        ContentStreamState {
            ctm: IDENTITY_MATRIX,
            text_matrix: IDENTITY_MATRIX,
            line_matrix: IDENTITY_MATRIX,
            leading: 0.0,
            text_state: TextState::default(),
            ctm_stack: Vec::new(),
            text_state_stack: Vec::new(),
        }
    }
}

impl ContentStreamState {
    /// `apply_state_operator(op, operands)` — dispatch a state operator; false
    /// when the op is not a tracked state op.
    pub fn apply_state_operator(&mut self, op: &str, operands: &[Operand]) -> bool {
        match op {
            "q" => self.push_graphics_state(),
            "Q" => self.pop_graphics_state(),
            "cm" => self.concat_matrix(operands),
            "BT" => self.begin_text(),
            "Tm" => self.set_text_matrix(operands),
            "Td" => self.move_text_from_operands(operands),
            "TD" => self.set_leading_and_move_text(operands),
            "TL" => self.set_leading(operands),
            "Tf" => self.set_font(operands),
            "Tc" => self.set_char_spacing(operands),
            "Tw" => self.set_word_spacing(operands),
            "Tz" => self.set_horizontal_scaling(operands),
            "Ts" => self.set_rise(operands),
            "Tr" => self.set_render_mode(operands),
            "T*" => self.next_line(),
            _ => return false,
        }
        true
    }

    /// `prepare_quote_text_show(op, operands)` — `"` sets word/char spacing then
    /// moves to the next line.
    pub fn prepare_quote_text_show(&mut self, op: &str, operands: &[Operand]) {
        if op == "\"" && operands.len() >= 3 {
            let word_default = self.text_state.word_spacing;
            let char_default = self.text_state.char_spacing;
            self.text_state
                .set_word_spacing(to_float(&operands[0], word_default));
            self.text_state
                .set_char_spacing(to_float(&operands[1], char_default));
        }
        self.move_text(0.0, -self.leading);
    }

    /// `advance_text(operands, text_metrics)` — advance the text matrix.
    pub fn advance_text(&mut self, operands: &[Operand], text_metrics: Option<&TextOperandMetrics>) {
        let tx = text_advance_tx(
            &self.text_matrix,
            operands,
            None,
            text_metrics,
            Some(&self.text_state),
        );
        self.text_matrix = mul_matrix(&self.text_matrix, &PdfMatrix([1.0, 0.0, 0.0, 1.0, tx, 0.0]));
    }

    /// `move_text(tx, ty)`.
    pub fn move_text(&mut self, tx: f64, ty: f64) {
        let move_mat = PdfMatrix([1.0, 0.0, 0.0, 1.0, tx, ty]);
        self.line_matrix = mul_matrix(&self.line_matrix, &move_mat);
        self.text_matrix = self.line_matrix;
    }

    /// `push_graphics_state`.
    pub fn push_graphics_state(&mut self) {
        self.ctm_stack.push(self.ctm);
        self.text_state_stack.push(self.text_state.copy());
    }

    /// `pop_graphics_state`.
    pub fn pop_graphics_state(&mut self) {
        self.ctm = self.ctm_stack.pop().unwrap_or(IDENTITY_MATRIX);
        self.text_state = self.text_state_stack.pop().unwrap_or_default();
    }

    /// `concat_matrix(operands)` — `cm`.
    pub fn concat_matrix(&mut self, operands: &[Operand]) {
        if let Some(matrix) = matrix_from_operands(operands) {
            self.ctm = mul_matrix(&self.ctm, &matrix);
        }
    }

    /// `begin_text` — `BT`.
    pub fn begin_text(&mut self) {
        self.text_matrix = IDENTITY_MATRIX;
        self.line_matrix = self.text_matrix;
    }

    /// `set_text_matrix` — `Tm`.
    pub fn set_text_matrix(&mut self, operands: &[Operand]) {
        if let Some(matrix) = matrix_from_operands(operands) {
            self.text_matrix = matrix;
            self.line_matrix = matrix;
        }
    }

    /// `move_text_from_operands` — `Td`.
    pub fn move_text_from_operands(&mut self, operands: &[Operand]) {
        if operands.len() < 2 {
            return;
        }
        self.move_text(to_float(&operands[0], 0.0), to_float(&operands[1], 0.0));
    }

    /// `set_leading_and_move_text` — `TD`.
    pub fn set_leading_and_move_text(&mut self, operands: &[Operand]) {
        if operands.len() < 2 {
            return;
        }
        let ty = to_float(&operands[1], 0.0);
        self.leading = -ty;
        self.move_text(to_float(&operands[0], 0.0), ty);
    }

    /// `set_leading` — `TL`.
    pub fn set_leading(&mut self, operands: &[Operand]) {
        if !operands.is_empty() {
            self.leading = to_float(&operands[0], 0.0);
        }
    }

    /// `set_font` — `Tf` (operand[1] is the font size).
    pub fn set_font(&mut self, operands: &[Operand]) {
        if operands.len() >= 2 {
            let default = self.text_state.font_size;
            self.text_state.set_font_size(to_float(&operands[1], default));
        }
    }

    pub fn set_char_spacing(&mut self, operands: &[Operand]) {
        if !operands.is_empty() {
            let default = self.text_state.char_spacing;
            self.text_state.set_char_spacing(to_float(&operands[0], default));
        }
    }

    pub fn set_word_spacing(&mut self, operands: &[Operand]) {
        if !operands.is_empty() {
            let default = self.text_state.word_spacing;
            self.text_state.set_word_spacing(to_float(&operands[0], default));
        }
    }

    pub fn set_horizontal_scaling(&mut self, operands: &[Operand]) {
        if !operands.is_empty() {
            let default = self.text_state.horizontal_scaling * 100.0;
            self.text_state.set_horizontal_scaling(to_float(&operands[0], default));
        }
    }

    pub fn set_rise(&mut self, operands: &[Operand]) {
        if !operands.is_empty() {
            let default = self.text_state.rise;
            self.text_state.set_rise(to_float(&operands[0], default));
        }
    }

    pub fn set_render_mode(&mut self, operands: &[Operand]) {
        if !operands.is_empty() {
            self.text_state
                .set_render_mode(to_float(&operands[0], TEXT_DEFAULT_RENDER_MODE as f64) as i64);
        }
    }

    /// `next_line` — `T*`.
    pub fn next_line(&mut self) {
        self.move_text(0.0, -self.leading);
    }
}

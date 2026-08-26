//! Port of backend/scripts/services/rendering/source_cleanup/pdf/pdf_math.py.
//!
//! `Operand` is the normalized form of a pikepdf content-stream operand: numbers,
//! decoded text strings, names, and TJ arrays (mixed text + numeric
//! adjustments). The source_cleanup decision modules consume only this enum,
//! never pikepdf.

/// A normalized content-stream operand.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// Numeric operand (PDF number, or a TJ adjustment in points).
    Num(f64),
    /// Decoded text-string operand (`Tj` value, or a TJ array string item).
    Str(String),
    /// Raw-byte text-string operand (literal `(...)` or hex `<...>`), kept
    /// byte-exact so a rewritten content stream round-trips losslessly (CJK
    /// UTF-16BE strings are not corrupted).
    Bytes(Vec<u8>),
    /// PDF name operand (e.g. the font resource name in `Tf`).
    Name(String),
    /// `TJ` array operand: interleaved strings and numeric adjustments.
    Array(Vec<Operand>),
}

/// A PDF transformation matrix as the six-tuple (a, b, c, d, e, f).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfMatrix(pub [f64; 6]);

impl PdfMatrix {
    /// Indexed access (`matrix[i]`), 0-based over the six tuple elements.
    pub fn idx(&self, i: usize) -> f64 {
        self.0[i]
    }
}

pub const IDENTITY_MATRIX: PdfMatrix = PdfMatrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

/// `mul_matrix(left, right)` — matrix concatenation (left applied first).
pub fn mul_matrix(left: &PdfMatrix, right: &PdfMatrix) -> PdfMatrix {
    let [a, b, c, d, e, f] = left.0;
    let [g, h, i, j, k, l] = right.0;
    PdfMatrix([
        a * g + c * h,
        b * g + d * h,
        a * i + c * j,
        b * i + d * j,
        a * k + c * l + e,
        b * k + d * l + f,
    ])
}

/// `matrix_point(matrix)` — the translation component (e, f).
pub fn matrix_point(matrix: &PdfMatrix) -> (f64, f64) {
    (matrix.0[4], matrix.0[5])
}

/// `transform_point(matrix, x, y)` — apply the affine transform.
pub fn transform_point(matrix: &PdfMatrix, x: f64, y: f64) -> (f64, f64) {
    let [a, b, c, d, e, f] = matrix.0;
    (a * x + c * y + e, b * x + d * y + f)
}

/// `invert_matrix(matrix)` — inverse of the affine transform.
///
/// Returns `None` when the linear part is singular (|det| ~ 0).
pub fn invert_matrix(matrix: &PdfMatrix) -> Option<PdfMatrix> {
    let [a, b, c, d, e, f] = matrix.0;
    let det = a * d - b * c;
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    Some(PdfMatrix([
        d * inv,
        -b * inv,
        -c * inv,
        a * inv,
        (c * f - d * e) * inv,
        (b * e - a * f) * inv,
    ]))
}

/// `transform_rect(matrix, rect)` — the axis-aligned bbox of `rect` transformed
/// by `matrix` (transform all four corners, take the min/max box).
pub fn transform_rect(matrix: &PdfMatrix, rect: &[f64; 4]) -> [f64; 4] {
    let (x0, y0) = transform_point(matrix, rect[0], rect[1]);
    let (x1, y0b) = transform_point(matrix, rect[2], rect[1]);
    let (x1b, y1) = transform_point(matrix, rect[2], rect[3]);
    let (x0b, y1b) = transform_point(matrix, rect[0], rect[3]);
    [
        x0.min(x1).min(x1b).min(x0b),
        y0.min(y0b).min(y1).min(y1b),
        x0.max(x1).max(x1b).max(x0b),
        y0.max(y0b).max(y1).max(y1b),
    ]
}

/// `to_float(value, default=0.0)` — numeric operand value, else default.
pub fn to_float(value: &Operand, default: f64) -> f64 {
    match value {
        Operand::Num(n) => *n,
        Operand::Str(s) => s.trim().parse().unwrap_or(default),
        Operand::Bytes(b) => String::from_utf8_lossy(b).trim().parse().unwrap_or(default),
        _ => default,
    }
}

/// `matrix_from_operands(operands)` — first six numeric operands, or None.
pub fn matrix_from_operands(operands: &[Operand]) -> Option<PdfMatrix> {
    if operands.len() < 6 {
        return None;
    }
    let mut out = [0.0; 6];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = to_float(&operands[i], 0.0);
    }
    Some(PdfMatrix(out))
}

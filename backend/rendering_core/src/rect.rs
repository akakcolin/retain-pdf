// Data shape mirroring fitz.Rect, used by the profile collectors
// (services/rendering/analysis/profile/rect_area.py + page geometry) and the
// source-cleanup planning port (services/rendering/source_cleanup/planning).
//
// Method semantics match `services/rendering/source/rects.py` (which mirrors
// fitz.Rect exactly): `is_empty` = width<=0 or height<=0, `intersect` =
// max/max/min/min, `union` = min/min/max/max with empty special-casing,
// `transformed` = four-corner affine bbox.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

// PyMuPDF's infinite-rect constants; `fitz.Rect.is_infinite` is True exactly
// when x0 == y0 == FZ_MIN_INF_RECT and x1 == y1 == FZ_MAX_INF_RECT.
pub const FZ_MIN_INF_RECT: f64 = -2147483648.0;
pub const FZ_MAX_INF_RECT: f64 = 2147483520.0;

impl Rect {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Rect { x0, y0, x1, y1 }
    }

    /// Identity-like empty rect (fitz `Rect()`), all zeros.
    pub fn empty() -> Self {
        Rect::new(0.0, 0.0, 0.0, 0.0)
    }

    /// `rect.width` — PyMuPDF reports x1 - x0.
    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// `rect_area(rect)` = max(0, x1-x0) * max(0, y1-y0).
    pub fn area(&self) -> f64 {
        (self.x1 - self.x0).max(0.0) * (self.y1 - self.y0).max(0.0)
    }

    /// `fitz.Rect.is_empty` — x0 >= x1 or y0 >= y1.
    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }

    /// `fitz.Rect.is_infinite` — the exact PyMuPDF infinite rect.
    pub fn is_infinite(&self) -> bool {
        self.x0 == self.y0 && self.x0 == FZ_MIN_INF_RECT && self.x1 == self.y1 && self.x1 == FZ_MAX_INF_RECT
    }

    /// `rect & other` — PyMuPDF intersection (clamped rect, possibly empty).
    pub fn intersect(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    /// `rect | other` — PyMuPDF union with empty special-casing: an empty
    /// operand returns the other operand unchanged.
    pub fn union(&self, other: &Rect) -> Rect {
        if other.is_empty() {
            return *self;
        }
        if self.is_empty() {
            return *other;
        }
        Rect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// `rect.include_rect(other)` — min/min/max/max with no empty special-casing.
    pub fn include(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// `rect + (dx0, dy0, dx1, dy1)` — per-side padding.
    pub fn padded(&self, dx0: f64, dy0: f64, dx1: f64, dy1: f64) -> Rect {
        Rect {
            x0: self.x0 + dx0,
            y0: self.y0 + dy0,
            x1: self.x1 + dx1,
            y1: self.y1 + dy1,
        }
    }

    /// `rect.intersects(other)` — strict coordinate overlap with
    /// is_empty/is_infinite guards (NOT `!(self & other).is_empty()`, which
    /// disagrees on zero-area sliver intersections).
    pub fn intersects(&self, other: &Rect) -> bool {
        !self.is_empty()
            && !self.is_infinite()
            && !other.is_empty()
            && !other.is_infinite()
            && self.x0 < other.x1
            && other.x0 < self.x1
            && self.y0 < other.y1
            && other.y0 < self.y1
    }

    /// `rect * matrix` — affine transform: the bounding box of all four corners,
    /// matching `fitz.Rect.__mul__` exactly (four-corner bbox, no empty
    /// special-casing).
    pub fn transformed(&self, m: &Matrix) -> Rect {
        let x0 = self.x0;
        let y0 = self.y0;
        let x1 = self.x1;
        let y1 = self.y1;
        let xs = [
            m.a * x0 + m.c * y0 + m.e,
            m.a * x1 + m.c * y0 + m.e,
            m.a * x0 + m.c * y1 + m.e,
            m.a * x1 + m.c * y1 + m.e,
        ];
        let ys = [
            m.b * x0 + m.d * y0 + m.f,
            m.b * x1 + m.d * y0 + m.f,
            m.b * x0 + m.d * y1 + m.f,
            m.b * x1 + m.d * y1 + m.f,
        ];
        let xmin = xs.iter().cloned().fold(f64::INFINITY, f64::min);
        let xmax = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let ymin = ys.iter().cloned().fold(f64::INFINITY, f64::min);
        let ymax = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        Rect::new(xmin, ymin, xmax, ymax)
    }
}

/// Pure 2D affine matrix duck-compatible with `fitz.Matrix` (`a`..`f`
/// attributes, default identity), mirroring
/// `services/rendering/source/rects.py::Matrix`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Matrix {
    pub fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Matrix { a, b, c, d, e, f }
    }

    /// Identity matrix, matching `fitz.Matrix()`.
    pub fn identity() -> Self {
        Matrix::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
    }

    /// Pure inverse of a 2D affine matrix, matching `~fitz.Matrix` numerically
    /// (`services/rendering/source/rects.py::inverse_affine`).
    pub fn inverse(&self) -> Matrix {
        let det = self.a * self.d - self.b * self.c;
        Matrix::new(
            self.d / det,
            -self.b / det,
            -self.c / det,
            self.a / det,
            (self.c * self.f - self.d * self.e) / det,
            (self.b * self.e - self.a * self.f) / det,
        )
    }
}

impl Default for Matrix {
    fn default() -> Self {
        Matrix::identity()
    }
}

impl Default for Rect {
    fn default() -> Self {
        Rect::empty()
    }
}

/// `rect_key(rect)` — the (rounded) int tuple key used for dedup, matching
/// `services/rendering/source/rects.py::rect_key`.
pub fn rect_key(rect: &Rect) -> (i64, i64, i64, i64) {
    (
        round_ties_even(rect.x0 * 10.0) as i64,
        round_ties_even(rect.y0 * 10.0) as i64,
        round_ties_even(rect.x1 * 10.0) as i64,
        round_ties_even(rect.y1 * 10.0) as i64,
    )
}

/// `round(x, n)` with Python's correctly-rounded ties-to-even semantics (the
/// modern CPython float path). CPython rounds the exact binary value against
/// the decimal grid, so multiply-then-round (which can land on a spurious .5
/// tie, e.g. `round(2.675, 2) == 2.67`) is wrong; we instead scale the exact
/// mantissa in integer arithmetic and round half-to-even there.
pub fn round_to_digits(x: f64, digits: i32) -> f64 {
    if !x.is_finite() {
        return x;
    }
    if x == 0.0 || x.abs() >= 1e16 {
        return x;
    }
    let bits = x.to_bits();
    let sign = if bits >> 63 == 1 { -1.0 } else { 1.0 };
    let exp_bits = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & 0xfffffffffffff;
    let (mantissa, exponent) = if exp_bits == 0 {
        (frac as i128, -1074i32)
    } else {
        ((frac | (1u64 << 52)) as i128, exp_bits - 1075)
    };
    if exponent < -120 {
        return sign * 0.0;
    }
    let scale = 10i128.pow(digits.max(0) as u32);
    let numerator = mantissa * scale;
    let rounded: i128 = if exponent >= 0 {
        numerator << exponent
    } else {
        let denom = 1i128 << (-exponent);
        let quotient = numerator / denom;
        let remainder = numerator % denom;
        if remainder * 2 < denom {
            quotient
        } else if remainder * 2 > denom {
            quotient + 1
        } else if quotient % 2 == 0 {
            quotient
        } else {
            quotient + 1
        }
    };
    sign * (rounded as f64) / (scale as f64)
}

/// `int(round(x))` — nearest int with ties to even, matching Python's float
/// `round(x)` for the integer-key paths.
pub fn round_ties_even(x: f64) -> f64 {
    x.round_ties_even()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_matches_python() {
        let a = Rect::new(1.0, 2.0, 3.0, 4.0);
        let b = Rect::new(0.0, 1.0, 5.0, 6.0);
        let u = a.union(&b);
        assert_eq!(u, Rect::new(0.0, 1.0, 5.0, 6.0));
        // empty special-casing
        let empty = Rect::empty();
        assert_eq!(empty.union(&b), b);
        assert_eq!(a.union(&empty), a);
    }

    #[test]
    fn transformed_matches_four_corner_bbox() {
        let m = Matrix::new(1.0, 0.0, 0.0, 1.0, 10.0, 20.0);
        let r = Rect::new(0.0, 0.0, 5.0, 5.0);
        assert_eq!(r.transformed(&m), Rect::new(10.0, 20.0, 15.0, 25.0));
    }

    #[test]
    fn inverse_matches_reference() {
        // ~fitz.Matrix(0, 1, -1, 0, 100, 200) via the pure formula
        let m = Matrix::new(0.0, 1.0, -1.0, 0.0, 100.0, 200.0);
        let inv = m.inverse();
        // det = 0*0 - 1*(-1) = 1; e' = (c*f - d*e)/det, f' = (b*e - a*f)/det
        assert!((inv.a - 0.0).abs() < 1e-12);
        assert!((inv.b - (-1.0)).abs() < 1e-12);
        assert!((inv.c - 1.0).abs() < 1e-12);
        assert!((inv.d - 0.0).abs() < 1e-12);
        assert!((inv.e - (-200.0)).abs() < 1e-12);
        assert!((inv.f - 100.0).abs() < 1e-12);
    }

    #[test]
    fn round_ties_even_matches_python() {
        assert_eq!(round_to_digits(2.5, 0), 2.0);
        assert_eq!(round_to_digits(3.5, 0), 4.0);
        assert_eq!(round_to_digits(2.675, 2), 2.67);
        assert_eq!(round_to_digits(-2.675, 2), -2.67);
        assert_eq!(round_to_digits(123.456, 2), 123.46);
        assert_eq!(round_to_digits(0.5, 0), 0.0);
        assert_eq!(round_to_digits(1.25, 1), 1.2);
        assert_eq!(round_ties_even(2.5), 2.0);
        assert_eq!(round_ties_even(3.5), 4.0);
    }
}

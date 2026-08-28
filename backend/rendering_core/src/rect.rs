// Data shape mirroring fitz.Rect, used by the profile collectors
// (services/rendering/analysis/profile/rect_area.py + page geometry).

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Rect { x0, y0, x1, y1 }
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

    /// `rect & other` — PyMuPDF intersection (clamped rect, possibly empty).
    pub fn intersect(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    /// `fitz.Rect.is_empty` — x0 >= x1 or y0 >= y1.
    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
}

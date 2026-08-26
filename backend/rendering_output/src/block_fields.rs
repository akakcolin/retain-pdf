//! Port of `output/typst/block_fields.py`.

#[derive(Debug, Clone, PartialEq)]
pub struct TypstBlockFields {
    pub var_prefix: String,
    pub x0: f64,
    pub y0: f64,
    pub width: f64,
    pub height: f64,
    pub font_size: f64,
    pub leading: f64,
    pub font_weight: String,
}

fn rect_fields(rect: &[f64]) -> (f64, f64, f64, f64) {
    let x0 = rect[0];
    let y0 = rect[1];
    let x1 = rect[2];
    let y1 = rect[3];
    (x0, y0, (x1 - x0).max(8.0), (y1 - y0).max(8.0))
}

pub fn typst_block_fields(
    block_id: &str,
    rect: &[f64],
    font_size_pt: f64,
    leading_em: f64,
    font_weight: Option<&str>,
) -> TypstBlockFields {
    let (x0, y0, width, height) = rect_fields(rect);
    let weight = match font_weight {
        Some(w) if !w.trim().is_empty() => w.to_string(),
        _ => "regular".to_string(),
    };
    TypstBlockFields {
        var_prefix: block_id.replace('-', "_"),
        x0,
        y0,
        width,
        height,
        font_size: font_size_pt.max(1.0),
        leading: leading_em.max(0.1),
        font_weight: weight,
    }
}

pub fn typst_rgb(color: [f64; 3]) -> String {
    let clamp = |v: f64| -> i64 { (v.clamp(0.0, 1.0) * 255.0) as i64 };
    format!("rgb({}, {}, {})", clamp(color[0]), clamp(color[1]), clamp(color[2]))
}

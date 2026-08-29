// Port of services/rendering/layout/payload/line_structure.py — only
// `fit_preserved_line_block_metrics`. The structured-line detection / splitting
// helpers feed `seed_render_fields`, which runs in Python before the native
// call, and are not ported here.

use crate::util::py_round;

pub const PRESERVED_LINE_LEADING_CANDIDATES: [f64; 6] = [0.12, 0.16, 0.2, 0.24, 0.28, 0.32];
pub const PRESERVED_LINE_HEIGHT_FILL: f64 = 0.96;
pub const PRESERVED_LINE_MIN_FONT_PT: f64 = 7.2;
pub const PRESERVED_LINE_IDEAL_LEADING: f64 = 0.22;
pub const PRESERVED_LINE_IDEAL_FONT_PT: f64 = 10.6;

/// `fit_preserved_line_block_metrics`: pick the leading candidate that best
/// packs `line_count` preserved lines into `inner`, scoring pitch fit, leading
/// preference and font-size deviation.
pub fn fit_preserved_line_block_metrics(
    inner: &[f64],
    protected_text: &str,
    font_size_pt: f64,
    leading_em: f64,
) -> (f64, f64) {
    if inner.len() != 4 {
        return (font_size_pt, leading_em);
    }
    let line_count = protected_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
        .max(1);
    if line_count <= 1 {
        return (font_size_pt, leading_em);
    }
    let height = (inner[3] - inner[1]).max(1.0);
    if height <= 0.0 {
        return (font_size_pt, leading_em);
    }

    let mut best: Option<(f64, f64, f64)> = None; // (score, font, leading)
    let source_font_hint = font_size_pt.max(PRESERVED_LINE_IDEAL_FONT_PT);
    for &candidate_leading in &PRESERVED_LINE_LEADING_CANDIDATES {
        let candidate_font = height * PRESERVED_LINE_HEIGHT_FILL
            / (line_count as f64 * (1.0 + candidate_leading)).max(1.0);
        if candidate_font < PRESERVED_LINE_MIN_FONT_PT {
            continue;
        }
        let line_pitch = candidate_font * (1.0 + candidate_leading);
        let target_pitch = height / (line_count as f64).max(1.0);
        let pitch_error = (line_pitch - target_pitch).abs() / target_pitch.max(1.0);
        let leading_error = (candidate_leading - PRESERVED_LINE_IDEAL_LEADING).abs() * 0.9;
        let font_error = (candidate_font - source_font_hint.min(PRESERVED_LINE_IDEAL_FONT_PT + 1.0)).abs() / 18.0;
        let score = pitch_error + leading_error + font_error;
        if best.is_none() || score < best.unwrap().0 {
            best = Some((score, candidate_font, candidate_leading));
        }
    }

    match best {
        Some((_score, font, leading)) => (py_round(font, 2), py_round(leading, 2)),
        None => {
            let fallback_leading = PRESERVED_LINE_LEADING_CANDIDATES[0];
            let fallback_font = height * PRESERVED_LINE_HEIGHT_FILL
                / (line_count as f64 * (1.0 + fallback_leading)).max(1.0);
            (py_round(fallback_font.max(PRESERVED_LINE_MIN_FONT_PT), 2), fallback_leading)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_returns_unchanged() {
        assert_eq!(fit_preserved_line_block_metrics(&[0.0, 0.0, 100.0, 20.0], "single", 10.0, 0.4), (10.0, 0.4));
    }

    #[test]
    fn short_box_forces_fallback() {
        // Three lines in a 24pt-tall box: every candidate font lands below 7.2.
        let (font, leading) = fit_preserved_line_block_metrics(&[0.0, 0.0, 100.0, 24.0], "a\nb\nc", 10.6, 0.4);
        assert_eq!(leading, 0.12);
        assert_eq!(font, 7.2);
    }
}

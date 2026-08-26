// Port of backend/scripts/devtools/tests/text_layout/test_body_font_estimation.py.

use rendering_core::font_fit::{estimate_font_size_pt, is_body_text_candidate, local_font_size_pt};
use rendering_core::item::{Item, Line, Span};

fn sample_item(wide_aspect: bool) -> Item {
    Item {
        block_type: Some("text".into()),
        source_text: "This document offers initial ideas for an industrial policy agenda to keep people first during the transition to superintelligence.".into(),
        bbox: Some([40.0, 100.0, 512.0, 205.0]),
        lines: vec![
            Line { bbox: Some([40.0, 100.0, 505.0, 113.0]), spans: vec![Span { span_type: "text".into(), content: "This document offers initial ideas".into() }] },
            Line { bbox: Some([40.0, 115.0, 503.0, 128.0]), spans: vec![Span { span_type: "text".into(), content: "for an industrial policy agenda".into() }] },
            Line { bbox: Some([40.0, 130.0, 506.0, 143.0]), spans: vec![Span { span_type: "text".into(), content: "to keep people first during".into() }] },
            Line { bbox: Some([40.0, 145.0, 504.0, 158.0]), spans: vec![Span { span_type: "text".into(), content: "the transition to".into() }] },
            Line { bbox: Some([40.0, 160.0, 500.0, 173.0]), spans: vec![Span { span_type: "text".into(), content: "superintelligence.".into() }] },
        ],
        is_body_text_candidate: true,
        wide_aspect_body_text: wide_aspect,
        ..Default::default()
    }
}

#[test]
fn local_font_size_uses_glyph_height_not_loose_line_pitch() {
    let item = Item {
        block_type: Some("text".into()),
        source_text: "Line one with normal glyphs. Line two has very loose leading.".into(),
        bbox: Some([40.0, 100.0, 420.0, 160.0]),
        lines: vec![
            Line { bbox: Some([40.0, 100.0, 410.0, 112.0]), spans: vec![Span { span_type: "text".into(), content: "Line one with normal glyphs.".into() }] },
            Line { bbox: Some([40.0, 140.0, 410.0, 152.0]), spans: vec![Span { span_type: "text".into(), content: "Line two has very loose leading.".into() }] },
        ],
        ..Default::default()
    };
    assert!(local_font_size_pt(&item) < 12.0);
}

#[test]
fn local_font_size_can_grow_for_large_source_glyphs() {
    let item = Item {
        block_type: Some("text".into()),
        source_text: "Large source text should not be capped at small body defaults.".into(),
        bbox: Some([40.0, 100.0, 420.0, 150.0]),
        lines: vec![
            Line { bbox: Some([40.0, 100.0, 410.0, 116.0]), spans: vec![Span { span_type: "text".into(), content: "Large source text should not".into() }] },
            Line { bbox: Some([40.0, 124.0, 410.0, 140.0]), spans: vec![Span { span_type: "text".into(), content: "be capped at small body defaults.".into() }] },
        ],
        ..Default::default()
    };
    assert!(local_font_size_pt(&item) > 12.0);
}

#[test]
fn wide_aspect_body_keeps_font_closer_to_local_ocr() {
    let base_item = sample_item(false);
    let wide_item = sample_item(true);
    let page_font_size = 11.6;
    let page_line_pitch = 14.0;
    let page_line_height = 12.6;
    let density_baseline = 28.0;

    let base_font = estimate_font_size_pt(&base_item, page_font_size, page_line_pitch, page_line_height, density_baseline);
    let wide_font = estimate_font_size_pt(&wide_item, page_font_size, page_line_pitch, page_line_height, density_baseline);

    assert!(wide_font > base_font);
}

#[test]
fn body_font_estimate_does_not_apply_page_factor_twice() {
    let item = sample_item(false);
    let page_font_size = 11.0;
    let page_line_pitch = 15.0;
    let page_line_height = 13.0;
    let density_baseline = 28.0;

    let font = estimate_font_size_pt(&item, page_font_size, page_line_pitch, page_line_height, density_baseline);

    assert!(font >= 10.5);
}

#[test]
fn caption_font_is_visibly_smaller_than_body_font() {
    let body = sample_item(false);
    let caption = Item {
        block_kind: Some("text".into()),
        raw_block_type: Some("figure_title".into()),
        layout_role: Some("caption".into()),
        semantic_role: Some("metadata".into()),
        structure_role: Some("figure_caption".into()),
        normalized_sub_type: Some("figure_caption".into()),
        source_text: "FIG. 1. Cross sections of surfaces of revolution.".into(),
        bbox: Some([311.5, 529.5, 562.0, 587.0]),
        lines: vec![
            Line { bbox: Some([311.5, 529.5, 562.0, 541.5]), spans: vec![Span { span_type: "text".into(), content: "FIG. 1. Cross sections of surfaces".into() }] },
            Line { bbox: Some([311.5, 545.5, 562.0, 557.5]), spans: vec![Span { span_type: "text".into(), content: "of revolution.".into() }] },
        ],
        ..Default::default()
    };
    let page_font_size = 10.8;
    let page_line_pitch = 14.0;
    let page_line_height = 12.0;
    let density_baseline = 28.0;

    let body_font = estimate_font_size_pt(&body, page_font_size, page_line_pitch, page_line_height, density_baseline);
    let caption_font = estimate_font_size_pt(&caption, page_font_size, page_line_pitch, page_line_height, density_baseline);

    assert!(caption_font <= 9.8);
    assert!(caption_font < body_font - 0.5);
}

#[test]
fn vision_footnote_font_is_annotation_sized() {
    let body = sample_item(false);
    let footnote = Item {
        block_type: Some("text".into()),
        block_kind: Some("text".into()),
        raw_block_type: Some("vision_footnote".into()),
        layout_role: Some("footnote".into()),
        semantic_role: Some("unknown".into()),
        structure_role: Some("footnote".into()),
        normalized_sub_type: Some("footnote".into()),
        tags: vec!["footnote".into()],
        source_text: "a P < 0.05; b adjusted confidence interval.".into(),
        bbox: Some([58.0, 720.0, 520.0, 742.0]),
        lines: vec![
            Line { bbox: Some([58.0, 720.0, 520.0, 731.0]), spans: vec![Span { span_type: "text".into(), content: "a P < 0.05; b adjusted confidence interval.".into() }] },
        ],
        ..Default::default()
    };
    let page_font_size = 10.8;
    let page_line_pitch = 14.0;
    let page_line_height = 12.0;
    let density_baseline = 28.0;

    let body_font = estimate_font_size_pt(&body, page_font_size, page_line_pitch, page_line_height, density_baseline);
    let footnote_font = estimate_font_size_pt(&footnote, page_font_size, page_line_pitch, page_line_height, density_baseline);

    assert!(footnote_font <= 8.8);
    assert!(footnote_font < body_font - 1.0);
    assert!(!is_body_text_candidate(&footnote, 300.0));
}

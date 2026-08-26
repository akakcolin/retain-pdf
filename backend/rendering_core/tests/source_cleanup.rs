// Port of the pure-logic subset of
// backend/scripts/devtools/tests/rendering/test_bbox_text_strip_document.py
// (hit_test, is_protected_text_op, text_ops), plus hand-written sample tests for
// pdf_math, stream_state, path_removal, and text_removal.
//
// PDF-bound integration (fitz/pikepdf fixtures) is not portable; those tests
// stay in Python.

use rendering_core::source_cleanup::hit_test::{inside_any_rect, intersects_any_rect, is_protected_text_op, RectIndex};
use rendering_core::source_cleanup::path_removal::{
    decide_path_paint_rewrite, PathTracker, PATH_CONSTRUCTION_OPERATORS, PATH_PAINT_OPERATORS,
};
use rendering_core::source_cleanup::pdf_math::{matrix_from_operands, mul_matrix, transform_point, Operand, PdfMatrix, IDENTITY_MATRIX};
use rendering_core::source_cleanup::stream_state::ContentStreamState;
use rendering_core::source_cleanup::text_ops::{
    estimated_user_text_geometry, text_advance_tx, text_operand_length, text_operand_profile, TextState,
};
use rendering_core::source_cleanup::text_removal::decide_text_show_rewrite;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "mismatch: actual={actual}, expected={expected}"
    );
}

// --- hit_test (ported from test_bbox_text_strip_document.py) -----------------

#[test]
fn text_strip_hit_test_ignores_tiny_edge_intersections() {
    let strip_index = RectIndex::build([[100.0, 100.0, 160.0, 120.0]]);

    assert!(!strip_index.matches_text_for_removal(20.0, 110.0, &[20.0, 100.0, 101.0, 120.0]));
    assert!(strip_index.matches_text_for_removal(20.0, 110.0, &[20.0, 100.0, 145.0, 120.0]));
    assert!(strip_index.matches_text_for_removal(120.0, 110.0, &[20.0, 100.0, 101.0, 120.0]));
}

#[test]
fn formula_guard_edge_touch_does_not_protect_whole_text_op() {
    let protected_index = RectIndex::build([[68.5, 667.0, 249.0, 681.0]]);

    assert!(!is_protected_text_op(
        (72.02, 684.22),
        &[72.02, 680.73, 136.76, 694.68],
        None,
        Some(&protected_index),
    ));
}

#[test]
fn formula_guard_protects_substantial_text_overlap() {
    let protected_index = RectIndex::build([[68.5, 667.0, 249.0, 681.0]]);

    assert!(is_protected_text_op(
        (72.02, 676.0),
        &[72.02, 672.0, 136.76, 686.0],
        None,
        Some(&protected_index),
    ));
}

// --- text_ops (ported from test_bbox_text_strip_document.py) -----------------

#[test]
fn text_state_advance_uses_font_size_spacing_and_tj_adjustments() {
    let state = TextState {
        font_size: 12.0,
        char_spacing: 1.0,
        word_spacing: 3.0,
        ..TextState::default()
    };
    let plain = text_advance_tx(&IDENTITY_MATRIX, &[Operand::Str("hello".into())], None, None, Some(&state));
    let with_space = text_advance_tx(&IDENTITY_MATRIX, &[Operand::Str("a b".into())], None, None, Some(&state));
    let with_tj_pull = text_advance_tx(
        &IDENTITY_MATRIX,
        &[Operand::Array(vec![Operand::Str("a".into()), Operand::Num(-120.0), Operand::Str("b".into())])],
        None,
        None,
        Some(&state),
    );
    let plain_ab = text_advance_tx(&IDENTITY_MATRIX, &[Operand::Str("ab".into())], None, None, Some(&state));

    assert_close(plain, 35.0);
    assert_close(with_space, 24.0);
    assert!(with_tj_pull > plain_ab);
}

#[test]
fn estimated_text_rect_uses_font_size_from_text_state() {
    let state = TextState {
        font_size: 12.0,
        ..TextState::default()
    };
    let (_point, rect) = estimated_user_text_geometry(
        &IDENTITY_MATRIX,
        &PdfMatrix([1.0, 0.0, 0.0, 1.0, 20.0, 40.0]),
        &state,
        4,
    );

    assert_close(rect[0], 20.0);
    assert!(rect[1] < 40.0);
    assert!(rect[2] >= 44.0);
    assert!(rect[3] > 50.0);
}

#[test]
fn text_operand_metrics_profiles_and_lengths() {
    let metrics = text_operand_profile(&[Operand::Str("a b".into())]);
    assert_eq!(metrics.chars, 3);
    assert_eq!(metrics.spaces, 1);
    assert_eq!(text_operand_length(&[Operand::Str("hello".into())]), 5);
    assert_eq!(text_operand_length(&[Operand::Num(1.0)]), 1);
}

// --- pdf_math (hand-written) -------------------------------------------------

#[test]
fn pdf_matrix_multiplication_is_faithful() {
    let a = PdfMatrix([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    let b = PdfMatrix([7.0, 8.0, 9.0, 10.0, 11.0, 12.0]);
    let c = mul_matrix(&a, &b);
    // a*g+c*h=31, b*g+d*h=46, a*i+c*j=39, b*i+d*j=58, a*k+c*l+e=52, b*k+d*l+f=76
    assert_eq!(c.0, [31.0, 46.0, 39.0, 58.0, 52.0, 76.0]);
}

#[test]
fn transform_point_applies_affine() {
    let m = PdfMatrix([2.0, 0.0, 0.0, 3.0, 10.0, 20.0]);
    assert_eq!(transform_point(&m, 4.0, 5.0), (18.0, 35.0));
}

#[test]
fn matrix_from_operands_requires_six() {
    assert!(matrix_from_operands(&[Operand::Num(1.0), Operand::Num(2.0)]).is_none());
    let m = matrix_from_operands(&[
        Operand::Num(1.0),
        Operand::Num(0.0),
        Operand::Num(0.0),
        Operand::Num(1.0),
        Operand::Num(3.0),
        Operand::Num(4.0),
    ])
    .unwrap();
    assert_eq!(m.0, [1.0, 0.0, 0.0, 1.0, 3.0, 4.0]);
}

// --- stream_state (hand-written) ---------------------------------------------

#[test]
fn stream_state_font_and_matrix_ops() {
    let mut s = ContentStreamState::default();
    assert!(s.apply_state_operator("Tf", &[Operand::Name("F1".into()), Operand::Num(14.0)]));
    assert_eq!(s.text_state.font_size, 14.0);
    assert!(!s.apply_state_operator("nope", &[]));

    s.apply_state_operator(
        "Tm",
        &[Operand::Num(1.0), Operand::Num(0.0), Operand::Num(0.0), Operand::Num(1.0), Operand::Num(50.0), Operand::Num(60.0)],
    );
    assert_eq!(s.text_matrix.0, [1.0, 0.0, 0.0, 1.0, 50.0, 60.0]);

    s.apply_state_operator("q", &[]);
    s.apply_state_operator(
        "cm",
        &[Operand::Num(2.0), Operand::Num(0.0), Operand::Num(0.0), Operand::Num(2.0), Operand::Num(0.0), Operand::Num(0.0)],
    );
    assert_eq!(s.ctm.0, [2.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
    s.apply_state_operator("Q", &[]);
    assert_eq!(s.ctm.0, IDENTITY_MATRIX.0);
}

#[test]
fn stream_state_quote_prep_sets_spacing_and_moves() {
    let mut s = ContentStreamState::default();
    s.apply_state_operator("TD", &[Operand::Num(0.0), Operand::Num(-14.0)]);
    assert_eq!(s.leading, 14.0);

    s.prepare_quote_text_show(
        "\"",
        &[Operand::Num(2.0), Operand::Num(1.0), Operand::Str("ab".into())],
    );
    assert_eq!(s.text_state.word_spacing, 2.0);
    assert_eq!(s.text_state.char_spacing, 1.0);
    // move_text(0, -leading) after the initial TD(0,-14): -14 + -14 = -28.
    assert_eq!(s.text_matrix.0[5], -28.0);
}

#[test]
fn stream_state_advance_text_advances_text_matrix() {
    let mut s = ContentStreamState::default();
    s.apply_state_operator("BT", &[]);
    let before = s.text_matrix.0[4];
    s.advance_text(&[Operand::Str("hello".into())], None);
    assert!(s.text_matrix.0[4] > before);
}

// --- path_removal (hand-written) ---------------------------------------------

#[test]
fn path_tracker_tracks_and_computes_bbox() {
    let mut t = PathTracker::empty();
    let ctm = IDENTITY_MATRIX;
    t.record("m", &[Operand::Num(10.0), Operand::Num(20.0)], &ctm);
    t.record("l", &[Operand::Num(30.0), Operand::Num(40.0)], &ctm);
    assert_eq!(t.rect(), Some([10.0, 20.0, 30.0, 40.0]));
    t.clear();
    assert_eq!(t.rect(), None);
}

#[test]
fn path_tracker_rect_operator_records_corners() {
    let mut t = PathTracker::empty();
    let ctm = IDENTITY_MATRIX;
    t.record("re", &[Operand::Num(5.0), Operand::Num(6.0), Operand::Num(10.0), Operand::Num(20.0)], &ctm);
    assert_eq!(t.rect(), Some([5.0, 6.0, 15.0, 26.0]));
}

#[test]
fn path_paint_rewrite_decides_removal() {
    let strip = RectIndex::build([[0.0, 0.0, 100.0, 100.0]]);
    let protected = RectIndex::build([[10.0, 10.0, 20.0, 20.0]]);
    // Text-like (height 20 <= 32pt, area 1200 <= 3500pt2), inside strip, clear
    // of the protected region.
    let path_rect = [30.0, 0.0, 90.0, 20.0];

    let d = decide_path_paint_rewrite("f", Some(path_rect), &strip, &protected);
    assert!(d.remove);
    assert_eq!(d.rect, Some(path_rect));

    // Non-fill paint op never removes.
    let d2 = decide_path_paint_rewrite("S", Some(path_rect), &strip, &protected);
    assert!(!d2.remove);

    // Text-unlike path (height 200 > 32pt) is not removed.
    let tall = [0.0, 0.0, 100.0, 200.0];
    let d3 = decide_path_paint_rewrite("f", Some(tall), &strip, &protected);
    assert!(!d3.remove);

    // Protected overlap blocks removal.
    let d4 = decide_path_paint_rewrite("f", Some([10.0, 10.0, 20.0, 20.0]), &strip, &protected);
    assert!(!d4.remove);
}

#[test]
fn path_operator_sets_are_complete() {
    assert_eq!(PATH_CONSTRUCTION_OPERATORS.len(), 7);
    assert_eq!(PATH_PAINT_OPERATORS.len(), 10);
}

// --- text_removal (hand-written) ---------------------------------------------

fn strip_at(x0: f64, y0: f64, x1: f64, y1: f64) -> RectIndex {
    RectIndex::build([[x0, y0, x1, y1]])
}

#[test]
fn decide_text_show_rewrite_removes_when_in_strip_and_unprotected() {
    let strip = strip_at(0.0, 0.0, 200.0, 200.0);
    let protected = RectIndex::build([]);
    let text_matrix = PdfMatrix([1.0, 0.0, 0.0, 1.0, 50.0, 100.0]);
    let state = TextState {
        font_size: 12.0,
        ..TextState::default()
    };
    let d = decide_text_show_rewrite(
        &[Operand::Str("hello".into())],
        &IDENTITY_MATRIX,
        &text_matrix,
        &state,
        &strip,
        &protected,
    );

    assert!(d.remove);
    assert_eq!(d.text_metrics.chars, 5);
    assert_eq!(d.user_point, (50.0, 100.0));
}

#[test]
fn decide_text_show_rewrite_keeps_when_outside_strip() {
    let strip = strip_at(1000.0, 1000.0, 1100.0, 1100.0);
    let protected = RectIndex::build([]);
    let text_matrix = PdfMatrix([1.0, 0.0, 0.0, 1.0, 50.0, 100.0]);
    let state = TextState {
        font_size: 12.0,
        ..TextState::default()
    };
    let d = decide_text_show_rewrite(
        &[Operand::Str("hello".into())],
        &IDENTITY_MATRIX,
        &text_matrix,
        &state,
        &strip,
        &protected,
    );
    assert!(!d.remove);
}

// --- hit_test helpers (hand-written) -----------------------------------------

#[test]
fn inside_and_intersects_any_rect_helpers() {
    let rects = [[0.0, 0.0, 100.0, 100.0]];
    assert!(inside_any_rect(50.0, 50.0, &rects));
    assert!(!inside_any_rect(150.0, 50.0, &rects));
    assert!(intersects_any_rect(&[90.0, 90.0, 110.0, 110.0], &rects));
    assert!(!intersects_any_rect(&[150.0, 150.0, 160.0, 160.0], &rects));
}

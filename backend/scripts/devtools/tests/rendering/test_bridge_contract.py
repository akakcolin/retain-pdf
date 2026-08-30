from __future__ import annotations

import sys
from pathlib import Path

import pytest


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))


from services.rendering.contracts.bridge_shapes import (
    BRIDGE_BOUNDARIES,
    ContractKeyMismatch,
    EmitterBlockShape,
    EmitterPageSpecShape,
    FillMapShape,
    PrecleanedPageIndicesShape,
    RedactionItemShape,
    RedactionPageSpecShape,
    RenderBundleShape,
    assert_keys_match,
    expected_keys,
)


def _block(block_id: str = "item-0", page_index: int = 0) -> dict:
    return {
        "block_id": block_id,
        "page_index": page_index,
        "background_rect": [0.0, 0.0, 100.0, 100.0],
        "content_rect": [5.0, 5.0, 95.0, 95.0],
        "content_kind": "text",
        "content_text": "Hello",
        "plain_text": "Hello",
        "math_map": [],
        "font_size_pt": 12.0,
        "leading_em": 1.4,
        "font_weight": "normal",
        "fit_to_box": False,
        "fit_single_line": False,
        "fit_min_font_size_pt": 6.0,
        "fit_max_font_size_pt": 48.0,
        "fit_min_leading_em": 1.0,
        "fit_max_height_pt": 200.0,
        "fit_target_width_pt": 100.0,
        "fit_target_height_pt": 50.0,
        "fit_shift_up_pt": 0.0,
        "first_line_indent_pt": 0.0,
        "justify_text": False,
        "text_color": [0.0, 0.0, 0.0],
        "cover_fill": [1.0, 1.0, 1.0],
        "use_cover_fill": False,
        "skip_reason": "",
        "preserve_line_breaks": False,
        "preserved_line_boxes": [],
        "toc_entries": [],
    }


def _page_spec() -> dict:
    return {
        "page_index": 0,
        "page_width_pt": 595.0,
        "page_height_pt": 842.0,
        "background_pdf_path": None,
        "blocks": [_block()],
    }


def _bundle() -> dict:
    return {
        "schema_version": "render.bundle.v1",
        "mode": "typst",
        "source_pdf": "/tmp/in.pdf",
        "output_pdf": "/tmp/out.pdf",
        "work_dir": "/tmp/work",
        "font_family": "Source Han Serif",
        "redaction_strategy": None,
        "precleaned_page_indices": [0, 2],
        "visual_profile_fill_map": {"item-0": [0.9, 0.9, 0.9]},
        "page_map": {"source_page_indices": [0]},
        "translated_pages": {0: [{"item_id": "item-0", "translated_text": "Hello"}]},
        "page_specs": [_page_spec()],
        "start_page": 0,
        "end_page": 1,
        "overlay_page_specs": None,
    }


def test_emitter_block_required_keys_match_producer() -> None:
    required, optional = expected_keys(EmitterBlockShape)
    assert required == {
        "block_id",
        "page_index",
        "background_rect",
        "content_rect",
        "content_kind",
        "content_text",
        "plain_text",
        "math_map",
        "font_size_pt",
        "leading_em",
        "font_weight",
        "fit_to_box",
        "fit_single_line",
        "fit_min_font_size_pt",
        "fit_max_font_size_pt",
        "fit_min_leading_em",
        "fit_max_height_pt",
        "fit_target_width_pt",
        "fit_target_height_pt",
        "fit_shift_up_pt",
        "first_line_indent_pt",
        "justify_text",
        "text_color",
        "cover_fill",
        "use_cover_fill",
        "skip_reason",
        "preserve_line_breaks",
        "preserved_line_boxes",
        "toc_entries",
    }
    assert optional == frozenset()


def test_redaction_item_vocabulary_is_optional_and_open() -> None:
    required, optional = expected_keys(RedactionItemShape)
    assert required == frozenset()
    assert optional == {
        "bbox",
        "translated_text",
        "item_id",
        "source_item_id",
        "block_id",
        "protected_translated_text",
        "translation_unit_protected_translated_text",
        "translation_unit_translated_text",
        "source_text",
        "protected_source_text",
        "render_source_text",
        "block_kind",
        "block_type",
        "raw_block_type",
        "normalized_sub_type",
        "continuation_group",
        "continuation_group_id",
        "_force_visual_cover_only",
        "_formula_guard_fragment",
        "_formula_guard_fragment_index",
        "_visual_profile_fill",
        "_render_cleanup_mode",
        "_render_overlay_fill",
        "_render_use_cover_fill",
        "render_policy",
    }


def test_boundary_registry_has_six_boundaries() -> None:
    assert set(BRIDGE_BOUNDARIES) == {
        "emitter_page_specs",
        "redaction_page_specs",
        "redaction_item",
        "fill_map",
        "precleaned_page_indices",
        "render_bundle",
    }


def test_emitter_page_spec_passes() -> None:
    assert_keys_match(_page_spec(), EmitterPageSpecShape)


def test_redaction_page_spec_passes() -> None:
    spec = {
        "page_index": 0,
        "page_width_pt": 595.0,
        "page_height_pt": 842.0,
        "background_pdf_path": "/tmp/bg.pdf",
        "blocks": [
            {
                "block_id": "item-0",
                "background_rect": [0.0, 0.0, 100.0, 100.0],
                "content_rect": [5.0, 5.0, 95.0, 95.0],
                "content_kind": "text",
                "content_text": "Hello",
                "plain_text": "Hello",
            }
        ],
    }
    assert_keys_match(spec, RedactionPageSpecShape)


def test_missing_required_key_rejected() -> None:
    spec = _page_spec()
    del spec["page_index"]
    with pytest.raises(ContractKeyMismatch):
        assert_keys_match(spec, EmitterPageSpecShape)


def test_extra_key_on_closed_shape_rejected() -> None:
    spec = _page_spec()
    spec["bogus_key"] = 1
    with pytest.raises(ContractKeyMismatch):
        assert_keys_match(spec, EmitterPageSpecShape)


def test_extra_key_on_nested_block_rejected() -> None:
    spec = _page_spec()
    spec["blocks"][0]["bogus_key"] = 1
    with pytest.raises(ContractKeyMismatch):
        assert_keys_match(spec, EmitterPageSpecShape)


def test_extra_key_on_bundle_rejected() -> None:
    bundle = _bundle()
    bundle["bogus_key"] = 1
    with pytest.raises(ContractKeyMismatch):
        assert_keys_match(bundle, RenderBundleShape)


def test_open_redaction_item_tolerates_extra_keys() -> None:
    item = {
        "bbox": [0.0, 0.0, 100.0, 100.0],
        "translated_text": "Hello",
        "item_id": "item-0",
        # Underscore-prefixed render fields the Rust DTO ignores; the open shape
        # tolerates them while still validating the recognized vocabulary.
        "_render_block_id": "block-0",
        "_render_block_index": 3,
        "render_policy": {"cleanup_mode": "visual_cover", "overlay_fill": "white"},
    }
    assert_keys_match(item, RedactionItemShape)


def test_render_bundle_passes() -> None:
    assert_keys_match(_bundle(), RenderBundleShape)


def test_fill_map_accepts_rgb_values() -> None:
    assert_keys_match({"item-0": [0.9, 0.9, 0.9]}, FillMapShape)


def test_fill_map_rejects_non_list_value() -> None:
    with pytest.raises(ContractKeyMismatch):
        assert_keys_match({"item-0": "not-a-list"}, FillMapShape)


def test_precleaned_page_indices_accepts_int_list() -> None:
    assert_keys_match([0, 2, 5], PrecleanedPageIndicesShape)


def test_precleaned_page_indices_rejects_non_list() -> None:
    with pytest.raises(ContractKeyMismatch):
        assert_keys_match("0,2,5", PrecleanedPageIndicesShape)

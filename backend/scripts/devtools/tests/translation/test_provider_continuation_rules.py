import importlib.util
import sys
import tempfile
import types
from pathlib import Path


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))


from services.document_schema.defaults import default_block_continuation_hint
from services.translation.core.ocr.json_extractor import extract_text_items
from services.translation.core.ocr.models import TextItem
from services.translation.core.payload.translations import export_translation_template
from services.translation.core.payload.translations import load_translations
from services.translation.services.continuation.orchestrator import _filter_boundary_candidate_pairs


def _ensure_package_stubs() -> None:
    package_paths = {
        "services": REPO_SCRIPTS_ROOT / "services",
        "services.translation": REPO_SCRIPTS_ROOT / "services" / "translation",
        "services.translation.services": REPO_SCRIPTS_ROOT / "services" / "translation" / "services",
        "services.translation.services.continuation": REPO_SCRIPTS_ROOT / "services" / "translation" / "services" / "continuation",
    }
    for name, path in package_paths.items():
        module = sys.modules.get(name)
        if module is None:
            module = types.ModuleType(name)
            module.__path__ = [str(path)]
            sys.modules[name] = module


def _load_module(name: str, path: Path):
    _ensure_package_stubs()
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _load_state_module():
    _load_module(
        "services.translation.services.continuation.rules",
        REPO_SCRIPTS_ROOT / "services" / "translation" / "services" / "continuation" / "rules.py",
    )
    return _load_module(
        "services.translation.services.continuation.state",
        REPO_SCRIPTS_ROOT / "services" / "translation" / "services" / "continuation" / "state.py",
    )


def _payload_item(
    *,
    item_id: str,
    page_idx: int,
    text: str,
    bbox: list[float],
    ocr_source: str = "",
    ocr_group_id: str = "",
    ocr_scope: str = "",
    ocr_order: int = -1,
    layout_mode: str = "",
    layout_zone: str = "",
    layout_boundary_role: str = "",
    provider_body_repair_applied: bool = False,
) -> dict:
    return {
        "item_id": item_id,
        "page_idx": page_idx,
        "block_idx": 0,
        "block_type": "text",
        "block_kind": "text",
        "layout_role": "paragraph",
        "semantic_role": "body",
        "structure_role": "body",
        "policy_translate": True,
        "raw_block_type": "text",
        "normalized_sub_type": "",
        "bbox": bbox,
        "protected_source_text": text,
        "ocr_continuation_source": ocr_source,
        "ocr_continuation_group_id": ocr_group_id,
        "ocr_continuation_scope": ocr_scope,
        "ocr_continuation_reading_order": ocr_order,
        "layout_mode": layout_mode,
        "layout_zone": layout_zone,
        "layout_boundary_role": layout_boundary_role,
        "body_repair_applied": provider_body_repair_applied,
        "provider_body_repair_applied": provider_body_repair_applied,
    }

def test_boundary_review_skips_body_repair_items() -> None:
    payload = [
        _payload_item(
            item_id="a",
            page_idx=0,
            text="The present work attempts to explain,",
            bbox=[0, 0, 100, 20],
            layout_mode="double",
            layout_zone="left_column",
            layout_boundary_role="tail",
            provider_body_repair_applied=True,
        ),
        _payload_item(
            item_id="b",
            page_idx=0,
            text="perhaps vaguely but completely based on the obtained results.",
            bbox=[120, 0, 220, 20],
            layout_mode="double",
            layout_zone="right_column",
            layout_boundary_role="head",
            provider_body_repair_applied=True,
        ),
    ]
    pairs = [
        {
            "prev_item_id": "a",
            "next_item_id": "b",
            "prev_text": payload[0]["protected_source_text"],
            "next_text": payload[1]["protected_source_text"],
            "prev_page_idx": 0,
            "next_page_idx": 0,
            "prev_bbox": payload[0]["bbox"],
            "next_bbox": payload[1]["bbox"],
        }
    ]

    assert _filter_boundary_candidate_pairs(payload, pairs) == []


def test_provider_intra_page_join_takes_priority_and_rule_fallback_still_runs() -> None:
    state = _load_state_module()
    payload = [
        _payload_item(
            item_id="a",
            page_idx=0,
            text="left column sentence",
            bbox=[0, 0, 100, 20],
            ocr_source="provider",
            ocr_group_id="provider-paddle-page-001-group-12",
            ocr_scope="intra_page",
            ocr_order=0,
        ),
        _payload_item(
            item_id="b",
            page_idx=0,
            text="right column continuation",
            bbox=[120, 0, 220, 20],
            ocr_source="provider",
            ocr_group_id="provider-paddle-page-001-group-12",
            ocr_scope="intra_page",
            ocr_order=1,
        ),
        _payload_item(
            item_id="c",
            page_idx=0,
            text="This sentence continues with",
            bbox=[0, 40, 180, 60],
        ),
        _payload_item(
            item_id="d",
            page_idx=1,
            text="and additional evidence from the experiment.",
            bbox=[0, 0, 180, 20],
        ),
    ]

    annotated = state.annotate_continuation_context(payload)
    summary = state.summarize_continuation_decisions(payload)

    assert annotated == 4
    assert payload[0]["continuation_decision"] == "provider_joined"
    assert payload[1]["continuation_decision"] == "provider_joined"
    assert payload[0]["continuation_group"] == "provider-paddle-page-001-group-12"
    assert payload[2]["continuation_decision"] == "joined"
    assert payload[3]["continuation_decision"] == "joined"
    assert summary["joined_items"] == 4
    assert summary["provider_joined_items"] == 2
    assert summary["rule_joined_items"] == 2


def test_provider_cross_page_boundary_pair_is_consumed() -> None:
    state = _load_state_module()
    payload = [
        _payload_item(
            item_id="a",
            page_idx=0,
            text="This sentence continues with",
            bbox=[0, 0, 180, 20],
            ocr_source="provider",
            ocr_group_id="provider-paddle-global-abc",
            ocr_scope="cross_page",
            ocr_order=0,
            layout_mode="single",
            layout_zone="single_column",
            layout_boundary_role="tail",
        ),
        _payload_item(
            item_id="b",
            page_idx=1,
            text="and additional evidence from the experiment.",
            bbox=[0, 0, 180, 20],
            ocr_source="provider",
            ocr_group_id="provider-paddle-global-abc",
            ocr_scope="cross_page",
            ocr_order=1,
            layout_mode="single",
            layout_zone="single_column",
            layout_boundary_role="head",
        ),
    ]

    state.annotate_continuation_context(payload)

    assert payload[0]["continuation_decision"] == "provider_joined"
    assert payload[0]["continuation_group"] == "provider-paddle-global-abc"



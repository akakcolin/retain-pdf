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

def test_chunk_source_text_fallback_keeps_inline_math_atomic() -> None:
    from services.translation.llm.shared.orchestration.common import chunk_source_text_fallback

    text = "h mode i of the excitation spectrum can be characterized by its dispersion relation $\\omega_i(Q)$ and lifetime $\\tau$."
    chunks = chunk_source_text_fallback(text, words_per_chunk=5)

    assert any("$\\omega_i(Q)$" in chunk for chunk in chunks)
    assert not any(chunk.endswith("$\\omega_i(Q)") or chunk.startswith("\\omega_i(Q)$") for chunk in chunks)


def test_group_translation_split_keeps_inline_math_atomic() -> None:
    from services.translation.core.payload.parts.apply import _split_group_protected_translation

    items = [
        {"protected_source_text": "prev part"},
        {"protected_source_text": "next part"},
    ]
    translated = "激发谱的每个模式 i 可由其色散关系 $\\omega^i(\\mathbf{Q})$、寿命 $\\tau_{\\mathrm{SW}}^i$ 和强度 I_0 表征。"
    chunks = _split_group_protected_translation(translated, items)

    assert len(chunks) == 2
    assert sum("$\\omega^i(\\mathbf{Q})$" in chunk for chunk in chunks) == 1
    assert all(chunk.count("$") % 2 == 0 for chunk in chunks if chunk)


def test_unbalanced_inline_math_blocks_do_not_join_across_pages() -> None:
    state = _load_state_module()
    payload = [
        _payload_item(
            item_id="a",
            page_idx=0,
            text="The objective function is $a",
            bbox=[0, 0, 180, 20],
            ocr_source="provider",
            ocr_group_id="provider-paddle-global-math",
            ocr_scope="cross_page",
            ocr_order=0,
            layout_mode="single",
            layout_zone="single_column",
            layout_boundary_role="tail",
        ),
        _payload_item(
            item_id="b",
            page_idx=1,
            text="+b $ and additional evidence from the experiment.",
            bbox=[0, 0, 180, 20],
            ocr_source="provider",
            ocr_group_id="provider-paddle-global-math",
            ocr_scope="cross_page",
            ocr_order=1,
            layout_mode="single",
            layout_zone="single_column",
            layout_boundary_role="head",
        ),
    ]

    state.annotate_continuation_context(payload)

    assert payload[0]["continuation_decision"] == ""
    assert payload[1]["continuation_decision"] == ""
    assert payload[0]["continuation_group"] == ""
    assert payload[1]["continuation_group"] == ""



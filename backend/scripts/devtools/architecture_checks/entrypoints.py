from __future__ import annotations

from pathlib import Path
import re

from devtools.architecture_checks.common import SCRIPTS_ROOT
from devtools.architecture_checks.common import read_text
from devtools.architecture_checks.common import scan_py_files


ENTRYPOINTS_ROOT = SCRIPTS_ROOT / "entrypoints"
STAGE_SPEC_CONTRACT_CHECK = SCRIPTS_ROOT / "devtools" / "check_stage_specs_contract.py"

ENTRYPOINT_IMPORT_ALLOWLIST: dict[Path, tuple[str, ...]] = {
    Path("console.py"): (
        "from collections.abc import",
        "from foundation.shared.structured_errors import",
        "from services.document_schema.normalize_pipeline import",
        "from services.ocr_provider.provider_pipeline import",
        "from services.rendering.workflow.render_only import",
        "from services.translation.entrypoints.from_ocr_pipeline import",
        "from services.translation.entrypoints.translate_only_pipeline import",
    ),
    Path("build_book.py"): ("from runtime.pipeline.book_pipeline import",),
    Path("build_page.py"): (
        "from services.translation.public import",
        "from services.rendering.legacy.pdf_overlay import",
        "from services.rendering.legacy.typst_page_renderer import",
    ),
    Path("diagnose_failure_with_ai.py"): (
        "from services.translation.public import",
    ),
    Path("run_book.py"): ("from services.translation.entrypoints.from_ocr_pipeline import main",),
    Path("run_document_flow.py"): (
        "from runtime.pipeline.book_pipeline import",
        "from services.translation.public import",
    ),
    Path("run_normalize_ocr.py"): ("from services.document_schema.normalize_pipeline import main",),
    Path("run_extract_text_layer.py"): (
        "from services.document_schema.providers import",
        "from services.pipeline_shared.io import",
    ),
    Path("run_provider_case.py"): ("from services.ocr_provider.provider_pipeline import main",),
    Path("run_provider_ocr.py"): ("from services.ocr_provider.provider_pipeline import main",),
    Path("run_render_delegate.py"): (
        "from runtime.pipeline.render_mode import",
        "from runtime.pipeline.translation_loader import",
        "from services.rendering.document.page_map import",
        "from services.rendering.layout._native import",
        "from services.rendering.layout.page_specs import",
        "from services.rendering.output.typst import",
        "from services.rendering.output.typst.book_renderer import",
        "from services.rendering.output.typst.book_support import",
        "from services.rendering.output.typst.overlay_color import",
        "from services.rendering.source.background._native import",
        "from services.rendering.source.render_source import",
        "from services.rendering.source_cleanup.protected_blocks import",
        "from services.rendering.visual_profile.runtime import",
        "from services.rendering.workflow.document_analysis import",
        "from services.rendering import _routing",
    ),
    Path("run_render_only.py"): ("from services.rendering.workflow.render_only import main",),
    Path("run_translate_from_ocr.py"): ("from services.translation.entrypoints.from_ocr_pipeline import main",),
    Path("run_translate_only.py"): ("from services.translation.entrypoints.translate_only_pipeline import main",),
    Path("translate_book.py"): ("from services.translation.entrypoints.translate_only_pipeline import main",),
    Path("translate_page.py"): (
        "from services.translation.public import",
    ),
    Path("validate_document_schema.py"): ("from services.document_schema import",),
}


def check_entrypoint_stable_imports(errors: list[str]) -> None:
    import_pattern = re.compile(r"^from\s+([A-Za-z0-9_\.]+)\s+import\s+(.+)$", re.MULTILINE)
    for path in scan_py_files(ENTRYPOINTS_ROOT):
        rel_name = path.relative_to(ENTRYPOINTS_ROOT)
        allowed_prefixes = ENTRYPOINT_IMPORT_ALLOWLIST.get(rel_name)
        if allowed_prefixes is None:
            errors.append(f"entrypoints/{rel_name}: missing explicit import allowlist entry in check_pipeline_architecture.py")
            continue
        text = read_text(path)
        for match in import_pattern.finditer(text):
            stmt = f"from {match.group(1)} import {match.group(2)}"
            if match.group(1).startswith(("foundation.", "pathlib", "__future__")):
                continue
            if any(stmt.startswith(prefix) for prefix in allowed_prefixes):
                continue
            errors.append(
                f"entrypoints/{rel_name}: entrypoint should import only its stable top-level pipeline/service entry, found '{stmt}'"
            )


def check_stage_spec_contract_checker(errors: list[str]) -> None:
    if not STAGE_SPEC_CONTRACT_CHECK.exists():
        errors.append(
            "devtools/check_stage_specs_contract.py: Rust/Python stage spec contract checker is missing"
        )
        return
    text = read_text(STAGE_SPEC_CONTRACT_CHECK)
    for loader_name in (
        "BookStageSpec",
        "NormalizeStageSpec",
        "ProviderStageSpec",
        "RenderStageSpec",
        "TranslateStageSpec",
    ):
        if loader_name not in text:
            errors.append(
                f"devtools/check_stage_specs_contract.py: missing Python loader coverage for {loader_name}"
            )
    if "stage_spec_contract=ok" not in text:
        errors.append(
            "devtools/check_stage_specs_contract.py: checker must emit a stable success marker"
        )


__all__ = [
    "check_entrypoint_stable_imports",
    "check_stage_spec_contract_checker",
]

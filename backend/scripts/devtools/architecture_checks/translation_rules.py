from __future__ import annotations

from pathlib import Path

from devtools.architecture_checks.common import SCRIPTS_ROOT

PIPELINE_ROOT = SCRIPTS_ROOT / "runtime" / "pipeline"
DOCUMENT_SCHEMA_ROOT = SCRIPTS_ROOT / "services" / "document_schema"
TRANSLATION_ROOT = SCRIPTS_ROOT / "services" / "translation"
DEVTOOLS_ROOT = SCRIPTS_ROOT / "devtools"

TRANSLATE_ONLY_ENTRYPOINT = SCRIPTS_ROOT / "services" / "translation" / "entrypoints" / "translate_only_pipeline.py"
TRANSLATION_ALLOWED_ROOT_DIRS = {
    "artifacts",
    "core",
    "entrypoints",
    "llm",
    "public",
    "services",
    "workflow",
}
TRANSLATION_ALLOWED_ROOT_FILES = {
    "__init__.py",
    "README.md",
}
TRANSLATION_WORKFLOW_ALLOWED_DIRS = {
    "__pycache__",
    ".ipynb_checkpoints",
    "batching",
    "legacy",
    "phases",
    "scheduling",
}
TRANSLATION_WORKFLOW_ALLOWED_FILES = {
    "__init__.py",
    "README.md",
    "batch_runner.py",
    "book_flow.py",
    "execution.py",
    "execution_plan.py",
    "execution_runner.py",
    "page_policies.py",
    "page_range.py",
    "pages.py",
    "translation_workflow.py",
    "workers.py",
}
TRANSLATION_WORKFLOW_SUBPACKAGE_RULES: dict[str, tuple[str, ...]] = {
    "phases": (
        "services.translation.workflow.phases",
        "services.translation.workflow.batching.pending_units",
        "services.translation.workflow.pages",
        "services.translation.workflow.page_policies",
        "services.translation.artifacts",
        "services.translation.llm.shared.control_context",
        "services.translation.llm.shared.provider_runtime",
        "services.translation.services.agents",
        "services.translation.services.finalization",
        "services.translation.services.policy",
        "services.translation.services.postprocess",
        "services.pipeline_shared.events",
    ),
    "scheduling": (
        "services.translation.workflow.scheduling",
        "services.translation.llm.shared.control_context",
        "services.translation.llm.shared.orchestration.batched_plain_single",
        "services.translation.llm.shared.orchestration.terminal_payloads",
        "services.translation.llm.shared.tail_retry_queue",
        "services.translation.services.results.applier",
        "services.translation.services.results.flush",
    ),
    "batching": (
        "services.translation.workflow.batching",
        "services.translation.workflow.batch_runner",
        "services.translation.workflow.scheduling",
        "services.translation.core",
        "services.translation.llm",
        "services.translation.services.context",
        "services.translation.services.fast_path",
        "services.translation.services.memory",
        "services.translation.services.results",
    ),
    "legacy": (
        "services.translation.workflow.legacy",
        "services.translation.core.payload",
        "services.translation.llm.shared.orchestration",
        "services.translation.llm.shared.provider_runtime",
        "services.translation.services.continuation",
        "services.translation.services.policy",
    ),
}
# 历史兼容例外已全部随私有名公开化移除（2026-09）；该表保留为空，
# 任何新的跨模块私有导入都会直接报错。
TRANSLATION_WORKFLOW_PRIVATE_IMPORT_EXCEPTIONS: dict[Path, tuple[str, ...]] = {}
TRANSLATION_LAYER_IMPORT_RULES: dict[str, tuple[str, ...]] = {
    "entrypoints": (
        "services.translation.entrypoints",
        "services.translation.public",
        "services.translation.artifacts",
        "services.translation.llm",
        "services.translation.services.terms",
        "services.translation.workflow",
    ),
    "core": (
        "services.translation.core",
    ),
    "workflow": (
        "services.translation.workflow",
        "services.translation.workflow.batching",
        "services.translation.workflow.legacy",
        "services.translation.workflow.phases",
        "services.translation.workflow.scheduling",
        "services.translation.services.classification",
        "services.translation.core",
        "services.translation.services.context",
        "services.translation.services.continuation",
        "services.translation.artifacts",
        "services.translation.services.fast_path",
        "services.translation.services.finalization",
        "services.translation.llm",
        "services.translation.services.memory",
        "services.translation.core.ocr",
        "services.translation.core.orchestration",
        "services.translation.core.payload",
        "services.translation.services.agents",
        "services.translation.services.policy",
        "services.translation.services.postprocess",
        "services.translation.services.results",
        "services.translation.services.terms",
    ),
    "llm": (
        "services.translation.llm",
        "services.translation.core",
        "services.translation.artifacts",
        "services.translation.core.payload",
    ),
    "services": (
        "services.translation.services",
        "services.translation.core",
        "services.translation.core.item_reader",
        "services.translation.llm",
        "services.translation.artifacts",
    ),
    "artifacts": (
        "services.translation.artifacts",
        "services.translation.core",
        "services.translation.core.payload",
    ),
    "public": (
        "services.translation.public",
        "services.translation.artifacts",
        "services.translation.core",
        "services.translation.core.payload",
        "services.translation.core.terms",
        "services.translation.llm.shared.provider_runtime",
        "services.translation.workflow",
    ),
    "policy": (
        "services.translation.services.policy",
        # Historical policy modules still inspect OCR contracts and LLM domain hints.
        # T17-T18 will narrow this to decision-only inputs.
        "services.translation.services.classification",
        "services.translation.core",
        "services.translation.services.context",
        "services.translation.llm.domain_context",
        "services.translation.llm.shared.provider_runtime",
        "services.translation.core.ocr",
        "services.translation.core.payload",
    ),
    "payload": (
        "services.translation.core.payload",
        "services.translation.core",
        "services.translation.core.ocr",
    ),
    "memory": (
        "services.translation.services.memory",
        "services.translation.services.terms",
    ),
    "context": (
        "services.translation.services.context",
        "services.translation.llm.shared.control_context",
        "services.translation.llm.style_hints",
        "services.translation.services.policy",
        "services.translation.services.terms",
    ),
    "ocr": (
        "services.translation.core.ocr",
    ),
    "orchestration": (
        "services.translation.core.orchestration",
        "services.translation.core",
        "services.translation.services.context",
        "services.translation.services.continuation",
        "services.translation.core.ocr",
        "services.translation.core.payload",
    ),
    "continuation": (
        "services.translation.services.continuation",
        "services.translation.services.context",
        # Continuation review currently asks LLM for borderline cases.
        "services.translation.llm",
    ),
    "classification": (
        "services.translation.services.classification",
        "services.translation.core",
        "services.translation.services.context",
        "services.translation.llm",
        "services.translation.core.ocr",
        "services.translation.services.policy",
    ),
    "terms": (
        "services.translation.services.terms",
    ),
    "diagnostics": (
        "services.translation.artifacts",
        "services.translation.services.agents",
        "services.translation.core",
        "services.translation.llm.shared.control_context",
        "services.translation.core.payload",
    ),
    "agents": (
        "services.translation.services.agents",
        "services.translation.llm",
        "services.translation.services.quality",
        "services.translation.services.terms",
    ),
    "quality": (
        "services.translation.core",
        "services.translation.core.item_reader",
        "services.translation.llm",
        "services.translation.services.quality",
        "services.translation.services.terms",
    ),
    "postprocess": (
        "services.translation.services.postprocess",
        "services.translation.llm",
    ),
}
TRANSLATION_LAYER_IMPORT_EXCEPTIONS: dict[Path, tuple[str, ...]] = {
    # Current llm orchestration still bridges workflow-ish retry behavior until T04-T10 migrate runtime flow.
    Path("llm/shared/orchestration/fallbacks.py"): (
        "services.translation.services.postprocess",
    ),
}
TRANSLATION_SHARED_COMPAT_IMPORTS = (
    "services.translation.core.item_reader",
    "services.translation.services.context.session_context",
)
TRANSLATION_REMOVED_COMPAT_IMPORTS = (
    "services.translation.from_ocr_pipeline",
    "services.translation.translate_only_pipeline",
    "services.translation.item_reader",
    "services.translation.session_context",
    "services.translation.services.context.models",
    "services.translation.services.context.unit_context",
    "services.translation.services.terms.glossary",
    "services.translation.services.terms.abbreviations",
    "services.translation.services.terms.injection",
    "services.translation.services.quality.checks",
)
DEVTOOLS_TRANSLATION_INTERNAL_IMPORT_ALLOWLIST = {
    Path("inspect_translation_repair_candidates.py"),
    Path("job_debug_runner.py"),
    Path("replay_translation_item.py"),
    Path("translation_repair_runner.py"),
}
DEVTOOLS_TRANSLATION_INTERNAL_DIR_ALLOWLIST = {
    "experiments",
    "promptfoo",
    "tests",
}


def translation_layer_for(path: Path) -> str | None:
    try:
        parts = path.relative_to(TRANSLATION_ROOT).parts
    except ValueError:
        return None
    if not parts:
        return None
    first = parts[0]
    return first if first in TRANSLATION_ALLOWED_ROOT_DIRS else None

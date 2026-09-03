from __future__ import annotations

from pathlib import Path

from devtools.architecture_checks.common import imported_modules
from devtools.architecture_checks.common import module_allowed
from devtools.architecture_checks.common import read_text
from devtools.architecture_checks.common import rel
from devtools.architecture_checks.common import scan_py_files
from devtools.architecture_checks.providers import MINERU_ROOT
from devtools.architecture_checks.translation_rules import DEVTOOLS_ROOT
from devtools.architecture_checks.translation_rules import DEVTOOLS_TRANSLATION_INTERNAL_DIR_ALLOWLIST
from devtools.architecture_checks.translation_rules import DEVTOOLS_TRANSLATION_INTERNAL_IMPORT_ALLOWLIST
from devtools.architecture_checks.translation_rules import DOCUMENT_SCHEMA_ROOT
from devtools.architecture_checks.translation_rules import TRANSLATE_ONLY_ENTRYPOINT
from devtools.architecture_checks.translation_rules import TRANSLATION_ROOT


def check_translation_worker_protocol(errors: list[str]) -> None:
    translate_only_text = read_text(TRANSLATE_ONLY_ENTRYPOINT)
    if "PipelineEventWriter(" not in translate_only_text:
        errors.append(
            "services/translation/entrypoints/translate_only_pipeline.py: translate-only worker must initialize PipelineEventWriter"
        )
    if "STDOUT_LABEL_EVENTS_JSONL" not in translate_only_text:
        errors.append(
            "services/translation/entrypoints/translate_only_pipeline.py: translate-only worker must publish pipeline_events.jsonl via stdout contract"
        )
    if 'artifact_key="pipeline_events_jsonl"' not in translate_only_text:
        errors.append(
            "services/translation/entrypoints/translate_only_pipeline.py: translate-only worker must publish pipeline_events_jsonl artifact"
        )
    if 'artifact_key="translation_diagnostics_json"' not in translate_only_text:
        errors.append(
            "services/translation/entrypoints/translate_only_pipeline.py: translate-only worker must publish translation_diagnostics_json artifact"
        )
    if '"translation_diagnostics.json"' not in translate_only_text:
        errors.append(
            "services/translation/entrypoints/translate_only_pipeline.py: translate-only worker must keep translation_diagnostics.json as stable diagnostics output"
        )


def check_translation_pipeline_facade_boundary(errors: list[str]) -> None:
    # runtime/pipeline 透传层已移除（2026-09）：translate-only 入口直接经
    # services.translation.public 门面构造 TranslationExecutionRequest 并执行。
    text = read_text(TRANSLATE_ONLY_ENTRYPOINT)
    required = (
        "from services.translation.public import TranslationExecutionRequest",
        "from services.translation.public import execute_translation_request",
    )
    for item in required:
        if item not in text:
            errors.append(
                f"services/translation/entrypoints/translate_only_pipeline.py: must call translation public facade via '{item}'"
            )
    forbidden = (
        "from services.translation.workflow import",
        "from services.translation.workflow.execution import",
        "from runtime.pipeline",
    )
    for item in forbidden:
        if item in text:
            errors.append(
                f"services/translation/entrypoints/translate_only_pipeline.py: must not import workflow internals directly: '{item}'"
            )


def check_translation_public_surface_usage(errors: list[str]) -> None:
    guarded_roots = (
        MINERU_ROOT,
        DOCUMENT_SCHEMA_ROOT,
    )
    allowed_prefixes = (
        "services.translation.public",
        "services.translation.entrypoints",
    )
    forbidden_prefixes = (
        "services.translation.artifacts",
        "services.translation.core",
        "services.translation.llm",
        "services.translation.services",
        "services.translation.workflow",
    )
    for root in guarded_roots:
        for path in scan_py_files(root):
            for module in imported_modules(path):
                if module_allowed(module, allowed_prefixes):
                    continue
                if module_allowed(module, forbidden_prefixes):
                    errors.append(
                        f"{rel(path)}: production code outside translation must import translation contracts through services.translation.public, not '{module}'"
                    )
                    break


def check_devtools_translation_internal_usage(errors: list[str]) -> None:
    forbidden_prefixes = (
        "services.translation.artifacts",
        "services.translation.core",
        "services.translation.llm",
        "services.translation.services",
        "services.translation.workflow",
    )
    for path in scan_py_files(DEVTOOLS_ROOT):
        rel_path = path.relative_to(DEVTOOLS_ROOT)
        if rel_path.parts and rel_path.parts[0] in DEVTOOLS_TRANSLATION_INTERNAL_DIR_ALLOWLIST:
            continue
        if path == Path(__file__).resolve():
            continue
        uses_translation_internal = any(
            module_allowed(module, forbidden_prefixes)
            for module in imported_modules(path)
        )
        if not uses_translation_internal:
            continue
        if rel_path in DEVTOOLS_TRANSLATION_INTERNAL_IMPORT_ALLOWLIST:
            continue
        errors.append(
            f"{rel(path)}: devtools script imports translation internals; add it to DEVTOOLS_TRANSLATION_INTERNAL_IMPORT_ALLOWLIST or use services.translation.public"
        )

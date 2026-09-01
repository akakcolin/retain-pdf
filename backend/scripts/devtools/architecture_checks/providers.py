from __future__ import annotations

from devtools.architecture_checks.common import SCRIPTS_ROOT
from devtools.architecture_checks.common import read_text
from devtools.architecture_checks.common import rel
from devtools.architecture_checks.common import scan_py_files


PIPELINE_ROOT = SCRIPTS_ROOT / "runtime" / "pipeline"
MINERU_ROOT = SCRIPTS_ROOT / "services" / "mineru"
TRANSLATION_ROOT = SCRIPTS_ROOT / "services" / "translation"

PROVIDER_PRIVATE_IMPORT_PATTERNS = (
    "from services.ocr_provider",
    "import services.ocr_provider",
    "from services.mineru",
    "import services.mineru",
)
PROVIDER_RAW_TOKENS = (
    "layoutParsingResults",
    "prunedResult",
    "content_list",
)
PROVIDER_ADAPTER_IMPORT_PATTERNS = (
    "from services.document_schema.provider_adapters",
    "import services.document_schema.provider_adapters",
)


def check_pipeline_provider_leaks(errors: list[str]) -> None:
    for path in scan_py_files(PIPELINE_ROOT):
        text = read_text(path)
        rel_path = rel(path)
        for pattern in PROVIDER_PRIVATE_IMPORT_PATTERNS:
            if pattern in text:
                errors.append(
                    f"{rel_path}: runtime/pipeline must not import provider-specific services directly"
                )
                break
        for token in PROVIDER_RAW_TOKENS:
            if token in text:
                errors.append(
                    f"{rel_path}: runtime/pipeline must not understand provider raw token '{token}'"
                )
        for pattern in PROVIDER_ADAPTER_IMPORT_PATTERNS:
            if pattern in text:
                errors.append(
                    f"{rel_path}: runtime/pipeline must not depend on document_schema provider adapters directly"
                )
                break


def check_service_provider_raw_leaks(errors: list[str]) -> None:
    guarded_roots = (TRANSLATION_ROOT,)
    for root in guarded_roots:
        for path in scan_py_files(root):
            text = read_text(path)
            rel_path = rel(path)
            for pattern in PROVIDER_PRIVATE_IMPORT_PATTERNS + PROVIDER_ADAPTER_IMPORT_PATTERNS:
                if pattern in text:
                    errors.append(
                        f"{rel_path}: translation services must not depend on provider-specific raw adapters"
                    )
                    break
            for token in PROVIDER_RAW_TOKENS:
                if token in text:
                    errors.append(
                        f"{rel_path}: translation services must not consume provider raw token '{token}'"
                    )


__all__ = [
    "check_pipeline_provider_leaks",
    "check_service_provider_raw_leaks",
]

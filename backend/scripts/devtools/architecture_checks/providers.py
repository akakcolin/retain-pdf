from __future__ import annotations

from devtools.architecture_checks.common import SCRIPTS_ROOT
from devtools.architecture_checks.common import read_text
from devtools.architecture_checks.common import rel
from devtools.architecture_checks.common import scan_py_files


PIPELINE_ROOT = SCRIPTS_ROOT / "runtime" / "pipeline"
OCR_PROVIDER_ROOT = SCRIPTS_ROOT / "services" / "ocr_provider"
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
OCR_PROVIDER_FORBIDDEN_IMPORT_PATTERNS = (
    "from runtime.pipeline",
    "import runtime.pipeline",
    "from services.translation",
    "import services.translation",
)
OCR_PROVIDER_DRIVER_REGISTRY = SCRIPTS_ROOT / "services" / "ocr_provider" / "drivers.py"
MINERU_PROVIDER_FLOW_IMPORT = "from services.mineru.job_flow import run_mineru_to_job_dir"
DOCUMENT_SCHEMA_ADAPTERS_ENTRY = SCRIPTS_ROOT / "services" / "document_schema" / "adapters.py"


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


def check_ocr_provider_boundaries(errors: list[str]) -> None:
    for path in scan_py_files(OCR_PROVIDER_ROOT):
        text = read_text(path)
        rel_path = rel(path)
        for pattern in OCR_PROVIDER_FORBIDDEN_IMPORT_PATTERNS:
            if pattern in text:
                errors.append(
                    f"{rel_path}: provider implementation modules must not depend on runtime/translation layers"
                )
                break

    driver_text = read_text(OCR_PROVIDER_DRIVER_REGISTRY)
    if MINERU_PROVIDER_FLOW_IMPORT not in driver_text:
        errors.append(
            "services/ocr_provider/drivers.py: provider registry must own MinerU provider handoff"
        )
    if "run_local_command_ocr_to_job_dir" not in driver_text:
        errors.append(
            "services/ocr_provider/drivers.py: provider registry must expose local OCR command driver"
        )
    if "_PROVIDER_DRIVERS" not in driver_text or "register_ocr_provider_driver" not in driver_text:
        errors.append(
            "services/ocr_provider/drivers.py: provider dispatch must use an explicit registry"
        )
    if "if provider ==" in driver_text:
        errors.append(
            "services/ocr_provider/drivers.py: provider dispatch must not grow provider-specific if chains"
        )

    adapters_text = read_text(DOCUMENT_SCHEMA_ADAPTERS_ENTRY)
    if "from services.mineru" in adapters_text:
        errors.append(
            "services/document_schema/adapters.py: provider registry must route MinerU through document_schema/provider_adapters/mineru"
        )


__all__ = [
    "check_ocr_provider_boundaries",
    "check_pipeline_provider_leaks",
    "check_service_provider_raw_leaks",
]

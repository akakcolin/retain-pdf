from __future__ import annotations

from devtools.architecture_checks.common import SCRIPTS_ROOT
from devtools.architecture_checks.common import read_text
from devtools.architecture_checks.common import rel
from devtools.architecture_checks.common import scan_py_files


PIPELINE_ROOT = SCRIPTS_ROOT / "runtime" / "pipeline"
OCR_PROVIDER_ROOT = SCRIPTS_ROOT / "services" / "ocr_provider"
MINERU_ROOT = SCRIPTS_ROOT / "services" / "mineru"
TRANSLATION_ROOT = SCRIPTS_ROOT / "services" / "translation"
RENDERING_ROOT = SCRIPTS_ROOT / "services" / "rendering"

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
    "from services.rendering",
    "import services.rendering",
)
OCR_PROVIDER_STABLE_ENTRYPOINT = SCRIPTS_ROOT / "services" / "ocr_provider" / "provider_pipeline.py"
OCR_PROVIDER_PACKAGE_INIT = SCRIPTS_ROOT / "services" / "ocr_provider" / "__init__.py"
OCR_PROVIDER_DRIVER_REGISTRY = SCRIPTS_ROOT / "services" / "ocr_provider" / "drivers.py"
MINERU_PROVIDER_FLOW_IMPORT = "from services.mineru.job_flow import run_mineru_to_job_dir"
# build_paddle_lines/tighten_paddle_text_bbox 已随桌面包剔除 Paddle 而内聚到
# paddle_normalize.py（包内排除），不再要求 provider_pipeline 保留兼容别名。
OCR_PROVIDER_COMPAT_SYMBOLS = (
    "adapt_path_to_document_v1_with_report",
    "validate_saved_document_path",
    "save_normalized_document_for_paddle",
)
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
    guarded_roots = (TRANSLATION_ROOT, RENDERING_ROOT)
    for root in guarded_roots:
        for path in scan_py_files(root):
            text = read_text(path)
            rel_path = rel(path)
            for pattern in PROVIDER_PRIVATE_IMPORT_PATTERNS + PROVIDER_ADAPTER_IMPORT_PATTERNS:
                if pattern in text:
                    errors.append(
                        f"{rel_path}: translation/rendering services must not depend on provider-specific raw adapters"
                    )
                    break
            for token in PROVIDER_RAW_TOKENS:
                if token in text:
                    errors.append(
                        f"{rel_path}: translation/rendering services must not consume provider raw token '{token}'"
                    )


def check_ocr_provider_boundaries(errors: list[str]) -> None:
    for path in scan_py_files(OCR_PROVIDER_ROOT):
        text = read_text(path)
        rel_path = rel(path)
        if path != OCR_PROVIDER_STABLE_ENTRYPOINT:
            for pattern in OCR_PROVIDER_FORBIDDEN_IMPORT_PATTERNS:
                if pattern in text:
                    errors.append(
                        f"{rel_path}: provider implementation modules must not depend on runtime/translation/rendering layers"
                    )
                    break

    init_text = read_text(OCR_PROVIDER_PACKAGE_INIT)
    if "from . import provider_pipeline" not in init_text:
        errors.append(
            "services/ocr_provider/__init__.py: package must explicitly re-export provider_pipeline"
        )
    if '__all__ = ["provider_pipeline"]' not in init_text:
        errors.append(
            "services/ocr_provider/__init__.py: package must pin provider_pipeline as explicit public surface"
        )

    entry_text = read_text(OCR_PROVIDER_STABLE_ENTRYPOINT)
    if "from runtime.pipeline.book_pipeline import run_book_pipeline" not in entry_text:
        errors.append(
            "services/ocr_provider/provider_pipeline.py: stable provider entry must own the handoff to run_book_pipeline"
        )
    if "run_registered_ocr_provider" not in entry_text:
        errors.append(
            "services/ocr_provider/provider_pipeline.py: stable provider entry must route OCR through provider registry"
        )
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
    for symbol in OCR_PROVIDER_COMPAT_SYMBOLS:
        if f"{symbol}" not in entry_text:
            errors.append(
                f"services/ocr_provider/provider_pipeline.py: stable provider entry must preserve compat symbol '{symbol}'"
            )

    for path in scan_py_files(SCRIPTS_ROOT / "entrypoints"):
        text = read_text(path)
        if MINERU_PROVIDER_FLOW_IMPORT in text:
            errors.append(
                f"{rel(path)}: entrypoints must route MinerU through services/ocr_provider/provider_pipeline.py"
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

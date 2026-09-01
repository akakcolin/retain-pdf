"""P0-1: the Python normalize engine is retired; normalize runs only via native
`render_rs --normalize-ocr`. Reintroducing any retired file path or import is a
regression to a second normalize implementation."""
from __future__ import annotations

from pathlib import Path

from devtools.architecture_checks.common import SCRIPTS_ROOT
from devtools.architecture_checks.common import imported_modules
from devtools.architecture_checks.common import rel
from devtools.architecture_checks.common import scan_py_files

RETIRED_NORMALIZE_FILES: tuple[Path, ...] = (
    Path("entrypoints/run_normalize_ocr.py"),
    Path("services/document_schema/normalize_pipeline.py"),
    Path("services/ocr_provider/paddle_normalize.py"),
    Path("services/mineru/normalize_pipeline.py"),
    # Retired second Python normalize implementation; only the native worker remains.
    Path("services/document_schema/adapters.py"),
    Path("services/document_schema/contract_v1.py"),
    Path("services/document_schema/toc.py"),
    Path("services/document_schema/markdown_serializer.py"),
    Path("services/document_schema/providers.py"),
    Path("services/document_schema/provider_adapters"),
    Path("services/ocr_provider"),
    Path("services/mineru/artifacts.py"),
    Path("services/mineru/document_v1.py"),
    Path("services/mineru/job_flow.py"),
    Path("services/mineru/mineru_api.py"),
    Path("services/mineru/mineru_job.py"),
    Path("services/mineru/ocr_pipeline.py"),
    Path("services/mineru/submission.py"),
)
RETIRED_NORMALIZE_IMPORTS: tuple[str, ...] = (
    "services.document_schema.normalize_pipeline",
    "services.ocr_provider.paddle_normalize",
    "services.mineru.normalize_pipeline",
    "services.document_schema.adapters",
    "services.document_schema.contract_v1",
    "services.document_schema.toc",
    "services.document_schema.markdown_serializer",
    "services.document_schema.providers",
    "services.document_schema.provider_adapters",
    "services.ocr_provider",
    "services.mineru.artifacts",
    "services.mineru.document_v1",
    "services.mineru.job_flow",
    "services.mineru.mineru_api",
    "services.mineru.mineru_job",
    "services.mineru.ocr_pipeline",
    "services.mineru.submission",
)


def _imports_retired_normalize(module: str) -> bool:
    return any(
        module == retired or module.startswith(retired + ".")
        for retired in RETIRED_NORMALIZE_IMPORTS
    )


def check_normalize_native_only(errors: list[str]) -> None:
    for rel_path in RETIRED_NORMALIZE_FILES:
        if (SCRIPTS_ROOT / rel_path).exists():
            errors.append(
                f"{rel_path}: Python normalize engine is retired; normalize must run via native render_rs --normalize-ocr"
            )
    for path in scan_py_files(SCRIPTS_ROOT):
        for module in imported_modules(path):
            if _imports_retired_normalize(module):
                errors.append(
                    f"{rel(path)}: import of retired Python normalize engine '{module}' is banned"
                )
    if not errors:
        print("normalize engine: single native implementation only (render_rs --normalize-ocr)")


__all__ = ["RETIRED_NORMALIZE_FILES", "RETIRED_NORMALIZE_IMPORTS", "check_normalize_native_only"]

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
)
RETIRED_NORMALIZE_IMPORTS: tuple[str, ...] = (
    "services.document_schema.normalize_pipeline",
    "services.ocr_provider.paddle_normalize",
    "services.mineru.normalize_pipeline",
)


def check_normalize_native_only(errors: list[str]) -> None:
    for rel_path in RETIRED_NORMALIZE_FILES:
        if (SCRIPTS_ROOT / rel_path).exists():
            errors.append(
                f"{rel_path}: Python normalize engine is retired; normalize must run via native render_rs --normalize-ocr"
            )
    for path in scan_py_files(SCRIPTS_ROOT):
        for module in imported_modules(path):
            if module in RETIRED_NORMALIZE_IMPORTS:
                errors.append(
                    f"{rel(path)}: import of retired Python normalize engine '{module}' is banned"
                )
    if not errors:
        print("normalize engine: single native implementation only (render_rs --normalize-ocr)")


__all__ = ["RETIRED_NORMALIZE_FILES", "RETIRED_NORMALIZE_IMPORTS", "check_normalize_native_only"]

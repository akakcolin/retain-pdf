from __future__ import annotations

from pathlib import Path

from devtools.architecture_checks.common import SCRIPTS_ROOT
from devtools.architecture_checks.common import imported_modules
from devtools.architecture_checks.common import rel
from devtools.architecture_checks.common import scan_py_files

#: Terminal-state criterion 4 (doc 15): the fitz import surface is an enumerable
#: allowlist. Every non-devtools module importing fitz/PyMuPDF must be registered
#: here with a category + reason; `devtools/` is blanket-exempt. Categories:
#:
#: - ``hard_boundary`` — fitz API with no mupdf-rs equivalent (words-clip,
#:   get_texttrace, per-drawing zigzag rects, get_pixmap sampling) or a
#:   Python-only PDF rewrite/redaction stage deliberately not ported.
#: - ``non_default_write`` — fitz writes an output PDF only on a non-production path.
#: - ``non_render_service`` — service-side fitz utilities (translation / ocr_provider).
FITZ_IMPORT_ALLOWLIST: dict[Path, tuple[str, str]] = {
    # ---- non_render_service: service-side fitz utilities (translation / ocr_provider)
    Path("services/translation/llm/domain_context.py"): (
        "non_render_service",
        "fitz.open + get_text('text') preview for LLM domain inference",
    ),
    # ---- hard_boundary: Python-only PDF merge (show_pdf_page) for the
    # side-by-side derived-artifact download; no mupdf-rs equivalent ported.
    Path("services/derived_artifacts/side_by_side_pdf.py"): (
        "hard_boundary",
        "fitz show_pdf_page left/right merge for the side-by-side download",
    ),
}

FITZ_IMPORT_CATEGORIES = (
    "hard_boundary",
    "non_default_write",
    "non_render_service",
)
_FITZ_MODULES = ("fitz", "pymupdf")


def _imports_fitz(path: Path) -> bool:
    return any(
        module in _FITZ_MODULES
        or module.startswith("fitz.")
        or module.startswith("pymupdf.")
        for module in imported_modules(path)
    )


def check_fitz_import_allowlist(errors: list[str]) -> None:
    """Criterion 4: every fitz-importing non-devtools module is allowlisted with a
    reason; stale entries (module dropped fitz) and un-allowlisted new imports fail."""
    allowlisted = set(FITZ_IMPORT_ALLOWLIST)
    seen: set[Path] = set()
    for path in scan_py_files(SCRIPTS_ROOT):
        rel_path = rel(path)
        if rel_path.parts and rel_path.parts[0] == "devtools":
            continue
        if not _imports_fitz(path):
            continue
        if rel_path not in allowlisted:
            errors.append(
                f"{rel_path}: fitz import without an allowlist entry; add it to "
                "FITZ_IMPORT_ALLOWLIST with a category + reason"
            )
        else:
            seen.add(rel_path)
    for rel_path, (category, reason) in FITZ_IMPORT_ALLOWLIST.items():
        if rel_path not in seen:
            errors.append(
                f"{rel_path}: stale fitz allowlist entry — module no longer imports fitz; remove it"
            )
        if category not in FITZ_IMPORT_CATEGORIES:
            errors.append(f"{rel_path}: unknown fitz allowlist category {category!r}")
        if not reason.strip():
            errors.append(f"{rel_path}: fitz allowlist entry missing a reason")
    if not errors:
        print(f"fitz import allowlist: {len(seen)} non-devtools modules enumerated")


__all__ = ["FITZ_IMPORT_ALLOWLIST", "check_fitz_import_allowlist"]

from __future__ import annotations

import re
from pathlib import Path

from devtools.architecture_checks.common import REPO_ROOT
from devtools.architecture_checks.common import SCRIPTS_ROOT
from devtools.architecture_checks.common import imported_modules
from devtools.architecture_checks.common import read_text
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


RUST_SRC_ROOT = REPO_ROOT / "backend" / "rust_api" / "src"
DESKTOP_REQUIREMENTS = REPO_ROOT / "desktop" / "requirements-desktop-posix.txt"
_INLINE_FITZ_RE = re.compile(r"^\s*import (fitz|pymupdf)\b", re.MULTILINE)


def _rust_production_text(path: Path) -> str:
    return read_text(path).split("\n#[cfg(test)]", 1)[0]


def check_desktop_bundle_covers_rust_fitz(errors: list[str]) -> None:
    """rust_api 的内联 python 脚本(preview/upload)同样消费 fitz,但对 Python 侧
    import 扫描不可见——desktop 打包曾因此剪掉 PyMuPDF 导致上传/预览接口 500。
    只要 Rust 生产代码还有内联 fitz,desktop 依赖清单就必须包含 PyMuPDF。"""
    rust_users: list[str] = []
    for path in sorted(RUST_SRC_ROOT.rglob("*.rs")):
        if path.name.startswith("._"):  # macOS 网络卷 AppleDouble 元数据
            continue
        rel_parts = path.relative_to(RUST_SRC_ROOT).parts
        if "api_tests" in rel_parts or path.stem in {"tests", "test"}:
            continue
        if _INLINE_FITZ_RE.search(_rust_production_text(path)):
            rust_users.append(str(path.relative_to(REPO_ROOT)))
    if not rust_users:
        print("desktop bundle fitz coverage: no inline fitz in rust_api, PyMuPDF not required")
        return
    requirements = read_text(DESKTOP_REQUIREMENTS).lower()
    if "pymupdf" not in requirements:
        errors.append(
            "desktop/requirements-desktop-posix.txt 缺少 PyMuPDF,但 rust_api 生产代码仍有内联 "
            f"fitz 调用: {', '.join(rust_users)};请在 pyproject.toml 的 desktop extra 恢复 "
            "PyMuPDF 并运行 devtools/sync_python_requirements.py"
        )
    else:
        print(
            "desktop bundle fitz coverage: "
            f"PyMuPDF covers {len(rust_users)} inline fitz call sites in rust_api"
        )


__all__ = [
    "FITZ_IMPORT_ALLOWLIST",
    "check_desktop_bundle_covers_rust_fitz",
    "check_fitz_import_allowlist",
]

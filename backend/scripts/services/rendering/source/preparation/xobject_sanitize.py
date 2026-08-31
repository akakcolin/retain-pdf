from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class XObjectSanitizeResult:
    changed: bool = False
    output_pdf_path: Path | None = None
    invalid_image_xobjects: int = 0
    pages_changed: int = 0
    elapsed_seconds: float = 0.0


def build_invalid_xobject_sanitized_pdf_copy(
    *,
    source_pdf_path: Path,
    output_pdf_path: Path,
) -> XObjectSanitizeResult:
    """Route to the native Rust sanitize (see `_native.py`)."""
    from services.rendering.source import _native

    return _native.sanitize_pdf_copy(
        source_pdf_path=source_pdf_path,
        output_pdf_path=output_pdf_path,
    )


__all__ = [
    "XObjectSanitizeResult",
    "build_invalid_xobject_sanitized_pdf_copy",
]

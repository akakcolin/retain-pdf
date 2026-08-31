from __future__ import annotations

from pathlib import Path


def compress_pdf_images_only_impl(
    pdf_path: Path,
    *,
    dpi: int = 200,
) -> bool:
    """Route to the native Rust image compression (see `_native.py`)."""
    from services.rendering.source import _native

    return _native.compress_images_only(pdf_path, dpi=dpi)

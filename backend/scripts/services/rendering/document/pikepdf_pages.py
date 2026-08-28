from __future__ import annotations

from pathlib import Path

import pikepdf


def copy_pdf_with_pikepdf(*, source_pdf_path: Path, output_pdf_path: Path) -> Path:
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    with pikepdf.Pdf.open(source_pdf_path) as pdf:
        pdf.save(
            output_pdf_path,
            object_stream_mode=pikepdf.ObjectStreamMode.generate,
            compress_streams=True,
            recompress_flate=False,
        )
    return output_pdf_path


def optimize_pdf_file_with_pikepdf(*, input_pdf_path: Path, output_pdf_path: Path | None = None) -> Path:
    target = output_pdf_path or input_pdf_path
    tmp_path = target.with_suffix(f"{target.suffix}.pikepdf-tmp.pdf")
    with pikepdf.Pdf.open(input_pdf_path, allow_overwriting_input=True) as pdf:
        pdf.save(
            tmp_path,
            object_stream_mode=pikepdf.ObjectStreamMode.generate,
            compress_streams=True,
            recompress_flate=False,
        )
    tmp_path.replace(target)
    return target


def _extract_pages_with_pikepdf_python(
    *,
    source_pdf_path: Path,
    output_pdf_path: Path,
    start_page: int,
    end_page: int,
) -> Path:
    """Pure-Python reference for `extract_pages_with_pikepdf` (routed through
    `_native.py` when the native module is built)."""
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    with pikepdf.Pdf.open(source_pdf_path) as source_pdf:
        last_page = len(source_pdf.pages) - 1
        start = max(0, int(start_page))
        end = last_page if end_page < 0 else min(int(end_page), last_page)
        output_pdf = pikepdf.Pdf.new()
        if start <= end:
            output_pdf.pages.extend(source_pdf.pages[start : end + 1])
        output_pdf.save(
            output_pdf_path,
            object_stream_mode=pikepdf.ObjectStreamMode.generate,
            compress_streams=True,
            recompress_flate=False,
        )
    return output_pdf_path


def extract_pages_with_pikepdf(
    *,
    source_pdf_path: Path,
    output_pdf_path: Path,
    start_page: int,
    end_page: int,
) -> Path:
    """Route to the native Rust page extraction when built (see `_native.py`);
    otherwise the pure-Python implementation above."""
    import services.rendering.source._native as _native

    return _native.extract_pages(
        source_pdf_path=source_pdf_path,
        output_pdf_path=output_pdf_path,
        start_page=start_page,
        end_page=end_page,
    )

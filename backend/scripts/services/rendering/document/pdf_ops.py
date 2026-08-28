from __future__ import annotations

from pathlib import Path
import time

import fitz


def save_optimized_pdf(doc: fitz.Document, output_pdf_path: Path) -> None:
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    print("save optimized pdf: subset fonts", flush=True)
    doc.subset_fonts()
    # Byte compaction (garbage collection + stream compression) runs in Rust when
    # the bridge is built; font subsetting stays on fitz (mupdf-rs has no subset
    # API). Fall back to the pure-fitz save on any native failure.
    import services.rendering.source._native as _native

    if _native.NATIVE:
        try:
            print("save optimized pdf: native compaction", flush=True)
            out = _native.save_optimized(doc.tobytes())
            output_pdf_path.write_bytes(out)
            print(f"save optimized pdf: done {output_pdf_path}", flush=True)
            return
        except Exception:
            print("save optimized pdf: native save failed, falling back to fitz", flush=True)
    _save_optimized_pdf_python(doc, output_pdf_path)


def _save_optimized_pdf_python(doc: fitz.Document, output_pdf_path: Path) -> None:
    print(f"save optimized pdf: writing {output_pdf_path}", flush=True)
    doc.save(
        output_pdf_path,
        garbage=4,
        deflate=True,
        deflate_images=True,
        deflate_fonts=True,
        use_objstms=1,
    )
    print(f"save optimized pdf: done {output_pdf_path}", flush=True)


def save_fast_pdf(doc: fitz.Document, output_pdf_path: Path) -> None:
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    started = time.perf_counter()
    print(f"save fast pdf: writing {output_pdf_path}", flush=True)
    doc.save(output_pdf_path)
    print(f"save fast pdf: done {output_pdf_path} elapsed={time.perf_counter() - started:.2f}s", flush=True)


def strip_page_links(page: fitz.Page) -> None:
    return

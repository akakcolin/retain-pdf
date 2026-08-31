from __future__ import annotations

from pathlib import Path
import time

from services.rendering import _routing


def save_optimized_pdf(doc: fitz.Document, output_pdf_path: Path) -> None:
    """Save `doc` with font subsetting + byte compaction (garbage collection +
    stream compression) run together in Rust (`subset_and_clean` in
    `source/_native.py`). Native-only: the bridge is mandatory; when it is
    unavailable this raises instead of running the retired fitz subset+save
    reference (a native bridge failure also propagates after recording the
    fallback for D5)."""
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    import services.rendering.source._native as _native

    if not _routing.routed("source", "save_optimized", _native.NATIVE):
        raise RuntimeError(
            "source.save_optimized is native-only: the rendering_bridge "
            "subset_and_save_optimized_pdf is required"
        )
    try:
        print("save optimized pdf: native subset + compaction", flush=True)
        out = _native.save_optimized(doc.tobytes())
        output_pdf_path.write_bytes(out)
        print(f"save optimized pdf: done {output_pdf_path}", flush=True)
    except Exception:
        _routing.record_fallback(
            "source",
            "save_optimized",
            _routing.FallbackReason.NATIVE_BRIDGE_ERROR,
        )
        raise


def save_fast_pdf(doc: fitz.Document, output_pdf_path: Path) -> None:
    """Raw save (no subset/compaction) routed to the native bridge. `doc.tobytes()`
    materializes the native-produced doc (accepted fitz boundary, like
    `save_optimized_pdf`); the bridge re-saves raw and Python writes the bytes.
    Native-only: the bridge is mandatory; when it is unavailable this raises
    instead of the retired fitz `doc.save` write."""
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    import services.rendering.source._native as _native

    if not _routing.routed("source", "save_fast", _native.NATIVE):
        raise RuntimeError(
            "source.save_fast is native-only: the rendering_bridge "
            "save_fast_pdf is required"
        )
    started = time.perf_counter()
    print(f"save fast pdf: writing {output_pdf_path}", flush=True)
    try:
        out = _native.save_fast(doc.tobytes())
        output_pdf_path.write_bytes(out)
    except Exception:
        _routing.record_fallback(
            "source", "save_fast", _routing.FallbackReason.NATIVE_BRIDGE_ERROR
        )
        raise
    print(f"save fast pdf: done {output_pdf_path} elapsed={time.perf_counter() - started:.2f}s", flush=True)


def strip_page_links(page: fitz.Page) -> None:
    return

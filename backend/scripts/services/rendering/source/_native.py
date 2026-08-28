"""Optional native (Rust) backend for the source write-path primitives.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes the write-path
primitives (image XObject sanitize, image-only compression, page extraction)
through the ported Rust implementations. Without the native module every call
falls back to the pure-Python implementation, so importing this module is always
safe.

The Rust side returns `(pdf_bytes, metadata_json)` for the transformations that
expose a production result contract (`sanitize_invalid_xobjects`,
`compress_images`); this shim parses the metadata to rebuild the production
result dataclasses / return values without re-deriving them.

Routed here: `build_invalid_xobject_sanitized_pdf_copy`,
`compress_pdf_images_only_impl`, `extract_pages_with_pikepdf`, the
prewarm page-count / page-width lookup
`prewarm_payload._read_source_page_sizes_and_count_python`, and
`document.pdf_ops.save_optimized_pdf`'s byte-compaction half (garbage
collection + stream compression; font subsetting stays on fitz). The other
write-path primitives are deliberately NOT routed:

Documented divergence — native `compress_images` uses the Rust `image` crate
JPEG encoder (fixed Huffman tables, 4:4:4 chroma), which emits larger output
than PIL's `optimize=True` + 4:2:0 subsampling (measured 1.04x-3.97x per image
at q78). On images displayed at >= native resolution native may therefore skip
a recompression the Python reference would apply. The per-image commit gate
(strictly smaller) and the file-level `replace_if_smaller` gate still hold, so
native never grows a file and never re-encodes an image Python would keep;
differential coverage stays on resize-dominated images where both commit.
  * `strip_hidden_text` — the Rust port rewrites every page while production
    pre-scans candidate pages (`page_is_pseudo_editable_scan`) within an
    optional `start_page`/`end_page` range, so routing would change behavior.
  * `strip_bbox_text_rects` — production's rich skip/candidate metadata has no
    bridge equivalent yet.
  * overlay family — bridge `overlay_page` is per-page (one open/save cycle per
    call), so a multi-page overlay would regress to N saves vs production's
    single save; the book/diagnostic call sites are also not covered.

Routed here (drawing-read family): `vector_text.collect_vector_text_rects` and
`vector_profile.page_drawing_count`. These take a file-backed `fitz.Page`
(production opens the source by path, so `page.parent.name` is the source
path); the shim re-opens that file natively and feeds the page bytes to the
bridge. In-memory pages / bridge failures fall back to the pure-Python
reference. `vector_text` reuses the Rust classifier directly (it must, since
mupdf always RGB-converts fills and only the native classifier's
`cs.n() == 3` gate matches Python's `len(fill) == 3`); `page_drawing_count`
routes through the native count primitive (exact parity — count is a
`drawings()` length on both sides).

`vector_profile.collect_page_drawing_rects` is deliberately NOT routed: the
native per-drawing rect can exceed fitz `get_cdrawings` on stroked zigzag
paths (PyMuPDF's lineart device drops the last path item's first point, so
fitz reports a smaller rect than the display-list bound). Replicating that
quirk is fragile and the rects feed per-item overlap decisions, so the shim
relays the pure-Python reference to preserve output exactly.

Routed here (background-image read family):
`background.detect.page_has_large_background_image` routes through the native
placement-rect read (`read_page_image_rects`) when the page is file-backed.
The native rects match fitz `get_image_info` bboxes (same `fz_bound_image`
path: display-list `fill_image` + `fill_image_mask` placements in content /
rotation-stripped space), so the coverage/tiling boolean is computed from them
in Python, reusing the reference's shared helpers. `background.detect.pick_primary_background_image`
stays on the pure-Python reference: it returns the xref that drives the actual
image rewrite, and mupdf-rs exposes no placement→xref association, so native
cannot reproduce it.

Routed here (cleanup text-read family): `cleanup.text_extract.extract_page_text_spans`
and `extract_page_text_blocks` (fitz `get_text("dict")`/`get_text("blocks")`
consumers) and `cleanup.math_spans.collect_page_math_protection_rects` /
`collect_page_non_math_span_heights`. The native collectors build the text page
with fitz's `get_text("dict")` flags (PRESERVE_LIGATURES | PRESERVE_IMAGES |
PRESERVE_WHITESPACE) in rotation-stripped content space, group spans by (font
name, size, RGB color, flags), and the shim deserializes the JSON back into
`fitz.Rect`/text tuples. `cleanup.text_extract.extract_item_word_entries`
stays on the pure-Python reference: fitz `get_text("words", clip=...)` truncates
words by glyph-ink bbox, which mupdf-rs cannot reproduce.
"""

from __future__ import annotations

import json
import time
from pathlib import Path

import fitz

from services.rendering.document.pikepdf_pages import _extract_pages_with_pikepdf_python
from services.rendering.source.compression.image_pipeline import _compress_pdf_images_only_impl_python
from services.rendering.source.preparation.xobject_sanitize import (
    XObjectSanitizeResult,
    _build_invalid_xobject_sanitized_pdf_copy_python,
)

try:
    from rendering_bridge import collect_vector_text_rects as _native_collect_vector_text_rects
    from rendering_bridge import compress_images as _native_compress_images
    from rendering_bridge import extract_pages as _native_extract_pages
    from rendering_bridge import read_page_drawing_count as _native_read_page_drawing_count
    from rendering_bridge import read_page_geometry as _native_read_page_geometry
    from rendering_bridge import read_page_image_rects as _native_read_page_image_rects
    from rendering_bridge import read_page_math_rects as _native_read_page_math_rects
    from rendering_bridge import read_page_span_heights as _native_read_page_span_heights
    from rendering_bridge import read_page_text_blocks as _native_read_page_text_blocks
    from rendering_bridge import read_page_text_spans as _native_read_page_text_spans
    from rendering_bridge import sanitize_invalid_xobjects as _native_sanitize_invalid_xobjects
    from rendering_bridge import save_optimized_pdf as _native_save_optimized_pdf

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def sanitize_pdf_copy(
    *,
    source_pdf_path: Path,
    output_pdf_path: Path,
) -> XObjectSanitizeResult:
    """`xobject_sanitize.build_invalid_xobject_sanitized_pdf_copy`, routed to the
    native bridge when built; otherwise the pure-Python implementation."""
    if not NATIVE:
        return _build_invalid_xobject_sanitized_pdf_copy_python(
            source_pdf_path=source_pdf_path,
            output_pdf_path=output_pdf_path,
        )
    started = time.perf_counter()
    out_bytes, meta_json = _native_sanitize_invalid_xobjects(source_pdf_path.read_bytes())
    meta = json.loads(meta_json)
    if not meta["changed"]:
        output_pdf_path.unlink(missing_ok=True)
        return XObjectSanitizeResult(elapsed_seconds=time.perf_counter() - started)
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    output_pdf_path.write_bytes(out_bytes)
    elapsed = time.perf_counter() - started
    print(
        f"invalid xobject sanitize: replaced_images={meta['invalid_image_xobjects']} "
        f"pages={meta['pages_changed']} elapsed={elapsed:.2f}s output={output_pdf_path}",
        flush=True,
    )
    return XObjectSanitizeResult(
        changed=True,
        output_pdf_path=output_pdf_path,
        invalid_image_xobjects=meta["invalid_image_xobjects"],
        pages_changed=meta["pages_changed"],
        elapsed_seconds=elapsed,
    )


def compress_images_only(pdf_path: Path, *, dpi: int = 200) -> bool:
    """`image_pipeline.compress_pdf_images_only_impl`, routed to the native
    bridge when built; otherwise the pure-Python implementation. Mirrors
    `replace_if_smaller`: commits only when the native output is strictly
    smaller than the input file."""
    if not NATIVE:
        return _compress_pdf_images_only_impl_python(pdf_path, dpi=dpi)
    if dpi <= 0 or not pdf_path.exists():
        return False
    input_bytes = pdf_path.read_bytes()
    out_bytes, meta_json = _native_compress_images(input_bytes, int(dpi))
    meta = json.loads(meta_json)
    if not meta["changed"] or len(out_bytes) >= len(input_bytes):
        return False
    pdf_path.write_bytes(out_bytes)
    return True


def extract_pages(
    *,
    source_pdf_path: Path,
    output_pdf_path: Path,
    start_page: int,
    end_page: int,
) -> Path:
    """`pikepdf_pages.extract_pages_with_pikepdf`, routed to the native bridge
    when built; otherwise the pure-Python implementation."""
    if not NATIVE:
        return _extract_pages_with_pikepdf_python(
            source_pdf_path=source_pdf_path,
            output_pdf_path=output_pdf_path,
            start_page=start_page,
            end_page=end_page,
        )
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    out_bytes = _native_extract_pages(
        source_pdf_path.read_bytes(),
        int(start_page),
        int(end_page),
    )
    output_pdf_path.write_bytes(out_bytes)
    return output_pdf_path


def save_optimized(pdf_bytes: bytes) -> bytes:
    """`document.pdf_ops.save_optimized_pdf`'s byte-compaction half (garbage
    collection + stream compression), routed to the native bridge when built;
    otherwise a bytes-level pure-Python reference. Font subsetting is applied by
    production (fitz `subset_fonts()` + `tobytes()`) before this is called, so
    native output stays within a few percent of the fitz `save` options
    (measured 1.000-1.034x on golden + synthetic CJK/image PDFs)."""
    if not NATIVE:
        return _save_optimized_pdf_bytes_python(pdf_bytes)
    return _native_save_optimized_pdf(pdf_bytes)


def _save_optimized_pdf_bytes_python(pdf_bytes: bytes) -> bytes:
    import io

    doc = fitz.open(stream=pdf_bytes, filetype="pdf")
    buf = io.BytesIO()
    try:
        doc.save(
            buf,
            garbage=4,
            deflate=True,
            deflate_images=True,
            deflate_fonts=True,
            use_objstms=1,
        )
    finally:
        doc.close()
    return buf.getvalue()


def read_page_sizes_and_count(*, source_pdf_path: Path) -> tuple[int, dict[int, float]]:
    """`prewarm_payload`'s page-count + page-width lookup, routed to the native
    bridge when built; otherwise the pure-Python reference. Returns
    `(page_count, {idx: width_pt})` with width == fitz `page.rect` width
    (rotation-applied bounds).

    The reference import is lazy: `prewarm_payload` imports this shim at module
    top, so a top-level reference import here would be circular."""
    if not NATIVE:
        from services.rendering.source.prewarm_payload import (
            _read_source_page_sizes_and_count_python,
        )

        return _read_source_page_sizes_and_count_python(source_pdf_path=source_pdf_path)
    try:
        raw = json.loads(_native_read_page_geometry(source_pdf_path.read_bytes()))
        page_count = int(raw["page_count"])
        widths = {int(idx): rect[2] - rect[0] for idx, rect in raw["rects"].items()}
        return page_count, widths
    except Exception:
        return 0, {}


def _page_source_pdf_path(page: fitz.Page) -> str:
    """The on-disk path backing `page`, or "" for an in-memory page. Production
    opens the source by path, so `page.parent.name` is the path; empty means the
    shim must fall back to the reference."""
    return getattr(getattr(page, "parent", None), "name", "") or ""


def collect_vector_text_rects(*, page: fitz.Page, target_rects: list[fitz.Rect]) -> list[fitz.Rect]:
    """`vector_text.collect_vector_text_rects`, routed to the native bridge
    when built on a file-backed page; otherwise the pure-Python reference.

    The reference import is lazy to avoid a circular import: `vector_text`
    imports this shim at module top."""
    path = _page_source_pdf_path(page)
    if NATIVE and path:
        try:
            targets = json.dumps([[r.x0, r.y0, r.x1, r.y1] for r in target_rects])
            raw = json.loads(
                _native_collect_vector_text_rects(
                    Path(path).read_bytes(),
                    int(page.number),
                    targets,
                )
            )
            return [fitz.Rect(rect) for rect in raw]
        except Exception:
            pass
    from services.rendering.source.vector_text import _collect_vector_text_rects_python

    return _collect_vector_text_rects_python(page, target_rects)


def collect_page_drawing_rects(*, page: fitz.Page) -> list[fitz.Rect]:
    """`vector_profile.collect_page_drawing_rects`, relayed to the pure-Python
    reference (see module docstring: native rects diverge from fitz on stroked
    zigzag paths, so routing here would change redaction output).

    The reference import is lazy to avoid a circular import: `vector_profile`
    imports this shim at module top."""
    from services.rendering.source.vector_profile import _collect_page_drawing_rects_python

    return _collect_page_drawing_rects_python(page)


def page_drawing_count(*, page: fitz.Page) -> int:
    """`vector_profile.page_drawing_count`, routed to the native bridge when
    built on a file-backed page; otherwise the pure-Python reference. Count
    parity is exact (`drawings()` length on both sides).

    The reference import is lazy to avoid a circular import: `vector_profile`
    imports this shim at module top."""
    path = _page_source_pdf_path(page)
    if NATIVE and path:
        try:
            return int(
                _native_read_page_drawing_count(Path(path).read_bytes(), int(page.number))
            )
        except Exception:
            pass
    from services.rendering.source.vector_profile import _page_drawing_count_python

    return _page_drawing_count_python(page)


def page_has_large_background_image(*, page: fitz.Page, coverage_ratio_threshold: float = 0.75) -> bool:
    """`background.detect.page_has_large_background_image`, routed to the native
    bridge when built on a file-backed page; otherwise the pure-Python
    reference. The native path reads image placement rects from the bridge
    (display-list `fill_image` bounds == fitz `get_image_info` bboxes) and
    computes the boolean from them in Python, sharing the reference's
    coverage/tiling helpers. `pick_primary_background_image` (the xref-bearing
    picker that drives the actual image rewrite) is NOT routed — see the module
    docstring.

    The reference import is lazy to avoid a circular import: `detect` imports
    this shim at module top."""
    path = _page_source_pdf_path(page)
    if NATIVE and path:
        try:
            from services.rendering.source.background.detect import (
                _has_large_background_image_from_rects,
            )

            raw = json.loads(
                _native_read_page_image_rects(Path(path).read_bytes(), int(page.number))
            )
            return _has_large_background_image_from_rects(
                [fitz.Rect(rect) for rect in raw],
                page.rect,
                coverage_ratio_threshold=coverage_ratio_threshold,
            )
        except Exception:
            pass
    from services.rendering.source.background.detect import _page_has_large_background_image_python

    return _page_has_large_background_image_python(
        page, coverage_ratio_threshold=coverage_ratio_threshold
    )


def _deserialize_text_entries(raw: str) -> list[tuple[fitz.Rect, str]]:
    """`[[x0,y0,x1,y1,text], ...]` (native text-spans / text-blocks output) →
    `(Rect, text)` pairs."""
    return [(fitz.Rect(item[:4]), str(item[4])) for item in json.loads(raw)]


def _deserialize_rects(raw: str) -> list[fitz.Rect]:
    return [fitz.Rect(item) for item in json.loads(raw)]


def extract_page_text_spans(*, page: fitz.Page) -> list[tuple[fitz.Rect, str]]:
    """`cleanup.text_extract.extract_page_text_spans`, routed to the native
    bridge when built on a file-backed page; otherwise the pure-Python
    reference.

    The reference import is lazy to avoid a circular import: `text_extract`
    imports this shim at module top."""
    path = _page_source_pdf_path(page)
    if NATIVE and path:
        try:
            return _deserialize_text_entries(
                _native_read_page_text_spans(Path(path).read_bytes(), int(page.number))
            )
        except Exception:
            pass
    from services.rendering.source.cleanup.text_extract import _extract_page_text_spans_python

    return _extract_page_text_spans_python(page)


def extract_page_text_blocks(*, page: fitz.Page) -> list[tuple[fitz.Rect, str]]:
    """`cleanup.text_extract.extract_page_text_blocks`, routed to the native
    bridge when built on a file-backed page; otherwise the pure-Python
    reference."""
    path = _page_source_pdf_path(page)
    if NATIVE and path:
        try:
            return _deserialize_text_entries(
                _native_read_page_text_blocks(Path(path).read_bytes(), int(page.number))
            )
        except Exception:
            pass
    from services.rendering.source.cleanup.text_extract import _extract_page_text_blocks_python

    return _extract_page_text_blocks_python(page)


def collect_page_math_protection_rects(*, page: fitz.Page) -> list[fitz.Rect]:
    """`cleanup.math_spans.collect_page_math_protection_rects`, routed to the
    native bridge when built on a file-backed page; otherwise the pure-Python
    reference."""
    path = _page_source_pdf_path(page)
    if NATIVE and path:
        try:
            return _deserialize_rects(
                _native_read_page_math_rects(Path(path).read_bytes(), int(page.number))
            )
        except Exception:
            pass
    from services.rendering.source.cleanup.math_spans import (
        _collect_page_math_protection_rects_python,
    )

    return _collect_page_math_protection_rects_python(page)


def collect_page_non_math_span_heights(*, page: fitz.Page) -> list[float]:
    """`cleanup.math_spans.collect_page_non_math_span_heights`, routed to the
    native bridge when built on a file-backed page; otherwise the pure-Python
    reference."""
    path = _page_source_pdf_path(page)
    if NATIVE and path:
        try:
            raw = _native_read_page_span_heights(Path(path).read_bytes(), int(page.number))
            return [float(height) for height in json.loads(raw)]
        except Exception:
            pass
    from services.rendering.source.cleanup.math_spans import (
        _collect_page_non_math_span_heights_python,
    )

    return _collect_page_non_math_span_heights_python(page)

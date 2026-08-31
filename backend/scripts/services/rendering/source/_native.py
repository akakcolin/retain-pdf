"""Optional native (Rust) backend for the source write-path primitives.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes the write-path
primitives (image XObject sanitize, image-only compression, page extraction)
through the ported Rust implementations. The write-path primitives are
native-only (they raise when the bridge is unavailable); the page-read family
below is native-only on file-backed pages (it raises when the bridge is
unavailable) and falls back to its pure-Python references only for in-memory
pages (the IN_MEMORY_PAGE capability boundary, which the native path cannot
serve).

The Rust side returns `(pdf_bytes, metadata_json)` for the transformations that
expose a production result contract (`sanitize_invalid_xobjects`,
`compress_images`); this shim parses the metadata to rebuild the production
result dataclasses / return values without re-deriving them.

Routed here: `build_invalid_xobject_sanitized_pdf_copy`,
`build_hidden_text_stripped_pdf_copy`, `compress_pdf_images_only_impl`,
`extract_pages_with_pikepdf`, the prewarm page-count / page-width lookup
`read_page_sizes_and_count`, and `document.pdf_ops.save_optimized_pdf` (mupdf
`pdf_subset_fonts` + garbage
collection + stream compression in one native pass). The other write-path
primitives are deliberately NOT routed:

Documented divergence — native `compress_images` uses the Rust `image` crate
JPEG encoder (fixed Huffman tables, 4:4:4 chroma), which emits larger output
than PIL's `optimize=True` + 4:2:0 subsampling (measured 1.04x-3.97x per image
at q78). On images displayed at >= native resolution native may therefore skip
a recompression the Python reference would apply. The per-image commit gate
(strictly smaller) and the file-level `replace_if_smaller` gate still hold, so
native never grows a file and never re-encodes an image Python would keep;
differential coverage stays on resize-dominated images where both commit.
  * `build_hidden_text_stripped_pdf_copy` is routed as a candidate-page
    filtered strip (C3-N9): production keeps the fitz pre-scan
    (`page_is_pseudo_editable_scan`, a hard-boundary fitz primitive) and the
    bridge strips exactly those pages, so the `start_page`/`end_page` range
    behavior is unchanged. The whole-document `strip_hidden_text` bridge export
    stays as the write-differential reference.
  * `strip_bbox_text_rects` — wired (C3-N10): the source_cleanup shim routes
    `strip_bbox_text_rects_from_pdf_copy` to the bridge, carrying the
    planning-level skip/candidate metadata through unchanged and gating to the
    Python reference for `skip_form_xobject_pages=True` / deadline budgets.
  * overlay family — bridge `overlay_page` is per-page (one open/save cycle per
    call), so a multi-page overlay would regress to N saves vs production's
    single save; the book/diagnostic call sites are also not covered.

Routed here (drawing-read family): `vector_text.collect_vector_text_rects` and
`vector_profile.page_drawing_count`. These take a file-backed `fitz.Page`
(production opens the source by path, so `page.parent.name` is the source
path); the shim re-opens that file natively and feeds the page bytes to the
bridge. File-backed pages are native-only (the bridge is mandatory and raises
when unavailable); only in-memory pages fall back to the pure-Python reference
(IN_MEMORY_PAGE boundary). `vector_text` reuses the Rust classifier directly
(it must, since mupdf always RGB-converts fills and only the native
classifier's `cs.n() == 3` gate matches Python's `len(fill) == 3`);
`page_drawing_count` routes through the native count primitive (exact parity —
count is a `drawings()` length on both sides).

`vector_profile.collect_page_drawing_rects` is deliberately NOT routed: the
native per-drawing rect can exceed fitz `get_cdrawings` on stroked zigzag
paths (PyMuPDF's lineart device drops the last path item's first point, so
fitz reports a smaller rect than the display-list bound). Replicating that
quirk is fragile and the rects feed per-item overlap decisions, so the shim
relays the pure-Python reference to preserve output exactly.

Routed here (background-image read family):
`background.detect.page_has_large_background_image` routes through the native
placement-rect read (`read_page_image_rects`) when the page is file-backed
(native-only — raises when the bridge is unavailable; only in-memory pages fall
back). The native rects match fitz `get_image_info` bboxes (same `fz_bound_image`
path: display-list `fill_image` + `fill_image_mask` placements in content /
rotation-stripped space), so the coverage/tiling boolean is computed from them
in Python, reusing the reference's shared helpers. `background.detect.pick_primary_background_image`
stays on the pure-Python reference: it returns the xref that drives the actual
image rewrite, and mupdf-rs exposes no placement→xref association, so native
cannot reproduce it.

Routed here (cleanup text-read family): `cleanup.text_extract.extract_page_text_spans`
and `extract_page_text_blocks` (fitz `get_text("dict")`/`get_text("blocks")`
consumers) and `cleanup.math_spans.collect_page_math_protection_rects` /
`collect_page_non_math_span_heights`. All four are native-only on file-backed
pages (they raise when the bridge is unavailable; only in-memory pages fall
back). The native collectors build the text page
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

from services.rendering import _routing
from services.rendering.source.rects import Rect
from services.rendering.source.preparation.hidden_text_strip import HiddenTextStripResult
from services.rendering.source.preparation.xobject_sanitize import XObjectSanitizeResult

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
    from rendering_bridge import strip_hidden_text_pages as _native_strip_hidden_text_pages
    from rendering_bridge import subset_and_save_optimized_pdf as _native_subset_and_save_optimized_pdf

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def sanitize_pdf_copy(
    *,
    source_pdf_path: Path,
    output_pdf_path: Path,
) -> XObjectSanitizeResult:
    """`xobject_sanitize.build_invalid_xobject_sanitized_pdf_copy`, routed to the
    native bridge. Native-only: the bridge is mandatory; when it is unavailable
    this raises instead of running the retired pure-Python reference."""
    if not _routing.routed("source", "sanitize_pdf_copy", NATIVE):
        raise RuntimeError(
            "source.sanitize_pdf_copy is native-only: the rendering_bridge "
            "sanitize_invalid_xobjects is required"
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
    _routing.record_native_hit("source", "sanitize_pdf_copy")
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


def build_hidden_text_stripped_pdf_copy(
    candidate_pages: set[int],
    *,
    source_pdf_path: Path,
    output_pdf_path: Path,
) -> HiddenTextStripResult:
    """`hidden_text_strip.build_hidden_text_stripped_pdf_copy`'s per-page strip,
    routed to the native bridge when built. Production keeps the fitz candidate
    pre-scan and passes the candidate page set here; the bridge strips exactly
    those pages, so the `start_page`/`end_page` range behavior is unchanged.
    Native-only: the bridge is mandatory; when it is unavailable this raises
    instead of running the retired pure-Python reference."""
    if not _routing.routed("source", "build_hidden_text_stripped_pdf_copy", NATIVE):
        raise RuntimeError(
            "source.build_hidden_text_stripped_pdf_copy is native-only: the "
            "rendering_bridge strip_hidden_text_pages is required"
        )
    started = time.perf_counter()
    out_bytes, meta_json = _native_strip_hidden_text_pages(
        source_pdf_path.read_bytes(),
        json.dumps(sorted(candidate_pages)),
    )
    meta = json.loads(meta_json)
    if not meta["changed"]:
        output_pdf_path.unlink(missing_ok=True)
        return HiddenTextStripResult(changed=False)
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    output_pdf_path.write_bytes(out_bytes)
    elapsed = time.perf_counter() - started
    _routing.record_native_hit("source", "build_hidden_text_stripped_pdf_copy")
    print(
        f"hidden text strip: pages={meta['pages_changed']} text_objects={meta['text_objects_removed']} "
        f"elapsed={elapsed:.2f}s output={output_pdf_path}",
        flush=True,
    )
    return HiddenTextStripResult(
        changed=True,
        output_pdf_path=output_pdf_path,
        pages_changed=meta["pages_changed"],
        text_objects_removed=meta["text_objects_removed"],
    )


def compress_images_only(pdf_path: Path, *, dpi: int = 200) -> bool:
    """`image_pipeline.compress_pdf_images_only_impl`, routed to the native
    bridge when built; otherwise the pure-Python implementation. Mirrors
    `replace_if_smaller`: commits only when the native output is strictly
    smaller than the input file. Native-only: the bridge is mandatory; when it
    is unavailable this raises instead of running the retired pure-Python
    reference."""
    if not _routing.routed("source", "compress_images_only", NATIVE):
        raise RuntimeError(
            "source.compress_images_only is native-only: the rendering_bridge "
            "compress_images is required"
        )
    if dpi <= 0 or not pdf_path.exists():
        return False
    input_bytes = pdf_path.read_bytes()
    out_bytes, meta_json = _native_compress_images(input_bytes, int(dpi))
    meta = json.loads(meta_json)
    if not meta["changed"] or len(out_bytes) >= len(input_bytes):
        return False
    _routing.record_native_hit("source", "compress_images_only")
    pdf_path.write_bytes(out_bytes)
    return True


def extract_pages(
    *,
    source_pdf_path: Path,
    output_pdf_path: Path,
    start_page: int,
    end_page: int,
) -> Path:
    """`pikepdf_pages.extract_pages_with_pikepdf`, routed to the native bridge.
    Native-only: the bridge is mandatory; when it is unavailable this raises
    instead of running the retired pure-Python reference."""
    if not _routing.routed("source", "extract_pages", NATIVE):
        raise RuntimeError(
            "source.extract_pages is native-only: the rendering_bridge "
            "extract_pages is required"
        )
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    out_bytes = _native_extract_pages(
        source_pdf_path.read_bytes(),
        int(start_page),
        int(end_page),
    )
    output_pdf_path.write_bytes(out_bytes)
    _routing.record_native_hit("source", "extract_pages")
    return output_pdf_path


def save_optimized(pdf_bytes: bytes) -> bytes:
    """`document.pdf_ops.save_optimized_pdf`'s full byte-compaction, routed to
    the native bridge. Native-only: the bridge is mandatory; when it is
    unavailable this raises instead of running the retired bytes-level
    pure-Python reference. The native path runs mupdf `pdf_subset_fonts`
    (fitz `subset_fonts()` equivalent) + garbage=4/stream compression in one
    pass."""
    if not _routing.routed("source", "save_optimized", NATIVE):
        raise RuntimeError(
            "source.save_optimized is native-only: the rendering_bridge "
            "subset_and_save_optimized_pdf is required"
        )
    try:
        result = _native_subset_and_save_optimized_pdf(pdf_bytes)
    except Exception:
        _routing.record_fallback(
            "source", "save_optimized", _routing.FallbackReason.NATIVE_BRIDGE_ERROR
        )
        raise
    _routing.record_native_hit("source", "save_optimized")
    return result


def read_page_sizes_and_count(*, source_pdf_path: Path) -> tuple[int, dict[int, float]]:
    """`prewarm_payload`'s page-count + page-width lookup, routed to the native
    bridge. Native-only: the bridge is mandatory; when it is unavailable this
    raises instead of running the retired pure-Python reference. Returns
    `(page_count, {idx: width_pt})` with width == fitz `page.rect` width
    (rotation-applied bounds)."""
    if not _routing.routed("source", "read_page_sizes_and_count", NATIVE):
        raise RuntimeError(
            "source.read_page_sizes_and_count is native-only: the rendering_bridge "
            "read_page_geometry is required"
        )
    try:
        raw = json.loads(_native_read_page_geometry(source_pdf_path.read_bytes()))
        page_count = int(raw["page_count"])
        widths = {int(idx): rect[2] - rect[0] for idx, rect in raw["rects"].items()}
        _routing.record_native_hit("source", "read_page_sizes_and_count")
        return page_count, widths
    except Exception:
        _routing.record_fallback(
            "source", "read_page_sizes_and_count", _routing.FallbackReason.NATIVE_BRIDGE_ERROR
        )
        raise


def _page_source_pdf_path(page: fitz.Page) -> str:
    """The on-disk path backing `page`, or "" for an in-memory page. Production
    opens the source by path, so `page.parent.name` is the path; empty means the
    shim must fall back to the reference."""
    return getattr(getattr(page, "parent", None), "name", "") or ""


def collect_vector_text_rects(*, page: fitz.Page, target_rects: list[fitz.Rect]) -> list[fitz.Rect]:
    """`vector_text.collect_vector_text_rects`, native-only on a file-backed
    page: the bridge is mandatory and this raises when it is unavailable. The
    pure-Python reference survives only for in-memory pages (the IN_MEMORY_PAGE
    capability boundary, which the native path cannot serve).

    The reference import is lazy to avoid a circular import: `vector_text`
    imports this shim at module top."""
    path = _page_source_pdf_path(page)
    if _routing.routed("source", "collect_vector_text_rects", NATIVE, path=path):
        try:
            targets = json.dumps([[r.x0, r.y0, r.x1, r.y1] for r in target_rects])
            raw = json.loads(
                _native_collect_vector_text_rects(
                    Path(path).read_bytes(),
                    int(page.number),
                    targets,
                )
            )
            _routing.record_native_hit("source", "collect_vector_text_rects")
            return [fitz.Rect(rect) for rect in raw]
        except Exception:
            _routing.record_fallback(
                "source", "collect_vector_text_rects", _routing.FallbackReason.NATIVE_BRIDGE_ERROR
            )
            raise
    if path:
        raise RuntimeError(
            "source.collect_vector_text_rects is native-only: the rendering_bridge "
            "collect_vector_text_rects is required"
        )
    from services.rendering.source.vector_text import _collect_vector_text_rects_python

    return _collect_vector_text_rects_python(page, target_rects)


def collect_page_drawing_rects(*, page: fitz.Page) -> list[Rect]:
    """`vector_profile.collect_page_drawing_rects`, relayed to the pure-Python
    reference (see module docstring: native rects diverge from fitz on stroked
    zigzag paths, so routing here would change redaction output).

    The reference import is lazy to avoid a circular import: `vector_profile`
    imports this shim at module top."""
    _routing.record_fallback(
        "source",
        "collect_page_drawing_rects",
        _routing.FallbackReason.DELIBERATELY_NOT_ROUTED,
    )
    from services.rendering.source.vector_profile import _collect_page_drawing_rects_python

    return _collect_page_drawing_rects_python(page)


def page_drawing_count(*, page: fitz.Page) -> int:
    """`vector_profile.page_drawing_count`, native-only on a file-backed page:
    the bridge is mandatory and this raises when it is unavailable. The
    pure-Python reference survives only for in-memory pages (the IN_MEMORY_PAGE
    capability boundary). Count parity is exact (`drawings()` length on both
    sides).

    The reference import is lazy to avoid a circular import: `vector_profile`
    imports this shim at module top."""
    path = _page_source_pdf_path(page)
    if _routing.routed("source", "page_drawing_count", NATIVE, path=path):
        try:
            count = int(
                _native_read_page_drawing_count(Path(path).read_bytes(), int(page.number))
            )
            _routing.record_native_hit("source", "page_drawing_count")
            return count
        except Exception:
            _routing.record_fallback(
                "source", "page_drawing_count", _routing.FallbackReason.NATIVE_BRIDGE_ERROR
            )
            raise
    if path:
        raise RuntimeError(
            "source.page_drawing_count is native-only: the rendering_bridge "
            "read_page_drawing_count is required"
        )
    from services.rendering.source.vector_profile import _page_drawing_count_python

    return _page_drawing_count_python(page)


def page_has_large_background_image(*, page: fitz.Page, coverage_ratio_threshold: float = 0.75) -> bool:
    """`background.detect.page_has_large_background_image`, native-only on a
    file-backed page: the bridge is mandatory and this raises when it is
    unavailable. The pure-Python reference survives only for in-memory pages
    (the IN_MEMORY_PAGE capability boundary). The native path reads image
    placement rects from the bridge (display-list `fill_image` bounds == fitz
    `get_image_info` bboxes) and computes the boolean from them in Python,
    sharing the reference's coverage/tiling helpers.
    `pick_primary_background_image` (the xref-bearing picker that drives the
    actual image rewrite) is NOT routed — see the module docstring.

    The reference import is lazy to avoid a circular import: `detect` imports
    this shim at module top."""
    path = _page_source_pdf_path(page)
    if _routing.routed("source", "page_has_large_background_image", NATIVE, path=path):
        try:
            from services.rendering.source.background.detect import (
                _has_large_background_image_from_rects,
            )

            raw = json.loads(
                _native_read_page_image_rects(Path(path).read_bytes(), int(page.number))
            )
            _routing.record_native_hit("source", "page_has_large_background_image")
            return _has_large_background_image_from_rects(
                [Rect(*rect) for rect in raw],
                page.rect,
                coverage_ratio_threshold=coverage_ratio_threshold,
            )
        except Exception:
            _routing.record_fallback(
                "source",
                "page_has_large_background_image",
                _routing.FallbackReason.NATIVE_BRIDGE_ERROR,
            )
            raise
    if path:
        raise RuntimeError(
            "source.page_has_large_background_image is native-only: the "
            "rendering_bridge read_page_image_rects is required"
        )
    from services.rendering.source.background.detect import _page_has_large_background_image_python

    return _page_has_large_background_image_python(
        page, coverage_ratio_threshold=coverage_ratio_threshold
    )


def _deserialize_text_entries(raw: str) -> list[tuple[Rect, str]]:
    """`[[x0,y0,x1,y1,text], ...]` (native text-spans / text-blocks output) →
    `(Rect, text)` pairs."""
    return [(Rect(*item[:4]), str(item[4])) for item in json.loads(raw)]


def _deserialize_rects(raw: str) -> list[Rect]:
    return [Rect(*item) for item in json.loads(raw)]


def extract_page_text_spans(*, page: fitz.Page) -> list[tuple[Rect, str]]:
    """`cleanup.text_extract.extract_page_text_spans`, native-only on a
    file-backed page: the bridge is mandatory and this raises when it is
    unavailable. The pure-Python reference survives only for in-memory pages
    (the IN_MEMORY_PAGE capability boundary).

    The reference import is lazy to avoid a circular import: `text_extract`
    imports this shim at module top."""
    path = _page_source_pdf_path(page)
    if _routing.routed("source", "extract_page_text_spans", NATIVE, path=path):
        try:
            spans = _deserialize_text_entries(
                _native_read_page_text_spans(Path(path).read_bytes(), int(page.number))
            )
            _routing.record_native_hit("source", "extract_page_text_spans")
            return spans
        except Exception:
            _routing.record_fallback(
                "source", "extract_page_text_spans", _routing.FallbackReason.NATIVE_BRIDGE_ERROR
            )
            raise
    if path:
        raise RuntimeError(
            "source.extract_page_text_spans is native-only: the rendering_bridge "
            "read_page_text_spans is required"
        )
    from services.rendering.source.cleanup.text_extract import _extract_page_text_spans_python

    return _extract_page_text_spans_python(page)


def extract_page_text_blocks(*, page: fitz.Page) -> list[tuple[Rect, str]]:
    """`cleanup.text_extract.extract_page_text_blocks`, native-only on a
    file-backed page: the bridge is mandatory and this raises when it is
    unavailable. The pure-Python reference survives only for in-memory pages
    (the IN_MEMORY_PAGE capability boundary)."""
    path = _page_source_pdf_path(page)
    if _routing.routed("source", "extract_page_text_blocks", NATIVE, path=path):
        try:
            blocks = _deserialize_text_entries(
                _native_read_page_text_blocks(Path(path).read_bytes(), int(page.number))
            )
            _routing.record_native_hit("source", "extract_page_text_blocks")
            return blocks
        except Exception:
            _routing.record_fallback(
                "source", "extract_page_text_blocks", _routing.FallbackReason.NATIVE_BRIDGE_ERROR
            )
            raise
    if path:
        raise RuntimeError(
            "source.extract_page_text_blocks is native-only: the rendering_bridge "
            "read_page_text_blocks is required"
        )
    from services.rendering.source.cleanup.text_extract import _extract_page_text_blocks_python

    return _extract_page_text_blocks_python(page)


def collect_page_math_protection_rects(*, page: fitz.Page) -> list[Rect]:
    """`cleanup.math_spans.collect_page_math_protection_rects`, native-only on a
    file-backed page: the bridge is mandatory and this raises when it is
    unavailable. The pure-Python reference survives only for in-memory pages
    (the IN_MEMORY_PAGE capability boundary)."""
    path = _page_source_pdf_path(page)
    if _routing.routed("source", "collect_page_math_protection_rects", NATIVE, path=path):
        try:
            rects = _deserialize_rects(
                _native_read_page_math_rects(Path(path).read_bytes(), int(page.number))
            )
            _routing.record_native_hit("source", "collect_page_math_protection_rects")
            return rects
        except Exception:
            _routing.record_fallback(
                "source",
                "collect_page_math_protection_rects",
                _routing.FallbackReason.NATIVE_BRIDGE_ERROR,
            )
            raise
    if path:
        raise RuntimeError(
            "source.collect_page_math_protection_rects is native-only: the "
            "rendering_bridge read_page_math_rects is required"
        )
    from services.rendering.source.cleanup.math_spans import (
        _collect_page_math_protection_rects_python,
    )

    return _collect_page_math_protection_rects_python(page)


def collect_page_non_math_span_heights(*, page: fitz.Page) -> list[float]:
    """`cleanup.math_spans.collect_page_non_math_span_heights`, native-only on a
    file-backed page: the bridge is mandatory and this raises when it is
    unavailable. The pure-Python reference survives only for in-memory pages
    (the IN_MEMORY_PAGE capability boundary)."""
    path = _page_source_pdf_path(page)
    if _routing.routed("source", "collect_page_non_math_span_heights", NATIVE, path=path):
        try:
            raw = _native_read_page_span_heights(Path(path).read_bytes(), int(page.number))
            heights = [float(height) for height in json.loads(raw)]
            _routing.record_native_hit("source", "collect_page_non_math_span_heights")
            return heights
        except Exception:
            _routing.record_fallback(
                "source",
                "collect_page_non_math_span_heights",
                _routing.FallbackReason.NATIVE_BRIDGE_ERROR,
            )
            raise
    if path:
        raise RuntimeError(
            "source.collect_page_non_math_span_heights is native-only: the "
            "rendering_bridge read_page_span_heights is required"
        )
    from services.rendering.source.cleanup.math_spans import (
        _collect_page_non_math_span_heights_python,
    )

    return _collect_page_non_math_span_heights_python(page)

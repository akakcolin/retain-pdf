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
#: - ``fallback_reference`` — the fitz path is the parity/fallback net for a
#:   native-routed ``_native.py`` primitive; runs only on bridge failure / NATIVE=False.
#: - ``non_default_write`` — fitz writes an output PDF only on a non-production path.
#: - ``non_render_service`` — outside services/rendering (ocr_provider/translation/entrypoints).
FITZ_IMPORT_ALLOWLIST: dict[Path, tuple[str, str]] = {
    # ---- fallback_reference: fitz path is the parity/fallback net for a native-routed primitive
    Path("services/rendering/analysis/classifier.py"): (
        "fallback_reference",
        "fitz reference for native classify_render_page (fitz.open)",
    ),
    Path("services/rendering/analysis/document/builder.py"): (
        "fallback_reference",
        "fitz reference for native build_render_document_analysis",
    ),
    Path("services/rendering/analysis/profile/builder.py"): (
        "fallback_reference",
        "build_render_page_profile(page: fitz.Page) reference primitive",
    ),
    Path("services/rendering/analysis/profile/background_coverage.py"): (
        "fallback_reference",
        "page.rect / rect & page.rect coverage reference",
    ),
    Path("services/rendering/analysis/profile/drawing_count.py"): (
        "fallback_reference",
        "get_cdrawings / get_drawings count reference",
    ),
    Path("services/rendering/analysis/profile/geometry.py"): (
        "fallback_reference",
        "page.rect / cropbox / rotation / number geometry reference",
    ),
    Path("services/rendering/analysis/profile/image_background.py"): (
        "fallback_reference",
        "primary-image coverage reference via page.rect",
    ),
    Path("services/rendering/analysis/profile/page_cropbox.py"): (
        "fallback_reference",
        "page.cropbox read reference",
    ),
    Path("services/rendering/analysis/profile/page_index.py"): (
        "fallback_reference",
        "page.number reference",
    ),
    Path("services/rendering/analysis/profile/page_rotation.py"): (
        "fallback_reference",
        "page.rotation reference",
    ),
    Path("services/rendering/analysis/profile/page_size.py"): (
        "fallback_reference",
        "page.rect width/height reference",
    ),
    Path("services/rendering/analysis/profile/primary_image.py"): (
        "fallback_reference",
        "pick_primary_background_image(page) reference",
    ),
    Path("services/rendering/analysis/profile/rect_area.py"): (
        "fallback_reference",
        "fitz.Rect area helper reference",
    ),
    Path("services/rendering/analysis/profile/registry.py"): (
        "fallback_reference",
        "PageProfileRegistry(fitz.Page) collector registry reference",
    ),
    Path("services/rendering/analysis/profile/text_layer.py"): (
        "fallback_reference",
        "get_text('words') + trace-count reference",
    ),
    Path("services/rendering/analysis/profile/text_traces.py"): (
        "fallback_reference",
        "get_texttrace reference (native classifier parity net)",
    ),
    Path("services/rendering/analysis/profile/vector_layer.py"): (
        "fallback_reference",
        "drawing-count / vector-heavy reference",
    ),
    Path("services/rendering/document/_native.py"): (
        "fallback_reference",
        "native shim for copy_toc / save_optimized_pdf; fitz fallback",
    ),
    Path("services/rendering/document/metadata.py"): (
        "fallback_reference",
        "get_toc / set_toc reference for native copy_toc",
    ),
    Path("services/rendering/document/pdf_ops.py"): (
        "fallback_reference",
        "subset_fonts + save reference for native save_optimized_pdf",
    ),
    Path("services/rendering/layout/page_specs.py"): (
        "fallback_reference",
        "fitz reference for native read_source_page_sizes",
    ),
    Path("services/rendering/layout/payload/_native.py"): (
        "fallback_reference",
        "native shim; fitz fallback reference",
    ),
    Path("services/rendering/layout/payload/first_line_indent.py"): (
        "fallback_reference",
        "get_pixmap / DisplayList pixel-sampling reference for native first-line-indent",
    ),
    Path("services/rendering/output/typst/_native.py"): (
        "fallback_reference",
        "native shim; fitz fallback reference",
    ),
    Path("services/rendering/output/typst/book_renderer.py"): (
        "fallback_reference",
        "build_dual_book_pdf fitz fallback for bytes-level native book render",
    ),
    Path("services/rendering/output/typst/book_support.py"): (
        "fallback_reference",
        "new_page + show_pdf_page reference for native build_dual_doc_pages",
    ),
    Path("services/rendering/output/typst/color_adapt.py"): (
        "fallback_reference",
        "get_pixmap / get_text('dict', clip=...) reference for native title-color",
    ),
    Path("services/rendering/output/typst/overlay_book.py"): (
        "fallback_reference",
        "build_dual_book_pdf fitz fallback for native overlay book",
    ),
    Path("services/rendering/output/typst/overlay_color.py"): (
        "fallback_reference",
        "fitz reference for native overlay fill/color",
    ),
    Path("services/rendering/output/typst/overlay_ops.py"): (
        "fallback_reference",
        "show_pdf_page(overlay=True) fallback compositor for native show_pdf_page_on_doc",
    ),
    Path("services/rendering/output/typst/overlay_runtime.py"): (
        "fallback_reference",
        "overlay_pdf_size_mismatches fitz open + rect is the parity net for native read_source_page_sizes",
    ),
    Path("services/rendering/output/typst/source_page_overlay.py"): (
        "fallback_reference",
        "delegates to show_pdf_page_on_doc compositor fallback",
    ),
    Path("services/rendering/pdf_structure_profile/sampler.py"): (
        "fallback_reference",
        "fitz reference for native build_pdf_structure_profile",
    ),
    Path("services/rendering/source/_native.py"): (
        "fallback_reference",
        "native shim; fitz.open / save fallback for save_optimized_pdf",
    ),
    Path("services/rendering/source/background/_native.py"): (
        "fallback_reference",
        "native shim; fitz.open / get_text('dict') fallbacks for build_clean_background_pdf",
    ),
    Path("services/rendering/source/background/detect.py"): (
        "fallback_reference",
        "get_image_info reference for native page_has_large_background_image",
    ),
    Path("services/rendering/source/background/page_overlay.py"): (
        "fallback_reference",
        "show_pdf_page(fitz.Rect(...), overlay=True) fallback compositor",
    ),
    Path("services/rendering/source/background/stage.py"): (
        "fallback_reference",
        "fitz reference for native build_clean_background_pdf",
    ),
    Path("services/rendering/source/cleanup/math_spans.py"): (
        "fallback_reference",
        "get_text('dict') references for native math-protection / non-math-span heights",
    ),
    Path("services/rendering/source/compression/image_pipeline.py"): (
        "fallback_reference",
        "fitz.open + extract_image reference for native compress_images_only",
    ),
    Path("services/rendering/source/prewarm_payload.py"): (
        "fallback_reference",
        "fitz reference for native read_source_page_sizes_and_count",
    ),
    Path("services/rendering/source/vector_profile.py"): (
        "fallback_reference",
        "get_cdrawings reference for native page_drawing_count",
    ),
    Path("services/rendering/source/vector_text.py"): (
        "fallback_reference",
        "get_drawings / get_cdrawings reference for native collect_vector_text_rects",
    ),
    Path("services/rendering/source_cleanup/planning/page_context.py"): (
        "fallback_reference",
        "fitz reference for native build_page_contexts",
    ),
    Path("services/rendering/visual_profile/sampler.py"): (
        "fallback_reference",
        "fitz reference for native build_document_visual_profile",
    ),
    # ---- hard_boundary: fitz API with no mupdf-rs equivalent / Python-only rewrite
    Path("services/rendering/source/cleanup/auto.py"): (
        "hard_boundary",
        "redaction planning; add_redact_annot / apply_redactions",
    ),
    Path("services/rendering/source/cleanup/cover_only.py"): (
        "hard_boundary",
        "visual-cover redaction; new_shape / draw covers",
    ),
    Path("services/rendering/source/cleanup/image_page.py"): (
        "hard_boundary",
        "image-only page redaction; get_images / apply_redactions",
    ),
    Path("services/rendering/source/cleanup/margin_text_cleanup.py"): (
        "hard_boundary",
        "add_redact_annot + PDF_REDACT_* text-strip",
    ),
    Path("services/rendering/source/cleanup/math_intrusion.py"): (
        "hard_boundary",
        "fitz.Rect coercion within redaction planning",
    ),
    Path("services/rendering/source/cleanup/plan_builder.py"): (
        "hard_boundary",
        "builds redaction plan from fitz.Rect items",
    ),
    Path("services/rendering/source/cleanup/plan_types.py"): (
        "hard_boundary",
        "fitz.Rect-typed plan dataclasses",
    ),
    Path("services/rendering/source/cleanup/redaction.py"): (
        "hard_boundary",
        "core redaction executor; add_redact_annot / apply_redactions",
    ),
    Path("services/rendering/source/cleanup/redaction_flow.py"): (
        "hard_boundary",
        "redaction orchestration; PDF_REDACT_*",
    ),
    Path("services/rendering/source/cleanup/route_context.py"): (
        "hard_boundary",
        "fitz.Page / fitz.Rect-typed routing context",
    ),
    Path("services/rendering/source/cleanup/routes.py"): (
        "hard_boundary",
        "redaction strategy router; get_texttrace / apply_redactions",
    ),
    Path("services/rendering/source/cleanup/standard.py"): (
        "hard_boundary",
        "standard redaction; get_text('words', clip) + apply_redactions",
    ),
    Path("services/rendering/source/cleanup/standard_execution.py"): (
        "hard_boundary",
        "add_redact_annot + PDF_REDACT_* standard path",
    ),
    Path("services/rendering/source/cleanup/text_extract.py"): (
        "hard_boundary",
        "get_text extraction + apply_redactions",
    ),
    Path("services/rendering/source/cleanup/text_matching.py"): (
        "hard_boundary",
        "get_text('words', clip) match + redact",
    ),
    Path("services/rendering/source/cleanup/text_safe_direct.py"): (
        "hard_boundary",
        "safe-direct text redaction; apply_redactions",
    ),
    Path("services/rendering/source/cleanup/valid_items.py"): (
        "hard_boundary",
        "fitz.Rect(bbox) coercion for redaction items",
    ),
    Path("services/rendering/source/cleanup/vector_heavy.py"): (
        "hard_boundary",
        "get_drawings heavy-vector detection",
    ),
    Path("services/rendering/source/cleanup/vector_item_policy.py"): (
        "hard_boundary",
        "fitz.Rect-based vector-item policy",
    ),
    Path("services/rendering/source/cleanup/vector_overlap.py"): (
        "hard_boundary",
        "fitz.Rect overlap merge for vector cleanup",
    ),
    Path("services/rendering/source/cleanup/vector_text_cleanup.py"): (
        "hard_boundary",
        "add_redact_annot + PDF_REDACT_* vector-text strip",
    ),
    Path("services/rendering/source/cleanup/visual_cover_execution.py"): (
        "hard_boundary",
        "new_shape / draw visual covers + apply_redactions",
    ),
    Path("services/rendering/source/cleanup/diagnostics.py"): (
        "hard_boundary",
        "fitz.Rect-typed diagnostics",
    ),
    Path("services/rendering/source/cleanup/item_rects.py"): (
        "hard_boundary",
        "fitz.Rect cover / text-removal rect math",
    ),
    Path("services/rendering/source/cleanup/layer_items.py"): (
        "hard_boundary",
        "fitz.Rect-typed layer-item rect math",
    ),
    Path("services/rendering/source/cleanup/page_facts.py"): (
        "hard_boundary",
        "fitz.Page-typed redaction page-facts",
    ),
    Path("services/rendering/source/text_redaction.py"): (
        "hard_boundary",
        "add_redact_annot / apply_redactions text redaction",
    ),
    Path("services/rendering/source/redaction.py"): (
        "hard_boundary",
        "redact_source_text_areas; redaction write",
    ),
    Path("services/rendering/source/preparation/hidden_text_strip.py"): (
        "hard_boundary",
        "get_texttrace hidden-text scan + fitz.open",
    ),
    Path("services/rendering/source/document_ops.py"): (
        "hard_boundary",
        "get_text('words') / get_texttrace PDF read/rewrite ops",
    ),
    Path("services/rendering/source/background/image_route.py"): (
        "hard_boundary",
        "page.replace_image / doc.update_stream image swap",
    ),
    Path("services/rendering/source/background/extract.py"): (
        "hard_boundary",
        "fitz.Pixmap(doc, xref) / doc.extract_image",
    ),
    Path("services/rendering/source/background/fill.py"): (
        "hard_boundary",
        "get_pixmap / insert_image / new_shape fill sampling",
    ),
    Path("services/rendering/source/background/redaction_plan.py"): (
        "hard_boundary",
        "fitz.Page-typed redaction decision",
    ),
    Path("services/rendering/source/background/source_overlay.py"): (
        "hard_boundary",
        "fitz.Page-typed source-overlay delegation",
    ),
    Path("services/rendering/source/compression/analysis.py"): (
        "hard_boundary",
        "get_images / get_image_rects compression analysis",
    ),
    Path("services/rendering/visual_profile/foreground.py"): (
        "hard_boundary",
        "get_pixmap + fitz.Matrix + fitz.csRGB pixel probe",
    ),
    Path("services/rendering/source/rects.py"): (
        "hard_boundary",
        "fitz.Rect coercion boundary for pure Rect type",
    ),
    Path("services/rendering/source/items.py"): (
        "hard_boundary",
        "fitz.Rect(bbox) coercion for redaction items",
    ),
    Path("services/rendering/source_cleanup/types.py"): (
        "hard_boundary",
        "fitz_page_rects constructs fitz.Rect for rewrite",
    ),
    Path("services/rendering/source_cleanup/policy/adapter.py"): (
        "hard_boundary",
        "fitz.Rect type hints only (formula-guard adapter)",
    ),
    Path("services/rendering/source_cleanup/planning/page_probe.py"): (
        "hard_boundary",
        "get_xobjects / get_contents / xref_stream page probing",
    ),
    Path("services/rendering/source_cleanup/planning/test_support.py"): (
        "hard_boundary",
        "fitz.open().new_page() fixture pages for planner functions",
    ),
    Path("services/rendering/source_cleanup/pdf/stream_engine.py"): (
        "hard_boundary",
        "pikepdf content-stream rewrite with fitz.Rect rects",
    ),
    Path("services/rendering/source_cleanup/pdf/xobject_ops.py"): (
        "hard_boundary",
        "pikepdf xobject rewrite with fitz.Rect rects",
    ),
    Path("services/rendering/source_cleanup/pdf/document.py"): (
        "hard_boundary",
        "pikepdf document rewrite (Python-only) with fitz.Rect",
    ),
    # ---- non_default_write: fitz writes PDFs only on non-default/non-production paths
    Path("services/rendering/source/dev_overlay/builders.py"): (
        "non_default_write",
        "legacy PyMuPDF direct-draw build_dev_pdf debug path",
    ),
    Path("services/rendering/source/dev_overlay/text_draw.py"): (
        "non_default_write",
        "insert_textbox / insert_text / insert_image / insert_font direct-draw",
    ),
    Path("services/rendering/workflow/direct_overlay.py"): (
        "non_default_write",
        "direct-overlay debug workflow; apply_translated_items_to_page + save_optimized_pdf",
    ),
    Path("services/rendering/tools/side_by_side_pdf.py"): (
        "non_default_write",
        "new_page + show_pdf_page + save side-by-side comparison tool",
    ),
    # ---- non_render_service: outside services/rendering
    Path("entrypoints/run_extract_text_layer.py"): (
        "non_render_service",
        "worker; fitz.open + get_text('dict') text-layer extraction",
    ),
    Path("services/translation/llm/domain_context.py"): (
        "non_render_service",
        "fitz.open + get_text('text') preview for LLM domain inference",
    ),
    Path("services/ocr_provider/paddle_normalize.py"): (
        "non_render_service",
        "fitz.open + page.rect rescale of OCR geometry",
    ),
    Path("services/ocr_provider/paddle_runner.py"): (
        "non_render_service",
        "fitz.open + len(doc) page count for progress",
    ),
}

FITZ_IMPORT_CATEGORIES = (
    "hard_boundary",
    "fallback_reference",
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

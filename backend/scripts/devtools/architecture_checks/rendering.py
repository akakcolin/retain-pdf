from __future__ import annotations

from pathlib import Path

from devtools.architecture_checks.common import SCRIPTS_ROOT
from devtools.architecture_checks.common import imported_from_symbols
from devtools.architecture_checks.common import imported_modules
from devtools.architecture_checks.common import module_allowed
from devtools.architecture_checks.common import read_text
from devtools.architecture_checks.common import rel
from devtools.architecture_checks.common import scan_py_files

PIPELINE_ROOT = SCRIPTS_ROOT / "runtime" / "pipeline"
RENDERING_ROOT = SCRIPTS_ROOT / "services" / "rendering"

RENDER_STAGE_PIPELINE = PIPELINE_ROOT / "render_stage.py"
RENDER_EXECUTION_PIPELINE = PIPELINE_ROOT / "render_execution.py"
RENDERING_WORKFLOW_ROOT = RENDERING_ROOT / "workflow"
RENDERING_ANALYSIS_ROOT = RENDERING_ROOT / "analysis"
RENDERING_PROFILE_ROOT = RENDERING_ANALYSIS_ROOT / "profile"
RENDERING_ROUTE_ROOT = RENDERING_ANALYSIS_ROOT / "route"
RENDERING_TYPST_ROOT = RENDERING_ROOT / "output" / "typst"
RENDERING_LAYOUT_ROOT = RENDERING_ROOT / "layout"
RENDERING_TEXT_TOKENS = RENDERING_LAYOUT_ROOT / "text_tokens.py"
RENDERING_TEXT_ANALYSIS_ROOT = RENDERING_LAYOUT_ROOT / "text_analysis"
RENDERING_SOURCE_ROOT = RENDERING_ROOT / "source"
RENDERING_SOURCE_CLEANUP_ROOT = RENDERING_SOURCE_ROOT / "cleanup"
RENDERING_ALLOWED_ROOT_DIRS = {
    "analysis",
    "contracts",
    "document",
    "layout",
    "legacy",
    "output",
    "pdf_structure_profile",
    "policy",
    "source",
    "source_cleanup",
    "tools",
    "visual_profile",
    "workflow",
}
RENDERING_ALLOWED_ROOT_FILES = {
    "__init__.py",
    "performance.py",
    "README.md",
    # Cross-layer routing/observability for the _native shims (stdlib-only,
    # imports neither the shims nor contracts); shared across every layer.
    "_routing.py",
}
RENDERING_LAYER_IMPORT_RULES: dict[str, tuple[str, ...]] = {
    "workflow": (
        "services.rendering.workflow",
        "services.rendering.analysis",
        "services.rendering.contracts",
        "services.rendering.document",
        "services.rendering.policy",
        "services.rendering.source",
        "services.rendering.source_cleanup",
        "services.rendering.layout",
        "services.rendering.output",
        "services.rendering.legacy",
        "services.rendering.visual_profile",
    ),
    "analysis": (
        "services.rendering.analysis",
        "services.rendering.contracts",
        # Page profiling may inspect source image metadata, but must not execute cleanup/output.
        "services.rendering.source.background.detect",
    ),
    "contracts": (
        "services.rendering.contracts",
        "services.rendering.analysis.profile.models",
        "services.rendering.analysis.route.models",
    ),
    "document": (
        "services.rendering.document",
        "services.rendering.layout.model",
        "services.rendering.contracts",
    ),
    "source": (
        "services.rendering.source",
        "services.rendering.contracts",
        "services.rendering.document",
        "services.rendering.policy",
        "services.rendering.layout",
        "services.rendering.layout.inline_content",
        # Existing source preparation still reuses the PDF compressor facade and Typst temp-root helper.
        "services.rendering.legacy.pdf_compress",
        "services.rendering.output.typst.shared",
        # Source prewarm owns cached render-source preparation and may build
        # precomputed Typst page/color profiles for the render stage.
        "services.rendering.output.typst.book_support",
        "services.rendering.output.typst.color_adapt",
        "services.rendering.pdf_structure_profile",
        "services.rendering.source_cleanup",
        "services.rendering.visual_profile",
    ),
    "layout": (
        "services.rendering.layout",
        "services.rendering.policy",
    ),
    "output": (
        "services.rendering.output",
        "services.rendering.layout",
        "services.rendering.document",
        "services.rendering.policy",
        # Output owns overlay composition and may sample/rebuild source backgrounds.
        "services.rendering.source.background",
        "services.rendering.visual_profile",
        # Pure geometry primitive (fitz.Rect duck-type); overlay color sampling
        # builds these rects without touching fitz.
        "services.rendering.source.rects",
    ),
    "policy": (
        "services.rendering.policy",
        "services.rendering.source_cleanup.planning.segments",
    ),
    "source_cleanup": (
        "services.rendering.source_cleanup",
        "services.rendering.contracts",
        "services.rendering.policy",
        "services.rendering.source.background.detect",
        "services.rendering.source.rects",
    ),
    "tools": (
        "services.rendering.tools",
    ),
    "pdf_structure_profile": (
        "services.rendering.pdf_structure_profile",
        "services.rendering.source.rects",
        "services.rendering.source_cleanup.planning.coordinate_resolver",
        "services.rendering.source_cleanup.planning.drawing_classifier",
    ),
    "visual_profile": (
        "services.rendering.visual_profile",
        "services.document_schema.semantics",
        "services.rendering.layout.font_roles",
        "services.rendering.layout.typography.geometry",
        "services.rendering.policy",
        # Covers sampler.py's `source.background.fill` imports and the native shim's
        # `source.background._native` primitive routing.
        "services.rendering.source.background",
        # Pure geometry primitive (fitz.Rect duck-type) used across the profile
        # pipeline instead of importing fitz directly.
        "services.rendering.source.rects",
    ),
    "legacy": (
        "services.rendering.workflow",
        "services.rendering.analysis",
        "services.rendering.document",
        "services.rendering.source",
        "services.rendering.layout",
        "services.rendering.output",
        "services.rendering.legacy",
    ),
}
RENDERING_LAYER_IMPORT_EXCEPTIONS: dict[Path, tuple[str, ...]] = {
    # Existing source/background overlay code still bridges source cleanup, layout blocks,
    # and overlay diagnostics. Keep this exception narrow so new cross-layer imports fail.
    Path("source/background/page_overlay.py"): (
        "services.rendering.output.typst.overlay_diagnostics",
    ),
    Path("source/background/redaction_plan.py"): (
        "services.rendering.layout.model.block_view",
        "services.rendering.layout.model.models",
    ),
    Path("source/background/redaction_items.py"): (
        "services.rendering.layout.model.block_view",
        "services.rendering.layout.model.models",
    ),
    Path("source/background/stage.py"): (
        "services.rendering.layout.model.models",
    ),
    Path("source/items.py"): (
        "services.rendering.layout.model.render_text",
    ),
    # The extract primitive routes through the write-path native shim (A2/A3);
    # scope the cross-layer import to the shim module only, not the whole source layer.
    Path("document/pikepdf_pages.py"): (
        "services.rendering.source._native",
    ),
    # The save-optimized byte-compaction half routes through the write-path
    # native shim (B2); font subsetting stays on fitz, so only the shim is
    # allowed cross-layer, not the whole source layer.
    Path("document/pdf_ops.py"): (
        "services.rendering.source._native",
    ),
    # The dual-book native path compacts output bytes through the write-path
    # shim (B2) only; the whole source layer stays off-limits cross-layer.
    Path("output/typst/book_renderer.py"): (
        "services.rendering.source._native",
    ),
}
REMOVED_SOURCE_PREPARATION_BBOX_MODULES = (
    "services.rendering.source.preparation.bbox_text_strip_accumulator",
    "services.rendering.source.preparation.bbox_text_strip_candidates",
    "services.rendering.source.preparation.bbox_text_strip_engine",
    "services.rendering.source.preparation.bbox_text_strip_hit_test",
    "services.rendering.source.preparation.bbox_text_strip_segments",
)
SOURCE_CLEANUP_NEXT_EXPERIMENTAL_MODULES = (
    "services.rendering.source_cleanup_next",
    "services.rendering.source_cleanup.planning.decision_builder",
    "services.rendering.source_cleanup.planning.deletion_contract",
    "services.rendering.source_cleanup.planning.formula_adjacency",
    "services.rendering.source_cleanup.planning.strategy",
    "services.rendering.source_cleanup.planning.text_groups",
    "services.rendering.source_cleanup.pdf.document_pages",
    "services.rendering.source_cleanup.pdf.document_parallel",
)
SOURCE_CLEANUP_NEXT_EXPERIMENTAL_SYMBOLS = {
    ("services.rendering.source_cleanup", "build_source_cleanup_plan"),
}
# A3 (wired write-path/background subsystems): production must reach the ported
# primitives ONLY through their `_native` shims. The pure-Python references below
# are the differential-corpus oracle and the NATIVE=False fallback; importing one
# from a production module is a dual-path regression. The shims own the fallback;
# differential corpus generators live outside services/rendering and are not
# scanned here. The Typst emitter is included because the Rust emitter is the
# wired path and production must route via `output.typst._native.emit_typst_source`.
WIRED_PYTHON_REFERENCE_SYMBOLS = {
    # write-path primitives routed via source/_native.py
    ("services.rendering.document.pikepdf_pages", "_extract_pages_with_pikepdf_python"),
    (
        "services.rendering.source.compression.image_pipeline",
        "_compress_pdf_images_only_impl_python",
    ),
    (
        "services.rendering.source.preparation.xobject_sanitize",
        "_build_invalid_xobject_sanitized_pdf_copy_python",
    ),
    # final save byte compaction routed via source/_native.py
    ("services.rendering.document.pdf_ops", "_save_optimized_pdf_python"),
    # background stage routed via source/background/_native.py
    ("services.rendering.source.background.stage", "_build_clean_background_pdf_python"),
    # pdf structure profile routed via pdf_structure_profile/_native.py
    (
        "services.rendering.pdf_structure_profile.sampler",
        "_build_pdf_structure_profile_python",
    ),
    # render-document analysis routed via analysis/_native.py
    (
        "services.rendering.analysis.document.builder",
        "_build_render_document_analysis_python",
    ),
    # Typst output layer routed via output/typst/_native.py
    ("services.rendering.output.typst.emitter", "build_typst_source_from_page_specs"),
    (
        "services.rendering.output.typst.source_builder",
        "build_typst_book_overlay_source",
    ),
    # layout page-size read routed via layout/_native.py
    ("services.rendering.layout.page_specs", "_read_source_page_sizes_python"),
    # prewarm page-count / page-width read routed via source/_native.py
    (
        "services.rendering.source.prewarm_payload",
        "_read_source_page_sizes_and_count_python",
    ),
    # vector drawing reads (get_cdrawings) routed via source/_native.py; the
    # shared `_rects_from_drawings` loop is production logic, not a wired symbol
    ("services.rendering.source.vector_text", "_collect_vector_text_rects_python"),
    (
        "services.rendering.source.vector_profile",
        "_collect_page_drawing_rects_python",
    ),
    ("services.rendering.source.vector_profile", "_page_drawing_count_python"),
    # background-image read (get_image_info) routed via source/_native.py; the
    # shared `_has_large_background_image_from_rects` / `_tiled_images_covered`
    # helpers are production logic, not wired symbols
    (
        "services.rendering.source.background.detect",
        "_page_has_large_background_image_python",
    ),
    # cleanup text-read (get_text("dict"/"blocks") consumers) routed via
    # source/_native.py; `extract_item_word_entries` is NOT wired (de-scoped —
    # fitz clip truncates words by glyph-ink bbox, which native cannot
    # reproduce), and `is_special_math_font`/`rect_key` stay shared logic
    (
        "services.rendering.source.cleanup.text_extract",
        "_extract_page_text_spans_python",
    ),
    (
        "services.rendering.source.cleanup.text_extract",
        "_extract_page_text_blocks_python",
    ),
    (
        "services.rendering.source.cleanup.math_spans",
        "_collect_page_math_protection_rects_python",
    ),
    (
        "services.rendering.source.cleanup.math_spans",
        "_collect_page_non_math_span_heights_python",
    ),
    # layout first-line-indent pixel detection routed via layout/payload/_native.py
    ("services.rendering.layout.payload.first_line_indent", "detect_first_line_indent_pt_with_displaylist"),
    # color-adapt decision tree + span/visual probes routed via output/typst/_native.py
    # and source/background/_native.py; fill.py primitives are intentionally NOT wired
    # here because color_adapt.py (reference) imports LocalBackgroundSampler directly.
    ("services.rendering.output.typst.color_adapt", "apply_adaptive_overlay_colors"),
    ("services.rendering.output.typst.color_adapt", "title_text_color_from_text_spans"),
    (
        "services.rendering.output.typst.color_adapt",
        "title_text_color_from_visual_components",
    ),
}
WIRED_PYTHON_REFERENCE_SHIMS = {
    RENDERING_ROOT / "source" / "_native.py",
    RENDERING_ROOT / "source" / "background" / "_native.py",
    RENDERING_ROOT / "output" / "typst" / "_native.py",
    RENDERING_ROOT / "layout" / "_native.py",
    RENDERING_ROOT / "layout" / "payload" / "_native.py",
    RENDERING_ROOT / "visual_profile" / "_native.py",
    RENDERING_ROOT / "source_cleanup" / "planning" / "_native.py",
    RENDERING_ROOT / "pdf_structure_profile" / "_native.py",
    RENDERING_ROOT / "analysis" / "_native.py",
}


def rendering_layer_for(path: Path) -> str | None:
    try:
        parts = path.relative_to(RENDERING_ROOT).parts
    except ValueError:
        return None
    if not parts:
        return None
    first = parts[0]
    return first if first in RENDERING_ALLOWED_ROOT_DIRS else None


def check_render_pipeline_facade_boundary(errors: list[str]) -> None:
    stage_text = read_text(RENDER_STAGE_PIPELINE)
    execution_text = read_text(RENDER_EXECUTION_PIPELINE)
    if "from services.rendering.workflow import execute_render_plan" not in execution_text:
        errors.append(
            "runtime/pipeline/render_execution.py: must delegate to services.rendering.workflow.execute_render_plan"
        )
    forbidden = (
        "import fitz",
        "from services.rendering.source.render_source import",
        "from services.rendering.source.preparation.hidden_text_strip import",
        "from services.rendering.output.typst",
        "from services.rendering.source.cleanup",
        "from services.rendering.layout",
        "from services.rendering.legacy.typst_page_renderer import",
        "from services.rendering.legacy.pdf_overlay import",
        "from services.rendering.legacy.pdf_compress import build_image_compressed_pdf_copy",
        "from services.rendering.legacy.pdf_compress import compress_pdf_images_only",
    )
    for item in forbidden:
        if item in stage_text or item in execution_text:
            errors.append(
                f"runtime/pipeline render facade must not import rendering internals directly: '{item}'"
            )


def check_rendering_internal_boundaries(errors: list[str]) -> None:
    for path in RENDERING_ROOT.iterdir():
        if path.name == "__pycache__":
            continue
        if path.is_dir() and path.name not in RENDERING_ALLOWED_ROOT_DIRS:
            errors.append(
                f"services/rendering/{path.name}: unexpected rendering root directory; use workflow/analysis/document/source/layout/output/legacy"
            )
        if path.is_file() and path.name not in RENDERING_ALLOWED_ROOT_FILES:
            errors.append(
                f"services/rendering/{path.name}: unexpected rendering root file; place entrypoints inside a named layer"
            )

    legacy_rendering_imports = (
        "from services.rendering.core",
        "import services.rendering.core",
        "from services.rendering.orchestrator",
        "import services.rendering.orchestrator",
        "from services.rendering.page_profile",
        "import services.rendering.page_profile",
        "from services.rendering.page_route",
        "import services.rendering.page_route",
        "from services.rendering.page_classifier",
        "import services.rendering.page_classifier",
        "from services.rendering.typst",
        "import services.rendering.typst",
        "from services.rendering.formula",
        "import services.rendering.formula",
        "from services.rendering.preprocess",
        "import services.rendering.preprocess",
        "from services.rendering.redaction",
        "import services.rendering.redaction",
        "from services.rendering.background",
        "import services.rendering.background",
        "from services.rendering.compress",
        "import services.rendering.compress",
        "from services.rendering.source_pdf",
        "import services.rendering.source_pdf",
    )
    legacy_rendering_wrappers = set()
    legacy_rendering_wrappers.update((RENDERING_ROOT / "core").glob("*.py"))
    legacy_rendering_wrappers.update((RENDERING_ROOT / "orchestrator").glob("*.py"))
    legacy_rendering_wrappers.update((RENDERING_ROOT / "page_profile").glob("*.py"))
    legacy_rendering_wrappers.update((RENDERING_ROOT / "page_route").glob("*.py"))
    legacy_rendering_wrappers.update((RENDERING_ROOT / "typst").glob("*.py"))
    legacy_rendering_wrappers.update((RENDERING_ROOT / "formula").glob("*.py"))
    legacy_rendering_wrappers.update((RENDERING_ROOT / "formula" / "core").glob("*.py"))
    legacy_rendering_wrappers.update((RENDERING_ROOT / "formula" / "fallback").glob("*.py"))
    legacy_rendering_wrappers.update((RENDERING_ROOT / "preprocess").glob("*.py"))
    legacy_rendering_wrappers.update((RENDERING_ROOT / "redaction").glob("*.py"))
    legacy_rendering_wrappers.update((RENDERING_ROOT / "background").glob("*.py"))
    legacy_rendering_wrappers.update((RENDERING_ROOT / "compress").glob("*.py"))
    legacy_rendering_wrappers.add(RENDERING_ROOT / "page_classifier.py")
    legacy_rendering_wrappers.add(RENDERING_ROOT / "source_pdf.py")
    for path in scan_py_files(RENDERING_ROOT):
        if path in legacy_rendering_wrappers:
            continue
        text = read_text(path)
        rel_path = rel(path)
        for item in legacy_rendering_imports:
            if item in text:
                errors.append(
                    f"{rel_path}: import new rendering modules directly instead of legacy compatibility wrappers"
                )
                break

    legacy_document_imports = (
        "from services.rendering.page_map",
        "import services.rendering.page_map",
        "from services.rendering.pdf_metadata",
        "import services.rendering.pdf_metadata",
    )
    legacy_document_wrappers = {
        RENDERING_ROOT / "page_map.py",
        RENDERING_ROOT / "pdf_metadata.py",
        RENDERING_ROOT / "source_pdf.py",
    }
    for path in scan_py_files(RENDERING_ROOT):
        if path in legacy_document_wrappers:
            continue
        text = read_text(path)
        rel_path = rel(path)
        for item in legacy_document_imports:
            if item in text:
                errors.append(
                    f"{rel_path}: import document helpers from services.rendering.document.* instead of rendering root wrappers"
                )
                break

    for path in scan_py_files(RENDERING_PROFILE_ROOT):
        text = read_text(path)
        rel_path = rel(path)
        forbidden = (
            "from services.rendering.analysis.route",
            "import services.rendering.analysis.route",
            "from services.rendering.source.cleanup",
            "import services.rendering.source.cleanup",
            "from services.rendering.output.typst",
            "import services.rendering.output.typst",
            "from services.rendering.layout",
            "import services.rendering.layout",
        )
        for item in forbidden:
            if item in text:
                errors.append(
                    f"{rel_path}: page_profile must collect facts only and must not depend on route/redaction/typst/layout"
                )
                break

    for path in scan_py_files(RENDERING_ROUTE_ROOT):
        text = read_text(path)
        rel_path = rel(path)
        forbidden = (
            "import fitz",
            "from services.rendering.source.cleanup",
            "import services.rendering.source.cleanup",
            "from services.rendering.output.typst",
            "import services.rendering.output.typst",
            "from services.rendering.layout",
            "import services.rendering.layout",
        )
        for item in forbidden:
            if item in text:
                errors.append(
                    f"{rel_path}: page_route must decide routes only and must not scan pages or call redaction/typst/layout"
                )
                break

    for path in scan_py_files(RENDERING_TYPST_ROOT):
        text = read_text(path)
        rel_path = rel(path)
        forbidden = (
            "from services.rendering.source.cleanup",
            "import services.rendering.source.cleanup",
        )
        for item in forbidden:
            if item in text:
                errors.append(
                    f"{rel_path}: typst layer must not import redaction directly; route background cleanup through rendering/background or orchestrator"
                )
                break

    for path in scan_py_files(RENDERING_LAYOUT_ROOT):
        text = read_text(path)
        rel_path = rel(path)
        forbidden = (
            "from services.rendering.source.cleanup",
            "import services.rendering.source.cleanup",
            "from services.rendering.output.typst",
            "import services.rendering.output.typst",
            "from services.rendering.source.render_source",
            "import services.rendering.source.render_source",
        )
        for item in forbidden:
            if item in text:
                errors.append(
                    f"{rel_path}: layout layer must not import redaction/typst/source_pdf"
                )
                break

    for path in scan_py_files(RENDERING_SOURCE_CLEANUP_ROOT):
        text = read_text(path)
        rel_path = rel(path)
        forbidden = (
            "from services.rendering.output.typst",
            "import services.rendering.output.typst",
            "import services.rendering.layout",
        )
        for item in forbidden:
            if item in text:
                errors.append(
                    f"{rel_path}: redaction layer must not import typst/layout"
                )
                break

    source_cleanup_rect_entrypoint = "strip_bbox_text_rects_from_pdf_copy"
    source_cleanup_rect_entrypoint_allowed = {
        RENDERING_ROOT / "source_cleanup" / "__init__.py",
        RENDERING_ROOT / "source_cleanup" / "executor.py",
        RENDERING_ROOT / "source_cleanup" / "pdf" / "__init__.py",
        RENDERING_ROOT / "source_cleanup" / "pdf" / "document.py",
    }
    for path in scan_py_files(RENDERING_ROOT / "source_cleanup"):
        if path in source_cleanup_rect_entrypoint_allowed:
            continue
        text = read_text(path)
        rel_path = rel(path)
        if source_cleanup_rect_entrypoint in text:
            errors.append(
                f"{rel_path}: source cleanup orchestration must use BBoxTextStripExecutionPlan instead of low-level rect entrypoint"
            )

    text_token_module = "services.rendering.layout.text_tokens"
    text_token_allowed_roots = {
        RENDERING_TEXT_TOKENS,
    }
    text_token_allowed_roots.update(scan_py_files(RENDERING_TEXT_ANALYSIS_ROOT))
    inline_math_boundary_markers = (
        r"(?<!\\)\$",
        r"\$\$",
        r"[^$\\\n]",
        r"[^$\n]",
    )
    inline_math_boundary_allowed_roots = set(text_token_allowed_roots)
    for path in scan_py_files(RENDERING_ROOT):
        rel_path = rel(path)
        if path not in text_token_allowed_roots:
            for module in imported_modules(path):
                if module_allowed(module, (text_token_module,)):
                    errors.append(
                        f"{rel_path}: use services.rendering.layout.text_analysis instead of text_tokens directly"
                    )
                    break
        if path in inline_math_boundary_allowed_roots:
            continue
        text = read_text(path)
        if "inline_math" not in path.name and "markdown" not in path.name and "formula" not in path.name and "$" not in text:
            continue
        if any(marker in text for marker in inline_math_boundary_markers):
            errors.append(
                f"{rel_path}: inline math token boundaries must be implemented in services.rendering.layout.text_analysis/text_tokens only"
            )

    removed_cleanup_modules = (
        "services.rendering.source.cleanup.analysis",
        "services.rendering.source.cleanup.document_ops",
        "services.rendering.source.cleanup.fill",
        "services.rendering.source.cleanup.geometry",
        "services.rendering.source.cleanup.math_protection",
        "services.rendering.source.cleanup.ops",
        "services.rendering.source.cleanup.plan",
        "services.rendering.source.cleanup.route_selection",
        "services.rendering.source.cleanup.shared",
        "services.rendering.source.cleanup.text_analysis",
        "services.rendering.source.cleanup.text_layer",
        "services.rendering.source.cleanup.text_match",
        "services.rendering.source.cleanup.vector_analysis",
        "services.rendering.source.cleanup.visual_cover",
    )
    for path in scan_py_files(RENDERING_ROOT):
        rel_path = rel(path)
        for module in imported_modules(path):
            if module in removed_cleanup_modules:
                errors.append(
                    f"{rel_path}: cleanup compatibility module '{module}' was removed; import the concrete implementation or source primitive"
                )
                break

    source_background_root = RENDERING_SOURCE_ROOT / "background"
    for path in scan_py_files(source_background_root):
        text = read_text(path)
        rel_path = rel(path)
        forbidden = (
            "from services.rendering.source.cleanup",
            "import services.rendering.source.cleanup",
        )
        for item in forbidden:
            if item in text:
                errors.append(
                    f"{rel_path}: source/background must not import source.cleanup directly; use source-level facades"
                )
                break

    source_preparation_root = RENDERING_SOURCE_ROOT / "preparation"
    preparation_compat_imports = (
        "services.rendering.source.cleanup.analysis",
        "services.rendering.source.cleanup.document_ops",
        "services.rendering.source.cleanup.fill",
        "services.rendering.source.cleanup.geometry",
        "services.rendering.source.cleanup.math_protection",
        "services.rendering.source.cleanup.ops",
        "services.rendering.source.cleanup.plan",
        "services.rendering.source.cleanup.route_selection",
        "services.rendering.source.cleanup.shared",
        "services.rendering.source.cleanup.text_analysis",
        "services.rendering.source.cleanup.text_layer",
        "services.rendering.source.cleanup.text_match",
        "services.rendering.source.cleanup.vector_analysis",
        "services.rendering.source.cleanup.visual_cover",
    )
    for path in scan_py_files(source_preparation_root):
        rel_path = rel(path)
        for module in imported_modules(path):
            if module in preparation_compat_imports:
                errors.append(
                    f"{rel_path}: source/preparation must import source primitives or concrete cleanup modules, not compatibility facade '{module}'"
                )
                break

    for path in scan_py_files(RENDERING_ROOT):
        rel_path = rel(path)
        for module in imported_modules(path):
            if module_allowed(module, REMOVED_SOURCE_PREPARATION_BBOX_MODULES):
                errors.append(
                    f"{rel_path}: removed bbox source-preparation module '{module}'; use services.rendering.source_cleanup boundary"
                )
                break

    for path in scan_py_files(RENDERING_ROOT):
        rel_path = rel(path)
        for module in imported_modules(path):
            if module_allowed(module, SOURCE_CLEANUP_NEXT_EXPERIMENTAL_MODULES):
                errors.append(
                    f"{rel_path}: source_cleanup_next experiment module '{module}' must not be imported by the beta10 cleanup mainline"
                )
                break
        for module, symbol in imported_from_symbols(path):
            if (module, symbol) in SOURCE_CLEANUP_NEXT_EXPERIMENTAL_SYMBOLS:
                errors.append(
                    f"{rel_path}: experimental source cleanup symbol '{symbol}' must not be imported by the beta10 cleanup mainline"
                )
                break

    dev_overlay_compat_imports = (
        "services.rendering.source.cleanup.builders",
        "services.rendering.source.cleanup.text_draw",
    )
    for path in scan_py_files(RENDERING_ROOT):
        rel_path = rel(path)
        for module in imported_modules(path):
            if module in dev_overlay_compat_imports:
                errors.append(
                    f"{rel_path}: cleanup dev overlay compatibility path was removed; import from services.rendering.source.dev_overlay instead of '{module}'"
                )
                break

    for path in scan_py_files(RENDERING_ROOT):
        if path in WIRED_PYTHON_REFERENCE_SHIMS:
            continue
        rel_path = rel(path)
        for module, symbol in imported_from_symbols(path):
            if (module, symbol) in WIRED_PYTHON_REFERENCE_SYMBOLS:
                errors.append(
                    f"{rel_path}: import pure-Python wired-subsystem reference '{symbol}' directly; route through the _native shim instead"
                )

    for path in scan_py_files(RENDERING_ROOT):
        layer = rendering_layer_for(path)
        if layer is None:
            continue
        allowed_prefixes = RENDERING_LAYER_IMPORT_RULES[layer]
        exception_prefixes = RENDERING_LAYER_IMPORT_EXCEPTIONS.get(path.relative_to(RENDERING_ROOT), ())
        for module in imported_modules(path):
            if not module.startswith("services.rendering."):
                continue
            if module_allowed(module, allowed_prefixes) or module_allowed(module, exception_prefixes):
                continue
            errors.append(
                f"{rel(path)}: rendering layer '{layer}' must not import '{module}' directly"
            )

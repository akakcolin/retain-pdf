#!/usr/bin/env python3
"""C1 render-orchestrator delegation entrypoint.

Reads the real render stage spec and produces the `render.bundle.v1` JSON that
the native `render_rs` chain consumes. Covers the prepare/page-specs segment of
the production `build_book_typst_background_pdf` flow (render source prep +
document analysis + translated-page prepare/color-adapt + page specs + visual
profile fill map); the background -> typst emit/compile -> save segment runs
natively in Rust.

Only `typst` / `typst_visual` modes are supported (the native-capable background
modes). Invoked by `rendering_orchestrator::delegate` as:

    python3 run_render_delegate.py --spec <stage-spec.json> --bundle-out <path>

The bundle's temp render-source files are intentionally NOT cleaned here: the
native stage chain reads `render_source.path` after this process exits.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from foundation.config import layout  # noqa: E402
from foundation.shared.stage_specs import RenderStageSpec  # noqa: E402
from runtime.pipeline.translation_loader import load_translated_pages  # noqa: E402
from runtime.pipeline.translation_loader import select_translated_pages  # noqa: E402
from services.rendering.document.page_map import RenderPageMap  # noqa: E402
from services.rendering.layout.page_specs import build_render_page_specs  # noqa: E402
from services.rendering.output.typst import _native as _typst_native  # noqa: E402
from services.rendering.output.typst.book_renderer import _apply_background_page_color_adapt  # noqa: E402
from services.rendering.output.typst.book_support import prepare_background_work_dir  # noqa: E402
from services.rendering.output.typst.book_support import prepare_translated_pages_for_render  # noqa: E402
from services.rendering.source.background._native import visual_profile_fill_map  # noqa: E402
from services.rendering.source.render_source import build_render_source_pdf  # noqa: E402
from services.rendering.source_cleanup.protected_blocks import protected_pages_from_document_path  # noqa: E402
from services.rendering.visual_profile.runtime import load_visual_profile_runtime  # noqa: E402
from services.rendering.workflow.document_analysis import build_sync_workflow_document_analysis  # noqa: E402

RENDER_BUNDLE_SCHEMA_VERSION = "render.bundle.v1"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Produce the render bundle for the native render_rs orchestrator.",
    )
    parser.add_argument("--spec", type=str, required=True, help="Path to render stage spec JSON.")
    parser.add_argument("--bundle-out", type=str, required=True, help="Path to write the render bundle JSON.")
    return parser.parse_args()


def build_bundle(spec: RenderStageSpec) -> dict:
    mode = spec.params.render_mode.strip() or "typst"
    if mode not in {"typst", "typst_visual"}:
        raise RuntimeError(f"run_render_delegate supports typst/typst_visual only, got {mode!r}")

    layout.apply_layout_tuning(
        body_font_size_factor=spec.params.body_font_size_factor,
        body_leading_factor=spec.params.body_leading_factor,
        inner_bbox_shrink_x=spec.params.inner_bbox_shrink_x,
        inner_bbox_shrink_y=spec.params.inner_bbox_shrink_y,
        inner_bbox_dense_shrink_x=spec.params.inner_bbox_dense_shrink_x,
        inner_bbox_dense_shrink_y=spec.params.inner_bbox_dense_shrink_y,
        font_unify_mode=spec.params.font_unify_mode,
        source_cleanup_strategy=spec.params.source_cleanup_strategy,
    )
    job_dirs = spec.job_dirs
    translated_pdf_name = (
        spec.params.translated_pdf_name.strip()
        or f"{Path(spec.inputs.source_pdf).stem}-translated.pdf"
    )
    output_pdf_path = job_dirs.rendered_dir / translated_pdf_name

    start_page = max(0, spec.params.start_page)
    end_page = spec.params.end_page
    translated_pages = load_translated_pages(
        spec.inputs.translations_dir,
        manifest_path=spec.inputs.translation_manifest,
    )
    selected_pages = select_translated_pages(
        translated_pages,
        start_page=start_page,
        end_page=end_page,
    )
    stop_page = max(selected_pages) if end_page < 0 else end_page

    document_analysis = build_sync_workflow_document_analysis(
        source_pdf_path=spec.inputs.source_pdf,
        translated_pages=selected_pages,
        start_page=start_page,
        end_page=stop_page,
    )
    protected_pages = protected_pages_from_document_path(
        spec.inputs.translations_dir.parent / "ocr" / "normalized" / "document.v1.json"
    )
    cleanup_strategy = layout.normalize_source_cleanup_strategy(spec.params.source_cleanup_strategy)
    render_source_pdf = build_render_source_pdf(
        source_pdf_path=spec.inputs.source_pdf,
        output_pdf_path=output_pdf_path,
        pdf_compress_dpi=spec.params.pdf_compress_dpi,
        translated_pages=selected_pages,
        protected_pages=protected_pages,
        strip_hidden_text=True,
        start_page=start_page,
        end_page=stop_page,
        artifact_mode=False,
        bbox_text_strip_candidates=None,
        source_cleanup_strategy=cleanup_strategy,
        document_analysis=document_analysis,
        pdf_structure_profile_path=None,
    )

    indent_pdf_path = spec.inputs.source_pdf
    prepared_pages = prepare_translated_pages_for_render(
        indent_pdf_path,
        selected_pages,
        first_line_indent_lookup=None,
        effective_inner_bbox_lookup=None,
    )
    prepared_pages = _apply_background_page_color_adapt(
        sample_pdf_path=indent_pdf_path,
        translated_pages=prepared_pages,
        precomputed_colors_by_item_id=None,
        visual_profile_path=None,
    )
    page_specs = build_render_page_specs(
        source_pdf_path=render_source_pdf.path,
        translated_pages=prepared_pages,
        prepared=True,
    )
    page_map = RenderPageMap.from_page_specs(page_specs)
    visual_profile_runtime = load_visual_profile_runtime(None)
    fill_map = visual_profile_fill_map(visual_profile_runtime)
    work_dir = prepare_background_work_dir(output_pdf_path, None)

    return {
        "schema_version": RENDER_BUNDLE_SCHEMA_VERSION,
        "mode": mode,
        "source_pdf": str(render_source_pdf.path),
        "output_pdf": str(output_pdf_path),
        "work_dir": str(work_dir),
        "font_family": spec.params.typst_font_family,
        "redaction_strategy": "visual_cover" if mode == "typst_visual" else None,
        "precleaned_page_indices": sorted(render_source_pdf.source_text_precleaned_page_indices),
        "visual_profile_fill_map": fill_map,
        "page_map": {"source_page_indices": page_map.source_page_indices},
        "translated_pages": prepared_pages,
        "page_specs": [_typst_native._page_spec_to_dict(spec) for spec in page_specs],
    }


def main() -> int:
    args = parse_args()
    spec = RenderStageSpec.load(Path(args.spec))
    bundle = build_bundle(spec)
    bundle_path = Path(args.bundle_out)
    bundle_path.parent.mkdir(parents=True, exist_ok=True)
    bundle_path.write_text(json.dumps(bundle, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"render bundle written: {bundle_path}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

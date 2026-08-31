from __future__ import annotations

from pathlib import Path
from typing import Callable

import fitz

from foundation.config import fonts
from services.rendering.output.pdf_writer import save_fast_pdf
from services.rendering.output.pdf_writer import save_optimized_pdf
from services.rendering.layout.payload.prepare import prepare_render_payloads_by_page
from services.rendering.document.page_map import RenderPageMap
from services.rendering.document.metadata import copy_toc
from services.rendering.document.metadata import copy_toc_for_page_map
from services.rendering.output.typst.compiler import compile_typst_book_background_pdf
from services.rendering.output.typst.sanitize import sanitize_page_specs_for_typst_book_background
from services.rendering.output.typst.shared import default_typst_temp_root
from services.rendering.output.typst.shared import prepare_typst_work_dir
from services.rendering.policy import apply_render_page_policy_fields
from services.rendering.policy import apply_render_pages_policy_fields

TypstRepairRequestFn = Callable[..., str]


def resolve_typst_temp_root(output_pdf_path: Path, temp_root: Path | None) -> Path:
    typst_temp_root = temp_root or default_typst_temp_root(output_pdf_path)
    typst_temp_root.mkdir(parents=True, exist_ok=True)
    return typst_temp_root


def prepare_single_page_items(
    translated_items: list[dict],
    page_idx: int,
    *,
    source_pdf_path: Path | None = None,
) -> list[dict]:
    prepared_pages = prepare_render_payloads_by_page({page_idx: translated_items}, source_pdf_path=source_pdf_path)
    prepared_items = prepared_pages.get(page_idx, translated_items)
    return apply_render_page_policy_fields(prepared_items)


def collect_background_page_specs(
    source_pdf_path: Path,
    translated_pages: dict[int, list[dict]],
    *,
    prepared: bool = False,
) -> list[tuple[int, float, float, list[dict]]]:
    prepared_pages = (
        apply_render_pages_policy_fields(translated_pages)
        if prepared
        else prepare_translated_pages_for_render(source_pdf_path, translated_pages)
    )
    source_doc = fitz.open(source_pdf_path)
    try:
        ordered_page_indices = sorted(page_idx for page_idx in prepared_pages if 0 <= page_idx < len(source_doc))
        return [
            (
                page_idx,
                source_doc[page_idx].rect.width,
                source_doc[page_idx].rect.height,
                prepared_pages[page_idx],
            )
            for page_idx in ordered_page_indices
        ]
    finally:
        source_doc.close()


def prepare_translated_pages_for_render(
    source_pdf_path: Path | None,
    translated_pages: dict[int, list[dict]],
    *,
    first_line_indent_lookup: dict[str, float] | None = None,
    effective_inner_bbox_lookup: dict[str, list[float]] | None = None,
    skip_policy_page_indices: frozenset[int] = frozenset(),
) -> dict[int, list[dict]]:
    prepared_pages = prepare_render_payloads_by_page(
        translated_pages,
        source_pdf_path=source_pdf_path,
        first_line_indent_lookup=first_line_indent_lookup,
        effective_inner_bbox_lookup=effective_inner_bbox_lookup,
    )
    return apply_render_pages_policy_fields(prepared_pages)


def compile_background_pdf_resilient(
    source_pdf_path: Path,
    page_specs: list[tuple[int, float, float, list[dict]]],
    *,
    api_key: str = "",
    model: str = "",
    base_url: str = "",
    font_family: str = fonts.TYPST_DEFAULT_FONT_FAMILY,
    font_paths: list[Path] | None = None,
    work_dir: Path,
    request_chat_content_fn: TypstRepairRequestFn | None = None,
) -> Path:
    try:
        return compile_typst_book_background_pdf(
            source_pdf_path=source_pdf_path,
            page_specs=page_specs,
            stem="book-background-overlay",
            font_family=font_family,
            font_paths=font_paths,
            work_dir=work_dir,
            request_chat_content_fn=request_chat_content_fn,
        )
    except RuntimeError as exc:
        print("typst background book compile failed; sanitizing pages", flush=True)
        print(str(exc), flush=True)
        sanitized_page_specs = sanitize_page_specs_for_typst_book_background(
            page_specs,
            stem="book-background-overlay",
            api_key=api_key,
            model=model,
            base_url=base_url,
            font_family=font_family,
            font_paths=font_paths,
            work_dir=work_dir,
        )
        return compile_typst_book_background_pdf(
            source_pdf_path=source_pdf_path,
            page_specs=sanitized_page_specs,
            stem="book-background-overlay-sanitized",
            font_family=font_family,
            font_paths=font_paths,
            work_dir=work_dir,
        )


def build_dual_doc_pages(
    source_doc: fitz.Document,
    translated_doc: fitz.Document,
    dual_doc: fitz.Document,
    *,
    start_page: int = 0,
    end_page: int = -1,
) -> fitz.Document:
    from services.rendering.output.typst._native import build_dual_doc_pages as _native_build_dual_doc_pages

    return _native_build_dual_doc_pages(
        source_doc,
        translated_doc,
        dual_doc,
        start_page=start_page,
        end_page=end_page,
    )


def save_background_pdf_to_output(
    background_pdf: Path,
    output_pdf_path: Path,
    *,
    source_pdf_path: Path | None = None,
    page_map: RenderPageMap | None = None,
    fast_save: bool = False,
) -> None:
    background_doc = fitz.open(background_pdf)
    source_doc = fitz.open(source_pdf_path) if source_pdf_path else None
    try:
        if source_doc is not None:
            if page_map is not None:
                replaced = copy_toc_for_page_map(source_doc, background_doc, page_map=page_map)
            else:
                replaced = copy_toc(source_doc, background_doc)
            if replaced is not background_doc:
                background_doc.close()
                background_doc = replaced
        if fast_save:
            save_fast_pdf(background_doc, output_pdf_path)
        else:
            save_optimized_pdf(background_doc, output_pdf_path)
    finally:
        if source_doc is not None:
            source_doc.close()
        background_doc.close()


def prepare_background_work_dir(output_pdf_path: Path, temp_root: Path | None) -> Path:
    typst_temp_root = resolve_typst_temp_root(output_pdf_path, temp_root)
    return prepare_typst_work_dir(typst_temp_root, "background-book")

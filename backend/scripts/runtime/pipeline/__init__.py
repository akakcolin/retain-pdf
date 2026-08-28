from __future__ import annotations

from typing import Any


_BOOK_PIPELINE_EXPORTS = {
    "build_book_from_translations",
    "build_book_pipeline",
    "is_editable_pdf",
    "resolve_page_range",
    "run_book_pipeline",
    "translate_book_pipeline",
}


def __getattr__(name: str) -> Any:
    if name in _BOOK_PIPELINE_EXPORTS:
        if name == "is_editable_pdf":
            from runtime.pipeline.render_mode import is_editable_pdf

            return is_editable_pdf
        from runtime.pipeline import book_pipeline

        return getattr(book_pipeline, name)
    raise AttributeError(name)


__all__ = sorted(_BOOK_PIPELINE_EXPORTS)

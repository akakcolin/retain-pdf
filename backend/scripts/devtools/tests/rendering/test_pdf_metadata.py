from __future__ import annotations

import sys
from pathlib import Path

import fitz


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))


from services.rendering.document.metadata import copy_toc
from services.rendering.document.metadata import copy_toc_for_page_map
from services.rendering.document.page_map import RenderPageMap


def _make_doc_with_toc() -> fitz.Document:
    doc = fitz.open()
    for _index in range(4):
        doc.new_page(width=200, height=300)
    doc.set_toc(
        [
            [1, "Chapter 1", 1],
            [2, "Section 1.1", 2],
            [1, "Chapter 2", 3],
            [2, "Section 2.1", 4],
        ]
    )
    return doc


def _live_doc(replaced, original):
    """Return the live doc: the swapped-in fresh doc when the shim replaced it,
    else the original. Ownership stays with the caller's `finally` (which closes
    `replaced` only when it is a different doc, and always closes `original`)."""
    return replaced if replaced is not original else original


def test_copy_toc_preserves_full_document_bookmarks() -> None:
    source = _make_doc_with_toc()
    target = fitz.open()
    replaced = None
    try:
        target.insert_pdf(source)

        replaced = copy_toc(source, target)
        live = _live_doc(replaced, target)

        assert len(live.get_toc()) == 4
        assert live.get_toc() == source.get_toc()
    finally:
        if replaced is not None and replaced is not target:
            replaced.close()
        target.close()
        source.close()


def test_copy_toc_remaps_selected_pages() -> None:
    source = _make_doc_with_toc()
    target = fitz.open()
    replaced = None
    try:
        target.insert_pdf(source, from_page=1, to_page=3)

        replaced = copy_toc(source, target, start_page=1, end_page=3)
        live = _live_doc(replaced, target)

        assert len(live.get_toc()) == 3
        assert live.get_toc() == [
            [1, "Section 1.1", 1],
            [1, "Chapter 2", 2],
            [2, "Section 2.1", 3],
        ]
    finally:
        if replaced is not None and replaced is not target:
            replaced.close()
        target.close()
        source.close()


def test_copy_toc_keeps_single_page_bookmark() -> None:
    source = _make_doc_with_toc()
    target = fitz.open()
    replaced = None
    try:
        target.insert_pdf(source, from_page=2, to_page=2)

        replaced = copy_toc(source, target, start_page=2, end_page=2)
        live = _live_doc(replaced, target)

        assert len(live.get_toc()) == 1
        assert live.get_toc() == [[1, "Chapter 2", 1]]
    finally:
        if replaced is not None and replaced is not target:
            replaced.close()
        target.close()
        source.close()


def test_copy_toc_for_page_map_preserves_non_contiguous_selected_pages() -> None:
    source = _make_doc_with_toc()
    target = fitz.open()
    replaced = None
    try:
        target.insert_pdf(source, from_page=0, to_page=0)
        target.insert_pdf(source, from_page=2, to_page=2)

        replaced = copy_toc_for_page_map(
            source, target, page_map=RenderPageMap(source_page_indices=[0, 2])
        )
        live = _live_doc(replaced, target)

        assert len(live.get_toc()) == 2
        assert live.get_toc() == [
            [1, "Chapter 1", 1],
            [1, "Chapter 2", 2],
        ]
    finally:
        if replaced is not None and replaced is not target:
            replaced.close()
        target.close()
        source.close()

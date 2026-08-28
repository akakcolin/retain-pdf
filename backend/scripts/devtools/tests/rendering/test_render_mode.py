from __future__ import annotations

import sys
from pathlib import Path


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))


from runtime.pipeline import render_mode
from services.rendering.contracts import RenderDocumentAnalysis
from services.rendering.contracts import RenderPageAnalysis


def _page_analysis(
    *,
    page_index: int,
    kind: str = "editable_text",
    editable_text: bool = True,
) -> RenderPageAnalysis:
    return RenderPageAnalysis(
        page_index=page_index,
        kind=kind,
        redaction="text_layer_only",
        background="source_pdf_page",
        compose="typst_overlay",
        layout="ocr_bbox_overlay",
        reason="test",
        has_large_background=False,
        background_coverage_ratio=0.0,
        visible_text=True,
        hidden_text=False,
        editable_text=editable_text,
        drawing_count=0,
        vector_heavy=False,
    )


def test_auto_render_mode_uses_typst_visual_for_non_editable_pdf(monkeypatch, tmp_path) -> None:
    source_pdf = tmp_path / "scan.pdf"
    source_pdf.write_bytes(b"%PDF-1.7\n")

    monkeypatch.setattr(render_mode, "is_pseudo_editable_scan_pdf", lambda *_args, **_kwargs: False)
    monkeypatch.setattr(render_mode, "is_editable_pdf", lambda *_args, **_kwargs: False)

    mode = render_mode.resolve_effective_render_mode(
        render_mode="auto",
        source_pdf_path=source_pdf,
        start_page=0,
        end_page=-1,
        translated_pages_map={0: [{"source_text": "hello world", "bbox": [0, 0, 10, 10]}]},
    )

    assert mode == "typst_visual"


def test_auto_render_mode_uses_typst_visual_for_pseudo_editable_scan_pdf(monkeypatch, tmp_path) -> None:
    source_pdf = tmp_path / "pseudo-scan.pdf"
    source_pdf.write_bytes(b"%PDF-1.7\n")

    monkeypatch.setattr(render_mode, "is_pseudo_editable_scan_pdf", lambda *_args, **_kwargs: True)
    monkeypatch.setattr(render_mode, "is_editable_pdf", lambda *_args, **_kwargs: True)

    mode = render_mode.resolve_effective_render_mode(
        render_mode="auto",
        source_pdf_path=source_pdf,
        start_page=0,
        end_page=-1,
        translated_pages_map={0: [{"source_text": "hello world", "bbox": [0, 0, 10, 10]}]},
    )

    assert mode == "typst_visual"


def test_auto_render_mode_uses_overlay_for_editable_pdf(monkeypatch, tmp_path) -> None:
    source_pdf = tmp_path / "editable.pdf"
    source_pdf.write_bytes(b"%PDF-1.7\n")

    monkeypatch.setattr(render_mode, "is_pseudo_editable_scan_pdf", lambda *_args, **_kwargs: False)
    monkeypatch.setattr(render_mode, "is_editable_pdf", lambda *_args, **_kwargs: True)

    mode = render_mode.resolve_effective_render_mode(
        render_mode="auto",
        source_pdf_path=source_pdf,
        start_page=0,
        end_page=-1,
        translated_pages_map={0: [{"source_text": "hello world", "bbox": [0, 0, 10, 10]}]},
    )

    assert mode == "overlay"


def test_explicit_overlay_render_mode_is_still_supported(tmp_path) -> None:
    source_pdf = tmp_path / "explicit.pdf"
    source_pdf.write_bytes(b"%PDF-1.7\n")

    mode = render_mode.resolve_effective_render_mode(
        render_mode="overlay",
        source_pdf_path=source_pdf,
        start_page=0,
        end_page=-1,
        translated_pages_map={0: [{"source_text": "hello world", "bbox": [0, 0, 10, 10]}]},
    )

    assert mode == "overlay"


def test_is_pseudo_editable_scan_analysis_requires_majority() -> None:
    analysis = RenderDocumentAnalysis(
        pages={
            0: _page_analysis(page_index=0, kind="pseudo_editable_scan"),
            1: _page_analysis(page_index=1, kind="pseudo_editable_scan"),
            2: _page_analysis(page_index=2),
        }
    )
    assert render_mode.is_pseudo_editable_scan_analysis(analysis) is True

    single = RenderDocumentAnalysis(pages={0: _page_analysis(page_index=0, kind="pseudo_editable_scan")})
    assert render_mode.is_pseudo_editable_scan_analysis(single) is True

    empty = RenderDocumentAnalysis(pages={})
    assert render_mode.is_pseudo_editable_scan_analysis(empty) is False


def test_is_editable_analysis_requires_editable_text_kind_and_majority() -> None:
    editable = RenderDocumentAnalysis(
        pages={0: _page_analysis(page_index=0), 1: _page_analysis(page_index=1)}
    )
    assert render_mode.is_editable_analysis(editable) is True

    non_editable = RenderDocumentAnalysis(
        pages={
            0: _page_analysis(page_index=0, editable_text=False),
            1: _page_analysis(page_index=1, editable_text=False),
        }
    )
    assert render_mode.is_editable_analysis(non_editable) is False

    minority = RenderDocumentAnalysis(
        pages={
            0: _page_analysis(page_index=0),
            1: _page_analysis(page_index=1, editable_text=False),
            2: _page_analysis(page_index=2, editable_text=False),
        }
    )
    assert render_mode.is_editable_analysis(minority) is False

    all_pseudo_scan = RenderDocumentAnalysis(
        pages={
            0: _page_analysis(page_index=0, kind="pseudo_editable_scan"),
            1: _page_analysis(page_index=1, kind="pseudo_editable_scan"),
        }
    )
    assert render_mode.is_editable_analysis(all_pseudo_scan) is False

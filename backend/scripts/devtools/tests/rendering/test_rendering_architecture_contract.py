from __future__ import annotations

import sys
from pathlib import Path
from unittest import mock


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))


from devtools import check_pipeline_architecture
from devtools.architecture_checks import common as architecture_common
from devtools.architecture_checks import rendering as rendering_checks


def test_pipeline_architecture_contract_passes() -> None:
    assert check_pipeline_architecture.main() == 0


def test_pipeline_architecture_rejects_removed_bbox_preparation_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    source_root = rendering_root / "source"
    source_root.mkdir(parents=True)
    offender = source_root / "bad_import.py"
    offender.write_text(
        "from services.rendering.source.preparation.bbox_text_strip_engine import run\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", source_root),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert any("removed bbox source-preparation module" in item for item in errors)


def test_pipeline_architecture_rejects_source_cleanup_next_mainline_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    source_cleanup_root = rendering_root / "source_cleanup"
    source_cleanup_root.mkdir(parents=True)
    offender = source_cleanup_root / "bad_import.py"
    offender.write_text(
        "from services.rendering.source_cleanup import build_source_cleanup_plan\n"
        "from services.rendering.source_cleanup.planning.decision_builder import build_decision\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            source_cleanup_root,
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert any("source_cleanup_next experiment module" in item for item in errors)
    assert any("experimental source cleanup symbol 'build_source_cleanup_plan'" in item for item in errors)


def test_pipeline_architecture_rejects_direct_wired_reference_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    source_root = rendering_root / "source"
    source_root.mkdir(parents=True)
    offender = source_root / "bad_import.py"
    offender.write_text(
        "from services.rendering.output.typst.color_adapt import title_text_color_from_visual_components\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", source_root),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert any("route through the _native shim instead" in item for item in errors)


def test_pipeline_architecture_accepts_document_shim_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    document_root = rendering_root / "document"
    document_root.mkdir(parents=True)
    (document_root / "pikepdf_pages.py").write_text(
        "import services.rendering.source._native as _native\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert not any("document' must not import" in item for item in errors)


def test_layout_accepts_layout_shim_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    layout_root = rendering_root / "layout"
    layout_root.mkdir(parents=True)
    (layout_root / "consumer.py").write_text(
        "from services.rendering.layout._native import read_source_page_sizes\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            layout_root,
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert not any("must not import" in item for item in errors)


def test_layout_payload_accepts_shim_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    layout_root = rendering_root / "layout"
    payload_root = layout_root / "payload"
    payload_root.mkdir(parents=True)
    (payload_root / "consumer.py").write_text(
        "from services.rendering.layout.payload._native import detect_first_line_indents\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            layout_root,
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert not any("must not import" in item for item in errors)


def test_color_adapt_accepts_shim_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    typst_root = rendering_root / "output" / "typst"
    typst_root.mkdir(parents=True)
    (typst_root / "consumer.py").write_text(
        "from services.rendering.output.typst._native import apply_adaptive_overlay_colors_batch\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            typst_root,
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert not any("must not import" in item for item in errors)
    assert not any("route through the _native shim instead" in item for item in errors)


def test_color_adapt_rejects_direct_wired_reference_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    typst_root = rendering_root / "output" / "typst"
    typst_root.mkdir(parents=True)
    (typst_root / "bad_import.py").write_text(
        "from services.rendering.output.typst.color_adapt import apply_adaptive_overlay_colors\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            typst_root,
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert any("route through the _native shim instead" in item for item in errors)


def test_visual_profile_accepts_shim_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    visual_profile_root = rendering_root / "visual_profile"
    visual_profile_root.mkdir(parents=True)
    (visual_profile_root / "consumer.py").write_text(
        "from services.rendering.visual_profile._native import build_document_visual_profile\n"
        "from services.rendering.source.background._native import sample_foreground_colors\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert not any("must not import" in item for item in errors)
    assert not any("route through the _native shim instead" in item for item in errors)


def test_visual_profile_rejects_cross_layer_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    visual_profile_root = rendering_root / "visual_profile"
    visual_profile_root.mkdir(parents=True)
    (visual_profile_root / "bad_import.py").write_text(
        "from services.rendering.output.typst.emitter import build_typst_source_from_page_specs\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert any("must not import" in item for item in errors)


def test_source_cleanup_planning_accepts_shim_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    source_cleanup_root = rendering_root / "source_cleanup"
    planning_root = source_cleanup_root / "planning"
    planning_root.mkdir(parents=True)
    (planning_root / "_native.py").write_text(
        "from rendering_bridge import read_page_cleanup_contexts\n",
        encoding="utf-8",
    )
    (source_cleanup_root / "consumer.py").write_text(
        "from services.rendering.source_cleanup.planning._native import build_page_contexts\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            source_cleanup_root,
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert not any("must not import" in item for item in errors)
    assert not any("route through the _native shim instead" in item for item in errors)


def test_source_cleanup_planning_rejects_cross_layer_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    source_cleanup_root = rendering_root / "source_cleanup"
    planning_root = source_cleanup_root / "planning"
    planning_root.mkdir(parents=True)
    (planning_root / "_native.py").write_text(
        "from rendering_bridge import read_page_cleanup_contexts\n",
        encoding="utf-8",
    )
    (source_cleanup_root / "bad_import.py").write_text(
        "from services.rendering.output.typst.emitter import build_typst_source_from_page_specs\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            source_cleanup_root,
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert any("must not import" in item for item in errors)


def test_rendering_rejects_direct_wired_reference_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    profile_root = rendering_root / "source"
    profile_root.mkdir(parents=True)
    (profile_root / "bad_import.py").write_text(
        "from services.rendering.source.vector_profile import _page_drawing_count_python\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert any("route through the _native shim instead" in item for item in errors)


def test_pdf_structure_profile_accepts_shim_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    profile_root = rendering_root / "pdf_structure_profile"
    profile_root.mkdir(parents=True)
    (profile_root / "consumer.py").write_text(
        "from services.rendering.pdf_structure_profile._native import build_pdf_structure_profile\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert not any("must not import" in item for item in errors)
    assert not any("route through the _native shim instead" in item for item in errors)


def test_rejects_direct_wired_reference_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    document_root = rendering_root / "analysis" / "document"
    document_root.mkdir(parents=True)
    (document_root / "bad_import.py").write_text(
        "from services.rendering.output.typst.color_adapt import title_text_color_from_visual_components\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert any("route through the _native shim instead" in item for item in errors)


def test_render_document_analysis_accepts_shim_import(tmp_path: Path) -> None:
    rendering_root = tmp_path / "services" / "rendering"
    document_root = rendering_root / "analysis" / "document"
    document_root.mkdir(parents=True)
    (document_root / "consumer.py").write_text(
        "from services.rendering.analysis._native import build_render_document_analysis\n",
        encoding="utf-8",
    )

    errors: list[str] = []
    with (
        mock.patch.object(rendering_checks, "RENDERING_ROOT", rendering_root),
        mock.patch.object(rendering_checks, "RENDERING_SOURCE_ROOT", rendering_root / "source"),
        mock.patch.object(
            rendering_checks,
            "RENDERING_SOURCE_CLEANUP_ROOT",
            rendering_root / "source_cleanup",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_PROFILE_ROOT",
            rendering_root / "analysis" / "profile",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_ROUTE_ROOT",
            rendering_root / "analysis" / "route",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_TYPST_ROOT",
            rendering_root / "output" / "typst",
        ),
        mock.patch.object(
            rendering_checks,
            "RENDERING_LAYOUT_ROOT",
            rendering_root / "layout",
        ),
        mock.patch.object(rendering_checks, "SCRIPTS_ROOT", tmp_path),
        mock.patch.object(architecture_common, "SCRIPTS_ROOT", tmp_path),
    ):
        rendering_checks.check_rendering_internal_boundaries(errors)

    assert not any("must not import" in item for item in errors)
    assert not any("route through the _native shim instead" in item for item in errors)

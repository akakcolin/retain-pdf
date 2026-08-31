from __future__ import annotations

import sys
from pathlib import Path

import pytest


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))


from services.rendering import _routing


@pytest.fixture(autouse=True)
def _clean_routing_state() -> None:
    _routing._FLAG_CACHE.clear()
    _routing.reset()


def test_default_enabled_when_no_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("RETAIN_PDF_NATIVE", raising=False)
    monkeypatch.delenv("RETAIN_PDF_NATIVE_SOURCE", raising=False)
    assert _routing.native_enabled("source") is True


def test_global_off_disables_all(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "0")
    for subsystem in _routing.SUBSYSTEMS:
        assert _routing.native_enabled(subsystem) is False


def test_subsystem_off_beats_global_on(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
    monkeypatch.setenv("RETAIN_PDF_NATIVE_SOURCE", "0")
    assert _routing.native_enabled("source") is False
    assert _routing.native_enabled("background") is True


def test_subsystem_on_beats_global_off(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "0")
    monkeypatch.setenv("RETAIN_PDF_NATIVE_LAYOUT", "1")
    assert _routing.native_enabled("layout") is True
    assert _routing.native_enabled("source") is False


@pytest.mark.parametrize("value", ["0", "false", "off", "no", " 0 ", "OFF"])
def test_false_like_values_disable(
    monkeypatch: pytest.MonkeyPatch, value: str
) -> None:
    monkeypatch.setenv("RETAIN_PDF_NATIVE", value)
    assert _routing.native_enabled("source") is False


@pytest.mark.parametrize("value", ["1", "true", "yes", "on", "anything-else"])
def test_truthy_values_enable(monkeypatch: pytest.MonkeyPatch, value: str) -> None:
    monkeypatch.setenv("RETAIN_PDF_NATIVE", value)
    assert _routing.native_enabled("source") is True


def test_native_eligible_respects_module_flag_at_call_time(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
    assert _routing.native_eligible("source", module_native=True) is True
    assert _routing.native_eligible("source", module_native=False) is False
    # Call-time read: the module flag changing after the first call must be
    # honored (smoke tests monkeypatch shim.NATIVE at runtime).
    assert _routing.native_eligible("source", module_native=False) is False
    assert _routing.native_eligible("source", module_native=True) is True


def test_native_eligible_requires_env_flag(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "0")
    assert _routing.native_eligible("source", module_native=True) is False


def test_routed_records_native_not_built(monkeypatch: pytest.MonkeyPatch) -> None:
    # Mandate off so the empty ALLOWLIST does not raise NativeMandatoryError;
    # this test exercises the reason-recording of the gate itself.
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "0")
    assert _routing.routed("source", "collect_vector_text_rects", module_native=False) is False
    snapshot = _routing.snapshot()
    assert snapshot["total_fallbacks"] == 1
    assert snapshot["fallbacks"]["source"] == {"native_not_built": 1}
    assert snapshot["fallbacks_by_reason"] == {"native_not_built": 1}


def test_routed_records_forced_off(monkeypatch: pytest.MonkeyPatch) -> None:
    # Bridge imported (module_native=True) but the feature flag is explicitly
    # off -> FORCED_OFF, distinct from NATIVE_NOT_BUILT.
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "0")
    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "0")
    assert _routing.routed("source", "collect_vector_text_rects", module_native=True) is False
    snapshot = _routing.snapshot()
    assert snapshot["total_fallbacks"] == 1
    assert snapshot["fallbacks"]["source"] == {"forced_off": 1}
    assert snapshot["fallbacks_by_reason"] == {"forced_off": 1}


def test_routed_in_memory_page(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
    assert _routing.routed("source", "collect_vector_text_rects", module_native=True, path="") is False
    snapshot = _routing.snapshot()
    assert snapshot["fallbacks_by_reason"] == {"in_memory_page": 1}


def test_native_only_shim_raises_when_not_routed(monkeypatch: pytest.MonkeyPatch) -> None:
    # Retired write-path primitives raise instead of falling back to the deleted
    # Python references. Mandate ON surfaces NativeMandatoryError first (via the
    # gate); mandate OFF hits the shim's own defense-in-depth RuntimeError.
    import services.rendering.source._native as source_native

    monkeypatch.setattr(source_native, "NATIVE", False)
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "0")
    with pytest.raises(RuntimeError, match="is native-only"):
        source_native.sanitize_pdf_copy(
            source_pdf_path=Path("/nonexistent.pdf"),
            output_pdf_path=Path("/nonexistent-out.pdf"),
        )

    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "1")
    with pytest.raises(_routing.NativeMandatoryError):
        source_native.sanitize_pdf_copy(
            source_pdf_path=Path("/nonexistent.pdf"),
            output_pdf_path=Path("/nonexistent-out.pdf"),
        )


def test_typst_native_only_shim_raises_when_not_routed(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # The 5 typst shim primitives are native-only; without the bridge they raise
    # (mandate-off: defense-in-depth RuntimeError; mandate-on: the gate surfaces
    # NativeMandatoryError first since the ALLOWLIST entries were removed).
    import services.rendering.output.typst._native as typst_native

    monkeypatch.setattr(typst_native, "NATIVE", False)
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "0")
    with pytest.raises(RuntimeError, match="is native-only"):
        typst_native.emit_typst_source(
            background_pdf_path=Path("/nonexistent.pdf"),
            page_specs=[],
            work_dir=Path("/nonexistent-workdir"),
        )

    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "1")
    with pytest.raises(_routing.NativeMandatoryError):
        typst_native.emit_typst_source(
            background_pdf_path=Path("/nonexistent.pdf"),
            page_specs=[],
            work_dir=Path("/nonexistent-workdir"),
        )


def test_source_cleanup_planning_native_only_shim_raises_when_not_routed(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # The 3 source-cleanup planning shim fns are native-only; without the bridge
    # they raise (mandate-off: defense-in-depth RuntimeError; mandate-on: the
    # gate surfaces NativeMandatoryError first since the ALLOWLIST entries were
    # removed).
    import services.rendering.source_cleanup.planning._native as planning_native

    monkeypatch.setattr(planning_native, "NATIVE", False)
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "0")
    with pytest.raises(RuntimeError, match="is native-only"):
        planning_native.build_page_contexts(
            source_pdf_path=Path("/nonexistent.pdf"),
            page_indices=[0],
        )

    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "1")
    with pytest.raises(_routing.NativeMandatoryError):
        planning_native.build_page_contexts(
            source_pdf_path=Path("/nonexistent.pdf"),
            page_indices=[0],
        )


def test_routed_success_records_no_hit() -> None:
    # routed() is a one-shot gate: success records nothing; the shim records the
    # hit via record_native_hit() after the native call returns.
    assert _routing.routed("typst", "emit_typst_source", module_native=True) is True
    snapshot = _routing.snapshot()
    assert snapshot["total_hits"] == 0
    assert snapshot["total_fallbacks"] == 0


def test_snapshot_shape_counts() -> None:
    _routing.record_native_hit("source", "sanitize_pdf_copy")
    _routing.record_native_hit("source", "sanitize_pdf_copy")
    _routing.record_fallback("background", "build_clean_background_pdf", "strategy_not_ported")
    snapshot = _routing.snapshot()
    assert snapshot["total_hits"] == 2
    assert snapshot["hits"] == {"source": 2}
    assert snapshot["total_fallbacks"] == 1
    assert snapshot["fallbacks"]["background"] == {"strategy_not_ported": 1}
    assert snapshot["fallbacks_by_reason"] == {"strategy_not_ported": 1}


def test_reset_clears_counters() -> None:
    _routing.record_native_hit("source", "sanitize_pdf_copy")
    _routing.record_fallback("source", "save_optimized", "native_bridge_error")
    _routing.reset()
    snapshot = _routing.snapshot()
    assert snapshot["total_hits"] == 0
    assert snapshot["total_fallbacks"] == 0
    assert snapshot["hits"] == {}
    assert snapshot["fallbacks"] == {}


def test_flush_to_writes_stats_json(tmp_path: Path) -> None:
    _routing.record_native_hit("source", "sanitize_pdf_copy")
    target = _routing.flush_to(tmp_path)
    assert target == tmp_path / "native_stats.json"
    assert target.exists()
    assert target.read_text().strip()


def test_flush_to_noop_when_dir_absent(tmp_path: Path) -> None:
    assert _routing.flush_to(tmp_path / "missing") is None


def test_flush_to_noop_when_none() -> None:
    assert _routing.flush_to(None) is None


def test_source_read_family_native_only_shims_raise_when_not_routed(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    # The 7 source page-read primitives are native-only on file-backed pages:
    # without the bridge they raise (mandate-off: defense-in-depth RuntimeError;
    # mandate-on: the gate surfaces NativeMandatoryError first).
    import fitz

    import services.rendering.source._native as source_native

    doc = fitz.open()
    doc.new_page()
    doc.save(str(tmp_path / "page.pdf"))
    doc.close()
    doc = fitz.open(str(tmp_path / "page.pdf"))
    page = doc[0]
    try:
        calls = [
            lambda: source_native.collect_vector_text_rects(page=page, target_rects=[]),
            lambda: source_native.page_drawing_count(page=page),
            lambda: source_native.page_has_large_background_image(page=page),
            lambda: source_native.extract_page_text_spans(page=page),
            lambda: source_native.extract_page_text_blocks(page=page),
            lambda: source_native.collect_page_math_protection_rects(page=page),
            lambda: source_native.collect_page_non_math_span_heights(page=page),
        ]
        monkeypatch.setattr(source_native, "NATIVE", False)
        monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
        monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "0")
        for call in calls:
            with pytest.raises(RuntimeError, match="is native-only"):
                call()
        monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "1")
        for call in calls:
            with pytest.raises(_routing.NativeMandatoryError):
                call()
    finally:
        doc.close()


def test_source_read_family_in_memory_page_uses_reference(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # In-memory pages keep the pure-Python reference (the IN_MEMORY_PAGE
    # capability boundary, which the native path cannot serve) even under the
    # mandate — never blocked. The bridge is present (module_native=True); the
    # empty-path branch of routed() records IN_MEMORY_PAGE and lets the shim
    # fall through to the reference.
    import fitz

    import services.rendering.source._native as source_native

    doc = fitz.open()
    page = doc.new_page()
    try:
        monkeypatch.setattr(source_native, "NATIVE", True)
        monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
        monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "1")
        assert source_native.page_drawing_count(page=page) == 0
        assert source_native.extract_page_text_spans(page=page) == []
        assert source_native.collect_page_math_protection_rects(page=page) == []
    finally:
        doc.close()


@pytest.mark.parametrize(
    "call",
    [
        lambda n: n.build_block_payloads(translated_items=[]),
        lambda n: n.seed_render_fields([]),
        lambda n: n.emit_render_blocks([]),
        lambda n: n.apply_body_pipeline([], page_text_width_med=0.0),
        lambda n: n.mark_adjacent_collision_risk([]),
        lambda n: n.resolve_book_body_font_target_from_payloads([]),
        lambda n: n.prepare_render_payloads_by_page({}),
    ],
)
def test_layout_payload_native_only_shims_raise_when_not_routed(
    monkeypatch: pytest.MonkeyPatch, call
) -> None:
    import services.rendering.layout.payload._native as payload_native

    monkeypatch.setattr(payload_native, "NATIVE", False)
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "0")
    with pytest.raises(RuntimeError, match="is native-only"):
        call(payload_native)
    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "1")
    with pytest.raises(_routing.NativeMandatoryError):
        call(payload_native)


@pytest.mark.parametrize(
    "call",
    [
        lambda n: n.apply_render_pages_policy_fields({}),
        lambda n: n.apply_render_page_policy_fields([]),
    ],
)
def test_policy_native_only_shims_raise_when_not_routed(
    monkeypatch: pytest.MonkeyPatch, call
) -> None:
    import services.rendering.policy._native as policy_native

    monkeypatch.setattr(policy_native, "NATIVE", False)
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "0")
    with pytest.raises(RuntimeError, match="is native-only"):
        call(policy_native)
    monkeypatch.setenv("RETAIN_PDF_NATIVE_MANDATE", "1")
    with pytest.raises(_routing.NativeMandatoryError):
        call(policy_native)

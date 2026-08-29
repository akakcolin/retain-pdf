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
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
    assert _routing.routed("source", "save_optimized", module_native=False) is False
    snapshot = _routing.snapshot()
    assert snapshot["total_fallbacks"] == 1
    assert snapshot["fallbacks"]["source"] == {"native_not_built": 1}
    assert snapshot["fallbacks_by_reason"] == {"native_not_built": 1}


def test_routed_records_forced_off(monkeypatch: pytest.MonkeyPatch) -> None:
    # Bridge imported (module_native=True) but the feature flag is explicitly
    # off -> FORCED_OFF, distinct from NATIVE_NOT_BUILT.
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "0")
    assert _routing.routed("source", "save_optimized", module_native=True) is False
    snapshot = _routing.snapshot()
    assert snapshot["total_fallbacks"] == 1
    assert snapshot["fallbacks"]["source"] == {"forced_off": 1}
    assert snapshot["fallbacks_by_reason"] == {"forced_off": 1}


def test_routed_in_memory_page(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("RETAIN_PDF_NATIVE", "1")
    assert _routing.routed("source", "collect_vector_text_rects", module_native=True, path="") is False
    snapshot = _routing.snapshot()
    assert snapshot["fallbacks_by_reason"] == {"in_memory_page": 1}


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

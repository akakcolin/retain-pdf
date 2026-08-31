from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))

from devtools import d5_baseline

FIXTURES = Path(__file__).resolve().parent / "fixtures"
COMMITTED_BASELINE = REPO_SCRIPTS_ROOT / "devtools" / "d5_baseline.json"
SAMPLE = FIXTURES / "d5_metrics_sample.txt"
REPO_ROOT = Path(__file__).resolve().parents[4]
DESKTOP_MANIFEST = REPO_ROOT / "desktop" / "app" / "backend" / "bundle-manifest.json"

SAMPLE_PARSED = d5_baseline.parse_metrics(SAMPLE.read_text(encoding="utf-8"))


def test_parse_metrics_skips_comments_and_blank_lines() -> None:
    text = "# a comment\n\nretainpdf_jobs_total{status=\"succeeded\"} 3\n"
    parsed = d5_baseline.parse_metrics(text)
    assert parsed == {"retainpdf_jobs_total": [({"status": "succeeded"}, 3.0)]}


def test_parse_metrics_handles_int_and_float_values() -> None:
    parsed = d5_baseline.parse_metrics("m 1\nm_sum 3.105\n")
    assert parsed["m"] == [({}, 1.0)]
    assert parsed["m_sum"] == [({}, 3.105)]


def test_series_filters_by_labels() -> None:
    values = d5_baseline.series(
        SAMPLE_PARSED,
        "retainpdf_render_jobs_total",
        renderer="render_rs",
    )
    assert values == [4.0]


def test_python_renderer_count_sums_all_python_statuses() -> None:
    assert d5_baseline.python_renderer_count(SAMPLE_PARSED) == 0


def test_render_elapsed_mean_is_sum_over_count() -> None:
    mean = d5_baseline.render_elapsed_mean(SAMPLE_PARSED)
    assert mean is not None
    assert abs(mean - 1.035) < 1e-9


def test_render_elapsed_mean_none_when_no_count() -> None:
    assert d5_baseline.render_elapsed_mean({}) is None


def test_native_hit_ratios_extract_subsystem_labels() -> None:
    ratios = d5_baseline.native_hit_ratios(SAMPLE_PARSED)
    assert ratios["source"] == 0.955
    assert ratios["typst"] == 1.0
    assert "background" in ratios


def test_desktop_size_from_manifest_sizes_bytes(tmp_path: Path) -> None:
    manifest = tmp_path / "bundle-manifest.json"
    manifest.write_text(json.dumps({"sizesBytes": {"total": 12345, "rustApi": 100}}))
    assert d5_baseline.desktop_size_bytes(manifest, None) == 12345


def test_desktop_size_dir_fallback_when_manifest_lacks_sizes(tmp_path: Path) -> None:
    manifest = tmp_path / "manifest.json"
    manifest.write_text(json.dumps({"version": "4.1.10"}))
    bundle_dir = tmp_path / "bundle"
    bundle_dir.mkdir()
    (bundle_dir / "a").mkdir()
    (bundle_dir / "a" / "one.bin").write_bytes(b"x" * 100)
    (bundle_dir / "two.bin").write_bytes(b"y" * 50)
    assert d5_baseline.desktop_size_bytes(manifest, bundle_dir) == 150


def test_desktop_size_none_when_no_source(tmp_path: Path) -> None:
    assert d5_baseline.desktop_size_bytes(None, tmp_path / "missing") is None


def test_dir_size_skips_symlinks(tmp_path: Path) -> None:
    target = tmp_path / "real"
    target.mkdir()
    (target / "f").write_bytes(b"z" * 10)
    (tmp_path / "link").symlink_to(target, target_is_directory=True)
    # dirSize lstat: symlinked dir is neither dir nor regular file -> skipped.
    assert d5_baseline._dir_size(tmp_path, "") == 10


def _baseline(**overrides: object) -> dict:
    return json.loads(COMMITTED_BASELINE.read_text(encoding="utf-8")) | overrides


def _observed() -> dict:
    return d5_baseline.observe(
        manifest_path=None,
        desktop_dir=None,
        parsed=SAMPLE_PARSED,
    )


def test_check_committed_baseline_against_sample_passes() -> None:
    observed = d5_baseline.observe(
        manifest_path=DESKTOP_MANIFEST,
        desktop_dir=None,
        parsed=SAMPLE_PARSED,
    )
    rows, ok = d5_baseline.evaluate(json.loads(COMMITTED_BASELINE.read_text(encoding="utf-8")), observed)
    assert ok
    statuses = {row[3] for row in rows}
    assert statuses == {"PASS", "PENDING"}  # desktop pending: committed manifest lacks sizesBytes


def test_evaluate_fails_on_oversized_desktop() -> None:
    baseline = _baseline()
    baseline["desktop"]["max_bytes"] = 1000
    observed = _observed()
    observed["desktop_bytes"] = 2000
    rows, ok = d5_baseline.evaluate(baseline, observed)
    assert not ok
    assert any(row[0] == "desktop_bytes" and row[3] == "FAIL" for row in rows)


def test_evaluate_fails_when_python_renderer_present() -> None:
    observed = _observed()
    observed["python_renderer_count"] = 3
    rows, ok = d5_baseline.evaluate(json.loads(COMMITTED_BASELINE.read_text(encoding="utf-8")), observed)
    assert not ok
    assert any(row[0] == "python_renderer_absent" and row[3] == "FAIL" for row in rows)


def test_evaluate_fails_on_slow_render() -> None:
    baseline = _baseline()
    baseline["render_elapsed"]["max_mean_seconds"] = 1.0
    observed = _observed()
    observed["render_elapsed_mean_seconds"] = 5.0
    rows, ok = d5_baseline.evaluate(baseline, observed)
    assert not ok
    assert any(row[0] == "render_elapsed_mean" and row[3] == "FAIL" for row in rows)


def test_evaluate_fails_on_low_native_ratio() -> None:
    observed = _observed()
    observed["native_ratios"] = {**observed["native_ratios"], "source": 0.5}
    rows, ok = d5_baseline.evaluate(json.loads(COMMITTED_BASELINE.read_text(encoding="utf-8")), observed)
    assert not ok
    assert any(row[0] == "native_hit_ratio source" and row[3] == "FAIL" for row in rows)


def test_evaluate_pending_when_nothing_observed() -> None:
    baseline = json.loads(COMMITTED_BASELINE.read_text(encoding="utf-8"))
    rows, ok = d5_baseline.evaluate(baseline, d5_baseline.observe(manifest_path=None, desktop_dir=None, parsed={}))
    assert ok
    assert all(row[3] == "PENDING" for row in rows)


def test_record_writes_observed_reference_values(tmp_path: Path) -> None:
    baseline_path = tmp_path / "d5_baseline.json"
    manifest = tmp_path / "manifest.json"
    manifest.write_text(json.dumps({"sizesBytes": {"total": 98765}}))
    code = d5_baseline.record(
        baseline_path,
        manifest_path=manifest,
        desktop_dir=None,
        metrics_file=SAMPLE,
        base_url=None,
    )
    assert code == 0
    payload = json.loads(baseline_path.read_text(encoding="utf-8"))
    assert payload["schema"] == d5_baseline.SCHEMA
    assert payload["desktop"]["observed_bytes"] == 98765
    assert payload["python_renderer"]["observed_count"] == 0
    assert payload["render_elapsed"]["observed_mean_seconds"] == 1.035
    assert payload["native_hit_ratio"]["observed"]["source"] == 0.955


def test_record_keeps_existing_targets(tmp_path: Path) -> None:
    baseline_path = tmp_path / "d5_baseline.json"
    baseline_path.write_text(json.dumps({"desktop": {"max_bytes": 111}, "render_elapsed": {"max_mean_seconds": 9.0}}))
    d5_baseline.record(baseline_path, manifest_path=None, desktop_dir=None, metrics_file=SAMPLE, base_url=None)
    payload = json.loads(baseline_path.read_text(encoding="utf-8"))
    assert payload["desktop"]["max_bytes"] == 111
    assert payload["render_elapsed"]["max_mean_seconds"] == 9.0


def test_check_cli_exit_zero_on_committed_sample() -> None:
    code = d5_baseline.main([
        "check",
        "--baseline",
        str(COMMITTED_BASELINE),
        "--metrics-file",
        str(SAMPLE),
        "--desktop-manifest",
        str(DESKTOP_MANIFEST),
    ])
    assert code == 0


def test_check_cli_exit_one_on_violation(tmp_path: Path) -> None:
    baseline_path = tmp_path / "d5_baseline.json"
    baseline_path.write_text(json.dumps({
        "schema": d5_baseline.SCHEMA,
        "updated_at": "2026-08-29",
        "desktop": {"max_bytes": 1},
        "python_renderer": {"target_absent": True},
        "render_elapsed": {"max_mean_seconds": 0.001},
        "native_hit_ratio": {"min": 0.999, "subsystems": ["source"]},
    }))
    code = d5_baseline.main([
        "check",
        "--baseline",
        str(baseline_path),
        "--metrics-file",
        str(SAMPLE),
    ])
    assert code == 1


def test_committed_baseline_schema_is_current() -> None:
    payload = json.loads(COMMITTED_BASELINE.read_text(encoding="utf-8"))
    assert payload["schema"] == d5_baseline.SCHEMA
    assert payload["desktop"]["max_bytes"] > payload["desktop"]["observed_bytes"]


@pytest.mark.parametrize("metric", [
    "retainpdf_jobs_total",
    "retainpdf_render_jobs_total",
    "retainpdf_render_elapsed_seconds_count",
    "retainpdf_render_elapsed_seconds_sum",
    "retainpdf_native_routing_hits",
    "retainpdf_native_routing_fallbacks",
    "retainpdf_native_hit_ratio",
])
def test_sample_contains_every_d5_metric(metric: str) -> None:
    assert metric in SAMPLE_PARSED

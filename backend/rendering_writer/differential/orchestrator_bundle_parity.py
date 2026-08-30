#!/usr/bin/env python3
"""C3-N11 native `build_bundle` vs production delegate differential gate.

Dual-producer bundle parity: produce the `render.bundle.v1` twice for the same
job fixture — once via `render_rs --dump-bundle` (native Rust `build_bundle`)
and once via `python3 entrypoints/run_render_delegate.py --bundle-out` (the
production Python reference) — then compare the two bundles semantically.
Python `json.dumps` and serde_json format floats differently (`1e-05` vs
`0.00001`) and canonicalize job-root paths, so comparison is float-normalized
and job-root-normalized, never byte-exact.

Each producer runs on its OWN job root (identical inputs) so the delegate's
render-source temp files and the native work_dir never cross-contaminate. The
native producer is incremental: `build_bundle` fills the 15 keys across
N11a..N11f, so `ASSERTED_KEYS` grows per batch. All five modes (`typst`,
`typst_visual`, `auto`, `overlay`, `dual`) run the full bundle compare; only
`page_specs` (N11f) stays unasserted.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python \
        ../rendering_writer/differential/orchestrator_bundle_parity.py
"""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
_REPO_ROOT = os.path.abspath(os.path.join(_HERE, "..", "..", ".."))
sys.path.insert(0, _SCRIPTS_DIR)
sys.path.insert(0, _HERE)

import orchestrator_parity as op  # noqa: E402

_PY_BIN = os.environ.get("RETAIN_PDF_PYTHON_BIN") or sys.executable
_DELEGATE_ENTRY = os.path.join(_SCRIPTS_DIR, "entrypoints", "run_render_delegate.py")

#: N11a+ keys the native producer already computes exactly (per mode). Grows
#: as N11b..N11f land the remaining keys. The auto fixture resolves to
#: typst_visual (non-editable text fixture), so it asserts the same keys.
#: N11c adds source_pdf + precleaned_page_indices (render-source prep); N11d adds
#: translated_pages (prepare + first-line indent + policy); N11e adds
#: overlay_page_specs for overlay/dual (color adapt writes _render_* onto every
#: translated_pages item, so the full translated_pages compare holds for all modes).
_ASSERTED_KEYS = {
    "typst": [
        "schema_version",
        "mode",
        "source_pdf",
        "font_family",
        "output_pdf",
        "work_dir",
        "redaction_strategy",
        "precleaned_page_indices",
        "visual_profile_fill_map",
        "page_map",
        "translated_pages",
        "start_page",
        "end_page",
        "overlay_page_specs",
    ],
    "typst_visual": [
        "schema_version",
        "mode",
        "source_pdf",
        "font_family",
        "output_pdf",
        "work_dir",
        "redaction_strategy",
        "precleaned_page_indices",
        "visual_profile_fill_map",
        "page_map",
        "translated_pages",
        "start_page",
        "end_page",
        "overlay_page_specs",
    ],
    "auto": [
        "schema_version",
        "mode",
        "source_pdf",
        "font_family",
        "output_pdf",
        "work_dir",
        "redaction_strategy",
        "precleaned_page_indices",
        "visual_profile_fill_map",
        "page_map",
        "translated_pages",
        "start_page",
        "end_page",
        "overlay_page_specs",
    ],
    "overlay": [
        "schema_version",
        "mode",
        "source_pdf",
        "font_family",
        "output_pdf",
        "work_dir",
        "redaction_strategy",
        "precleaned_page_indices",
        "visual_profile_fill_map",
        "page_map",
        "translated_pages",
        "overlay_page_specs",
        "start_page",
        "end_page",
    ],
    "dual": [
        "schema_version",
        "mode",
        "source_pdf",
        "font_family",
        "output_pdf",
        "work_dir",
        "redaction_strategy",
        "precleaned_page_indices",
        "visual_profile_fill_map",
        "page_map",
        "translated_pages",
        "overlay_page_specs",
        "start_page",
        "end_page",
    ],
}

def _normalize(value, job_root):
    """Float-normalize numbers, absorb the volatile job-root prefix in paths."""
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str):
        if job_root and job_root in value:
            return value.replace(job_root, "<job_root>")
        return value
    if isinstance(value, dict):
        return {k: _normalize(v, job_root) for k, v in value.items()}
    if isinstance(value, list):
        return [_normalize(v, job_root) for v in value]
    return value


def _run_native_dump(render_rs_bin, spec_path, dump_path):
    result = subprocess.run(
        [str(render_rs_bin), "--spec", str(spec_path), "--dump-bundle", str(dump_path)],
        cwd=_SCRIPTS_DIR,
        capture_output=True,
        text=True,
    )
    return result


def _run_python_bundle(spec_path, bundle_path):
    result = subprocess.run(
        [_PY_BIN, _DELEGATE_ENTRY, "--spec", str(spec_path), "--bundle-out", str(bundle_path)],
        cwd=_SCRIPTS_DIR,
        capture_output=True,
        text=True,
    )
    return result


def check_mode(mode):
    render_rs_bin = op._locate_render_rs_bin()
    with tempfile.TemporaryDirectory(prefix="rbs-orch-") as tmp_dir:
        root = Path(tmp_dir)
        source_pdf = root / "source.pdf"
        op.gate._build_source_pdf(source_pdf)

        rs_spec = op._write_job_fixture(root / "job_rs", source_pdf, mode)
        py_spec = op._write_job_fixture(root / "job_py", source_pdf, mode)

        native_bundle = root / "native.json"
        native_result = _run_native_dump(render_rs_bin, rs_spec, native_bundle)

        assert native_result.returncode == 0, (
            f"{mode}: native dump failed ({native_result.returncode}):\n{native_result.stderr}"
        )
        ref_bundle = root / "reference.json"
        ref_result = _run_python_bundle(py_spec, ref_bundle)
        assert ref_result.returncode == 0, (
            f"{mode}: delegate failed ({ref_result.returncode}):\n{ref_result.stderr}"
        )

        native = json.loads(native_bundle.read_text(encoding="utf-8"))
        reference = json.loads(ref_bundle.read_text(encoding="utf-8"))

        assert sorted(native) == sorted(reference), (
            f"{mode}: key set diverges\n  native: {sorted(native)}\n  ref:    {sorted(reference)}"
        )
        for key in _ASSERTED_KEYS[mode]:
            got = _normalize(native[key], str(rs_spec.parent))
            want = _normalize(reference[key], str(py_spec.parent))
            assert got == want, (
                f"{mode}: key {key!r} diverges\n  native: {got}\n  ref:    {want}"
            )
        print(f"orchestrator bundle parity PASS: {mode} (native {len(json.dumps(native))}B "
              f"vs ref {len(json.dumps(reference))}B)")


def check_orchestrator_bundle_parity() -> None:
    for mode in ("typst", "typst_visual", "auto", "overlay", "dual"):
        check_mode(mode)
    print("all orchestrator bundle parity tests pass")


if __name__ == "__main__":
    check_orchestrator_bundle_parity()

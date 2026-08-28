#!/usr/bin/env python3
"""C1 render_rs vs production Python differential gate.

Dual-binary subprocess parity: render the same synthetic job twice — once via
`render_rs --spec` (Rust orchestration, C1) and once via `python3
run_render_only.py --spec` (production Python flow) — then assert page-fact,
full-page pixel, and output-size parity. Reuses the smoke_end_to_end_parity /
smoke_end_to_end_pixel_parity tolerances.

Each binary runs on its OWN job root (identical inputs) so the delegate's
render-source temp files and Python's prewarm cache never cross-contaminate.
`render_rs` locates the delegate via `RETAIN_PDF_RENDER_DELEGATE_SCRIPT` /
`RETAIN_PDF_ENTRYPOINTS_DIR` (the harness sets both); the delegate's child
python is the current interpreter (RETAIN_PDF_PYTHON_BIN), which has the
backend deps.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python \
        ../rendering_writer/differential/orchestrator_parity.py
"""

import json
import os
import shutil
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

import fitz  # noqa: E402

import smoke_end_to_end_parity as gate  # noqa: E402
import smoke_end_to_end_pixel_parity as pixel_gate  # noqa: E402

from foundation.shared.job_dirs import ensure_job_dirs  # noqa: E402
from foundation.shared.job_dirs import resolve_job_dirs  # noqa: E402

WORD_TOL = 0.10
INK_TOL = 0.02
SIZE_RATIO_TOL = 1.5  # Rust save has no fitz subset_fonts; headroom on size

_PY_BIN = os.environ.get("RETAIN_PDF_PYTHON_BIN") or sys.executable
_RENDER_ONLY_ENTRY = os.path.join(_SCRIPTS_DIR, "entrypoints", "run_render_only.py")


def _locate_render_rs_bin() -> Path:
    env = os.environ.get("RETAIN_PDF_RENDER_RS_BIN")
    if env:
        return Path(env)
    for profile in ("release", "debug"):
        candidate = (
            Path(_REPO_ROOT)
            / "backend"
            / "rendering_orchestrator"
            / "target"
            / profile
            / "render_rs"
        )
        if candidate.exists():
            return candidate
    return Path("render_rs")  # PATH fallback


def _contract_item(
    item_id: str,
    page_idx: int,
    block_idx: int,
    bbox,
    translated: str,
) -> dict:
    x0, y0, x1, y1 = bbox
    return {
        "item_id": item_id,
        "page_idx": page_idx,
        "block_idx": block_idx,
        "block_type": "text",
        "block_kind": "text",
        "layout_role": "paragraph",
        "semantic_role": "body",
        "structure_role": "body",
        "policy_translate": True,
        "asset_id": "",
        "reading_order": block_idx,
        "raw_block_type": "text",
        "normalized_sub_type": "body",
        "bbox": [x0, y0, x1, y1],
        "source_text": "source text",
        "lines": [
            {
                "bbox": [x0, y0, x1, y1],
                "spans": [{"type": "text", "content": "source text", "bbox": [x0, y0, x1, y1]}],
            }
        ],
        "metadata": {
            "continuation_hint": {
                "source": "",
                "group_id": "",
                "role": "",
                "scope": "",
                "reading_order": -1,
                "confidence": 0.0,
            },
            "structure_role": "body",
            "structure_group": "",
            "pair_with": "",
        },
        "ocr_continuation_source": "",
        "ocr_continuation_group_id": "",
        "ocr_continuation_role": "",
        "ocr_continuation_scope": "",
        "ocr_continuation_reading_order": -1,
        "ocr_continuation_confidence": 0.0,
        "provider_cross_column_merge_suspected": False,
        "provider_reading_order_unreliable": False,
        "provider_structure_unreliable": False,
        "provider_text_missing_but_bbox_present": False,
        "provider_peer_block_absorbed_text": False,
        "provider_body_repair_applied": False,
        "provider_body_repair_role": "",
        "provider_body_repair_strategy": "",
        "provider_suspected_peer_block_id": "",
        "provider_continuation_suppressed": False,
        "provider_continuation_suppressed_reason": "",
        "provider_column_layout_mode": "",
        "provider_column_index_guess": "",
        "layout_mode": "single",
        "layout_split_x": 200.0,
        "layout_zone": "left_column",
        "layout_zone_rank": 0,
        "layout_zone_size": 1,
        "layout_boundary_role": "body",
        "math_mode": "placeholder",
        "protected_source_text": "source text",
        "mixed_original_protected_source_text": "source text",
        "formula_map": [],
        "protected_map": [],
        "classification_label": "",
        "should_translate": True,
        "skip_reason": "",
        "mixed_literal_action": "",
        "mixed_literal_prefix": "",
        "translation_unit_id": item_id,
        "translation_unit_kind": "single",
        "translation_unit_member_ids": [item_id],
        "translation_unit_protected_source_text": "source text",
        "translation_unit_formula_map": [],
        "translation_unit_protected_map": [],
        "translation_unit_protected_translated_text": translated,
        "translation_unit_translated_text": translated,
        "protected_translated_text": translated,
        "translated_text": translated,
        "continuation_group": "",
        "continuation_prev_text": "",
        "continuation_next_text": "",
        "continuation_decision": "",
        "continuation_candidate_prev_id": "",
        "continuation_candidate_next_id": "",
        "group_protected_source_text": "",
        "group_formula_map": [],
        "group_protected_map": [],
        "group_protected_translated_text": "",
        "group_translated_text": "",
        "final_status": "translated",
        "translation_diagnostics": {
            "item_id": item_id,
            "page_idx": page_idx,
            "route_path": ["block_level", "plain_text"],
            "output_mode_path": ["plain_text"],
            "fallback_to": "",
            "degradation_reason": "",
            "final_status": "translated",
        },
        "candidate_pair_prev_id": "",
        "candidate_pair_next_id": "",
        "translation_context_before": "",
        "translation_context_after": "",
    }


def _translated_pages() -> dict[int, list[dict]]:
    return {
        0: [
            _contract_item("p001-b001", 0, 1, [40.0, 40.0, 360.0, 80.0], "你好世界"),
            _contract_item("p001-b002", 0, 2, [40.0, 140.0, 360.0, 180.0], "第二段译文"),
        ],
        1: [
            _contract_item("p002-b001", 1, 1, [40.0, 40.0, 360.0, 80.0], "再见世界"),
        ],
    }


def _write_job_fixture(job_root: Path, source_pdf: Path, mode: str) -> Path:
    job_dirs = ensure_job_dirs(resolve_job_dirs(job_root))
    shutil.copyfile(source_pdf, job_dirs.source_dir / "source.pdf")

    translated_pages = _translated_pages()
    for page_idx, items in translated_pages.items():
        (job_dirs.translated_dir / f"page-{page_idx}.json").write_text(
            json.dumps(items, ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
    manifest = {
        "schema": "translation_manifest_v1",
        "schema_version": "translation_manifest_v1",
        "pages": [
            {"page_index": page_idx, "page_number": page_idx + 1, "path": f"page-{page_idx}.json"}
            for page_idx in sorted(translated_pages)
        ],
    }
    (job_dirs.translated_dir / "translation-manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )

    spec = {
        "schema_version": "render.stage.v1",
        "stage": "render",
        "job": {
            "job_id": "orchestrator-parity",
            "job_root": str(job_root),
            "workflow": "render",
        },
        "inputs": {
            "source_pdf": str(job_dirs.source_dir / "source.pdf"),
            "translations_dir": str(job_dirs.translated_dir),
            "translation_manifest": str(job_dirs.translated_dir / "translation-manifest.json"),
        },
        "params": {
            "start_page": 0,
            "end_page": -1,
            "render_mode": mode,
            "compile_workers": 0,
            "typst_font_family": "Source Han Serif SC",
            "pdf_compress_dpi": 0,
            "translated_pdf_name": "out.pdf",
            "body_font_size_factor": 1.0,
            "body_leading_factor": 1.0,
            "inner_bbox_shrink_x": 0.0,
            "inner_bbox_shrink_y": 0.0,
            "inner_bbox_dense_shrink_x": 0.0,
            "inner_bbox_dense_shrink_y": 0.0,
            "font_unify_mode": "role_min",
            "source_cleanup_strategy": "pikepdf_text_strip",
            "model": "",
            "base_url": "",
            "credential_ref": "",
        },
    }
    spec_path = job_root / "spec.json"
    spec_path.write_text(json.dumps(spec, ensure_ascii=False, indent=2), encoding="utf-8")
    return spec_path


def _run_rs(render_rs_bin: Path, spec_path: Path) -> Path:
    env = dict(os.environ)
    env["RETAIN_PDF_PYTHON_BIN"] = _PY_BIN
    env["RETAIN_PDF_ENTRYPOINTS_DIR"] = os.path.join(_SCRIPTS_DIR, "entrypoints")
    result = subprocess.run(
        [str(render_rs_bin), "--spec", str(spec_path)],
        cwd=_SCRIPTS_DIR,
        env=env,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(f"render_rs failed ({result.returncode}):\n{result.stderr}")
    return spec_path.parent / "rendered" / "out.pdf"


def _run_py(spec_path: Path) -> Path:
    env = dict(os.environ)
    env["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"
    result = subprocess.run(
        [_PY_BIN, _RENDER_ONLY_ENTRY, "--spec", str(spec_path)],
        cwd=_SCRIPTS_DIR,
        env=env,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(f"run_render_only.py failed ({result.returncode}):\n{result.stderr}")
    return spec_path.parent / "rendered" / "out.pdf"


def _assert_size_ratio(actual: Path, expected: Path, label: str) -> None:
    ratio = actual.stat().st_size / expected.stat().st_size
    assert ratio <= SIZE_RATIO_TOL, (
        f"{label}: size ratio {ratio:.3f} (rust {actual.stat().st_size}B vs "
        f"python {expected.stat().st_size}B) exceeds {SIZE_RATIO_TOL}"
    )


def check_mode(mode: str) -> None:
    render_rs_bin = _locate_render_rs_bin()
    with tempfile.TemporaryDirectory(prefix="rps-orch-") as tmp_dir:
        root = Path(tmp_dir)
        source_pdf = root / "source.pdf"
        gate._build_source_pdf(source_pdf)

        rs_spec = _write_job_fixture(root / "job_rs", source_pdf, mode)
        py_spec = _write_job_fixture(root / "job_py", source_pdf, mode)

        rs_out = _run_rs(render_rs_bin, rs_spec)
        py_out = _run_py(py_spec)

        gate.assert_close(
            gate.page_facts(fitz.open(rs_out)),
            gate.page_facts(fitz.open(py_out)),
            f"{mode} render_rs vs python",
        )
        pixel_gate.assert_pixel_close(rs_out, py_out, f"{mode} render_rs vs python")
        _assert_size_ratio(rs_out, py_out, f"{mode} render_rs vs python")
        print(f"orchestrator parity PASS: {mode} (rust {rs_out.stat().st_size}B vs "
              f"python {py_out.stat().st_size}B)")


def check_orchestrator_parity() -> None:
    for mode in ("typst", "typst_visual"):
        check_mode(mode)
    print("all orchestrator parity tests pass")


if __name__ == "__main__":
    check_orchestrator_parity()

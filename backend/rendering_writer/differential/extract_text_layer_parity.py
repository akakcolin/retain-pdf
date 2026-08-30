#!/usr/bin/env python3
"""C5-N1 native-vs-python parity gate for the skip-OCR text-layer extraction.

Runs the identical `extract_text_layer.stage.v1` job twice — once via
`render_rs --extract-text-layer --spec` (native, no interpreter) and once via
`python3 entrypoints/run_extract_text_layer.py --spec` (production reference) —
then asserts the two `generic_flat_ocr` outputs are semantically identical
(provider, per-page width/height/unit, per-block bbox/type/sub_type/text), so a
skip-OCR job needs no interpreter when routed to the native worker.

The native binary is located via `RETAIN_PDF_RENDER_RS_BIN` or the
rendering_orchestrator target dir. Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python \
        ../rendering_writer/differential/extract_text_layer_parity.py
"""

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
_REPO_ROOT = os.path.abspath(os.path.join(_HERE, "..", "..", ".."))
sys.path.insert(0, _SCRIPTS_DIR)
sys.path.insert(0, _HERE)

import fitz  # noqa: E402

import smoke_end_to_end_parity as gate  # noqa: E402

_PY_BIN = os.environ.get("RETAIN_PDF_PYTHON_BIN") or sys.executable
_EXTRACT_ENTRY = os.path.join(_SCRIPTS_DIR, "entrypoints", "run_extract_text_layer.py")

FLOAT_TOL = 1e-6


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


def _write_extract_spec(job_root: Path, source_pdf: Path) -> Path:
    output_json = job_root / "ocr" / "normalized_document_v1.json"
    spec = {
        "schema_version": "extract_text_layer.stage.v1",
        "stage": "extract_text_layer",
        "job": {"job_id": "extract-parity", "job_root": str(job_root), "workflow": "normalize"},
        "inputs": {"source_pdf": str(source_pdf), "output_json": str(output_json)},
        "params": {},
    }
    spec_path = job_root / "extract.spec.json"
    spec_path.parent.mkdir(parents=True, exist_ok=True)
    spec_path.write_text(json.dumps(spec, ensure_ascii=False, indent=2), encoding="utf-8")
    return spec_path


def _run_native(render_rs_bin: Path, spec_path: Path) -> tuple[dict, str]:
    result = subprocess.run(
        [str(render_rs_bin), "--extract-text-layer", "--spec", str(spec_path)],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"render_rs --extract-text-layer failed ({result.returncode}):\n{result.stderr}"
        )
    return _read_output(spec_path), result.stdout


def _read_output(spec_path: Path) -> dict:
    output_json = spec_path.parent / "ocr" / "normalized_document_v1.json"
    with output_json.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def _run_python(spec_path: Path) -> dict:
    result = subprocess.run(
        [_PY_BIN, _EXTRACT_ENTRY, "--spec", str(spec_path)],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"run_extract_text_layer.py failed ({result.returncode}):\n{result.stderr}"
        )
    return _read_output(spec_path)


def _run_native_error(render_rs_bin: Path, spec_path: Path) -> str:
    result = subprocess.run(
        [str(render_rs_bin), "--extract-text-layer", "--spec", str(spec_path)],
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0, "native extract should fail on a textless PDF"
    return result.stderr


def _run_python_error(spec_path: Path) -> str:
    result = subprocess.run(
        [_PY_BIN, _EXTRACT_ENTRY, "--spec", str(spec_path)],
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0, "python extract should fail on a textless PDF"
    return result.stderr


def _build_blank_pdf(path: Path) -> None:
    doc = fitz.open()
    doc.new_page(width=200, height=200)
    doc.save(path)
    doc.close()


def _assert_json_close(native: dict, python: dict, label: str) -> None:
    assert native.get("provider") == python.get("provider"), (
        f"{label}: provider {native.get('provider')} != {python.get('provider')}"
    )
    native_pages = native.get("pages", [])
    python_pages = python.get("pages", [])
    assert len(native_pages) == len(python_pages), (
        f"{label}: page count {len(native_pages)} != {len(python_pages)}"
    )
    for page_idx, (n_page, p_page) in enumerate(zip(native_pages, python_pages)):
        assert n_page.get("page_index") == p_page.get("page_index"), (
            f"{label} p{page_idx}: page_index mismatch"
        )
        for key in ("width", "height"):
            assert _float_close(n_page.get(key), p_page.get(key)), (
                f"{label} p{page_idx}: {key} {n_page.get(key)} != {p_page.get(key)}"
            )
        assert n_page.get("unit") == p_page.get("unit"), (
            f"{label} p{page_idx}: unit {n_page.get('unit')} != {p_page.get('unit')}"
        )
        n_blocks = n_page.get("blocks", [])
        p_blocks = p_page.get("blocks", [])
        assert len(n_blocks) == len(p_blocks), (
            f"{label} p{page_idx}: block count {len(n_blocks)} != {len(p_blocks)}"
        )
        for block_idx, (n_block, p_block) in enumerate(zip(n_blocks, p_blocks)):
            assert n_block.get("type") == p_block.get("type") == "text", (
                f"{label} p{page_idx} b{block_idx}: type mismatch"
            )
            assert n_block.get("sub_type") == p_block.get("sub_type"), (
                f"{label} p{page_idx} b{block_idx}: sub_type mismatch"
            )
            assert n_block.get("text") == p_block.get("text"), (
                f"{label} p{page_idx} b{block_idx}: text {n_block.get('text')!r} != {p_block.get('text')!r}"
            )
            for i, (n_val, p_val) in enumerate(zip(n_block.get("bbox", []), p_block.get("bbox", []))):
                assert _float_close(n_val, p_val), (
                    f"{label} p{page_idx} b{block_idx} bbox[{i}]: {n_val} != {p_val}"
                )


def _float_close(actual, expected) -> bool:
    try:
        return abs(float(actual) - float(expected)) <= FLOAT_TOL
    except (TypeError, ValueError):
        return False


def check_extract_text_layer_parity() -> None:
    render_rs_bin = _locate_render_rs_bin()
    with tempfile.TemporaryDirectory(prefix="rps-extract-") as tmp_dir:
        root = Path(tmp_dir)
        source_pdf = root / "source.pdf"
        gate._build_source_pdf(source_pdf)

        native_spec = _write_extract_spec(root / "job_native", source_pdf)
        python_spec = _write_extract_spec(root / "job_python", source_pdf)

        native_doc, native_stdout = _run_native(render_rs_bin, native_spec)
        python_doc = _run_python(python_spec)

        _assert_json_close(native_doc, python_doc, "extract_text_layer native vs python")

        match = re.search(r"text layer extracted: pages=(\d+) blocks=(\d+) path=", native_stdout)
        assert match, f"native stdout missing extract label:\n{native_stdout}"
        assert int(match.group(1)) == len(native_doc["pages"])
        assert int(match.group(2)) == sum(
            len(page.get("blocks", [])) for page in native_doc["pages"]
        )
        print("extract_text_layer parity PASS: native vs python")

        blank = root / "blank.pdf"
        _build_blank_pdf(blank)
        native_err = _run_native_error(render_rs_bin, _write_extract_spec(root / "blank_native", blank))
        python_err = _run_python_error(_write_extract_spec(root / "blank_python", blank))
        assert "源 PDF 不含可提取的文本层" in native_err, native_err
        assert "源 PDF 不含可提取的文本层" in python_err, python_err
        print("extract_text_layer error parity PASS: textless PDF bails in both workers")


if __name__ == "__main__":
    check_extract_text_layer_parity()

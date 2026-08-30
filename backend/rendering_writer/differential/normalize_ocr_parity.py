#!/usr/bin/env python3
"""C5-N2a..C5-N2d native-vs-python parity gate for the normalize_ocr worker
(mineru / mineru_content_list_v2 / paddle / generic_flat_ocr).

Runs the identical `normalize.stage.v1` job twice — once via
`render_rs --normalize-ocr --spec` (native, no interpreter) and once via
`python3 entrypoints/run_normalize_ocr.py --spec` (production reference) on the
same raw provider layout payload + source PDF — then asserts the two
`document.v1.json` artifacts are semantically identical (per-page width/height,
per-block type/sub_type/text/bbox/lines/segments, contract enrichment, and the
post-rescale paddle-style line rebuild), plus the normalization report's
validation + defaults counts and the stdout label numbers.

The native binary is located via `RETAIN_PDF_RENDER_RS_BIN` or the
rendering_orchestrator target dir. Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python \
        ../rendering_writer/differential/normalize_ocr_parity.py
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

_PY_BIN = os.environ.get("RETAIN_PDF_PYTHON_BIN") or sys.executable
_NORMALIZE_ENTRY = os.path.join(_SCRIPTS_DIR, "entrypoints", "run_normalize_ocr.py")

FLOAT_TOL = 1e-6
DOCUMENT_JSON = "document.v1.json"
REPORT_JSON = "document.v1.report.json"


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


def _text_block(raw_type: str, text: str, bbox: list, *, extra_spans=None) -> dict:
    spans = [
        {
            "type": "text",
            "content": text,
            "bbox": bbox,
            "score": 0.9,
        }
    ]
    if extra_spans:
        spans = extra_spans + spans
    return {
        "type": raw_type,
        "sub_type": "",
        "bbox": bbox,
        "lines": [{"bbox": bbox, "spans": spans}],
    }


def _build_layout_payload() -> dict:
    long_body = (
        "The dynamics of complex adaptive systems emerge from the nonlinear "
        "interactions among many constituent agents, each responding to local "
        "cues while collectively producing global structure. We formalize the "
        "coupling between these scales and show how collective behavior persists."
    )
    return {
        "pdf_info": [
            {
                "page_size": [595.0, 842.0],
                "para_blocks": [
                    _text_block(
                        "title",
                        "Understanding Complex Adaptive Systems",
                        [60, 60, 535, 100],
                    ),
                    _text_block("text", long_body, [60, 120, 535, 180]),
                    {
                        "type": "text",
                        "sub_type": "",
                        "bbox": [60, 200, 535, 240],
                        "lines": [
                            {
                                "bbox": [60, 200, 535, 240],
                                "spans": [
                                    {
                                        "type": "text",
                                        "content": "the cost function ",
                                        "bbox": [60, 200, 220, 240],
                                        "score": 0.95,
                                    },
                                    {
                                        "type": "inline_equation",
                                        "content": "C(\\theta) = \\alpha + \\beta",
                                        "bbox": [220, 200, 360, 240],
                                        "score": 0.85,
                                    },
                                    {
                                        "type": "text",
                                        "content": " is minimized over the feasible set.",
                                        "bbox": [360, 200, 535, 240],
                                        "score": 0.9,
                                    },
                                ],
                            }
                        ],
                    },
                    {
                        "type": "image",
                        "sub_type": "",
                        "bbox": [60, 260, 300, 420],
                        "lines": [],
                    },
                    _text_block(
                        "page_footer",
                        "Journal of Adaptive Systems, 2026",
                        [60, 810, 535, 830],
                    ),
                ],
            },
            {
                "page_size": [595.0, 842.0],
                "para_blocks": [
                    {
                        "type": "interline_equation",
                        "sub_type": "",
                        "bbox": [60, 120, 535, 170],
                        "lines": [
                            {
                                "bbox": [60, 120, 535, 170],
                                "spans": [
                                    {
                                        "type": "interline_equation",
                                        "content": "E = mc^2 + \\frac{1}{2} mv^2",
                                        "bbox": [60, 120, 535, 170],
                                        "score": 0.8,
                                    }
                                ],
                            }
                        ],
                    },
                    {
                        "type": "table",
                        "sub_type": "",
                        "bbox": [60, 190, 535, 340],
                        "lines": [],
                    },
                    {
                        "type": "text",
                        "sub_type": "",
                        "bbox": [60, 360, 535, 400],
                        "lines": [
                            {
                                "bbox": [60, 360, 535, 400],
                                "spans": [
                                    {
                                        "type": "text",
                                        "content": "the torsional angle \x01 represents the strain energy",
                                        "bbox": [60, 360, 535, 400],
                                        "score": 0.9,
                                    },
                                    {
                                        "type": "text",
                                        "content": " and drives conformer selection.",
                                        "bbox": [60, 360, 535, 400],
                                        "score": 0.88,
                                    },
                                ],
                            }
                        ],
                    },
                    _text_block(
                        "text",
                        "Conclusion: robustness follows from modularity.",
                        [60, 420, 535, 450],
                    ),
                ],
            },
        ]
    }


def _clv2_seg(raw_type: str, content: str) -> dict:
    return {"type": raw_type, "content": content}


def _build_content_list_v2_payload() -> list:
    return [
        [
            {
                "type": "title",
                "bbox": [60, 60, 535, 100],
                "content": {
                    "title_content": [
                        _clv2_seg("text", "Adaptive Systems in Complex Environments")
                    ]
                },
            },
            {
                "type": "page_header",
                "bbox": [60, 40, 535, 60],
                "content": {
                    "page_header_content": [_clv2_seg("text", "Journal of Adaptive Systems")]
                },
            },
            {
                "type": "paragraph",
                "bbox": [60, 120, 535, 190],
                "content": {
                    "paragraph_content": [
                        _clv2_seg(
                            "text",
                            "The dynamics of complex adaptive systems emerge from nonlinear "
                            "interactions, coupling local agent behavior to global structure",
                        ),
                        _clv2_seg("equation_inline", r"C(\theta) = \alpha + \beta"),
                        _clv2_seg("text", "under bounded rationality."),
                    ]
                },
            },
            {
                "type": "list",
                "bbox": [60, 200, 535, 260],
                "content": {
                    "list_items": [
                        {"item_content": [_clv2_seg("text", "first principle")]},
                        {"item_content": [_clv2_seg("text", "second principle")]},
                    ]
                },
            },
            {"type": "image", "bbox": [60, 280, 300, 420]},
            {
                "type": "page_number",
                "bbox": [280, 810, 320, 830],
                "content": {"page_number_content": [_clv2_seg("text", "1")]},
            },
        ],
        [
            {
                "type": "paragraph",
                "bbox": [60, 120, 535, 200],
                "content": {
                    "paragraph_content": [
                        _clv2_seg(
                            "text",
                            "1. first\n2. second\n3. third\n4. fourth\n5. fifth\n6. sixth",
                        )
                    ]
                },
            },
            {
                "type": "paragraph",
                "bbox": [60, 220, 535, 300],
                "content": {
                    "paragraph_content": [
                        _clv2_seg(
                            "text",
                            "Conclusion: robustness follows from modularity, and the torsional angle ",
                        ),
                        _clv2_seg("equation_inline", r"E = mc^2 + \frac{1}{2} mv^2"),
                    ]
                },
            },
            {
                "type": "page_footer",
                "bbox": [60, 810, 535, 830],
                "content": {
                    "page_footer_content": [_clv2_seg("text", "Adaptive Systems, 2026")]
                },
            },
        ],
    ]


def _paddle_block(label: str, content: str, bbox: list, **extra) -> dict:
    return {"block_label": label, "block_content": content, "block_bbox": bbox, **extra}


def _build_paddle_layout_payload() -> dict:
    long_body = (
        "The dynamics of layout parsing require rich spatial context; we formalize "
        "the coupling between text blocks and their geometry and show how the "
        "reading order can be recovered across the page."
    )
    second_body = (
        "Our experimental study compares three downstream parsers across a corpus "
        "of mixed academic pages and reports a consistent accuracy improvement."
    )
    page_two_left = (
        "The methods section applies the recovered reading order to a fresh corpus "
        "of biomedical preprints and measures the paragraph reconstruction score."
    )
    page_two_right = (
        "Baseline methods that ignore geometry lose up to twelve percent of the "
        "available signal on dense two-column pages."
    )
    return {
        "layoutParsingResults": [
            {
                "inputImage": "raw/page_0.png",
                "markdown": {
                    "text": "Understanding Paddle Layout\n" + long_body + "\n![Figure 1](fig_1.png)\n" + second_body,
                    "images": {"fig_1.png": "rendered/fig_1.png"},
                },
                "outputImages": {"1": "out_0.png"},
                "prunedResult": {
                    "page_count": 2,
                    "model_settings": {"enable_body_repair": True, "det_model": "PP-DocLayout"},
                    "layout_det_res": {
                        "boxes": [
                            {
                                "label": "title",
                                "cls_id": 1,
                                "score": 0.98,
                                "order": 0,
                                "coordinate": [60, 60, 535, 100],
                                "polygon_points": [60, 60, 535, 60, 535, 100, 60, 100],
                            },
                            {
                                "label": "text",
                                "cls_id": 2,
                                "score": 0.9,
                                "order": 1,
                                "coordinate": [60, 220, 535, 260],
                                "polygon_points": [60, 220, 535, 220, 535, 260, 60, 260],
                            },
                            {
                                "label": "footer",
                                "cls_id": 3,
                                "score": 0.95,
                                "order": 2,
                                "coordinate": [60, 810, 535, 830],
                                "polygon_points": [60, 810, 535, 810, 535, 830, 60, 830],
                            },
                        ]
                    },
                    "parsing_res_list": [
                        _paddle_block("doc_title", "Understanding Paddle Layout", [60, 60, 535, 100], block_id="b0", block_order=0),
                        _paddle_block("abstract", "We study layout parsing of complex documents with structured output.", [60, 120, 535, 160], block_id="b1"),
                        _paddle_block("paragraph_title", "Introduction", [60, 180, 535, 210], block_id="b2", block_order=1),
                        _paddle_block("text", long_body, [60, 220, 535, 260], group_id="para", block_order=2, block_id="b3"),
                        _paddle_block("text", second_body, [60, 270, 535, 310], group_id="para", block_order=3, block_id="b4"),
                        _paddle_block("paragraph_title", "Methods", [60, 330, 535, 360], block_id="b5", block_order=4),
                        _paddle_block("content", "1. Introduction\n2. Methods\n3. Results\n4. Conclusion", [60, 380, 535, 420], block_id="b6"),
                        _paddle_block("figure_title", "Figure 1: Layout parsing overview", [60, 440, 535, 470], block_id="b7"),
                        _paddle_block("image", '<img src="fig_1.png" />', [60, 480, 300, 600], block_id="b8"),
                        _paddle_block("table", "| col1 | col2 |\n|------|------|\n| a    | b    |", [320, 480, 535, 600], block_id="b9"),
                        _paddle_block("footer", "Journal of Layout Parsing, 2026", [60, 810, 535, 830], block_id="b10"),
                        _paddle_block("number", "1", [285, 800, 310, 820], block_id="b11"),
                    ],
                },
            },
            {
                "inputImage": "raw/page_1.png",
                "markdown": {"text": page_two_left + "\n" + page_two_right, "images": {}},
                "outputImages": {},
                "prunedResult": {
                    "page_count": 2,
                    "model_settings": {"enable_body_repair": True, "det_model": "PP-DocLayout"},
                    "layout_det_res": {"boxes": []},
                    "parsing_res_list": [
                        _paddle_block("paragraph_title", "Results", [60, 60, 535, 90], block_id="c0", block_order=5),
                        _paddle_block("text", page_two_left, [60, 110, 300, 150], group_id="para", block_order=6, block_id="c1"),
                        _paddle_block("text", page_two_right, [320, 110, 535, 150], block_id="c2"),
                        _paddle_block("text", "More text in the left column below the first body paragraph.", [60, 170, 300, 210], block_id="c3"),
                        _paddle_block("text", "And additional text in the right column to balance the layout.", [320, 170, 535, 210], block_id="c4"),
                        _paddle_block("formula_number", "(1)", [60, 230, 535, 260], block_id="c5"),
                        _paddle_block("display_formula", r"E = mc^2 + \frac{1}{2} mv^2", [60, 260, 535, 300], block_id="c6"),
                        _paddle_block("reference_content", "[1] Smith, J. A study of layout parsing. J. Doc. 2026.", [60, 320, 535, 360], block_id="c7"),
                        _paddle_block("footer", "Journal of Layout Parsing, 2026", [60, 810, 535, 830], block_id="c8"),
                        _paddle_block("number", "2", [285, 800, 310, 820], block_id="c9"),
                    ],
                },
            },
        ],
        "dataInfo": {
            "pages": [
                {"width": 595.0, "height": 842.0},
                {"width": 595.0, "height": 842.0},
            ]
        },
        "preprocessedImages": ["preprocessed/page_0.png", "preprocessed/page_1.png"],
    }


def _build_generic_flat_ocr_payload() -> dict:
    long_body = (
        "The dynamics of complex adaptive systems emerge from the nonlinear "
        "interactions among many constituent agents, each responding to local "
        "cues while collectively producing global structure."
    )
    page_two_body = (
        "Our experimental study compares three downstream parsers across a corpus "
        "of mixed academic pages and reports a consistent accuracy improvement."
    )
    return {
        "provider": "generic_flat_ocr",
        "pages": [
            {
                "width": 595.0,
                "height": 842.0,
                "unit": "pt",
                "blocks": [
                    {"type": "text", "sub_type": "title", "bbox": [60, 60, 535, 100], "text": "Understanding Complex Systems"},
                    {"type": "text", "sub_type": "body", "bbox": [60, 110, 535, 130], "text": "J. Author"},
                    {"type": "text", "sub_type": "abstract", "bbox": [60, 140, 535, 180], "text": "We study how local agent behavior produces global structure."},
                    {"type": "text", "sub_type": "heading", "bbox": [60, 190, 535, 220], "text": "Introduction"},
                    {
                        "type": "text",
                        "sub_type": "body",
                        "bbox": [60, 230, 535, 280],
                        "text": long_body,
                        "lines": [{"bbox": [60, 230, 535, 250], "spans": [{"type": "text", "raw_type": "text", "text": long_body[:40], "bbox": [60, 230, 300, 250], "score": 0.9}]}],
                        "segments": [{"type": "text", "raw_type": "text", "text": long_body, "bbox": [60, 230, 535, 250]}],
                        "tags": ["body"],
                        "derived": {"role": "body", "by": "fixture", "confidence": 0.9},
                        "continuation_hint": {"source": "provider", "group_id": "g1", "role": "head", "scope": "cross_page", "reading_order": 4, "confidence": 0.8},
                        "metadata": {"source_line": 5},
                    },
                    {"type": "image", "sub_type": "", "bbox": [60, 300, 300, 420], "text": ""},
                    {"type": "text", "sub_type": "reference_entry", "bbox": [60, 620, 535, 660], "text": "[1] Smith, J. A study of complex systems. J. Doc. 2026."},
                    {"type": "text", "sub_type": "footnote", "bbox": [60, 700, 535, 720], "text": "Footnote text."},
                    {"type": "text", "sub_type": "footer", "bbox": [60, 810, 535, 830], "text": "Journal of Complex Systems, 2026"},
                ],
            },
            {
                "width": 595.0,
                "height": 842.0,
                "unit": "pt",
                "blocks": [
                    {"type": "text", "sub_type": "header", "bbox": [60, 40, 535, 60], "text": "Adaptive Systems"},
                    {"type": "text", "sub_type": "heading", "bbox": [60, 60, 535, 90], "text": "Methods"},
                    {"type": "text", "sub_type": "body", "bbox": [60, 100, 535, 160], "text": page_two_body},
                    {"type": "text", "sub_type": "page_number", "bbox": [285, 800, 310, 820], "text": "2"},
                ],
            },
        ],
    }


def _build_source_pdf(path: Path) -> None:
    doc = fitz.open()
    for _ in range(2):
        page = doc.new_page(width=595, height=842)
        page.insert_text((72, 72), "Fixture source page", fontsize=12)
    doc.save(path)
    doc.close()


def _write_normalize_spec(job_root: Path, source_pdf: Path, source_json: Path, *, provider: str = "mineru") -> Path:
    spec = {
        "schema_version": "normalize.stage.v1",
        "stage": "normalize",
        "job": {"job_id": "normalize-parity", "job_root": str(job_root), "workflow": "normalize"},
        "inputs": {
            "provider": provider,
            "source_json": str(source_json),
            "source_pdf": str(source_pdf),
            "provider_version": "2025.11.1",
            "provider_result_json": "",
            "provider_zip": "",
            "provider_raw_dir": "",
        },
        "params": {},
    }
    spec_path = job_root / "normalize.spec.json"
    spec_path.parent.mkdir(parents=True, exist_ok=True)
    # The python reference requires the full job-dir contract to exist up front.
    for sub in ("source", "ocr", "translated", "rendered", "artifacts", "logs"):
        (job_root / sub).mkdir(parents=True, exist_ok=True)
    spec_path.write_text(json.dumps(spec, ensure_ascii=False, indent=2), encoding="utf-8")
    return spec_path


def _run_native(render_rs_bin: Path, spec_path: Path) -> tuple[dict, dict, str]:
    result = subprocess.run(
        [str(render_rs_bin), "--normalize-ocr", "--spec", str(spec_path)],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"render_rs --normalize-ocr failed ({result.returncode}):\n{result.stderr}"
        )
    return _read_outputs(spec_path), result.stdout


def _run_python(spec_path: Path) -> tuple[dict, dict, str]:
    result = subprocess.run(
        [_PY_BIN, _NORMALIZE_ENTRY, "--spec", str(spec_path)],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"run_normalize_ocr.py failed ({result.returncode}):\n{result.stderr}"
        )
    return _read_outputs(spec_path), result.stdout


def _read_outputs(spec_path: Path) -> tuple[dict, dict]:
    normalized_dir = spec_path.parent / "ocr" / "normalized"
    document = json.loads((normalized_dir / DOCUMENT_JSON).read_text(encoding="utf-8"))
    report = json.loads((normalized_dir / REPORT_JSON).read_text(encoding="utf-8"))
    return document, report


def _assert_semantic_equal(native, python, label: str, path: str = "$") -> None:
    if isinstance(native, dict) and isinstance(python, dict):
        native_keys = set(native.keys())
        python_keys = set(python.keys())
        assert native_keys == python_keys, (
            f"{label} {path}: keys {sorted(native_keys ^ python_keys)} only on one side"
        )
        for key in native_keys:
            _assert_semantic_equal(native[key], python[key], label, f"{path}.{key}")
        return
    if isinstance(native, list) and isinstance(python, list):
        assert len(native) == len(python), (
            f"{label} {path}: list len {len(native)} != {len(python)}"
        )
        for index, (n_item, p_item) in enumerate(zip(native, python)):
            _assert_semantic_equal(n_item, p_item, label, f"{path}[{index}]")
        return
    if isinstance(native, (int, float)) and isinstance(python, (int, float)):
        assert abs(float(native) - float(python)) <= FLOAT_TOL, (
            f"{label} {path}: {native} != {python}"
        )
        return
    assert type(native) is type(python), f"{label} {path}: type {type(native)} != {type(python)}"
    assert native == python, f"{label} {path}: {native!r} != {python!r}"


def _extract_label_numbers(stdout: str) -> tuple[int, int, int, int, int]:
    validated = re.search(r"pages=(\d+) blocks=(\d+)", stdout)
    assert validated, f"stdout missing validated label:\n{stdout}"
    report = re.search(
        r"pages_observed=(\d+) blocks_observed=(\d+) "
        r"defaulted_document_fields=(\d+) defaulted_page_fields=(\d+) "
        r"defaulted_block_fields=(\d+)",
        stdout,
    )
    assert report, f"stdout missing report label:\n{stdout}"
    return tuple(int(v) for v in (*validated.groups(), *report.groups()))


def _check_provider_parity(payload_builder, provider: str) -> None:
    render_rs_bin = _locate_render_rs_bin()
    assert render_rs_bin.exists(), f"render_rs binary not found: {render_rs_bin}"
    with tempfile.TemporaryDirectory(prefix="rps-normalize-") as tmp_dir:
        root = Path(tmp_dir)
        source_pdf = root / "source.pdf"
        _build_source_pdf(source_pdf)
        source_json = root / "layout.json"
        source_json.write_text(
            json.dumps(payload_builder(), ensure_ascii=False, indent=2),
            encoding="utf-8",
        )

        native_spec = _write_normalize_spec(root / "job_a" / "job", source_pdf, source_json, provider=provider)
        python_spec = _write_normalize_spec(root / "job_b" / "job", source_pdf, source_json, provider=provider)

        (native_doc, native_report), native_stdout = _run_native(render_rs_bin, native_spec)
        (python_doc, python_report), python_stdout = _run_python(python_spec)

        _assert_semantic_equal(native_doc, python_doc, "document")
        _assert_semantic_equal(
            native_report["validation"], python_report["validation"], "report.validation"
        )
        _assert_semantic_equal(native_report["defaults"], python_report["defaults"], "report.defaults")
        assert native_report["detected_provider"] == python_report["detected_provider"] == provider
        assert native_report["provider"] == python_report["provider"] == provider
        assert native_report["provider_was_explicit"] is python_report["provider_was_explicit"] is True
        assert native_report["provider_mismatch_allowed"] is False

        native_nums = _extract_label_numbers(native_stdout)
        python_nums = _extract_label_numbers(python_stdout)
        assert native_nums == python_nums, f"label numbers {native_nums} != {python_nums}"
        assert "schema version: document.v1" in native_stdout

        page_count = len(native_doc["pages"])
        block_count = sum(len(page["blocks"]) for page in native_doc["pages"])
        assert native_nums[0] == page_count and native_nums[1] == block_count
        print(f"normalize_ocr parity PASS ({provider}): native vs python ({page_count} pages, {block_count} blocks)")


def check_normalize_ocr_parity() -> None:
    _check_provider_parity(_build_layout_payload, "mineru")
    _check_provider_parity(_build_content_list_v2_payload, "mineru_content_list_v2")
    _check_provider_parity(_build_paddle_layout_payload, "paddle")
    _check_provider_parity(_build_generic_flat_ocr_payload, "generic_flat_ocr")


if __name__ == "__main__":
    check_normalize_ocr_parity()

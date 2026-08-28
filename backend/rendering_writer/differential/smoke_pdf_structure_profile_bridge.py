#!/usr/bin/env python3
"""Native bridge smoke test for `pdf_structure_profile` (Phase B2-Inc2).

Three parts:

1. Three-way: every `tests/pdf_structure_corpus.json` case, run on file-backed
   PDFs — `build_pdf_structure_profile` (routed through the
   `pdf_structure_profile/_native.py` shim, NATIVE=True) == the pure-Python
   reference `_build_pdf_structure_profile_python` == the corpus records.
   Positionally: bboxes within 0.01 pt, text / source / flags exact,
   `object_type` (kind) exact, page width/height within 0.01, `item_hits`
   object_id + ratio within 0.001. A call counter on the four native bridges
   confirms native was actually hit on at least one case. The corpus only pins
   single-level forms (pikepdf, identity placement) where fitz `get_xobjects()`
   and the native resource-dict scan agree; the nested-form divergence is
   documented in the shim docstring, not pinned here.

2. Primitive: `read_page_form_xobjects` reproduces the corpus
   `form_xobjects_primitive` facts (name / xref exact, bbox within 0.01 pt).

3. Boundary: corrupt bytes make the native path fall back and raise exactly
   like the reference; a vanished backing file likewise; an out-of-range
   `pages` key falls back to the reference's page filter (empty profile).

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_pdf_structure_profile_bridge.py
"""

import base64
import json
import os
import sys
import tempfile
from pathlib import Path

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering.pdf_structure_profile import _native as psp_native  # noqa: E402
from services.rendering.pdf_structure_profile.contracts import (  # noqa: E402
    PdfStructureDocumentProfile,
)
from services.rendering.pdf_structure_profile.sampler import (  # noqa: E402
    _build_pdf_structure_profile_python,
    build_pdf_structure_profile,
)

# Corpus lives next to its Rust replay (`rendering_reader/tests/form_xobjects_diff.rs`).
CORPUS = os.path.abspath(
    os.path.join(_HERE, "..", "..", "rendering_reader", "tests", "pdf_structure_corpus.json")
)

TOL_BBOX = 0.01
TOL_SIZE = 0.01
TOL_RATIO = 0.001

_BRIDGES = (
    "_native_read_page_cleanup_contexts",
    "_native_read_page_text_spans",
    "_native_read_page_form_xobjects",
    "_native_read_page_geometry",
)


def _close(a: float, b: float, tol: float) -> bool:
    return abs(a - b) <= tol


def _box_close(a: list, b: list) -> bool:
    if a[1] != b[1] or a[3] != b[3] or a[4] != b[4] or a[5] != b[5]:
        return False
    return all(_close(va, vb, TOL_BBOX) for va, vb in zip(a[2], b[2]))


def _hit_close(a: list, b: list) -> bool:
    return a[0] == b[0] and a[1] == b[1] and a[2] == b[2] and _close(a[3], b[3], TOL_RATIO)


def _box(entry) -> list:
    return [
        entry.object_id,
        entry.object_type,
        [float(v) for v in entry.bbox],
        entry.source,
        entry.text,
        [str(flag) for flag in entry.flags],
    ]


def _profile_pages(profile: PdfStructureDocumentProfile) -> dict:
    pages = {}
    for idx, pr in sorted(profile.pages.items()):
        pages[str(idx)] = {
            "page_width_pt": float(pr.page_width_pt),
            "page_height_pt": float(pr.page_height_pt),
            "text_objects": [_box(obj) for obj in pr.text_objects],
            "text_spans": [_box(obj) for obj in pr.text_spans],
            "path_objects": [_box(obj) for obj in pr.path_objects],
            "image_objects": [_box(obj) for obj in pr.image_objects],
            "form_xobjects": [_box(obj) for obj in pr.form_xobjects],
            "item_hits": [
                [hit.item_id, hit.object_id, hit.object_type, float(hit.overlap_ratio)]
                for hit in pr.item_hits
            ],
        }
    return pages


def _assert_pages_equal(actual: dict, expected: dict, label: str) -> None:
    assert set(actual) == set(expected), f"{label}: page sets {set(actual)} vs {set(expected)}"
    for idx, page in expected.items():
        got = actual[idx]
        for key in ("page_width_pt", "page_height_pt"):
            assert _close(got[key], page[key], TOL_SIZE), f"{label} p{idx}: {key} {got[key]} vs {page[key]}"
        for coll in ("text_objects", "text_spans", "path_objects", "image_objects", "form_xobjects"):
            assert len(got[coll]) == len(page[coll]), f"{label} p{idx}: {coll} count"
            for n, (ga, ea) in enumerate(zip(got[coll], page[coll])):
                assert _box_close(ga, ea), f"{label} p{idx}: {coll} [{n}] {ga} vs {ea}"
        assert len(got["item_hits"]) == len(page["item_hits"]), f"{label} p{idx}: item_hits count"
        for n, (ga, ea) in enumerate(zip(got["item_hits"], page["item_hits"])):
            assert _hit_close(ga, ea), f"{label} p{idx}: item_hits [{n}] {ga} vs {ea}"


def _install_counters(calls: dict) -> dict:
    saved = {}
    for name in _BRIDGES:
        orig = getattr(psp_native, name)
        saved[name] = orig

        def counting(*args, _orig=orig, **kwargs):
            calls["n"] += 1
            return _orig(*args, **kwargs)

        setattr(psp_native, name, counting)
    return saved


def _restore_counters(saved: dict) -> None:
    for name, orig in saved.items():
        setattr(psp_native, name, orig)


def check_three_way(tmp: Path) -> None:
    corpus = json.loads(Path(CORPUS).read_text())
    assert corpus["schema"] == "retainpdf_pdf_structure_corpus_v1"
    assert psp_native.NATIVE

    calls = {"n": 0}
    saved = _install_counters(calls)
    try:
        for case in corpus["cases"]:
            raw = base64.b64decode(case["pdf_b64"])
            src = tmp / f"psp-{case['name']}.pdf"
            src.write_bytes(raw)
            items = (
                {int(k): [dict(item) for item in v] for k, v in case["items_by_page"].items()}
                if case["items_by_page"]
                else None
            )
            label = case["name"]

            ref = _build_pdf_structure_profile_python(src, items)
            native = build_pdf_structure_profile(src, items)
            _assert_pages_equal(_profile_pages(native), _profile_pages(ref), f"{label} native vs ref")
            _assert_pages_equal(_profile_pages(ref), case["pages"], f"{label} ref vs corpus")
    finally:
        _restore_counters(saved)
    assert calls["n"] > 0, "pdf structure profile production never hit a native bridge"


def check_form_xobjects_primitive() -> None:
    corpus = json.loads(Path(CORPUS).read_text())
    for case in corpus["cases"]:
        raw = base64.b64decode(case["pdf_b64"])
        for idx_str, page in case["pages"].items():
            entries = json.loads(psp_native._native_read_page_form_xobjects(raw, int(idx_str)))
            expected = page["form_xobjects_primitive"]
            assert len(entries) == len(expected), f"{case['name']} p{idx_str}: form count"
            for n, (entry, exp) in enumerate(zip(entries, expected)):
                assert entry["name"] == exp[0], f"{case['name']} p{idx_str} form[{n}] name"
                assert int(entry["xref"]) == exp[1], f"{case['name']} p{idx_str} form[{n}] xref"
                assert all(_close(entry["bbox"][i], exp[2][i], TOL_BBOX) for i in range(4)), (
                    f"{case['name']} p{idx_str} form[{n}] bbox {entry['bbox']} vs {exp[2]}"
                )


def check_boundaries(tmp: Path) -> None:
    corrupt = tmp / "corrupt.pdf"
    corrupt.write_bytes(b"\x00\x01\x02 not a pdf")
    try:
        build_pdf_structure_profile(corrupt)
        raise AssertionError("native must raise on corrupt bytes")
    except Exception:
        pass
    try:
        _build_pdf_structure_profile_python(corrupt)
        raise AssertionError("reference must raise on corrupt bytes")
    except Exception:
        pass

    vanished = tmp / "vanished.pdf"
    vanished.write_bytes(base64.b64decode(json.loads(Path(CORPUS).read_text())["cases"][0]["pdf_b64"]))
    vanished.unlink()
    try:
        build_pdf_structure_profile(vanished)
        raise AssertionError("native must raise on vanished backing file")
    except Exception:
        pass
    try:
        _build_pdf_structure_profile_python(vanished)
        raise AssertionError("reference must raise on vanished backing file")
    except Exception:
        pass

    # Out-of-range `pages` key: the native per-page read raises, the shim falls
    # back, and the reference filters the index out -> empty profile.
    src = tmp / "range.pdf"
    src.write_bytes(base64.b64decode(json.loads(Path(CORPUS).read_text())["cases"][0]["pdf_b64"]))
    profile = build_pdf_structure_profile(src, {999: [{"item_id": "i0", "bbox": [0, 0, 1, 1]}]})
    assert profile.pages == {}, "out-of-range pages key must yield an empty profile"


def main() -> None:
    assert psp_native.NATIVE, "pdf_structure_profile native module not built"
    with tempfile.TemporaryDirectory(prefix="rps-psp-") as tmp_dir:
        tmp = Path(tmp_dir)
        check_three_way(tmp)
        check_form_xobjects_primitive()
        check_boundaries(tmp)
    print("all smoke tests pass")


if __name__ == "__main__":
    main()

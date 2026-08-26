#!/usr/bin/env python3
"""Golden PDF corpus generator for backend/rendering_reader.

Reads the real golden sample PDFs (resources/samples/golden-pdfs/{1,2}.pdf) via
fitz, records per page the reader-contract PageSnapshot (achievable fields only;
text_traces / image bboxes are Phase 5) plus the Python-computed render-page
profile, and writes backend/rendering_reader/tests/golden_pdf_corpus.json.

Rust integration tests open the same PDFs with mupdf-rs and assert the reader
extracts the same achievable page facts (snapshot parity) and that those facts
flow through rendering_core to matching geometry/vector profile fields.

Deterministic: fixed PDFs, no RNG, sort_keys + indent serialization.

Run from backend/scripts:
    /tmp/rpdf-venv/bin/python ../rendering_reader/differential/gen_golden_pdf_corpus.py
"""

import json
import os
import sys

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
# rendering_reader/differential -> backend/rendering_core/differential (for gen_corpus).
_CORE_DIFF_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "rendering_core", "differential"))
# -> backend/scripts (for services.*, pulled in by gen_corpus too).
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _CORE_DIFF_DIR)
sys.path.insert(0, _SCRIPTS_DIR)

from gen_corpus import FakePage, profile_to_dict  # noqa: E402
from services.rendering.analysis.profile.builder import build_render_page_profile  # noqa: E402

REPO_ROOT = os.path.abspath(os.path.join(_HERE, "..", "..", ".."))
GOLDEN_SAMPLE_ROOT = os.path.join(REPO_ROOT, "resources", "samples", "golden-pdfs")
OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "golden_pdf_corpus.json"))

# (file, category) pairs to process; 3.pdf (18MB) is skipped for smoke-only.
GOLDEN_PDFS = [
    ("1.pdf", "editable-paper"),
    ("2.pdf", "pseudo-editable"),
]

BACKGROUND_THRESHOLD = 0.75


def _safe_len(getter, page):
    try:
        return len(getter(page))
    except Exception:
        return 0


def snapshot_for_page(page):
    """Reader-contract PageSnapshot dict (achievable fields + Phase 5 placeholders)."""
    try:
        xrefs = [int(e[0]) for e in page.get_images(full=True)]
    except Exception:
        xrefs = []
    try:
        rect = [float(v) for v in page.rect]
        cropbox = [float(v) for v in page.cropbox]
        rotation = int(page.rotation or 0)
        number = int(page.number)
    except Exception:
        rect, cropbox, rotation, number = [0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], 0, 0
    return {
        "number": number,
        "rotation": rotation,
        "rect": rect,
        "cropbox": cropbox,
        "text_traces": [],  # Phase 5: mupdf-rs has no texttrace type/opacity API.
        "word_count": _safe_len(lambda p: p.get_text("words"), page),
        "drawing_count": _safe_len(lambda p: p.get_cdrawings(), page),
        "image_infos": [{"xref": x, "bbox": [0.0, 0.0, 0.0, 0.0]} for x in xrefs if x > 0],
        "image_entries": [x for x in xrefs if x > 0],
        "image_rects": {},  # Phase 5: no get_image_rects-equivalent.
    }


def main():
    payload = {
        "schema": "retainpdf_golden_pdf_corpus_v1",
        "background_threshold": BACKGROUND_THRESHOLD,
        "pdfs": {},
    }
    for filename, category in GOLDEN_PDFS:
        path = os.path.join(GOLDEN_SAMPLE_ROOT, filename)
        if not os.path.exists(path):
            raise RuntimeError(f"golden sample not found: {path}")
        doc = fitz.open(path)
        page_count = doc.page_count
        pages = {}
        for idx in range(page_count):
            page = doc.load_page(idx)
            snapshot = snapshot_for_page(page)
            profile = build_render_page_profile(
                FakePage(snapshot),
                ocr_items=[],
                background_threshold=BACKGROUND_THRESHOLD,
            )
            pages[str(idx)] = {
                "snapshot": snapshot,
                "expected_profile": profile_to_dict(profile),
            }
        payload["pdfs"][filename] = {
            "category": category,
            "page_count": page_count,
            "pages": pages,
        }
        doc.close()
        print(f"generated {filename} {page_count} pages -> {category}")
    os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
    with open(OUT_PATH, "w", encoding="utf-8") as fh:
        json.dump(payload, fh, ensure_ascii=False, sort_keys=True, indent=1)
    total = sum(len(e["pages"]) for e in payload["pdfs"].values())
    print(f"wrote {total} pages -> {OUT_PATH}")


if __name__ == "__main__":
    main()

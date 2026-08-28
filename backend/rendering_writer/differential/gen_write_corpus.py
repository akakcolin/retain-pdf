#!/usr/bin/env python3
"""Write-corpus generator for backend/rendering_writer (Phase 5C).

Builds deterministic synthetic PDFs with fitz, runs the REAL production strip
pipeline (`strip_bbox_text_rects_from_pdf_copy`), and records the semantic
facts the Rust replay must reproduce: per-page word counts and ink ratios for
the input and cleaned PDFs, plus the strip result counts. The input PDF bytes
are embedded base64 so the Rust test replays the exact same input.

The strip rects are fitz-space word/line bboxes converted to PDF user space via
`rect * ~page.transformation_matrix`, mirroring the production planner's
`ocr_bbox_to_pdf_rect`.

Deterministic: fixed synthetic content, no RNG, stable iteration,
sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_write_corpus.py
"""

import base64
import io
import json
import os
import random
import sys
import tempfile
from pathlib import Path

import fitz
import pikepdf
from PIL import Image

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering.source.compression.image_pipeline import (  # noqa: E402
    _compress_pdf_images_only_impl_python,
)
from services.rendering.source.preparation.hidden_text_strip import (  # noqa: E402
    build_hidden_text_stripped_pdf_copy,
)
from services.rendering.source.preparation.xobject_sanitize import (  # noqa: E402
    _build_invalid_xobject_sanitized_pdf_copy_python,
)
from services.rendering.source_cleanup.pdf.document import (  # noqa: E402
    strip_bbox_text_rects_from_pdf_copy,
)

REPO_ROOT = os.path.abspath(os.path.join(_HERE, "..", "..", ".."))
OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "write_corpus.json"))

RENDER_SCALE = 2.0
INK_THRESHOLD = 250


def ink_ratio(page: fitz.Page) -> float:
    pix = page.get_pixmap(
        matrix=fitz.Matrix(RENDER_SCALE, RENDER_SCALE),
        colorspace=fitz.csGRAY,
        alpha=False,
    )
    total = pix.width * pix.height
    if total == 0:
        return 0.0
    dark = sum(1 for b in pix.samples if b < INK_THRESHOLD)
    return dark / total


def page_words(page: fitz.Page) -> int:
    return len(page.get_text("words"))


def pdf_space_rect(page: fitz.Page, rect: fitz.Rect) -> list[float]:
    converted = fitz.Rect(rect) * ~page.transformation_matrix
    return [float(converted.x0), float(converted.y0), float(converted.x1), float(converted.y1)]


def page_facts(doc: fitz.Document) -> list[dict]:
    facts = []
    for idx in range(doc.page_count):
        page = doc[idx]
        facts.append({"words": page_words(page), "ink_ratio": ink_ratio(page)})
    return facts


FIXED_ID = (
    b"/ID[<30303030303030303030303030303030>"
    b"<30303030303030303030303030303030>]"
)


def _array_end(raw: bytes, open_bracket: int) -> int:
    """Index of the `]` closing the array at `open_bracket`, skipping literal
    strings `(...)` (with backslash escapes) and hex strings `<...>`."""
    i = open_bracket + 1
    n = len(raw)
    while i < n:
        c = raw[i]
        if c == 0x28:  # '(' literal string
            i += 1
            while i < n:
                c2 = raw[i]
                if c2 == 0x5C:  # backslash escapes next byte
                    i += 2
                    continue
                if c2 == 0x29:  # ')'
                    i += 1
                    break
                i += 1
        elif c == 0x3C:  # '<' hex string
            j = raw.find(b">", i)
            if j < 0:
                return -1
            i = j + 1
        elif c == 0x5D:  # ']'
            return i
        else:
            i += 1
    return -1


def _pin_pdf_id(raw: bytes) -> bytes:
    """Replace the trailer /ID array with a fixed value (determinism).

    fitz and pikepdf both seed /ID from wall-clock time, so the embedded input
    PDF bytes differ across runs. The /ID lives in the trailer (after the xref
    table), so replacing it at any length keeps `startxref` valid."""
    start = 0
    while True:
        idx = raw.find(b"/ID", start)
        if idx < 0:
            return raw
        j = idx + 3
        while j < len(raw) and raw[j] in b" \t\r\n":
            j += 1
        if j < len(raw) and raw[j] == 0x5B:  # '['
            end = _array_end(raw, j)
            if end > 0:
                return raw[:idx] + FIXED_ID + raw[end + 1 :]
        start = idx + 3


def deterministic_tobytes(doc: fitz.Document) -> bytes:
    """fitz tobytes with the trailer /ID pinned to a fixed value."""
    return _pin_pdf_id(doc.tobytes())


def text_page(texts: list[str], *, width: float = 300.0, height: float = 200.0):
    doc = fitz.open()
    page = doc.new_page(width=width, height=height)
    for i, text in enumerate(texts):
        page.insert_text((20.0, 40.0 + i * 30.0), text, fontsize=12)
    return doc


def case_strip_lines(name: str, texts: list[str], strip_line_indices: list[int]) -> dict:
    doc = text_page(texts)
    page = doc[0]
    words = page.get_text("words")
    # Group words into lines by y0 band, pick strip rects per line index.
    lines: dict[tuple, list] = {}
    for w in words:
        key = (round(w[1], 1), round(w[3], 1))
        lines.setdefault(key, []).append(w)
    ordered_lines = sorted(lines.values(), key=lambda line: line[0][1])
    strip_pdf_rects = []
    for line_index in strip_line_indices:
        line = ordered_lines[line_index]
        line_rect = fitz.Rect()
        for w in line:
            if line_rect.is_empty:
                line_rect = fitz.Rect(w[:4])
            else:
                line_rect.include_rect(fitz.Rect(w[:4]))
        strip_pdf_rects.append(pdf_space_rect(page, line_rect))

    input_bytes = deterministic_tobytes(doc)
    input_facts = page_facts(doc)

    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "in.pdf")
        out = os.path.join(tmp, "out.pdf")
        with open(src, "wb") as fh:
            fh.write(input_bytes)
        result = strip_bbox_text_rects_from_pdf_copy(
            source_pdf_path=src,
            output_pdf_path=out,
            page_rects={0: [fitz.Rect(r) for r in strip_pdf_rects]},
            recurse_forms=True,
        )
        if not result.changed:
            raise RuntimeError(f"case {name}: expected changed=True, got unchanged")
        with fitz.open(out) as out_doc:
            output_facts = page_facts(out_doc)

    return {
        "name": name,
        "input_pdf_b64": base64.b64encode(input_bytes).decode("ascii"),
        "page_count": 1,
        "page_rects": {"0": [tuple(r) for r in strip_pdf_rects]},
        "page_protected_rects": {},
        "recurse_forms": True,
        "expected": {
            "pages_changed": result.pages_changed,
            "text_show_ops_removed": result.text_show_ops_removed,
            "forms_changed": result.forms_changed,
            "changed_page_indices": sorted(int(i) for i in result.changed_page_indices),
            "input": {"pages": input_facts},
            "output": {"pages": output_facts},
        },
    }


def case_strip_form() -> dict:
    """A Form XObject (page shown via show_pdf_page) contains strippable text."""
    source_doc = text_page(["FORM TEXT TO STRIP"], width=120.0, height=40.0)
    doc = fitz.open()
    page = doc.new_page(width=300.0, height=200.0)
    page.insert_text((20.0, 180.0), "KEEP THIS", fontsize=12)
    page.show_pdf_page(fitz.Rect(20.0, 90.0, 200.0, 130.0), source_doc, 0)
    source_doc.close()
    page = doc[0]
    words = page.get_text("words")
    strip_words = [w for w in words if w[4] == "FORM" or w[4] == "TO" or w[4] == "STRIP"]
    union = fitz.Rect()
    for w in strip_words:
        if union.is_empty:
            union = fitz.Rect(w[:4])
        else:
            union.include_rect(fitz.Rect(w[:4]))
    strip_pdf_rect = pdf_space_rect(page, union)

    input_bytes = deterministic_tobytes(doc)
    input_facts = page_facts(doc)
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "in.pdf")
        out = os.path.join(tmp, "out.pdf")
        with open(src, "wb") as fh:
            fh.write(input_bytes)
        result = strip_bbox_text_rects_from_pdf_copy(
            source_pdf_path=src,
            output_pdf_path=out,
            page_rects={0: [fitz.Rect(strip_pdf_rect)]},
            recurse_forms=True,
        )
        if not result.changed:
            raise RuntimeError("case strip_form: expected changed=True")
        with fitz.open(out) as out_doc:
            output_facts = page_facts(out_doc)

    return {
        "name": "strip_form",
        "input_pdf_b64": base64.b64encode(input_bytes).decode("ascii"),
        "page_count": 1,
        "page_rects": {"0": [tuple(strip_pdf_rect)]},
        "page_protected_rects": {},
        "recurse_forms": True,
        "expected": {
            "pages_changed": result.pages_changed,
            "text_show_ops_removed": result.text_show_ops_removed,
            "forms_changed": result.forms_changed,
            "changed_page_indices": sorted(int(i) for i in result.changed_page_indices),
            "input": {"pages": input_facts},
            "output": {"pages": output_facts},
        },
    }


def case_shared_form(name: str, strip_site_index: int) -> dict:
    """A source content form is shown at two sites (two parent Form XObjects,
    both sharing the same source content). The strip rect covers only the
    words of `strip_site_index`, so only that site's parent form is cloned and
    rewritten; the other site and the shared original stay untouched."""
    source_doc = text_page(["FORM TEXT TO STRIP"], width=120.0, height=40.0)
    doc = fitz.open()
    page = doc.new_page(width=300.0, height=200.0)
    sites = [fitz.Rect(20.0, 100.0, 200.0, 140.0), fitz.Rect(20.0, 20.0, 200.0, 60.0)]
    for rect in sites:
        page.show_pdf_page(rect, source_doc, 0)
    source_doc.close()
    page = doc[0]
    words = page.get_text("words")
    strip_words = []
    for w in words:
        x0, y0, x1, y1, _text, *_rest = w
        rect = sites[strip_site_index]
        if rect.x0 <= x0 <= rect.x1 and rect.y0 <= y0 <= rect.y1 and w[4] in ("FORM", "TO", "STRIP"):
            strip_words.append(w)
    if not strip_words:
        raise RuntimeError(f"case {name}: no strip words found at site {strip_site_index}")
    union = fitz.Rect()
    for w in strip_words:
        if union.is_empty:
            union = fitz.Rect(w[:4])
        else:
            union.include_rect(fitz.Rect(w[:4]))
    strip_pdf_rect = pdf_space_rect(page, union)

    input_bytes = deterministic_tobytes(doc)
    input_facts = page_facts(doc)
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "in.pdf")
        out = os.path.join(tmp, "out.pdf")
        with open(src, "wb") as fh:
            fh.write(input_bytes)
        result = strip_bbox_text_rects_from_pdf_copy(
            source_pdf_path=src,
            output_pdf_path=out,
            page_rects={0: [fitz.Rect(strip_pdf_rect)]},
            recurse_forms=True,
        )
        if not result.changed:
            raise RuntimeError(f"case {name}: expected changed=True")
        with fitz.open(out) as out_doc:
            output_facts = page_facts(out_doc)

    return {
        "name": name,
        "input_pdf_b64": base64.b64encode(input_bytes).decode("ascii"),
        "page_count": 1,
        "page_rects": {"0": [tuple(strip_pdf_rect)]},
        "page_protected_rects": {},
        "recurse_forms": True,
        "expected": {
            "pages_changed": result.pages_changed,
            "text_show_ops_removed": result.text_show_ops_removed,
            "forms_changed": result.forms_changed,
            "changed_page_indices": sorted(int(i) for i in result.changed_page_indices),
            "input": {"pages": input_facts},
            "output": {"pages": output_facts},
        },
    }


def multi_page_doc(texts_per_page: list[list[str]]) -> fitz.Document:
    doc = fitz.open()
    for texts in texts_per_page:
        page = doc.new_page(width=300.0, height=200.0)
        for i, text in enumerate(texts):
            page.insert_text((20.0, 40.0 + i * 30.0), text, fontsize=12)
    return doc


def case_subset(name: str, texts_per_page: list[list[str]], start_page: int, end_page: int) -> dict:
    """Page extraction via the pure-Python reference
    `_extract_pages_with_pikepdf_python`; expected facts are per-page
    words/ink_ratio of input and selected output."""
    from services.rendering.document.pikepdf_pages import _extract_pages_with_pikepdf_python

    doc = multi_page_doc(texts_per_page)
    input_bytes = deterministic_tobytes(doc)
    input_facts = page_facts(doc)
    doc.close()

    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "in.pdf")
        out = os.path.join(tmp, "out.pdf")
        with open(src, "wb") as fh:
            fh.write(input_bytes)
        _extract_pages_with_pikepdf_python(
            source_pdf_path=Path(src),
            output_pdf_path=Path(out),
            start_page=start_page,
            end_page=end_page,
        )
        with fitz.open(out) as out_doc:
            output_facts = page_facts(out_doc)

    return {
        "name": name,
        "input_pdf_b64": base64.b64encode(input_bytes).decode("ascii"),
        "start_page": start_page,
        "end_page": end_page,
        "expected": {
            "input": {"pages": input_facts},
            "output": {"pages": output_facts},
            "output_page_count": len(output_facts),
        },
    }


def case_overlay(
    name: str,
    source_texts: list[str],
    overlay_texts: list[str],
    *,
    source_rotate: int = 0,
    overlay_rotate: int = 0,
) -> dict:
    """Overlay page 0 of an overlay PDF onto page 0 of the source via the
    production `overlay_pdf_pages_with_pikepdf` (`source_page.add_overlay(
    rect=cropbox, push_stack=True, shrink=False, expand=False)`)."""
    from services.rendering.document.pikepdf_overlay import overlay_pdf_pages_with_pikepdf

    source_doc = text_page(source_texts)
    if source_rotate:
        source_doc[0].set_rotation(source_rotate)
    overlay_doc = text_page(overlay_texts)
    if overlay_rotate:
        overlay_doc[0].set_rotation(overlay_rotate)

    input_bytes = deterministic_tobytes(source_doc)
    input_facts = page_facts(source_doc)
    source_doc.close()
    overlay_bytes = deterministic_tobytes(overlay_doc)
    overlay_doc.close()

    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "in.pdf")
        ovl = os.path.join(tmp, "ovl.pdf")
        out = os.path.join(tmp, "out.pdf")
        with open(src, "wb") as fh:
            fh.write(input_bytes)
        with open(ovl, "wb") as fh:
            fh.write(overlay_bytes)
        result = overlay_pdf_pages_with_pikepdf(
            source_pdf_path=Path(src),
            overlay_pdf_path=Path(ovl),
            output_pdf_path=Path(out),
            source_page_indices=[0],
        )
        with fitz.open(out) as out_doc:
            output_facts = page_facts(out_doc)

    return {
        "name": name,
        "input_pdf_b64": base64.b64encode(input_bytes).decode("ascii"),
        "overlay_pdf_b64": base64.b64encode(overlay_bytes).decode("ascii"),
        "expected": {
            "pages_merged": result.pages_merged,
            "input": {"pages": input_facts},
            "output": {"pages": output_facts},
        },
    }


def case_image_compress(name: str, dpi: int = 200) -> dict:
    """A pikepdf-built page showing one noisy DeviceRGB DCTDecode image (800x600,
    encoded well over the 20K small-check threshold) at 72x54 pt. Production
    recompresses it to the display target (200x150 at dpi=200), committing only
    because the re-encoded output is strictly smaller."""
    random.seed(0)
    raw = bytes(random.getrandbits(8) for _ in range(600 * 800 * 3))
    img = Image.frombytes("RGB", (800, 600), raw)
    buf = io.BytesIO()
    img.save(buf, format="JPEG", quality=85)
    jpeg_bytes = buf.getvalue()

    pdf = pikepdf.Pdf.new()
    page = pdf.add_blank_page(page_size=(300, 200))
    img_obj = pdf.make_stream(jpeg_bytes)
    img_obj["/Type"] = pikepdf.Name("/XObject")
    img_obj["/Subtype"] = pikepdf.Name("/Image")
    img_obj["/Width"] = 800
    img_obj["/Height"] = 600
    img_obj["/ColorSpace"] = pikepdf.Name("/DeviceRGB")
    img_obj["/BitsPerComponent"] = 8
    img_obj["/Filter"] = pikepdf.Name("/DCTDecode")
    page.Resources = pikepdf.Dictionary({"/XObject": pikepdf.Dictionary({"/Im0": img_obj})})
    page.Contents = pdf.make_stream(b"q\n72 0 0 54 20 46 cm\n/Im0 Do\nQ\n")
    # pikepdf always regenerates the trailer /ID's second half on save; pin
    # both halves post-save (equal-length replacement, so xref offsets hold).
    pdf.trailer["/ID"] = pikepdf.Array(
        [
            pikepdf.String("00000000000000000000000000000000"),
            pikepdf.String("00000000000000000000000000000000"),
        ]
    )
    out_buf = io.BytesIO()
    pdf.save(out_buf)
    input_bytes = _pin_pdf_id(out_buf.getvalue())
    pdf.close()

    input_doc = fitz.open(stream=input_bytes, filetype="pdf")
    input_facts = page_facts(input_doc)
    input_doc.close()

    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "in.pdf")
        with open(src, "wb") as fh:
            fh.write(input_bytes)
        changed = _compress_pdf_images_only_impl_python(Path(src), dpi=dpi)
        if not changed:
            raise RuntimeError(f"case {name}: expected changed=True, got unchanged")
        with fitz.open(src) as out_doc:
            output_facts = page_facts(out_doc)
        with pikepdf.open(src) as out_pdf:
            out_img = out_pdf.pages[0].Resources.XObject["/Im0"]
            out_width = int(out_img["/Width"])
            out_height = int(out_img["/Height"])
            out_filter = str(out_img.get("/Filter"))
            out_encoded_len = len(bytes(out_img.read_raw_bytes()))

    return {
        "name": name,
        "input_pdf_b64": base64.b64encode(input_bytes).decode("ascii"),
        "dpi": dpi,
        "expected": {
            "changed": True,
            "input": {"pages": input_facts},
            "output": {"pages": output_facts},
            "input_image": {"width": 800, "height": 600, "encoded_len": len(jpeg_bytes)},
            "output_image": {
                "width": out_width,
                "height": out_height,
                "encoded_len": out_encoded_len,
                "filter": out_filter,
            },
        },
    }


def _int_or_none(value: object) -> int | None:
    try:
        return int(value)  # type: ignore[arg-type]
    except Exception:
        return None


def _scan_xobject_subtypes(container: pikepdf.Object) -> list[list[str]]:
    """Sorted [(resource name, subtype)] of every XObject in a page/Form tree
    (objgen cycle protection, mirroring `_sanitize_container_xobjects`)."""
    out: list[list[str]] = []
    seen: set[tuple[int, int]] = set()
    stack = [container]
    while stack:
        obj = stack.pop()
        resources = obj.get("/Resources")
        if resources is None:
            continue
        xobjects = resources.get("/XObject")
        if xobjects is None:
            continue
        for key, xo in list(xobjects.items()):
            identity = (int(xo.objgen[0]), int(xo.objgen[1]))
            if identity in seen:
                continue
            seen.add(identity)
            subtype = xo.get("/Subtype")
            # mupdf-rs `as_name` yields names without the leading slash; strip
            # pikepdf's `/Name` rendering to match.
            out.append([str(key).lstrip("/"), str(subtype).lstrip("/")])
            if subtype == pikepdf.Name("/Form"):
                stack.append(xo)
    return sorted(out)


def _scan_image_counts(container: pikepdf.Object) -> tuple[int, int]:
    """(total_images, invalid_images) across a page/Form resource tree.
    An image is invalid when /Width or /Height is missing or <= 0 (mirrors the
    production `_invalid_image_xobject`); objgen cycle protection mirrors
    `_sanitize_container_xobjects`."""
    total = 0
    invalid = 0
    seen: set[tuple[int, int]] = set()
    stack = [container]
    while stack:
        obj = stack.pop()
        resources = obj.get("/Resources")
        if resources is None:
            continue
        xobjects = resources.get("/XObject")
        if xobjects is None:
            continue
        for _, xo in list(xobjects.items()):
            identity = (int(xo.objgen[0]), int(xo.objgen[1]))
            if identity in seen:
                continue
            seen.add(identity)
            subtype = xo.get("/Subtype")
            if subtype == pikepdf.Name("/Image"):
                total += 1
                width = _int_or_none(xo.get("/Width"))
                height = _int_or_none(xo.get("/Height"))
                if width is None or height is None or width <= 0 or height <= 0:
                    invalid += 1
            elif subtype == pikepdf.Name("/Form"):
                stack.append(xo)
    return total, invalid


def case_hidden_text() -> dict:
    """A page with a full-page gray background image (a `page_is_pseudo_editable_scan`
    candidate), visible text, and render-mode-3 (invisible) text. Production
    drops the all-hidden BT..ET group, leaving the visible text and the image."""
    doc = fitz.open()
    page = doc.new_page(width=300.0, height=200.0)
    pix = fitz.Pixmap(fitz.csGRAY, fitz.IRect(0, 0, 300, 200))
    pix.clear_with(200)
    page.insert_image(fitz.Rect(0, 0, 300, 200), pixmap=pix)
    page.insert_text((20.0, 40.0), "VISIBLE TEXT HERE", fontsize=12)
    page.insert_text((20.0, 70.0), "HIDDEN TEXT STRIP ME", fontsize=12, render_mode=3)

    input_bytes = deterministic_tobytes(doc)
    input_facts = page_facts(doc)
    doc.close()

    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "in.pdf")
        out = os.path.join(tmp, "out.pdf")
        with open(src, "wb") as fh:
            fh.write(input_bytes)
        result = build_hidden_text_stripped_pdf_copy(Path(src), Path(out))
        if not result.changed:
            raise RuntimeError("case hidden_text: expected changed=True, got unchanged")
        with fitz.open(out) as out_doc:
            output_facts = page_facts(out_doc)

    return {
        "name": "hidden_text",
        "input_pdf_b64": base64.b64encode(input_bytes).decode("ascii"),
        "expected": {
            "pages_changed": result.pages_changed,
            "text_objects_removed": result.text_objects_removed,
            "input": {"pages": input_facts},
            "output": {"pages": output_facts},
        },
    }


def case_sanitize() -> dict:
    """A page whose /Resources/XObject holds a valid 1x1 DeviceGray image
    (drawn over the page) and an invalid image missing /Width//Height (never
    drawn). Production replaces the invalid image with an empty Form XObject,
    leaving the valid image intact."""
    pdf = pikepdf.Pdf.new()
    page = pdf.add_blank_page(page_size=(300, 200))
    valid = pdf.make_stream(b"\xff" * 100)
    valid["/Type"] = pikepdf.Name("/XObject")
    valid["/Subtype"] = pikepdf.Name("/Image")
    valid["/Width"] = 1
    valid["/Height"] = 1
    valid["/ColorSpace"] = pikepdf.Name("/DeviceGray")
    valid["/BitsPerComponent"] = 8
    invalid = pdf.make_stream(b"\x00" * 10)
    invalid["/Type"] = pikepdf.Name("/XObject")
    invalid["/Subtype"] = pikepdf.Name("/Image")
    invalid["/ColorSpace"] = pikepdf.Name("/DeviceGray")
    invalid["/BitsPerComponent"] = 8
    page.Resources = pikepdf.Dictionary(
        {
            "/XObject": pikepdf.Dictionary({"/Valid": valid, "/Invalid": invalid}),
        }
    )
    page.Contents = pdf.make_stream(b"q\n300 0 0 200 0 0 cm\n/Valid Do\nQ\n")
    pdf.trailer["/ID"] = pikepdf.Array(
        [
            pikepdf.String("00000000000000000000000000000000"),
            pikepdf.String("00000000000000000000000000000000"),
        ]
    )
    out_buf = io.BytesIO()
    pdf.save(out_buf)
    input_bytes = _pin_pdf_id(out_buf.getvalue())
    pdf.close()

    input_doc = fitz.open(stream=input_bytes, filetype="pdf")
    input_facts = page_facts(input_doc)
    input_doc.close()

    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "in.pdf")
        out = os.path.join(tmp, "out.pdf")
        with open(src, "wb") as fh:
            fh.write(input_bytes)
        result = _build_invalid_xobject_sanitized_pdf_copy_python(
            source_pdf_path=Path(src),
            output_pdf_path=Path(out),
        )
        if not result.changed:
            raise RuntimeError("case sanitize: expected changed=True, got unchanged")
        with fitz.open(out) as out_doc:
            output_facts = page_facts(out_doc)
        with pikepdf.open(out) as out_pdf:
            out_total, out_invalid = _scan_image_counts(out_pdf.pages[0].obj)
            out_subtypes = _scan_xobject_subtypes(out_pdf.pages[0].obj)

    return {
        "name": "sanitize",
        "input_pdf_b64": base64.b64encode(input_bytes).decode("ascii"),
        "expected": {
            "invalid_image_xobjects": result.invalid_image_xobjects,
            "pages_changed": result.pages_changed,
            "input": {"pages": input_facts},
            "output": {"pages": output_facts},
            "output_image_count": out_total,
            "output_invalid_image_count": out_invalid,
            "output_xobject_subtypes": out_subtypes,
        },
    }


def main():
    cases = [
        case_strip_lines("strip_one_line", ["FIRST LINE KEEP", "SECOND LINE STRIP"], [1]),
        case_strip_lines(
            "strip_middle_of_three",
            ["ALPHA LINE", "BETA LINE STRIP", "GAMMA LINE"],
            [1],
        ),
        case_strip_form(),
        case_shared_form("shared_form_strip_site_a", 0),
        case_shared_form("shared_form_strip_site_b", 1),
    ]
    image_cases = [
        case_image_compress("image_compress_rgb_noise"),
    ]
    prep_cases = [
        case_hidden_text(),
        case_sanitize(),
    ]
    overlay_cases = [
        case_overlay(
            "overlay_simple",
            ["SOURCE LINE ONE", "SOURCE LINE TWO"],
            ["OVERLAY TEXT"],
        ),
        case_overlay(
            "overlay_source_rot90",
            ["SOURCE LINE ONE"],
            ["OVERLAY TEXT"],
            source_rotate=90,
        ),
        case_overlay(
            "overlay_source_rot180",
            ["SOURCE LINE ONE"],
            ["OVERLAY TEXT"],
            source_rotate=180,
        ),
        case_overlay(
            "overlay_source_rot270",
            ["SOURCE LINE ONE"],
            ["OVERLAY TEXT"],
            source_rotate=270,
        ),
        case_overlay(
            "overlay_overlay_rot90",
            ["SOURCE LINE ONE"],
            ["OVERLAY TEXT"],
            overlay_rotate=90,
        ),
        case_overlay(
            "overlay_overlay_rot180",
            ["SOURCE LINE ONE"],
            ["OVERLAY TEXT"],
            overlay_rotate=180,
        ),
        case_overlay(
            "overlay_both_rot90",
            ["SOURCE LINE ONE"],
            ["OVERLAY TEXT"],
            source_rotate=90,
            overlay_rotate=90,
        ),
    ]
    subset_cases = [
        case_subset(
            "subset_first_two",
            [
                ["PAGE ONE ALPHA", "PAGE ONE BETA"],
                ["PAGE TWO GAMMA", "PAGE TWO DELTA"],
                ["PAGE THREE EPSILON"],
            ],
            0,
            1,
        ),
        case_subset(
            "subset_last_page",
            [
                ["PAGE ONE ALPHA", "PAGE ONE BETA"],
                ["PAGE TWO GAMMA", "PAGE TWO DELTA"],
                ["PAGE THREE EPSILON"],
            ],
            2,
            2,
        ),
    ]
    payload = {
        "schema": "retainpdf_write_corpus_v1",
        "render_scale": RENDER_SCALE,
        "ink_threshold": INK_THRESHOLD,
        "cases": cases,
        "subset_cases": subset_cases,
        "overlay_cases": overlay_cases,
        "image_cases": image_cases,
        "prep_cases": prep_cases,
    }
    os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
    with open(OUT_PATH, "w", encoding="utf-8") as fh:
        json.dump(payload, fh, ensure_ascii=False, sort_keys=True, indent=1)
    size = os.path.getsize(OUT_PATH)
    print(
        f"wrote {len(cases)} strip + {len(subset_cases)} subset "
        f"+ {len(overlay_cases)} overlay + {len(image_cases)} image "
        f"+ {len(prep_cases)} prep cases ({size / 1024:.0f} KiB) -> {OUT_PATH}"
    )


if __name__ == "__main__":
    main()

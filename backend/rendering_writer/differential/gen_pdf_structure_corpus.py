#!/usr/bin/env python3
"""pdf_structure_profile corpus generator (Phase B2-Inc2).

Records the pure-Python reference outputs of the pdf_structure_profile sampler
(`sampler._build_pdf_structure_profile_python`) for deterministic synthetic
PDFs. Each case pins:

  * the source PDF bytes (base64, trailer /ID pinned),
  * the `items_by_page` input (the OCR item bboxes) used to compute `item_hits`,
  * per-page `page_width_pt` / `page_height_pt` and the five object collections
    (`text_objects` / `text_spans` / `path_objects` / `image_objects` /
    `form_xobjects`) plus `item_hits`, all from the `_python` reference oracle,
  * per-page `form_xobjects_primitive` — `get_xobjects()` mapped to
    `[name, xref, bbox]` — so the Rust replay (`tests/form_xobjects_diff.rs`)
    can pin the native `page_form_xobjects` primitive.

Synthetic coverage pins the parity surface: plain multi-line text, a filled
rect (fill-path + stroke-path bboxlog), an image (fill-image), a mixed page
(text + path + image), a single-level Form XObject (built with pikepdf and
placed with an identity matrix — the only shape where fitz `get_xobjects()`
and the native `/Resources/XObject` resource-dict scan agree; the nested
form-invokes-form divergence is documented in `pdf_structure_profile/_native.py`),
90/180-degree-rotated pages (fitz clears /rotate, so the native collectors must
too), a cropbox offset (page width/height follow the cropbox; bboxlog stays in
the content space), and an item-hit page (an OCR item bbox that overlaps a text
object, exercising the `item_hits` overlap path).

Deterministic: fixed synthetic content, no RNG, stable iteration,
sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_pdf_structure_corpus.py
"""

import base64
import json
import os
import sys

import fitz
import pikepdf

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

# Corpus lives next to its replay (`rendering_reader/tests/form_xobjects_diff.rs`).
OUT_PATH = os.path.abspath(
    os.path.join(_HERE, "..", "..", "rendering_reader", "tests", "pdf_structure_corpus.json")
)

FIXED_ID = (
    b"/ID[<30303030303030303030303030303030>"
    b"<30303030303030303030303030303030>]"
)


def _array_end(raw: bytes, open_bracket: int) -> int:
    i = open_bracket + 1
    n = len(raw)
    while i < n:
        c = raw[i]
        if c == 0x28:
            i += 1
            while i < n:
                c2 = raw[i]
                if c2 == 0x5C:
                    i += 2
                    continue
                if c2 == 0x29:
                    i += 1
                    break
                i += 1
        elif c == 0x3C:
            j = raw.find(b">", i)
            if j < 0:
                return -1
            i = j + 1
        elif c == 0x5D:
            return i
        else:
            i += 1
    return -1


def _pin_pdf_id(raw: bytes) -> bytes:
    start = 0
    while True:
        idx = raw.find(b"/ID", start)
        if idx < 0:
            return raw
        j = idx + 3
        while j < len(raw) and raw[j] in b" \t\r\n":
            j += 1
        if j < len(raw) and raw[j] == 0x5B:
            end = _array_end(raw, j)
            if end > 0:
                raw = raw[:j] + FIXED_ID + raw[end + 1 :]
                start = j + len(FIXED_ID)
                continue
        start = idx + 3


def _build_page(draw: callable) -> bytes:
    """Open a blank 300x400 page, run `draw(page)`, pin /ID, return bytes."""
    doc = fitz.open()
    page = doc.new_page(width=300.0, height=400.0)
    draw(page)
    raw = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()
    return raw


def _build_form_pdf() -> bytes:
    """A 200x200 page whose single XObject is a Form placed with an identity
    matrix. `get_xobjects()` reports `(xref, 'Fm1', 0, (0,0,100,100))` and the
    native `/Resources/XObject` scan reports `{name:'Fm1', xref, bbox:[0,0,100,100]}`
    — one entry each, agreeing exactly."""
    import tempfile

    pdf = pikepdf.new()
    page = pdf.add_blank_page(page_size=(200, 200))
    form = pdf.make_stream(b"0.5 0.5 0.9 rg\n0 0 100 100 re\nf\n")
    form.Type = pikepdf.Name("/XObject")
    form.Subtype = pikepdf.Name("/Form")
    form.FormType = 1
    form.BBox = pikepdf.Array([0, 0, 100, 100])
    pdf.make_indirect(form)
    page.Resources = pikepdf.Dictionary(XObject=pikepdf.Dictionary(Fm1=form))
    contents = pdf.make_stream(b"1 0 0 1 0 0 cm\n/Fm1 Do\n")
    page.Contents = contents
    pdf.make_indirect(contents)
    tmp = tempfile.NamedTemporaryFile(suffix=".pdf", delete=False)
    pdf.save(tmp.name)
    tmp.close()
    with open(tmp.name, "rb") as f:
        raw = f.read()
    os.unlink(tmp.name)
    return _pin_pdf_id(raw)


def _box(entry) -> list:
    return [
        entry.object_id,
        entry.object_type,
        [float(v) for v in entry.bbox],
        entry.source,
        entry.text,
        [str(flag) for flag in entry.flags],
    ]


def _record_pages(raw: bytes, items_by_page: dict | None) -> dict:
    """Record the reference pdf_structure_profile outputs per page."""
    from services.rendering.pdf_structure_profile.sampler import (
        _build_pdf_structure_profile_python,
    )

    items = items_by_page or None
    profile = _build_pdf_structure_profile_python(_pdf_path(raw), items)
    primitives = _record_form_primitives(raw)
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
            "form_xobjects_primitive": primitives.get(str(idx), []),
        }
    return pages


def _pdf_path(raw: bytes) -> "os.PathLike[str]":
    import tempfile

    tmp = tempfile.NamedTemporaryFile(suffix=".pdf", delete=False)
    tmp.write(raw)
    tmp.close()
    return tmp.name


def _record_form_primitives(raw: bytes) -> list:
    """The reference `get_xobjects()` facts `[name, xref, [bbox]]` per page."""
    doc = fitz.open(stream=raw, filetype="pdf")
    out = {}
    for idx in range(doc.page_count):
        page = doc.load_page(idx)
        out[str(idx)] = [
            [str(name), int(xref), [float(v) for v in bbox]]
            for xref, name, _kind, bbox in page.get_xobjects()
        ]
    doc.close()
    return out


def _run_case(name: str, raw: bytes, items_by_page: dict | None = None) -> dict:
    return {
        "name": name,
        "pdf_b64": base64.b64encode(raw).decode("ascii"),
        "items_by_page": {str(k): [dict(item) for item in v] for k, v in (items_by_page or {}).items()},
        "pages": _record_pages(raw, items_by_page),
    }


def main() -> None:
    cases = []

    # 1. Plain multi-line text: two spans (one per line), both text_objects.
    def _simple(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "Alpha line one", fontsize=14)
        page.insert_text((20.0, 82.0), "Beta line two", fontsize=14)

    cases.append(_run_case("simple_text", _build_page(_simple)))

    # 2. Filled rect: fill-path + stroke-path bboxlog entries.
    def _rect(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "Rect label", fontsize=14)
        shape = page.new_shape()
        shape.draw_rect(fitz.Rect(50.0, 200.0, 150.0, 260.0))
        shape.finish(color=(1.0, 0.0, 0.0), fill=(1.0, 0.9, 0.9))
        shape.commit()

    cases.append(_run_case("path_rect", _build_page(_rect)))

    # 3. Image: fill-image bboxlog entry.
    def _image(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "Image label", fontsize=14)
        pix = fitz.Pixmap(fitz.csRGB, fitz.IRect(0, 0, 40, 30))
        pix.clear_with(0x808080)
        page.insert_image(fitz.Rect(180.0, 120.0, 260.0, 180.0), pixmap=pix)

    cases.append(_run_case("image_page", _build_page(_image)))

    # 4. Mixed page: text + path + image on one page.
    def _mixed(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "Mixed page", fontsize=14)
        shape = page.new_shape()
        shape.draw_rect(fitz.Rect(50.0, 200.0, 150.0, 260.0))
        shape.finish(color=(1.0, 0.0, 0.0), fill=(1.0, 0.9, 0.9))
        shape.commit()
        pix = fitz.Pixmap(fitz.csRGB, fitz.IRect(0, 0, 40, 30))
        pix.clear_with(0x808080)
        page.insert_image(fitz.Rect(180.0, 120.0, 260.0, 180.0), pixmap=pix)

    cases.append(_run_case("mixed_page", _build_page(_mixed)))

    # 5. Single-level Form XObject (pikepdf, identity placement).
    cases.append(_run_case("form_xobject", _build_form_pdf()))

    # 6. Rotated pages: fitz get_bboxlog/get_text clear /rotate; page width/height
    #    swap for 90/270.
    def _rot_draw(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "Rotated text", fontsize=14)
        shape = page.new_shape()
        shape.draw_rect(fitz.Rect(50.0, 200.0, 150.0, 260.0))
        shape.finish(color=(1.0, 0.0, 0.0), fill=(1.0, 0.9, 0.9))
        shape.commit()

    def _rot90(page: fitz.Page) -> None:
        _rot_draw(page)
        page.set_rotation(90)

    def _rot180(page: fitz.Page) -> None:
        _rot_draw(page)
        page.set_rotation(180)

    cases.append(_run_case("rotated_90", _build_page(_rot90)))
    cases.append(_run_case("rotated_180", _build_page(_rot180)))

    # 7. Cropbox offset: page width/height follow the cropbox; bboxlog rects stay
    #    in the content space (both fitz and mupdf report the same).
    def _crop(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "Cropped text", fontsize=14)
        page.set_cropbox(fitz.Rect(10.0, 10.0, 210.0, 310.0))

    cases.append(_run_case("cropbox_offset", _build_page(_crop)))

    # 8. Item hit: an OCR item bbox (PDF coords) that overlaps the text object.
    #    The bbox is captured from the live page (the PDF-space rect of the
    #    first span) so the item is guaranteed to overlap its text object.
    holder: list[list[float]] = []

    def _hit(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "Alpha hit line", fontsize=14)
        span = page.get_text("dict")["blocks"][0]["lines"][0]["spans"][0]
        span_rect = fitz.Rect(span["bbox"])
        pdf_rect = span_rect * ~page.transformation_matrix
        holder.append([round(float(v), 3) for v in pdf_rect])

    raw_hit = _build_page(_hit)
    assert holder, "item-hit span bbox missing"
    cases.append(
        _run_case(
            "item_hit",
            raw_hit,
            items_by_page={0: [{"item_id": "i0", "bbox": holder[0]}]},
        )
    )

    corpus = {"schema": "retainpdf_pdf_structure_corpus_v1", "cases": cases}
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()

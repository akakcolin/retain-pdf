#!/usr/bin/env python3
"""Deterministic differential corpus generator for backend/rendering_core.

Generates random inputs for the Python reference implementation, records the
expected outputs, and writes them to backend/rendering_core/tests/corpus.json.
Rust integration tests replay this corpus and assert parity (golden replay).

Run from backend/scripts:
    /tmp/rpdf-venv/bin/python ../rendering_core/differential/gen_corpus.py [--cases N] [--item-cases N] [--only fn_id]
"""

import argparse
import json
import os
import random
import sys

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
# backend/rendering_core/differential -> backend/scripts (for `services.*`).
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)
sys.path.insert(0, os.getcwd())

# --- services imports -------------------------------------------------------
from services.rendering.layout.chinese_body_fit import (  # noqa: E402
    estimate_chinese_body_height_pt,
    estimate_chinese_body_lines,
    formula_unit_ratio,
    solve_chinese_body_font_size_pt,
    tokenize_chinese_body_text,
)
from services.rendering.layout.fit_decision.planner import plan_chinese_body_fit  # noqa: E402
from services.rendering.layout.payload.capacity import (  # noqa: E402
    box_capacity_units,
    estimated_render_height_pt,
    estimated_required_lines,
    formula_estimate_discount,
    text_demand_units,
)
from services.rendering.layout.payload.continuation_split import split_protected_text_for_boxes  # noqa: E402
from services.rendering.layout.payload.formula_cost import approx_formula_visible_text, token_units  # noqa: E402
from services.rendering.layout.payload.text_common import (  # noqa: E402
    layout_density_ratio,
    normalize_render_text,
    same_meaningful_render_text,
    strip_formula_placeholders,
    tokenize_protected_text,
    translated_zh_char_count,
)
from services.rendering.layout.text_tokens import (  # noqa: E402
    is_formula_token as text_is_formula_token,
    tokenize_text as text_tokenize_text,
)
from services.rendering.layout.text_analysis import analyze_text  # noqa: E402
from services.rendering.layout.inline_content.fallback.latex_normalizer import (  # noqa: E402
    aggressively_simplify_formula_for_latex_math,
    normalize_formula_for_latex_math,
)
from services.rendering.layout.font_size_fit import estimate_font_size_pt, local_font_size_pt  # noqa: E402
from services.rendering.layout.leading_fit import (  # noqa: E402
    estimate_leading_em,
    normalize_leading_em_for_font_size,
)
from services.rendering.layout.title_fit_limits import resolve_title_fill_max_font_size_pt  # noqa: E402
from services.rendering.layout.font_roles import is_body_text_candidate  # noqa: E402
from services.rendering.layout.typography.scalars import percentile_value  # noqa: E402
from services.rendering.layout.typography.line_count import (  # noqa: E402
    source_visual_line_count,
    visual_line_count,
)
from services.rendering.layout.typography.line_metrics import (  # noqa: E402
    bbox_height,
    bbox_width,
    effective_text_height,
    line_height,
    local_font_metric,
    local_glyph_height,
    local_line_pitch,
    median_line_height,
    median_line_pitch,
)
from services.rendering.layout.typography.geometry import cover_bbox, inner_bbox  # noqa: E402
from services.rendering.layout.typography.cover_geometry import expanded_cover_bbox  # noqa: E402
from services.rendering.layout.typography.compactness import (  # noqa: E402
    line_widths,
    occupied_ratio,
    occupied_ratio_x,
    source_compactness_score,
)
from services.rendering.layout.typography.content import formula_ratio, plain_text_chars_per_line  # noqa: E402
from services.rendering.layout.typography.baseline import page_baseline_font_size  # noqa: E402
from services.rendering.analysis.profile.kind import classify_profile_kind  # noqa: E402
from services.rendering.analysis.profile.models import RenderPageProfile  # noqa: E402
from services.rendering.analysis.profile.geometry import PageGeometryProfile  # noqa: E402
from services.rendering.analysis.profile.text_layer import TextLayerProfile  # noqa: E402
from services.rendering.analysis.profile.image_background import ImageBackgroundProfile  # noqa: E402
from services.rendering.analysis.profile.vector_layer import VectorLayerProfile  # noqa: E402
from services.rendering.analysis.profile.ocr_blocks import OcrBlockProfile  # noqa: E402
from services.rendering.analysis.profile.builder import build_render_page_profile  # noqa: E402
from services.rendering.analysis.profile.text_layer import build_text_layer_profile  # noqa: E402
from services.rendering.analysis.profile.text_traces import text_trace_visibility_counts  # noqa: E402
from services.rendering.analysis.profile.image_background import build_image_background_profile  # noqa: E402
from services.rendering.analysis.profile.vector_layer import build_vector_layer_profile  # noqa: E402
from services.rendering.analysis.profile.ocr_blocks import build_ocr_block_profile  # noqa: E402
from services.rendering.analysis.profile.background_coverage import background_coverage_ratio  # noqa: E402
from services.rendering.source.background.detect import pick_primary_background_image  # noqa: E402
from services.rendering.analysis.classifier import classify_render_page  # noqa: E402
from services.rendering.analysis.route.builder import build_render_page_route  # noqa: E402
from services.document_schema.semantics import (  # noqa: E402
    block_kind,
    is_bodylike_block,
    is_caption_like_block,
    is_footnote_like_block,
    is_metadata_semantic,
    is_plain_text_block,
    is_textual_block,
    is_title_like_block,
    layout_role,
    structure_role,
)

# --- shared character pools ------------------------------------------------
CJK = (
    "的一是在不了有和人这中大为上个国我以要他时来用们生到作地于出就分对成会可主发年动同工也能下过子说产种面而方后"
    "多定行学法所民得经十三之进着等部度家电力里如水化高自二理起小物现实加量都两体制机当使点从业本去把性好应开它合还"
    "因由其些然前外天政四日那社义事平形相全表间样与关各重新线内数正心反你明看原又么利比或但质气第向道命此变条只没结"
    "解问意建月公无系军很情者最立代想已通并提直题党程展五果料象员革位入常文总次品式活设及管特件长求老头基资边流路级"
    "少图山统接知较将组见计别她手角期根论运农指几九区强放决西被干做必战先回则任取据处队南给色光门即保治北造百规热领"
    "七海口东导器压志世金增争济阶油思术极交受联什认六共权收证改清己美再采转更单风切打白教速花带安场身车例真务具万每"
    "目至达走积示议声报斗完类八离华名确才科张信马节话米整空元况今集温传土许步群广石记需段研界拉林律叫且究观越织装影"
    "算低持音众书布复容儿须际商非验连断深难近矿千周委素技备半办青省列习响约支般史感劳便团往酸历市克何除消构府称太准"
    "精值号率族维划选标写存候毛亲快效斯院查江型眼王按格养易置派层片始却专状育厂京识适属圆包火住调满县局照参红细引听"
    "该铁价严龙飞"
)
ASCII = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
PUNCT = "，。！？；：、,.!?;:()[]{}<>《》“”‘’\"'"
LATEX_FORMULAS = [
    "\\frac{a}{b}",
    "\\sum_{i=1}^{n} i^2",
    "\\sqrt{x^2+y^2}",
    "\\int_0^1 f(x)\\,dx",
    "\\mathbf{R}",
    "a+b=c",
    "\\frac{\\partial}{\\partial x}",
    "\\left(\\frac{1}{2}\\right)",
    "E=mc^2",
    "\\alpha + \\beta",
    "\\lim_{x\\to 0} \\frac{\\sin x}{x}",
    "x_{1}+x_{2}",
]
LATEX_POOL = LATEX_FORMULAS + [
    "\\mathrm{abc}\\mathbf{R}",
    "a+b\\,",
    "\\sqrt[3]{8}",
    "f(x)=\\sum_{k=1}^{n} k",
    "\\{a,b,c\\}",
    "",
    "  ",
    "\\alpha\\beta\\gamma",
    "\\partial x",
    "x^2+y^2=z^2",
]

# --- shared semantic vocabularies -------------------------------------------
CAPTION_TAGS = ["caption", "figure_caption", "image_caption", "table_caption", "table_footnote", "image_footnote"]
FOOTNOTE_TAGS = ["footnote", "image_footnote", "table_footnote", "vision_footnote"]
OTHER_TAGS = ["reference_heading", "reference_entry", "reference_zone", "algorithm", "metadata", "not_a_real_tag"]
LAYOUT_ROLES = ["", "paragraph", "list_item", "title", "heading", "caption", "footnote", "table", "figure", "sidebar", "formula"]
SEMANTIC_ROLES = ["", "body", "abstract", "title", "caption", "footnote", "reference", "formula", "metadata"]
STRUCTURE_ROLES = [
    "", "body", "abstract", "example_line", "option_header", "option_description",
    "example_intro", "title", "heading", "section_heading", "figure_caption",
    "reference_heading", "formula", "table_caption",
]
BLOCK_KINDS = ["", "text", "formula", "table", "figure", "list", "title", "caption"]
BLOCK_TYPES = [
    "", "paragraph", "text", "figure_caption", "image_caption", "table_caption",
    "table_footnote", "image_footnote", "footnote", "vision_footnote", "algorithm",
    "reference_heading", "reference_entry", "reference_zone", "raw",
]
DERIVED_ROLES = ["", "caption", "figure_caption", "footnote", "body", "title", "reference", "algorithm"]
SUB_TYPES = ["", "paragraph", "list_item", "table_caption", "footnote", "text", "algorithm", "metadata"]

# --- shared generators -------------------------------------------------------
def gen_plain_chunk(rng):
    pool = CJK + ASCII + PUNCT
    return "".join(rng.choice(pool) for _ in range(rng.randint(2, 30)))


def gen_plain_text_body(rng):
    parts = []
    for _ in range(rng.randint(1, 4)):
        parts.append(gen_plain_chunk(rng))
        if rng.random() < 0.3:
            parts.append(rng.choice(["", "\n", "\n"]))
    return "".join(parts)


def gen_text_and_formula_map(rng):
    mode = rng.random()
    formula_map = []
    if mode < 0.45:
        text = gen_plain_text_body(rng)
    elif mode < 0.8:
        n = rng.randint(1, 3)
        parts = []
        for i in range(n):
            placeholder = "[[FORMULA_%d]]" % i
            formula_map.append({"placeholder": placeholder, "formula_text": rng.choice(LATEX_FORMULAS)})
            parts.append(gen_plain_chunk(rng))
            parts.append(placeholder)
        parts.append(gen_plain_chunk(rng))
        text = "".join(parts)
    else:
        parts = [gen_plain_chunk(rng)]
        parts.append("$%s$" % rng.choice(LATEX_FORMULAS))
        parts.append(gen_plain_chunk(rng))
        if rng.random() < 0.5:
            parts.append("$$%s$$" % rng.choice(LATEX_FORMULAS))
        parts.append(gen_plain_chunk(rng))
        text = "".join(parts)
        if rng.random() < 0.3:
            formula_map = [{"placeholder": "[[FORMULA_0]]", "formula_text": rng.choice(LATEX_FORMULAS)}]
    if rng.random() < 0.15:
        text = "占位符 %s 之后是正文。%s 再来一段。" % ("[[FORMULA_0]]", "[[FORMULA_1]]")
        formula_map = []
    if rng.random() < 0.1 and not formula_map:
        formula_map = [{"placeholder": "[[FORMULA_9]]", "formula_text": rng.choice(LATEX_FORMULAS)}]
    return text, formula_map


def gen_bbox(rng):
    x0 = round(rng.uniform(0, 80), 1)
    y0 = round(rng.uniform(0, 80), 1)
    return [x0, y0, round(x0 + rng.uniform(20, 300), 1), round(y0 + rng.uniform(12, 120), 1)]


def _with_bbox(value, bbox):
    if bbox is not None:
        value["bbox"] = bbox
    return value


def gen_lines(rng):
    lines = []
    for _ in range(rng.randint(0, 4)):
        spans = []
        for _ in range(rng.randint(1, 3)):
            if rng.random() < 0.75:
                spans.append({"type": "text", "content": gen_plain_chunk(rng)})
            else:
                spans.append({"type": "inline_equation", "content": rng.choice(LATEX_FORMULAS)})
        bbox = gen_bbox(rng) if rng.random() < 0.85 else None
        lines.append(_with_bbox({"spans": spans}, bbox))
    return lines


def gen_item(rng):
    text, formula_map = gen_text_and_formula_map(rng)
    item = {
        "lines": gen_lines(rng),
        "source_text": text,
        "layout_role": rng.choice(LAYOUT_ROLES),
        "semantic_role": rng.choice(SEMANTIC_ROLES),
        "structure_role": rng.choice(STRUCTURE_ROLES),
        "block_kind": rng.choice(BLOCK_KINDS),
        "block_type": rng.choice(BLOCK_TYPES),
        "sub_type": rng.choice(SUB_TYPES),
        "normalized_sub_type": rng.choice(SUB_TYPES),
        "raw_block_type": rng.choice(BLOCK_TYPES),
        "derived": {"role": rng.choice(DERIVED_ROLES)},
        "policy_translate": rng.choice([None, True, False]),
        "tags": rng.sample(CAPTION_TAGS + FOOTNOTE_TAGS + OTHER_TAGS, rng.randint(0, 4)),
        "formula_map": formula_map,
        "_is_body_text_candidate": rng.random() < 0.5,
        "_wide_aspect_body_text": rng.random() < 0.3,
        "_cover_with_inner_bbox": rng.random() < 0.3,
    }
    bbox = gen_bbox(rng) if rng.random() < 0.9 else None
    return _with_bbox(item, bbox)


def gen_fit_item(rng):
    item = gen_item(rng)
    if rng.random() < 0.6:
        item["_is_body_text_candidate"] = True
        item["_wide_aspect_body_text"] = rng.random() < 0.4
        if rng.random() < 0.5:
            item["block_kind"] = "text"
            item["block_type"] = "text"
            item["layout_role"] = "paragraph"
            item["source_text"] = "这是一段用于字体拟合的正文文本，长度足够长，包含中英文与数字混合内容。"
            item["lines"] = [
                {"bbox": gen_bbox(rng), "spans": [{"type": "text", "content": gen_plain_chunk(rng)}]}
                for _ in range(rng.randint(2, 4))
            ]
    return item


def gen_baseline_items(rng):
    items = []
    for _ in range(rng.randint(2, 4)):
        item = gen_item(rng)
        if rng.random() < 0.7:
            item["block_kind"] = "text"
            item["block_type"] = ""
            item["layout_role"] = "paragraph"
            item["source_text"] = "这是用于基线测量的一段足够长的正文内容，包含中英文与数字。" * rng.randint(1, 3)
            item["lines"] = [
                {"bbox": gen_bbox(rng), "spans": [{"type": "text", "content": gen_plain_chunk(rng)}]}
                for _ in range(rng.randint(3, 5))
            ]
        items.append(item)
    return items


def gen_scalar_font(rng):
    return round(rng.uniform(7.0, 16.0), 2)


def gen_scalar_leading(rng):
    return round(rng.uniform(0.2, 1.0), 3)


def gen_scalar_width(rng):
    return round(rng.uniform(60, 400), 1)


def gen_scalar_height(rng):
    return round(rng.uniform(20, 400), 1)


def gen_inner(rng):
    return gen_bbox(rng)


def gen_formula_lookup(rng):
    lookup = {}
    if rng.random() < 0.6:
        for i in range(rng.randint(1, 3)):
            lookup["[[FORMULA_%d]]" % i] = rng.choice(LATEX_FORMULAS)
    return lookup


def gen_token(rng):
    kind = rng.random()
    if kind < 0.3:
        return "[[FORMULA_%d]]" % rng.randint(0, 2)
    if kind < 0.45:
        return "$%s$" % rng.choice(LATEX_FORMULAS)
    if kind < 0.6:
        return rng.choice(CJK)
    if kind < 0.75:
        return rng.choice(["alpha", "123", "hello-world", "x1", "ABC123", "e-mail"])
    if kind < 0.85:
        return "  "
    return rng.choice(["，", ".", "(", "、", "foo-bar"])


# --- serializers -------------------------------------------------------------
def result_to_dict(r):
    return {
        "font_size_pt": r.font_size_pt,
        "estimated_height_pt": r.estimated_height_pt,
        "line_count": r.line_count,
        "overflow_ratio": r.overflow_ratio,
        "formula_ratio": r.formula_ratio,
        "confidence": r.confidence,
        "max_safe_shrink_pt": r.max_safe_shrink_pt,
    }


def decision_to_dict(d):
    return {
        "font_size_pt": d.font_size_pt,
        "mode": d.mode,
        "confidence": d.confidence,
        "reason_codes": list(d.reason_codes),
        "estimated_height_pt": d.estimated_height_pt,
        "overflow_ratio": d.overflow_ratio,
        "formula_ratio": d.formula_ratio,
        "growth_pt": d.growth_pt,
        "shrink_pt": d.shrink_pt,
    }


def stats_to_dict(a):
    s = a.stats
    return {
        "word_count": s.word_count,
        "zh_char_count": s.zh_char_count,
        "formula_count": s.formula_count,
        "raw_math_count": s.raw_math_count,
        "latex_command_count": s.latex_command_count,
        "placeholder_count": s.placeholder_count,
        "protected_formula_count": s.protected_formula_count,
    }


def text_layer_to_dict(p):
    return {
        "visible_traces": p.visible_traces,
        "hidden_traces": p.hidden_traces,
        "has_visible_text": p.has_visible_text,
        "has_hidden_text": p.has_hidden_text,
        "editable": p.editable,
    }


def image_background_to_dict(p):
    return {
        "has_large_background": p.has_large_background,
        "coverage_ratio": p.coverage_ratio,
        "xref": p.xref,
        "bbox": list(p.bbox) if p.bbox else None,
    }


def vector_layer_to_dict(p):
    return {
        "drawing_count": p.drawing_count,
        "vector_heavy": p.vector_heavy,
        "cover_only_preferred": p.cover_only_preferred,
    }


def geometry_to_dict(p):
    return {
        "page_index": p.page_index,
        "width_pt": p.width_pt,
        "height_pt": p.height_pt,
        "rotation": p.rotation,
        "cropbox": list(p.cropbox),
    }


def ocr_blocks_to_dict(p):
    return {
        "block_count": p.block_count,
        "valid_bbox_count": p.valid_bbox_count,
        "total_bbox_area": p.total_bbox_area,
        "page_area_ratio": p.page_area_ratio,
    }


# --- profile generators ------------------------------------------------------
def gen_text_layer(rng):
    editable = rng.random() < 0.4
    visible = rng.randint(0, 20)
    hidden = rng.randint(0, 20)
    return TextLayerProfile(
        visible_traces=visible,
        hidden_traces=hidden,
        has_visible_text=rng.random() < 0.5 or editable,
        has_hidden_text=rng.random() < 0.4,
        editable=editable,
    )


def gen_image_background(rng):
    large = rng.random() < 0.45
    return ImageBackgroundProfile(
        has_large_background=large,
        coverage_ratio=round(rng.uniform(0.0, 0.95), 3),
        xref=rng.choice([None, 1, 2, 3]) if large else None,
        bbox=gen_bbox(rng) if large else None,
    )


def gen_vector_layer(rng):
    heavy = rng.random() < 0.3
    return VectorLayerProfile(
        drawing_count=rng.randint(0, 2000),
        vector_heavy=heavy,
        cover_only_preferred=heavy or rng.random() < 0.3,
    )


def gen_render_page_profile(rng):
    geometry = PageGeometryProfile(
        page_index=rng.randint(0, 3),
        width_pt=round(rng.uniform(200, 600), 1),
        height_pt=round(rng.uniform(300, 900), 1),
        rotation=rng.choice([0, 0, 90]),
        cropbox=gen_bbox(rng),
    )
    text_layer = gen_text_layer(rng)
    image_background = gen_image_background(rng)
    vector_layer = gen_vector_layer(rng)
    kind = classify_profile_kind(
        text_layer=text_layer,
        image_background=image_background,
        vector_layer=vector_layer,
    )
    if rng.random() < 0.15:
        kind = rng.choice(["editable_text", "scan_image", "pseudo_editable_scan", "vector_heavy", "mixed_complex"])
    ocr_blocks = OcrBlockProfile(
        block_count=rng.randint(0, 40),
        valid_bbox_count=rng.randint(0, 40),
        total_bbox_area=round(rng.uniform(0, 500000), 1),
        page_area_ratio=round(rng.uniform(0, 1), 4),
    )
    return RenderPageProfile(
        geometry=geometry,
        text_layer=text_layer,
        image_background=image_background,
        vector_layer=vector_layer,
        ocr_blocks=ocr_blocks,
        kind=kind,
    )


def profile_to_dict(p):
    return {
        "geometry": geometry_to_dict(p.geometry),
        "text_layer": text_layer_to_dict(p.text_layer),
        "image_background": image_background_to_dict(p.image_background),
        "vector_layer": vector_layer_to_dict(p.vector_layer),
        "ocr_blocks": ocr_blocks_to_dict(p.ocr_blocks),
        "kind": p.kind,
    }


# --- Phase 3: profile collectors (PageSnapshot / FakePage) -------------------
class FakePage:
    """Mimics the fitz.Page surface used by analysis/profile/*. See page.rs."""

    def __init__(self, data):
        self.data = data
        self.number = data["number"]
        self.rotation = data["rotation"]
        self.rect = fitz.Rect(data["rect"])
        self.cropbox = fitz.Rect(data["cropbox"])

    def get_texttrace(self):
        return [{"type": t["type"], "opacity": t["opacity"]} for t in self.data["text_traces"]]

    def get_text(self, mode):
        return [[0.0, 0.0, 0.0, 0.0, "w", 0, 0, 0]] * self.data["word_count"]

    def get_cdrawings(self):
        return [None] * self.data["drawing_count"]

    def get_image_info(self, hashes=False, xrefs=False):
        return [{"xref": i["xref"], "bbox": i["bbox"]} for i in self.data["image_infos"]]

    def get_images(self, full=True):
        return [[xref] for xref in self.data["image_entries"]]

    def get_image_rects(self, xref):
        return self.data["image_rects"].get(str(xref), [])


def gen_image_bbox(rng, rect):
    mode = rng.random()
    if mode < 0.3:
        return [float(v) for v in rect]
    if mode < 0.6:
        x0 = round(rng.uniform(0, rect[2]), 1)
        y0 = round(rng.uniform(0, rect[3]), 1)
        x1 = round(rng.uniform(x0, rect[2]), 1)
        y1 = round(rng.uniform(y0, rect[3]), 1)
        return [x0, y0, x1, y1]
    if mode < 0.8:
        return [
            round(rng.uniform(-200, rect[2] - 100), 1),
            round(rng.uniform(-200, rect[3] - 100), 1),
            round(rng.uniform(100, rect[2] + 200), 1),
            round(rng.uniform(100, rect[3] + 200), 1),
        ]
    x = round(rng.uniform(0, rect[2]), 1)
    y = round(rng.uniform(0, rect[3]), 1)
    return [x, y, x, y]


def gen_page_snapshot(rng):
    width = round(rng.uniform(100, 800), 1)
    height = round(rng.uniform(150, 1000), 1)
    rect = [0.0, 0.0, width, height]
    text_traces = [
        {"type": rng.choice([0, 0, 1, 2, 3, 4]), "opacity": rng.choice([0.0, 0.5, 1.0])}
        for _ in range(rng.randint(0, 4))
    ]
    image_infos = []
    for _ in range(rng.randint(0, 2)):
        image_infos.append({"xref": rng.randint(1, 5), "bbox": gen_image_bbox(rng, rect)})
    image_entries = []
    image_rects = {}
    for _ in range(rng.randint(0, 2)):
        xref = rng.randint(1, 5)
        image_entries.append(xref)
        image_rects[str(xref)] = [gen_image_bbox(rng, rect) for _ in range(rng.randint(0, 2))]
    return {
        "number": rng.randint(0, 3),
        "rotation": rng.choice([0, 0, 90]),
        "rect": rect,
        "cropbox": rect if rng.random() < 0.7 else gen_image_bbox(rng, rect),
        "text_traces": text_traces,
        "word_count": rng.choice([0, 1, 2, 5, 19, 20, 21, 25]),
        "drawing_count": rng.choice([0, 1, 5, 1999, 2000, 2001, 4999, 5000, 5001]),
        "image_infos": image_infos,
        "image_entries": image_entries,
        "image_rects": image_rects,
    }


def gen_ocr_items(rng):
    return [gen_bbox(rng) for _ in range(rng.randint(0, 3))]


# --- handlers -----------------------------------------------------------------
def h_chinese_formula_unit_ratio(rng, n):
    out = []
    for _ in range(n):
        text, formula_map = gen_text_and_formula_map(rng)
        out.append({"input": {"text": text, "formula_map": formula_map}, "expected": formula_unit_ratio(text, formula_map)})
    return out


def h_chinese_tokenize(rng, n):
    out = []
    for _ in range(n):
        text, formula_map = gen_text_and_formula_map(rng)
        tokens = tokenize_chinese_body_text(text, formula_map)
        out.append({
            "input": {"text": text, "formula_map": formula_map},
            "expected": [{"text": t.text, "units": t.units, "formula": t.formula} for t in tokens],
        })
    return out


def h_chinese_estimate_lines(rng, n):
    out = []
    for _ in range(n):
        text, formula_map = gen_text_and_formula_map(rng)
        width = gen_scalar_width(rng)
        font = gen_scalar_font(rng)
        out.append({
            "input": {"bbox_width_pt": width, "text": text, "formula_map": formula_map, "font_size_pt": font},
            "expected": list(estimate_chinese_body_lines(width, text, formula_map, font)),
        })
    return out


def h_chinese_estimate_height(rng, n):
    out = []
    for _ in range(n):
        text, formula_map = gen_text_and_formula_map(rng)
        width = gen_scalar_width(rng)
        font = gen_scalar_font(rng)
        leading = gen_scalar_leading(rng)
        out.append({
            "input": {"bbox_width_pt": width, "text": text, "formula_map": formula_map, "font_size_pt": font, "leading_em": leading},
            "expected": result_to_dict(estimate_chinese_body_height_pt(width, text, formula_map, font, leading)),
        })
    return out


def h_chinese_solve(rng, n):
    out = []
    for _ in range(n):
        text, formula_map = gen_text_and_formula_map(rng)
        width = gen_scalar_width(rng)
        height = gen_scalar_height(rng)
        leading = gen_scalar_leading(rng)
        low = round(rng.uniform(7.0, 9.0), 2)
        high = round(rng.uniform(low, 14.0), 2)
        out.append({
            "input": {
                "bbox_width_pt": width,
                "bbox_height_pt": height,
                "text": text,
                "formula_map": formula_map,
                "leading_em": leading,
                "min_font_size_pt": low,
                "max_font_size_pt": high,
            },
            "expected": result_to_dict(
                solve_chinese_body_font_size_pt(
                    width, height, text, formula_map, leading_em=leading, min_font_size_pt=low, max_font_size_pt=high
                )
            ),
        })
    return out


def h_fit_decision(rng, n):
    out = []
    for _ in range(n):
        text, formula_map = gen_text_and_formula_map(rng)
        w = gen_scalar_width(rng)
        h = gen_scalar_height(rng)
        fs = gen_scalar_font(rng)
        le = gen_scalar_leading(rng)
        growth = rng.choice([None, round(rng.uniform(7.8, 13.0), 2)])
        decision = plan_chinese_body_fit(
            bbox_width_pt=w, bbox_height_pt=h, text=text, formula_map=formula_map,
            font_size_pt=fs, leading_em=le, max_growth_font_size_pt=growth,
        )
        out.append({
            "input": {
                "bbox_width_pt": w,
                "bbox_height_pt": h,
                "text": text,
                "formula_map": formula_map,
                "font_size_pt": fs,
                "leading_em": le,
                "max_growth_font_size_pt": growth,
            },
            "expected": decision_to_dict(decision),
        })
    return out


def h_capacity_formula_discount(rng, n):
    out = []
    for _ in range(n):
        text, formula_map = gen_text_and_formula_map(rng)
        out.append({"input": {"text": text, "formula_map": formula_map}, "expected": formula_estimate_discount(text, formula_map)})
    return out


def h_capacity_box_units(rng, n):
    out = []
    for _ in range(n):
        inner = gen_inner(rng)
        visual_lines = rng.choice([None, rng.randint(1, 6)])
        font = gen_scalar_font(rng)
        leading = gen_scalar_leading(rng)
        out.append({
            "input": {"inner": inner, "font_size_pt": font, "leading_em": leading, "visual_lines": visual_lines},
            "expected": box_capacity_units(inner, font, leading, visual_lines),
        })
    return out


def h_capacity_text_demand(rng, n):
    out = []
    for _ in range(n):
        text, formula_map = gen_text_and_formula_map(rng)
        out.append({"input": {"text": text, "formula_map": formula_map}, "expected": text_demand_units(text, formula_map)})
    return out


def h_capacity_required_lines(rng, n):
    out = []
    for _ in range(n):
        inner = gen_inner(rng)
        text, formula_map = gen_text_and_formula_map(rng)
        font = gen_scalar_font(rng)
        out.append({
            "input": {"inner": inner, "text": text, "formula_map": formula_map, "font_size_pt": font},
            "expected": estimated_required_lines(inner, text, formula_map, font),
        })
    return out


def h_capacity_render_height(rng, n):
    out = []
    for _ in range(n):
        inner = gen_inner(rng)
        text, formula_map = gen_text_and_formula_map(rng)
        font = gen_scalar_font(rng)
        leading = gen_scalar_leading(rng)
        out.append({
            "input": {"inner": inner, "text": text, "formula_map": formula_map, "font_size_pt": font, "leading_em": leading},
            "expected": estimated_render_height_pt(inner, text, formula_map, font, leading),
        })
    return out


def h_split_protected_text(rng, n):
    out = []
    for _ in range(n):
        text, formula_map = gen_text_and_formula_map(rng)
        # The reference produces out-of-range ranges when there are more boxes
        # than tokens, so keep the box count within the token count.
        n_tokens = len(tokenize_protected_text(text))
        capacity_count = rng.randint(1, 4) if n_tokens == 0 else min(rng.randint(1, 4), max(1, n_tokens))
        capacities = [round(rng.uniform(1, 60), 1) for _ in range(capacity_count)]
        preferred = None
        if rng.random() < 0.5:
            preferred = [round(rng.uniform(1, 60), 1) for _ in range(capacity_count)]
        direct_math = rng.random() < 0.3
        out.append({
            "input": {
                "text": text,
                "formula_map": formula_map,
                "capacities": capacities,
                "preferred_weights": preferred,
                "direct_math_mode": direct_math,
            },
            "expected": split_protected_text_for_boxes(
                text, formula_map, capacities, preferred_weights=preferred, direct_math_mode=direct_math
            ),
        })
    return out


def h_approx_formula_visible(rng, n):
    out = []
    for _ in range(n):
        formula = rng.choice(LATEX_POOL)
        out.append({"input": {"formula": formula}, "expected": approx_formula_visible_text(formula)})
    return out


def h_token_units(rng, n):
    out = []
    for _ in range(n):
        token = gen_token(rng)
        lookup = gen_formula_lookup(rng)
        out.append({"input": {"token": token, "formula_lookup": lookup}, "expected": token_units(token, lookup)})
    return out


def h_text_tokenize_protected(rng, n):
    out = []
    for _ in range(n):
        text, _ = gen_text_and_formula_map(rng)
        out.append({"input": {"text": text}, "expected": tokenize_protected_text(text)})
    return out


def h_strip_placeholders(rng, n):
    out = []
    for _ in range(n):
        text, _ = gen_text_and_formula_map(rng)
        out.append({"input": {"text": text}, "expected": strip_formula_placeholders(text)})
    return out


def h_normalize_render_text(rng, n):
    out = []
    for _ in range(n):
        text = rng.choice([
            gen_plain_chunk(rng),
            "  %s  " % gen_plain_chunk(rng),
            "a  b\n\tc",
            "",
            "   ",
        ])
        out.append({"input": {"text": text}, "expected": normalize_render_text(text)})
    return out


def h_same_meaningful(rng, n):
    out = []
    for _ in range(n):
        base = "中文  english  混排"
        a = rng.choice([base, " 中文 english 混排  ", "不同文本", ""])
        b = rng.choice([base, "中文english混排", "中文 english 混排", "完全不同"])
        out.append({"input": {"a": a, "b": b}, "expected": same_meaningful_render_text(a, b)})
    return out


def h_zh_char_count(rng, n):
    out = []
    for _ in range(n):
        text, _ = gen_text_and_formula_map(rng)
        out.append({"input": {"text": text}, "expected": translated_zh_char_count(text)})
    return out


def h_layout_density(rng, n):
    out = []
    for _ in range(n):
        inner = gen_inner(rng)
        text, _ = gen_text_and_formula_map(rng)
        font = gen_scalar_font(rng)
        line_step = round(rng.uniform(10, 20), 1)
        out.append({
            "input": {"inner": inner, "text": text, "font_size_pt": font, "line_step_pt": line_step},
            "expected": layout_density_ratio(inner, text, font_size_pt=font, line_step_pt=line_step),
        })
    return out


def h_text_tokenize(rng, n):
    out = []
    for _ in range(n):
        text, _ = gen_text_and_formula_map(rng)
        out.append({"input": {"text": text}, "expected": text_tokenize_text(text)})
    return out


def h_text_is_formula_token(rng, n):
    out = []
    for _ in range(n):
        token = gen_token(rng)
        out.append({"input": {"token": token}, "expected": text_is_formula_token(token)})
    return out


def h_analyze_stats(rng, n):
    out = []
    for _ in range(n):
        text, _ = gen_text_and_formula_map(rng)
        out.append({"input": {"text": text}, "expected": stats_to_dict(analyze_text(text))})
    return out


def h_latex_normalize(rng, n):
    out = []
    for _ in range(n):
        formula = rng.choice(LATEX_POOL)
        out.append({"input": {"formula": formula}, "expected": normalize_formula_for_latex_math(formula)})
    return out


def h_latex_aggressive(rng, n):
    out = []
    for _ in range(n):
        formula = rng.choice(LATEX_POOL)
        out.append({"input": {"formula": formula}, "expected": aggressively_simplify_formula_for_latex_math(formula)})
    return out


def h_local_font_size(rng, n):
    out = []
    for _ in range(n):
        item = gen_fit_item(rng)
        out.append({"input": {"item": item}, "expected": local_font_size_pt(item)})
    return out


def h_estimate_font_size(rng, n):
    out = []
    for _ in range(n):
        item = gen_fit_item(rng)
        page_font = round(rng.uniform(8.0, 13.0), 2)
        page_pitch = round(rng.uniform(10, 20), 1)
        page_height = round(rng.uniform(8, 16), 1)
        out.append({
            "input": {
                "item": item,
                "page_font_size": page_font,
                "page_line_pitch": page_pitch,
                "page_line_height": page_height,
            },
            "expected": estimate_font_size_pt(
                item,
                page_font_size=page_font,
                page_line_pitch=page_pitch,
                page_line_height=page_height,
                density_baseline=0.0,
            ),
        })
    return out


def h_estimate_leading(rng, n):
    out = []
    for _ in range(n):
        item = gen_fit_item(rng)
        pitch = round(rng.uniform(10, 20), 1)
        font = gen_scalar_font(rng)
        out.append({
            "input": {"item": item, "page_line_pitch": pitch, "font_size_pt": font},
            "expected": estimate_leading_em(item, pitch, font),
        })
    return out


def h_normalize_leading(rng, n):
    out = []
    for _ in range(n):
        font = gen_scalar_font(rng)
        leading = round(rng.uniform(0.2, 1.0), 3)
        reference = round(rng.uniform(8.0, 13.0), 2)
        min_leading = round(rng.uniform(0.2, 0.4), 3)
        max_leading = round(rng.uniform(0.5, 0.9), 3)
        strength = round(rng.uniform(0.2, 1.0), 3)
        floor_min = rng.choice([None, round(rng.uniform(0.2, 0.5), 3)])
        out.append({
            "input": {
                "font_size_pt": font,
                "leading_em": leading,
                "reference_font_size_pt": reference,
                "min_leading_em": min_leading,
                "max_leading_em": max_leading,
                "strength": strength,
                "floor_min_leading_em": floor_min,
            },
            "expected": normalize_leading_em_for_font_size(
                font, leading,
                reference_font_size_pt=reference,
                min_leading_em=min_leading,
                max_leading_em=max_leading,
                strength=strength,
                floor_min_leading_em=floor_min,
            ),
        })
    return out


def h_title_fill(rng, n):
    out = []
    for _ in range(n):
        item = gen_item(rng)
        base = round(rng.uniform(10.0, 30.0), 2)
        out.append({
            "input": {"item": item, "base_font_size_pt": base},
            "expected": resolve_title_fill_max_font_size_pt(item, base),
        })
    return out


def h_is_body_text_candidate(rng, n):
    out = []
    for _ in range(n):
        item = gen_fit_item(rng)
        width_med = round(rng.uniform(0, 200), 1)
        out.append({
            "input": {"item": item, "page_text_width_med": width_med},
            "expected": is_body_text_candidate(item, width_med),
        })
    return out


def h_percentile(rng, n):
    out = []
    for _ in range(n):
        values = [round(rng.uniform(0, 50), 1) for _ in range(rng.randint(0, 8))]
        q = round(rng.uniform(0, 1), 3)
        out.append({"input": {"values": values, "q": q}, "expected": percentile_value(values, q)})
    return out


def h_visual_line_count(rng, n):
    out = []
    for _ in range(n):
        item = gen_item(rng)
        out.append({"input": {"item": item}, "expected": visual_line_count(item)})
    return out


def h_source_visual_line_count(rng, n):
    out = []
    for _ in range(n):
        item = gen_item(rng)
        out.append({"input": {"item": item}, "expected": source_visual_line_count(item)})
    return out


def h_line_height(rng, n):
    out = []
    for _ in range(n):
        line = {"spans": []}
        if rng.random() < 0.85:
            line["bbox"] = gen_bbox(rng)
        out.append({"input": {"line": line}, "expected": line_height(line)})
    return out


def _item_float_handler(fn):
    def handler(rng, n):
        out = []
        for _ in range(n):
            item = gen_item(rng)
            out.append({"input": {"item": item}, "expected": fn(item)})
        return out
    return handler


def _item_bool_handler(fn):
    def handler(rng, n):
        out = []
        for _ in range(n):
            item = gen_item(rng)
            out.append({"input": {"item": item}, "expected": fn(item)})
        return out
    return handler


def _item_str_handler(fn):
    def handler(rng, n):
        out = []
        for _ in range(n):
            item = gen_item(rng)
            out.append({"input": {"item": item}, "expected": fn(item)})
        return out
    return handler


def h_inner_bbox(rng, n):
    out = []
    for _ in range(n):
        item = gen_item(rng)
        out.append({"input": {"item": item}, "expected": inner_bbox(item)})
    return out


def h_cover_bbox(rng, n):
    out = []
    for _ in range(n):
        item = gen_item(rng)
        out.append({"input": {"item": item}, "expected": cover_bbox(item)})
    return out


def h_expanded_cover_bbox(rng, n):
    out = []
    for _ in range(n):
        item = gen_item(rng)
        bbox = gen_bbox(rng)
        out.append({"input": {"item": item, "bbox": bbox}, "expected": expanded_cover_bbox(item, bbox)})
    return out


def h_line_widths(rng, n):
    out = []
    for _ in range(n):
        item = gen_item(rng)
        out.append({"input": {"item": item}, "expected": line_widths(item)})
    return out


def h_page_baseline(rng, n):
    out = []
    for _ in range(n):
        items = gen_baseline_items(rng)
        out.append({"input": {"items": items}, "expected": list(page_baseline_font_size(items))})
    return out


def h_classify_profile_kind(rng, n):
    out = []
    for _ in range(n):
        tl = gen_text_layer(rng)
        ib = gen_image_background(rng)
        vl = gen_vector_layer(rng)
        out.append({
            "input": {
                "text_layer": text_layer_to_dict(tl),
                "image_background": image_background_to_dict(ib),
                "vector_layer": vector_layer_to_dict(vl),
            },
            "expected": classify_profile_kind(text_layer=tl, image_background=ib, vector_layer=vl),
        })
    return out


def h_build_render_page_route(rng, n):
    out = []
    for _ in range(n):
        profile = gen_render_page_profile(rng)
        route = build_render_page_route(profile)
        out.append({
            "input": {"profile": profile_to_dict(profile)},
            "expected": {
                "redaction": route.redaction,
                "background": route.background,
                "compose": route.compose,
                "layout": route.layout,
                "reason": route.reason,
                "render_mode_hint": route.render_mode_hint,
                "text_cleanup": route.text_cleanup,
                "overlay_fallback": route.overlay_fallback,
            },
        })
    return out


def h_profile_build_render_page_profile(rng, n):
    out = []
    for _ in range(n):
        snapshot = gen_page_snapshot(rng)
        page = FakePage(snapshot)
        ocr_items = gen_ocr_items(rng)
        threshold = round(rng.choice([0.5, 0.75, 0.9]), 2)
        profile = build_render_page_profile(
            page,
            ocr_items=[{"bbox": b} for b in ocr_items],
            background_threshold=threshold,
        )
        out.append({
            "input": {
                "page_snapshot": snapshot,
                "ocr_items": ocr_items,
                "background_threshold": threshold,
            },
            "expected": profile_to_dict(profile),
        })
    return out


def h_profile_build_ocr_block_profile(rng, n):
    out = []
    for _ in range(n):
        ocr_items = gen_ocr_items(rng)
        page_width = round(rng.uniform(100, 800), 1)
        page_height = round(rng.uniform(150, 1000), 1)
        p = build_ocr_block_profile(
            [{"bbox": b} for b in ocr_items],
            page_width=page_width,
            page_height=page_height,
        )
        out.append({
            "input": {"ocr_items": ocr_items, "page_width": page_width, "page_height": page_height},
            "expected": ocr_blocks_to_dict(p),
        })
    return out


def h_profile_build_text_layer_profile(rng, n):
    out = []
    for _ in range(n):
        snapshot = gen_page_snapshot(rng)
        page = FakePage(snapshot)
        out.append({
            "input": {"page_snapshot": snapshot},
            "expected": text_layer_to_dict(build_text_layer_profile(page)),
        })
    return out


def h_profile_build_image_background_profile(rng, n):
    out = []
    for _ in range(n):
        snapshot = gen_page_snapshot(rng)
        page = FakePage(snapshot)
        threshold = round(rng.choice([0.5, 0.75, 0.9]), 2)
        out.append({
            "input": {"page_snapshot": snapshot, "background_threshold": threshold},
            "expected": image_background_to_dict(
                build_image_background_profile(page, background_threshold=threshold)
            ),
        })
    return out


def h_profile_build_vector_layer_profile(rng, n):
    out = []
    for _ in range(n):
        snapshot = gen_page_snapshot(rng)
        page = FakePage(snapshot)
        out.append({
            "input": {"page_snapshot": snapshot},
            "expected": vector_layer_to_dict(build_vector_layer_profile(page)),
        })
    return out


def h_profile_text_trace_visibility_counts(rng, n):
    out = []
    for _ in range(n):
        snapshot = gen_page_snapshot(rng)
        page = FakePage(snapshot)
        visible, hidden = text_trace_visibility_counts(page)
        out.append({
            "input": {"page_snapshot": snapshot},
            "expected": [visible, hidden],
        })
    return out


def h_profile_background_coverage_ratio(rng, n):
    out = []
    for _ in range(n):
        snapshot = gen_page_snapshot(rng)
        page = FakePage(snapshot)
        rect = gen_image_bbox(rng, snapshot["rect"]) if rng.random() < 0.8 else None
        out.append({
            "input": {"page_snapshot": snapshot, "rect": rect},
            "expected": background_coverage_ratio(page, fitz.Rect(rect) if rect else None),
        })
    return out


def h_profile_pick_primary_background_image(rng, n):
    out = []
    for _ in range(n):
        snapshot = gen_page_snapshot(rng)
        page = FakePage(snapshot)
        threshold = round(rng.choice([0.0, 0.5, 0.75]), 2)
        result = pick_primary_background_image(page, coverage_ratio_threshold=threshold)
        out.append({
            "input": {"page_snapshot": snapshot, "coverage_ratio_threshold": threshold},
            "expected": {"xref": int(result[0]), "bbox": [float(v) for v in result[1]]}
            if result is not None
            else None,
        })
    return out


def h_profile_classify_render_page(rng, n):
    out = []
    for _ in range(n):
        snapshot = gen_page_snapshot(rng)
        page = FakePage(snapshot)
        threshold = round(rng.choice([0.5, 0.75, 0.9]), 2)
        c = classify_render_page(page, background_threshold=threshold)
        route = c.route
        out.append({
            "input": {"page_snapshot": snapshot, "background_threshold": threshold},
            "expected": {
                "kind": c.kind,
                "large_background_image": c.large_background_image,
                "visible_text_traces": c.visible_text_traces,
                "hidden_text_traces": c.hidden_text_traces,
                "drawing_count": c.drawing_count,
                "background_coverage_ratio": c.background_coverage_ratio,
                "route": {
                    "redaction": route.redaction,
                    "background": route.background,
                    "compose": route.compose,
                    "layout": route.layout,
                    "reason": route.reason,
                },
            },
        })
    return out


# --- registry -----------------------------------------------------------------
SCALAR_DEFAULT = 40
ITEM_DEFAULT = 25

REGISTRY = {
    "chinese_body_fit.formula_unit_ratio": (h_chinese_formula_unit_ratio, SCALAR_DEFAULT),
    "chinese_body_fit.tokenize_chinese_body_text": (h_chinese_tokenize, SCALAR_DEFAULT),
    "chinese_body_fit.estimate_chinese_body_lines": (h_chinese_estimate_lines, SCALAR_DEFAULT),
    "chinese_body_fit.estimate_chinese_body_height_pt": (h_chinese_estimate_height, SCALAR_DEFAULT),
    "chinese_body_fit.solve_chinese_body_font_size_pt": (h_chinese_solve, SCALAR_DEFAULT),
    "fit_decision.plan_chinese_body_fit": (h_fit_decision, SCALAR_DEFAULT),
    "payload.capacity.formula_estimate_discount": (h_capacity_formula_discount, SCALAR_DEFAULT),
    "payload.capacity.box_capacity_units": (h_capacity_box_units, SCALAR_DEFAULT),
    "payload.capacity.text_demand_units": (h_capacity_text_demand, SCALAR_DEFAULT),
    "payload.capacity.estimated_required_lines": (h_capacity_required_lines, SCALAR_DEFAULT),
    "payload.capacity.estimated_render_height_pt": (h_capacity_render_height, SCALAR_DEFAULT),
    "payload.continuation_split.split_protected_text_for_boxes": (h_split_protected_text, SCALAR_DEFAULT),
    "payload.formula_cost.approx_formula_visible_text": (h_approx_formula_visible, SCALAR_DEFAULT),
    "payload.formula_cost.token_units": (h_token_units, SCALAR_DEFAULT),
    "payload.text_common.tokenize_protected_text": (h_text_tokenize_protected, SCALAR_DEFAULT),
    "payload.text_common.strip_formula_placeholders": (h_strip_placeholders, SCALAR_DEFAULT),
    "payload.text_common.normalize_render_text": (h_normalize_render_text, SCALAR_DEFAULT),
    "payload.text_common.same_meaningful_render_text": (h_same_meaningful, SCALAR_DEFAULT),
    "payload.text_common.translated_zh_char_count": (h_zh_char_count, SCALAR_DEFAULT),
    "payload.text_common.layout_density_ratio": (h_layout_density, SCALAR_DEFAULT),
    "text.tokens.tokenize_text": (h_text_tokenize, SCALAR_DEFAULT),
    "text.tokens.is_formula_token": (h_text_is_formula_token, SCALAR_DEFAULT),
    "text.analysis.analyze_text_stats": (h_analyze_stats, SCALAR_DEFAULT),
    "text.latex_normalizer.normalize_formula_for_latex_math": (h_latex_normalize, SCALAR_DEFAULT),
    "text.latex_normalizer.aggressively_simplify_formula_for_latex_math": (h_latex_aggressive, SCALAR_DEFAULT),
    "font_fit.local_font_size_pt": (h_local_font_size, ITEM_DEFAULT),
    "font_fit.estimate_font_size_pt": (h_estimate_font_size, ITEM_DEFAULT),
    "font_fit.estimate_leading_em": (h_estimate_leading, ITEM_DEFAULT),
    "font_fit.normalize_leading_em_for_font_size": (h_normalize_leading, SCALAR_DEFAULT),
    "font_fit.resolve_title_fill_max_font_size_pt": (h_title_fill, ITEM_DEFAULT),
    "font_fit.is_body_text_candidate": (h_is_body_text_candidate, ITEM_DEFAULT),
    "typography.percentile_value": (h_percentile, SCALAR_DEFAULT),
    "typography.visual_line_count": (h_visual_line_count, ITEM_DEFAULT),
    "typography.source_visual_line_count": (h_source_visual_line_count, ITEM_DEFAULT),
    "typography.line_height": (h_line_height, SCALAR_DEFAULT),
    "typography.median_line_height": (_item_float_handler(median_line_height), ITEM_DEFAULT),
    "typography.median_line_pitch": (_item_float_handler(median_line_pitch), ITEM_DEFAULT),
    "typography.local_line_pitch": (_item_float_handler(local_line_pitch), ITEM_DEFAULT),
    "typography.local_glyph_height": (_item_float_handler(local_glyph_height), ITEM_DEFAULT),
    "typography.local_font_metric": (_item_float_handler(local_font_metric), ITEM_DEFAULT),
    "typography.bbox_width": (_item_float_handler(bbox_width), ITEM_DEFAULT),
    "typography.bbox_height": (_item_float_handler(bbox_height), ITEM_DEFAULT),
    "typography.effective_text_height": (_item_float_handler(effective_text_height), ITEM_DEFAULT),
    "typography.inner_bbox": (h_inner_bbox, ITEM_DEFAULT),
    "typography.cover_bbox": (h_cover_bbox, ITEM_DEFAULT),
    "typography.expanded_cover_bbox": (h_expanded_cover_bbox, ITEM_DEFAULT),
    "typography.occupied_ratio": (_item_float_handler(occupied_ratio), ITEM_DEFAULT),
    "typography.occupied_ratio_x": (_item_float_handler(occupied_ratio_x), ITEM_DEFAULT),
    "typography.source_compactness_score": (_item_float_handler(source_compactness_score), ITEM_DEFAULT),
    "typography.plain_text_chars_per_line": (_item_float_handler(plain_text_chars_per_line), ITEM_DEFAULT),
    "typography.formula_ratio": (_item_float_handler(formula_ratio), ITEM_DEFAULT),
    "typography.line_widths": (h_line_widths, ITEM_DEFAULT),
    "typography.page_baseline_font_size": (h_page_baseline, ITEM_DEFAULT),
    "semantics.layout_role": (_item_str_handler(layout_role), ITEM_DEFAULT),
    "semantics.block_kind": (_item_str_handler(block_kind), ITEM_DEFAULT),
    "semantics.structure_role": (_item_str_handler(structure_role), ITEM_DEFAULT),
    "semantics.is_caption_like_block": (_item_bool_handler(is_caption_like_block), ITEM_DEFAULT),
    "semantics.is_footnote_like_block": (_item_bool_handler(is_footnote_like_block), ITEM_DEFAULT),
    "semantics.is_title_like_block": (_item_bool_handler(is_title_like_block), ITEM_DEFAULT),
    "semantics.is_bodylike_block": (_item_bool_handler(is_bodylike_block), ITEM_DEFAULT),
    "semantics.is_textual_block": (_item_bool_handler(is_textual_block), ITEM_DEFAULT),
    "semantics.is_plain_text_block": (_item_bool_handler(is_plain_text_block), ITEM_DEFAULT),
    "semantics.is_metadata_semantic": (_item_bool_handler(is_metadata_semantic), ITEM_DEFAULT),
    "route.classify_profile_kind": (h_classify_profile_kind, ITEM_DEFAULT),
    "route.build_render_page_route": (h_build_render_page_route, ITEM_DEFAULT),
    "profile.build_render_page_profile": (h_profile_build_render_page_profile, SCALAR_DEFAULT),
    "profile.ocr_blocks.build_ocr_block_profile": (h_profile_build_ocr_block_profile, SCALAR_DEFAULT),
    "profile.text_layer.build_text_layer_profile": (h_profile_build_text_layer_profile, SCALAR_DEFAULT),
    "profile.image_background.build_image_background_profile": (
        h_profile_build_image_background_profile,
        SCALAR_DEFAULT,
    ),
    "profile.vector_layer.build_vector_layer_profile": (h_profile_build_vector_layer_profile, SCALAR_DEFAULT),
    "profile.text_traces.text_trace_visibility_counts": (h_profile_text_trace_visibility_counts, SCALAR_DEFAULT),
    "profile.background_coverage.background_coverage_ratio": (h_profile_background_coverage_ratio, SCALAR_DEFAULT),
    "profile.detect.pick_primary_background_image": (h_profile_pick_primary_background_image, SCALAR_DEFAULT),
    "profile.classifier.classify_render_page": (h_profile_classify_render_page, SCALAR_DEFAULT),
}

SEED = 20260826
SCHEMA = "retainpdf_diff_corpus_v1"
CORPUS_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "corpus.json"))


def main():
    parser = argparse.ArgumentParser(description="Generate rendering_core differential corpus.")
    parser.add_argument("--cases", type=int, default=SCALAR_DEFAULT, help="case count for scalar handlers")
    parser.add_argument("--item-cases", type=int, default=ITEM_DEFAULT, help="case count for item handlers")
    parser.add_argument("--only", type=str, default=None, help="regenerate a single fn_id")
    parser.add_argument("--seed", type=int, default=SEED, help="random seed")
    parser.add_argument("--out", type=str, default=CORPUS_PATH, help="output JSON path")
    args = parser.parse_args()

    rng = random.Random(args.seed)
    cases = {}
    # Process the Phase 3 profile.* handlers last so they consume fresh RNG
    # after the existing handlers, keeping the pre-existing corpus byte-identical.
    keys = [args.only] if args.only else sorted(REGISTRY, key=lambda k: (k.startswith("profile."), k))
    for fn_id in keys:
        handler, default_n = REGISTRY[fn_id]
        n = args.cases if default_n == SCALAR_DEFAULT else args.item_cases
        cases[fn_id] = handler(rng, n)
        print("generated %-55s %d cases" % (fn_id, len(cases[fn_id])))

    payload = {"schema": SCHEMA, "seed": args.seed, "cases": cases}
    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump(payload, fh, ensure_ascii=False, sort_keys=True, indent=1)
        fh.write("\n")
    total = sum(len(cases[k]) for k in keys)
    print("wrote %d cases (%d fn_ids) -> %s" % (total, len(keys), args.out))


if __name__ == "__main__":
    main()

from __future__ import annotations

from bisect import bisect_right
from dataclasses import dataclass
from typing import Iterable
from typing import Callable

from services.rendering.source.rects import Matrix
from services.rendering.source.rects import Rect
from services.rendering.source.rects import coerce
from services.rendering.source.rects import inverse_affine
from services.rendering.source.rects import rect_area
from services.rendering.source_cleanup.planning.spatial_index import RectOverlapIndex
from services.rendering.source_cleanup.planning.drawing_classifier import bboxlog_path_blocks_text_strip
from services.rendering.source_cleanup.planning.page_context import PlanningPageContext
from services.rendering.source_cleanup.planning.page_context import _build_context_from_fitz


# The coordinate transform takes the inverse page matrix (rotation-stripped),
# not a live page, so the core planning path needs no fitz primitive access.
BBoxTransform = Callable[[Matrix, Rect], Rect]


@dataclass(frozen=True)
class BBoxCoordinateCandidate:
    name: str
    transform: BBoxTransform


@dataclass(frozen=True)
class BBoxCoordinateScore:
    candidate: BBoxCoordinateCandidate
    rect: Rect
    text_overlap_count: int
    text_overlap_area: float


@dataclass(frozen=True)
class TextRectIndex:
    rects: tuple[Rect, ...]
    y0_sorted: tuple[float, ...]

    @classmethod
    def build(cls, rects: Iterable[Rect]) -> "TextRectIndex":
        # Coerce so fitz-built indexes (e.g. pdf_structure_profile sampler) and
        # pure rects interoperate: `text_rect & target` stays pure/pure.
        ordered = tuple(sorted((coerce(rect) for rect in rects), key=lambda rect: rect.y0))
        return cls(rects=ordered, y0_sorted=tuple(float(rect.y0) for rect in ordered))

    def score(self, target_rect: Rect) -> tuple[int, float]:
        target = coerce(target_rect)
        if target.is_empty or not self.rects:
            return 0, 0.0
        count = 0
        area = 0.0
        limit = bisect_right(self.y0_sorted, float(target.y1))
        for index in range(limit):
            text_rect = self.rects[index]
            if text_rect.y1 < target.y0:
                continue
            overlap = rect_area(text_rect & target)
            if overlap <= 0.0:
                continue
            count += 1
            area += overlap
        return count, area

    def overlaps_any(self, target_rects: Iterable[Rect]) -> bool:
        return any(self.score(target_rect)[0] > 0 for target_rect in target_rects)


@dataclass(frozen=True)
class PageBBoxResolver:
    inverse_ctm: Matrix
    page_rect: Rect
    text_rects: tuple[Rect, ...]
    text_index: TextRectIndex
    image_rects: tuple[Rect, ...]
    unsafe_vector_rects: tuple[Rect, ...]
    unsafe_vector_index: RectOverlapIndex
    preferred_candidate: BBoxCoordinateCandidate

    @classmethod
    def build(cls, ctx: PlanningPageContext, bboxes: Iterable[object] = ()) -> "PageBBoxResolver":
        text_rects, image_rects, unsafe_vector_rects = page_bboxlog_rect_groups(ctx.bboxlog_entries)
        text_index = TextRectIndex.build(text_rects)
        return cls(
            inverse_ctm=ctx.inverse_ctm,
            page_rect=ctx.page_rect,
            text_rects=text_rects,
            text_index=text_index,
            image_rects=image_rects,
            unsafe_vector_rects=unsafe_vector_rects,
            unsafe_vector_index=RectOverlapIndex.build(unsafe_vector_rects),
            preferred_candidate=choose_page_coordinate_candidate_with_inverse_ctm(
                ctx.inverse_ctm,
                bboxes,
                text_index,
            ),
        )

    @classmethod
    def build_from_page(cls, page, bboxes: Iterable[object] = ()) -> "PageBBoxResolver":
        return cls.build(_build_context_from_fitz(None, page), bboxes=bboxes)

    def resolve_bbox_rect(self, bbox: object) -> Rect | None:
        raw_rect = raw_bbox_rect(bbox)
        if raw_rect is None:
            return None
        rect = self.preferred_candidate.transform(self.inverse_ctm, raw_rect)
        return None if rect.is_empty else rect

    def resolve_bbox_probe_rects(self, bbox: object) -> tuple[Rect, ...]:
        raw_rect = raw_bbox_rect(bbox)
        if raw_rect is None:
            return ()
        rects: dict[tuple[int, int, int, int], Rect] = {}
        for candidate in BBOX_COORDINATE_CANDIDATES:
            rect = candidate.transform(self.inverse_ctm, raw_rect)
            if not rect.is_empty:
                rects.setdefault(_rect_probe_key(rect), rect)
        return tuple(rects.values())

    def ocr_bbox_to_pdf_rect(self, bbox: object) -> Rect | None:
        raw_rect = raw_bbox_rect(bbox)
        if raw_rect is None:
            return None
        pdf_rect = raw_rect * self.inverse_ctm
        return None if pdf_rect.is_empty else pdf_rect

    def has_large_background_image(self, *, coverage_ratio_threshold: float = 0.75) -> bool:
        if not self.image_rects:
            return False
        page_area = max(rect_area(self.page_rect), 1.0)
        if any(rect_area(rect & self.page_rect) / page_area >= coverage_ratio_threshold for rect in self.image_rects):
            return True
        return page_has_tiled_background_images_from_rects(self.page_rect, self.image_rects)


BBOX_COORDINATE_CANDIDATES: tuple[BBoxCoordinateCandidate, ...] = (
    BBoxCoordinateCandidate(
        name="pdf_matrix",
        transform=lambda inverse_ctm, rect: rect * inverse_ctm,
    ),
    BBoxCoordinateCandidate(
        name="raw_top_left",
        transform=lambda _inverse_ctm, rect: Rect(
            float(rect.x0),
            float(rect.y0),
            float(rect.x1),
            float(rect.y1),
        ),
    ),
)


def choose_page_coordinate_candidate_with_inverse_ctm(
    inverse_ctm: Matrix,
    bboxes: Iterable[object],
    text_index: TextRectIndex,
) -> BBoxCoordinateCandidate:
    raw_rects = tuple(rect for bbox in bboxes if (rect := raw_bbox_rect(bbox)) is not None)
    if not raw_rects:
        return BBOX_COORDINATE_CANDIDATES[0]
    scores = [
        aggregate_candidate_score_with_inverse_ctm(inverse_ctm, candidate, raw_rects, text_index)
        for candidate in BBOX_COORDINATE_CANDIDATES
    ]
    return max(scores, key=lambda score: (score.text_overlap_count, score.text_overlap_area)).candidate


def choose_page_coordinate_candidate(
    page,
    bboxes: Iterable[object],
    text_index: TextRectIndex,
) -> BBoxCoordinateCandidate:
    return choose_page_coordinate_candidate_with_inverse_ctm(
        inverse_affine(page.transformation_matrix),
        bboxes,
        text_index,
    )


def aggregate_candidate_score_with_inverse_ctm(
    inverse_ctm: Matrix,
    candidate: BBoxCoordinateCandidate,
    raw_rects: tuple[Rect, ...],
    text_index: TextRectIndex,
) -> BBoxCoordinateScore:
    count = 0
    area = 0.0
    union_rect = Rect()
    for raw_rect in raw_rects:
        rect = candidate.transform(inverse_ctm, raw_rect)
        union_rect = union_rect | rect
        rect_count, rect_area_sum = text_index.score(rect)
        count += rect_count
        area += rect_area_sum
    return BBoxCoordinateScore(
        candidate=candidate,
        rect=union_rect,
        text_overlap_count=count,
        text_overlap_area=area,
    )


def aggregate_candidate_score(
    page,
    candidate: BBoxCoordinateCandidate,
    raw_rects: tuple[Rect, ...],
    text_index: TextRectIndex,
) -> BBoxCoordinateScore:
    return aggregate_candidate_score_with_inverse_ctm(
        inverse_affine(page.transformation_matrix),
        candidate,
        raw_rects,
        text_index,
    )


def resolve_bbox_rect(page, bbox: object) -> Rect | None:
    raw_rect = raw_bbox_rect(bbox)
    if raw_rect is None:
        return None
    scores = tuple(score_bbox_candidate(page, candidate, raw_rect) for candidate in BBOX_COORDINATE_CANDIDATES)
    best = max(scores, key=lambda score: (score.text_overlap_count, score.text_overlap_area))
    return None if best.rect.is_empty else best.rect


def raw_bbox_rect(bbox: object) -> Rect | None:
    if not isinstance(bbox, list) or len(bbox) != 4:
        return None
    rect = Rect(*(to_float(value) for value in bbox))
    return None if rect.is_empty else rect


def _rect_probe_key(rect: Rect) -> tuple[int, int, int, int]:
    return (
        int(round(rect.x0 * 10)),
        int(round(rect.y0 * 10)),
        int(round(rect.x1 * 10)),
        int(round(rect.y1 * 10)),
    )


def score_bbox_candidate(
    page,
    candidate: BBoxCoordinateCandidate,
    raw_rect: Rect,
) -> BBoxCoordinateScore:
    rect = candidate.transform(inverse_affine(page.transformation_matrix), raw_rect)
    count, area = text_overlap_score(page, rect)
    return BBoxCoordinateScore(
        candidate=candidate,
        rect=rect,
        text_overlap_count=count,
        text_overlap_area=area,
    )


def score_bbox_candidate_with_text_rects(
    page,
    candidate: BBoxCoordinateCandidate,
    raw_rect: Rect,
    text_rects: tuple[Rect, ...],
) -> BBoxCoordinateScore:
    rect = candidate.transform(inverse_affine(page.transformation_matrix), raw_rect)
    count, area = TextRectIndex.build(text_rects).score(rect)
    return BBoxCoordinateScore(
        candidate=candidate,
        rect=rect,
        text_overlap_count=count,
        text_overlap_area=area,
    )


def text_overlap_score(page, target_rect: Rect) -> tuple[int, float]:
    return text_overlap_score_from_rects(target_rect, page_text_rects(page))


def text_overlap_score_from_rects(target_rect: Rect, text_rects: tuple[Rect, ...]) -> tuple[int, float]:
    return TextRectIndex.build(text_rects).score(target_rect)


def page_text_rects(page) -> tuple[Rect, ...]:
    return page_bboxlog_rect_groups_from_page(page)[0]


def page_bboxlog_rect_groups(
    entries: object,
) -> tuple[tuple[Rect, ...], tuple[Rect, ...], tuple[Rect, ...]]:
    rects: list[Rect] = []
    image_rects: list[Rect] = []
    unsafe_vector_rects: list[Rect] = []
    for entry in entries:
        kind = bboxlog_kind(entry)
        rect = bboxlog_rect(entry)
        if rect is None:
            continue
        if "text" in kind:
            rects.append(rect)
            continue
        if "image" in kind:
            image_rects.append(rect)
            continue
        if bboxlog_path_blocks_text_strip(kind, rect):
            unsafe_vector_rects.append(rect)
    return tuple(rects), tuple(image_rects), tuple(unsafe_vector_rects)


def page_bboxlog_rect_groups_from_page(
    page,
) -> tuple[tuple[Rect, ...], tuple[Rect, ...], tuple[Rect, ...]]:
    try:
        entries = page.get_bboxlog()
    except Exception:
        return (), (), ()
    return page_bboxlog_rect_groups(entries)


def bboxlog_text_rect(entry: object) -> Rect | None:
    if "text" not in bboxlog_kind(entry):
        return None
    return bboxlog_rect(entry)


def bboxlog_kind(entry: object) -> str:
    try:
        return str(entry[0]).strip().lower()
    except Exception:
        return ""


def bboxlog_rect(entry: object) -> Rect | None:
    try:
        value = entry[1]
    except Exception:
        return None
    try:
        rect = Rect(*(float(item) for item in value))
    except Exception:
        return None
    return None if rect.is_empty else rect


def page_has_tiled_background_images_from_rects(
    page_rect: Rect,
    image_rects: tuple[Rect, ...],
    *,
    coverage_ratio_threshold: float = 0.65,
    min_image_count: int = 8,
    min_width_ratio: float = 0.60,
) -> bool:
    if len(image_rects) < min_image_count:
        return False
    page_area = max(rect_area(page_rect), 1.0)
    page_width = max(float(page_rect.width), 1.0)
    page_wide_rects = [
        rect & page_rect
        for rect in image_rects
        if not (rect & page_rect).is_empty and (rect & page_rect).width / page_width >= min_width_ratio
    ]
    if len(page_wide_rects) < min_image_count:
        return False
    covered_area = sum(rect_area(rect & page_rect) for rect in _merge_vertical_image_bands(page_wide_rects))
    return covered_area / page_area >= coverage_ratio_threshold


def _merge_vertical_image_bands(rects: list[Rect], *, y_tolerance: float = 1.0) -> list[Rect]:
    merged: list[Rect] = []
    for rect in sorted(rects, key=lambda value: (round(value.y0, 3), round(value.x0, 3))):
        if not merged:
            merged.append(rect)
            continue
        previous = merged[-1]
        if rect.y0 <= previous.y1 + y_tolerance:
            merged[-1] = previous.include_rect(rect)
        else:
            merged.append(rect)
    return merged


def to_float(value: object, default: float = 0.0) -> float:
    try:
        return float(value)
    except Exception:
        return default

from __future__ import annotations

from dataclasses import dataclass

import fitz


RECT_MERGE_GAP_X_PT = 3.0
RECT_MERGE_MAX_VERTICAL_MISALIGN_PT = 6.0
RECT_MERGE_MAX_AREA_GROWTH_RATIO = 2.4
RECT_MERGE_MIN_OVERLAP_RATIO = 0.8

# PyMuPDF's infinite-rect constants; `fitz.Rect.is_infinite` is True exactly
# when x0 == y0 == FZ_MIN_INF_RECT and x1 == y1 == FZ_MAX_INF_RECT.
FZ_MIN_INF_RECT = -2147483648.0
FZ_MAX_INF_RECT = 2147483520.0


@dataclass(frozen=True)
class Rect:
    """Pure-rectangle type duck-compatible with the `fitz.Rect` surface this
    module uses. No PDF dependency; `coerce`/`preserve` bridge to `fitz.Rect`
    for the modules that still consume it. Semantics match fitz exactly:
    `is_empty` = width<=0 or height<=0, `__and__` = max/max/min/min,
    `__or__` = min/min/max/max (no empty special-casing)."""

    x0: float = 0.0
    y0: float = 0.0
    x1: float = 0.0
    y1: float = 0.0

    @property
    def width(self) -> float:
        return float(self.x1) - float(self.x0)

    @property
    def height(self) -> float:
        return float(self.y1) - float(self.y0)

    @property
    def is_empty(self) -> bool:
        return self.width <= 0 or self.height <= 0

    @property
    def is_valid(self) -> bool:
        return self.width > 0 and self.height > 0

    @property
    def is_infinite(self) -> bool:
        """True only for PyMuPDF's exact infinite rect, matching
        `fitz.Rect.is_infinite` (x0 == y0 == FZ_MIN_INF_RECT and
        x1 == y1 == FZ_MAX_INF_RECT)."""
        return self.x0 == self.y0 == FZ_MIN_INF_RECT and self.x1 == self.y1 == FZ_MAX_INF_RECT

    def __and__(self, other) -> Rect:
        r = coerce(other)
        if r.is_empty:
            return r
        if self.is_empty:
            return self
        return Rect(
            max(self.x0, r.x0),
            max(self.y0, r.y0),
            min(self.x1, r.x1),
            min(self.y1, r.y1),
        )

    def __or__(self, other) -> Rect:
        r = coerce(other)
        if r.is_empty:
            return self
        if self.is_empty:
            return r
        return Rect(
            min(self.x0, r.x0),
            min(self.y0, r.y0),
            max(self.x1, r.x1),
            max(self.y1, r.y1),
        )

    def __add__(self, other) -> Rect:
        if isinstance(other, (tuple, list)):
            if len(other) == 4:
                dx0, dy0, dx1, dy1 = other
                return Rect(self.x0 + dx0, self.y0 + dy0, self.x1 + dx1, self.y1 + dy1)
            raise ValueError("Rect: bad seq len")
        r = coerce(other)
        return Rect(self.x0 + r.x0, self.y0 + r.y0, self.x1 + r.x1, self.y1 + r.y1)

    def __sub__(self, other) -> Rect:
        if isinstance(other, (tuple, list)):
            if len(other) == 4:
                dx0, dy0, dx1, dy1 = other
                return Rect(self.x0 - dx0, self.y0 - dy0, self.x1 - dx1, self.y1 - dy1)
            raise ValueError("Rect: bad seq len")
        r = coerce(other)
        return Rect(self.x0 - r.x0, self.y0 - r.y0, self.x1 - r.x1, self.y1 - r.y1)

    def include_rect(self, other) -> Rect:
        r = coerce(other)
        return Rect(
            min(self.x0, r.x0),
            min(self.y0, r.y0),
            max(self.x1, r.x1),
            max(self.y1, r.y1),
        )

    def intersects(self, other) -> bool:
        """Match `fitz.Rect.intersects` exactly: strict coordinate overlap with
        `is_empty`/`is_infinite` guards (NOT `(self & other).is_empty`, which
        disagrees on zero-area sliver intersections)."""
        r = coerce(other)
        return bool(
            not self.is_empty
            and not self.is_infinite
            and not r.is_empty
            and not r.is_infinite
            and self.x0 < r.x1
            and r.x0 < self.x1
            and self.y0 < r.y1
            and r.y0 < self.y1
        )

    def __iter__(self):
        return iter((self.x0, self.y0, self.x1, self.y1))

    def __len__(self) -> int:
        # fitz operators require a `__len__` operand (they reject arbitrary
        # attr objects); exposing sequence-ness lets fitz coerce a pure rect.
        return 4

    def __getitem__(self, index):
        return (self.x0, self.y0, self.x1, self.y1)[index]

    def __mul__(self, other) -> Rect:
        """Affine transform: the bounding box of all four corners under the
        matrix, matching `fitz.Rect.__mul__` exactly (four-corner bbox, no empty
        special-casing). `other` is a `Matrix`, a `fitz.Matrix`, or a 6-sequence."""
        a, b, c, d, e, f = matrix_components(other)
        x0, y0, x1, y1 = self.x0, self.y0, self.x1, self.y1
        xs = [a * x0 + c * y0 + e, a * x1 + c * y0 + e, a * x0 + c * y1 + e, a * x1 + c * y1 + e]
        ys = [b * x0 + d * y0 + f, b * x1 + d * y0 + f, b * x0 + d * y1 + f, b * x1 + d * y1 + f]
        return Rect(min(xs), min(ys), max(xs), max(ys))

    def to_fitz(self):
        return fitz.Rect(float(self.x0), float(self.y0), float(self.x1), float(self.y1))


@dataclass(frozen=True)
class Matrix:
    """Pure 2D affine matrix duck-compatible with the `fitz.Matrix` surface this
    module's consumers read (`a`..`f` attributes) and tuple-iterable. Defaults to
    the identity matrix, matching `fitz.Matrix()`."""

    a: float = 1.0
    b: float = 0.0
    c: float = 0.0
    d: float = 1.0
    e: float = 0.0
    f: float = 0.0

    def __iter__(self):
        return iter((self.a, self.b, self.c, self.d, self.e, self.f))


def matrix_components(value) -> tuple[float, float, float, float, float, float]:
    """Read a 2D affine `(a,b,c,d,e,f)` from a `Matrix`, `fitz.Matrix`, or a
    6-sequence. Wrong-length sequences raise like fitz: `Matrix: bad seq len`."""
    if hasattr(value, "a") and hasattr(value, "b") and hasattr(value, "c") and hasattr(value, "d"):
        return (
            float(value.a),
            float(value.b),
            float(value.c),
            float(value.d),
            float(value.e),
            float(value.f),
        )
    if hasattr(value, "__len__"):
        if len(value) == 6:
            return tuple(float(item) for item in value)
    raise ValueError("Matrix: bad seq len")


def inverse_affine(ctm) -> Matrix:
    """Pure inverse of a 2D affine matrix, matching `~fitz.Matrix` numerically."""
    a, b, c, d, e, f = matrix_components(ctm)
    det = a * d - b * c
    return Matrix(
        d / det,
        -b / det,
        -c / det,
        a / det,
        (c * f - d * e) / det,
        (b * e - a * f) / det,
    )


def transform_rect(rect, matrix) -> Rect:
    """Apply `matrix` to a rect (pure or fitz), returning a pure `Rect`."""
    return coerce(rect) * matrix


def coerce(value) -> Rect:
    """Normalize a pure Rect, fitz.Rect, or 4-sequence to a pure Rect."""
    if isinstance(value, Rect):
        return value
    if hasattr(value, "x0") and hasattr(value, "y0") and hasattr(value, "x1") and hasattr(value, "y1"):
        return Rect(float(value.x0), float(value.y0), float(value.x1), float(value.y1))
    x0, y0, x1, y1 = value
    return Rect(float(x0), float(y0), float(x1), float(y1))


def preserve(reference, rect):
    """Return `rect` in the same type family as `reference`: pure Rect stays
    pure, fitz.Rect comes back as fitz.Rect. Passes None through."""
    if rect is None:
        return None
    if isinstance(reference, Rect):
        return rect
    return fitz.Rect(float(rect.x0), float(rect.y0), float(rect.x1), float(rect.y1))


def preserve_list(reference, rects) -> list:
    if isinstance(reference, Rect):
        return list(rects)
    return [
        fitz.Rect(float(r.x0), float(r.y0), float(r.x1), float(r.y1))
        for r in rects
    ]


def rect_key(rect) -> tuple[int, int, int, int]:
    return (
        int(round(rect.x0 * 10)),
        int(round(rect.y0 * 10)),
        int(round(rect.x1 * 10)),
        int(round(rect.y1 * 10)),
    )


def clip_rect(rect) -> Rect | object:
    return preserve(rect, Rect(rect.x0 - 1, rect.y0 - 1, rect.x1 + 1, rect.y1 + 1))


def rect_area(rect) -> float:
    return max(0.0, float(rect.x1) - float(rect.x0)) * max(0.0, float(rect.y1) - float(rect.y0))


def rects_overlap_area(a, b) -> float:
    inter = coerce(a) & b
    if inter.is_empty:
        return 0.0
    return rect_area(inter)


def rects_should_merge(left, right) -> bool:
    l = coerce(left)
    r = coerce(right)
    union = l | r
    combined_area = rect_area(l) + rect_area(r)
    if combined_area <= 0.0:
        return False
    area_growth_ratio = rect_area(union) / combined_area
    if area_growth_ratio > RECT_MERGE_MAX_AREA_GROWTH_RATIO:
        return False

    same_row = (
        abs(l.y0 - r.y0) <= RECT_MERGE_MAX_VERTICAL_MISALIGN_PT
        and abs(l.y1 - r.y1) <= RECT_MERGE_MAX_VERTICAL_MISALIGN_PT
    )
    inter = l & r
    if not inter.is_empty:
        min_area = max(1.0, min(rect_area(l), rect_area(r)))
        overlap_ratio = rect_area(inter) / min_area
        return same_row or overlap_ratio >= RECT_MERGE_MIN_OVERLAP_RATIO

    horizontal_gap = max(0.0, max(l.x0, r.x0) - min(l.x1, r.x1))
    return bool(same_row and horizontal_gap <= RECT_MERGE_GAP_X_PT)


def merge_rects(rects) -> list:
    reference = rects[0] if rects else None
    pure_rects = [coerce(r) for r in rects]
    merged: list[Rect] = []
    for rect in sorted(pure_rects, key=lambda value: (round(value.y0, 2), round(value.x0, 2), round(value.y1, 2))):
        current = rect
        changed = True
        while changed:
            changed = False
            kept: list[Rect] = []
            for existing in merged:
                if rects_should_merge(existing, current):
                    current |= existing
                    changed = True
                else:
                    kept.append(existing)
            merged = kept
        merged.append(current)
    result = sorted(merged, key=lambda value: (round(value.y0, 2), round(value.x0, 2), round(value.y1, 2)))
    if reference is None:
        return result
    return preserve_list(reference, result)


def rect_intersects_protected(rect, protected_rects) -> bool:
    for protected in protected_rects:
        inter = coerce(rect) & protected
        if not inter.is_empty and rect_area(inter) > 0.5:
            return True
    return False


def normalize_rect(rect):
    normalized = coerce(rect)
    if normalized.is_empty or rect_area(normalized) <= 0.5:
        return None
    return preserve(rect, normalized)


def subtract_one_rect(rect, protected) -> list:
    r = coerce(rect)
    inter = r & protected
    if inter.is_empty or rect_area(inter) <= 0.5:
        return [rect]

    pieces: list[Rect] = []
    if r.x0 < inter.x0:
        pieces.append(Rect(r.x0, r.y0, inter.x0, r.y1))
    if inter.x1 < r.x1:
        pieces.append(Rect(inter.x1, r.y0, r.x1, r.y1))
    if r.y0 < inter.y0:
        pieces.append(Rect(inter.x0, r.y0, inter.x1, inter.y0))
    if inter.y1 < r.y1:
        pieces.append(Rect(inter.x0, inter.y1, inter.x1, r.y1))

    normalized: list[Rect] = []
    for piece in pieces:
        fixed = normalize_rect(piece)
        if fixed is not None:
            normalized.append(fixed)
    return preserve_list(rect, normalized)


def subtract_protected_rects(rects: list[fitz.Rect], protected_rects: list[fitz.Rect]) -> list[fitz.Rect]:
    if not protected_rects:
        return rects

    current = rects
    for protected in protected_rects:
        next_rects: list[fitz.Rect] = []
        for rect in current:
            next_rects.extend(subtract_one_rect(rect, protected))
        current = next_rects
        if not current:
            break
    return current


def merge_dedup_rects(*rect_groups: list[fitz.Rect]) -> list[fitz.Rect]:
    merged: list[fitz.Rect] = []
    seen: set[tuple[int, int, int, int]] = set()
    for group in rect_groups:
        for rect in group:
            key = rect_key(rect)
            if key in seen:
                continue
            seen.add(key)
            merged.append(rect)
    return merged

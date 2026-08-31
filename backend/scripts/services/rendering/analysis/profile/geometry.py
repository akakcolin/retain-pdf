from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class PageGeometryProfile:
    page_index: int
    width_pt: float
    height_pt: float
    rotation: int
    cropbox: tuple[float, float, float, float]

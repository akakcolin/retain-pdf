from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class ImageBackgroundProfile:
    has_large_background: bool
    coverage_ratio: float
    xref: int | None
    bbox: tuple[float, float, float, float] | None

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class VectorLayerProfile:
    drawing_count: int
    vector_heavy: bool
    cover_only_preferred: bool

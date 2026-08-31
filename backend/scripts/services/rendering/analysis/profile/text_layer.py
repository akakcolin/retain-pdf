from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class TextLayerProfile:
    visible_traces: int
    hidden_traces: int
    has_visible_text: bool
    has_hidden_text: bool
    editable: bool

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class PageCleanupFeatures:
    content_stream_size: int = 0
    has_form_xobjects: bool = False

    def to_manifest(self) -> dict[str, Any]:
        return {
            "content_stream_size": int(self.content_stream_size),
            "has_form_xobjects": bool(self.has_form_xobjects),
        }

    @classmethod
    def from_manifest(cls, value: object) -> "PageCleanupFeatures | None":
        payload = dict(value or {})
        if not payload:
            return None
        return cls(
            content_stream_size=_int_or_zero(payload.get("content_stream_size")),
            has_form_xobjects=bool(payload.get("has_form_xobjects")),
        )


def _int_or_zero(value: object) -> int:
    try:
        return max(0, int(value))
    except Exception:
        return 0

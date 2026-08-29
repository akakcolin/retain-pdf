"""Optional native (Rust) backend for the render-policy fields application.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes
`apply_render_pages_policy_fields` / `apply_render_page_policy_fields` (the
C3-N8 boundary, `policy/cleanup_policy.py`) through the Rust port. Without the
native module every call falls back to the pure-Python reference, so importing
this module is always safe.

The two layout-config flags (`use_typst_fill_cleanup` /
`use_default_text_overlay_cover_fill`) are resolved from
`foundation.config.layout` at call time so `layout.apply_layout_tuning` stays
authoritative.
"""

from __future__ import annotations

import json

from services.rendering import _routing

try:
    from rendering_bridge import apply_render_pages_policy_fields as _native_apply_render_pages_policy_fields

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def apply_render_pages_policy_fields(translated_pages: dict[int, list[dict]]) -> dict[int, list[dict]]:
    """The C3-N8 boundary, routed to the native Rust port when built; otherwise
    the pure-Python reference."""
    if not _routing.routed("policy", "apply_render_pages_policy_fields", NATIVE):
        return _apply_render_pages_policy_fields_python(translated_pages)
    from foundation.config import layout

    raw = json.loads(
        _native_apply_render_pages_policy_fields(
            json.dumps(translated_pages),
            layout.use_typst_fill_cleanup(),
            layout.use_default_text_overlay_cover_fill(),
        )
    )
    result: dict[int, list[dict]] = {}
    for page_idx_str, items in raw.items():
        result[int(page_idx_str)] = items
    _routing.record_native_hit("policy", "apply_render_pages_policy_fields")
    return result


def apply_render_page_policy_fields(translated_items: list[dict]) -> list[dict]:
    """Single-page variant of the C3-N8 boundary, routed like the page map."""
    if not _routing.routed("policy", "apply_render_page_policy_fields", NATIVE):
        return _apply_render_page_policy_fields_python(translated_items)
    prepared = apply_render_pages_policy_fields({0: translated_items})
    return prepared.get(0, translated_items)


def _apply_render_pages_policy_fields_python(
    translated_pages: dict[int, list[dict]],
) -> dict[int, list[dict]]:
    from services.rendering.policy.cleanup_policy import _apply_render_pages_policy_fields_python as _reference

    return _reference(translated_pages)


def _apply_render_page_policy_fields_python(translated_items: list[dict]) -> list[dict]:
    from services.rendering.policy.cleanup_policy import _apply_render_page_policy_fields_python as _reference

    return _reference(translated_items)

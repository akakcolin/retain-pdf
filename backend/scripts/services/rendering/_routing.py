"""Unified native-vs-python routing and observability for the rendering shims.

Every `_native.py` shim routes each call through :func:`native_eligible` with
its own module-level ``NATIVE`` constant passed in **at call time**, so the
differential smoke tests that monkeypatch ``shim.NATIVE`` at runtime keep
working. Fallbacks are recorded with a structured reason line plus process-wide
counters (``snapshot`` / ``flush_to``) consumed by the D5 metrics layer.

D1 (双实现清零): production is native-mandatory. Every routed fn whose Python
dual is retained must be documented in :data:`ALLOWLIST` with a reason, or
:func:`routed` raises :class:`NativeMandatoryError` when the native path is
unavailable (gate ``RETAIN_PDF_NATIVE_MANDATE``, default on). The
``IN_MEMORY_PAGE`` capability boundary is never blocked, and
``NATIVE_BRIDGE_ERROR`` recovery never passes through :func:`routed`.

This module is stdlib-only and imports neither the shims nor ``contracts/`` to
avoid circular imports.
"""

from __future__ import annotations

import enum
import json
import os
from pathlib import Path


class FallbackReason(str, enum.Enum):
    NATIVE_NOT_BUILT = "native_not_built"
    NATIVE_BRIDGE_ERROR = "native_bridge_error"
    IN_MEMORY_PAGE = "in_memory_page"
    STAGE_INSTRUMENTED = "stage_instrumented"
    DELIBERATELY_NOT_ROUTED = "deliberately_not_routed"
    FORCED_OFF = "forced_off"
    UNREADABLE_PAGE = "unreadable_page"


SUBSYSTEMS = (
    "source",
    "background",
    "typst",
    "layout",
    "layout_payload",
    "policy",
    "visual_profile",
    "source_cleanup_planning",
    "pdf_structure_profile",
    "analysis",
)


class NativeMandatoryError(RuntimeError):
    """D1 violation: a routed fn fell back to Python without an allowlist entry.

    Add the ``(subsystem, fn)`` to :data:`ALLOWLIST` with a reason, or remove
    the ``routed()`` call. Raised only when the mandate is enabled (default).
    """


#: Retained Python dual-implementations, keyed by ``(subsystem, fn)``. Reasons:
#:
#: - ``parity_reference`` — the Python side is the differential parity harness
#:   reference (bridges re-run it with ``NATIVE=False`` for byte-exact diffing);
#:   must survive until the harness migrates.
#: - ``hard_boundary`` — no Rust equivalent / native diverges (e.g.
#:   ``collect_page_drawing_rects`` changes redaction output on stroked zigzag
#:   paths); Python is deliberately the only implementation.
#:
#: The D1 gate asserts this list matches the routed call sites in both
#: directions: no routed fn may be missing, and no entry may be stale.
ALLOWLIST: dict[tuple[str, str], str] = {
    ("source", "sanitize_pdf_copy"): "parity_reference",
    ("source", "compress_images_only"): "parity_reference",
    ("source", "extract_pages"): "parity_reference",
    ("source", "save_optimized"): "parity_reference",
    ("source", "read_page_sizes_and_count"): "parity_reference",
    ("source", "collect_vector_text_rects"): "parity_reference",
    ("source", "page_drawing_count"): "parity_reference",
    ("source", "page_has_large_background_image"): "parity_reference",
    ("source", "extract_page_text_spans"): "parity_reference",
    ("source", "extract_page_text_blocks"): "parity_reference",
    ("source", "collect_page_math_protection_rects"): "parity_reference",
    ("source", "collect_page_non_math_span_heights"): "parity_reference",
    ("source", "copy_toc"): "parity_reference",
    ("source", "copy_toc_for_page_map"): "parity_reference",
    ("source", "build_hidden_text_stripped_pdf_copy"): "parity_reference",
    ("source", "collect_page_drawing_rects"): "hard_boundary",
    ("background", "build_clean_background_pdf"): "parity_reference",
    ("background", "sample_page_color_fills"): "parity_reference",
    ("background", "extract_page_span_dicts"): "parity_reference",
    ("background", "sample_title_visual_colors"): "parity_reference",
    ("background", "sample_foreground_colors"): "parity_reference",
    ("typst", "emit_typst_source"): "parity_reference",
    ("typst", "emit_typst_book_overlay_source"): "parity_reference",
    ("typst", "apply_adaptive_overlay_colors_batch"): "parity_reference",
    ("typst", "show_pdf_page"): "parity_reference",
    ("typst", "build_dual_doc_pages"): "parity_reference",
    ("layout", "read_source_page_sizes"): "parity_reference",
    ("layout_payload", "detect_first_line_indents"): "parity_reference",
    ("layout_payload", "build_block_payloads"): "parity_reference",
    ("layout_payload", "emit_render_blocks"): "parity_reference",
    ("layout_payload", "apply_body_pipeline"): "parity_reference",
    ("layout_payload", "mark_adjacent_collision_risk"): "parity_reference",
    ("layout_payload", "prepare_render_payloads_by_page"): "parity_reference",
    ("layout_payload", "seed_render_fields"): "parity_reference",
    ("policy", "apply_render_pages_policy_fields"): "parity_reference",
    ("policy", "apply_render_page_policy_fields"): "parity_reference",
    ("layout_payload", "resolve_book_body_font_target"): "parity_reference",
    ("visual_profile", "build_document_visual_profile"): "parity_reference",
    ("pdf_structure_profile", "build_pdf_structure_profile"): "parity_reference",
    ("analysis", "build_render_document_analysis"): "parity_reference",
    ("analysis", "classify_render_page"): "parity_reference",
    ("source_cleanup_planning", "build_page_contexts"): "parity_reference",
    ("source_cleanup_planning", "plan_source_cleanup"): "parity_reference",
    ("source_cleanup_planning", "item_ids_with_uncovered_unsafe_vector_overlap"): "parity_reference",
}

_GLOBAL_FLAG_ENV = "RETAIN_PDF_NATIVE"
_SUBSYSTEM_FLAG_PREFIX = "RETAIN_PDF_NATIVE_"
_MANDATE_FLAG_ENV = "RETAIN_PDF_NATIVE_MANDATE"

#: (subsystem, value) of already-resolved env-derived flags; the module-native
#: constant is NEVER cached here (callers pass it in), so smoke-test
#: monkeypatches of `shim.NATIVE` stay effective.
_FLAG_CACHE: dict[tuple[str | None, str], bool] = {}

_hits: dict[str, int] = {}
_fallbacks: dict[tuple[str, str, str], int] = {}
_hit_detail: dict[tuple[str, str], int] = {}


def _env_flag(subsystem: str | None) -> bool:
    global_raw = os.getenv(_GLOBAL_FLAG_ENV, "")
    subsystem_raw = (
        os.environ.get(f"{_SUBSYSTEM_FLAG_PREFIX}{subsystem.upper()}", "")
        if subsystem is not None
        else ""
    )
    key = (subsystem, global_raw, subsystem_raw)
    if key in _FLAG_CACHE:
        return _FLAG_CACHE[key]
    raw = subsystem_raw if subsystem_raw else global_raw
    value = _env_truthy(raw)
    _FLAG_CACHE[key] = value
    return value


def _env_truthy(raw: str) -> bool:
    """Env flag is truthy unless explicitly `0/false/off/no` (default ON)."""
    return raw.strip().lower() not in {"0", "false", "off", "no"}


def native_enabled(subsystem: str) -> bool:
    """Whether the native path is enabled by feature flags for `subsystem`.

    Only reflects env config — the caller still ANDs this with its own
    import-time ``NATIVE`` constant via :func:`native_eligible`.
    """
    return _env_flag(subsystem)


def native_eligible(subsystem: str, module_native: bool) -> bool:
    """Routing decision: module imported the bridge AND flags allow it.

    ``module_native`` is the shim's module-level ``NATIVE`` read by the caller
    at call time; never inspect it here or cache it.
    """
    return bool(module_native) and _env_flag(subsystem)


def _mandate_enabled() -> bool:
    """D1 mandate is on unless ``RETAIN_PDF_NATIVE_MANDATE`` is explicitly off."""
    return _env_truthy(os.getenv(_MANDATE_FLAG_ENV, ""))


def _enforce_mandate(subsystem: str, fn: str, reason: FallbackReason) -> None:
    """Raise when an undocumented Python fallback would run under the mandate.

    Allowlisted fns and the ``IN_MEMORY_PAGE`` capability boundary always pass;
    ``NATIVE_BRIDGE_ERROR`` recovery never routes through here (callers record
    it directly in their ``except`` blocks).
    """
    if not _mandate_enabled():
        return
    if (subsystem, fn) in ALLOWLIST or reason is FallbackReason.IN_MEMORY_PAGE:
        return
    raise NativeMandatoryError(
        f"native-mandatory fallback for un-allowlisted ({subsystem}, {fn}): "
        f"reason={reason.value}. Add it to _routing.ALLOWLIST with a reason or "
        "remove the routed() call."
    )


def _fallback_reason_for_route(subsystem: str, module_native: bool) -> FallbackReason:
    """Why the native path is unavailable (routed() already proved ineligible).

    The bridge missing at import is ``NATIVE_NOT_BUILT``; the bridge present but
    the feature flag force-disabling it is ``FORCED_OFF`` — the operator escape
    hatch, not an absence.
    """
    if not module_native:
        return FallbackReason.NATIVE_NOT_BUILT
    return FallbackReason.FORCED_OFF


def routed(
    subsystem: str,
    fn: str,
    module_native: bool,
    *,
    path: str | None = None,
) -> bool:
    """One-shot routing gate used by the shims: True when the native path should
    run for ``fn``, recording the fallback reason otherwise.

    Pass ``path`` (the file path backing a ``fitz.Page``, "" for in-memory) to
    route file-backed primitives; when a non-None empty path is given the
    ``IN_MEMORY_PAGE`` reason is recorded (never blocked by the mandate).

    The fallback reason distinguishes the bridge missing (``NATIVE_NOT_BUILT``)
    from the feature flag explicitly off (``FORCED_OFF``). Under the D1 mandate
    a non-allowlisted fn that falls back raises :class:`NativeMandatoryError`
    instead of silently running the Python dual.
    """
    if not native_eligible(subsystem, module_native):
        reason = _fallback_reason_for_route(subsystem, module_native)
        _enforce_mandate(subsystem, fn, reason)
        record_fallback(subsystem, fn, reason)
        return False
    if path is not None and not path:
        record_fallback(subsystem, fn, FallbackReason.IN_MEMORY_PAGE)
        return False
    return True


def record_native_hit(subsystem: str, fn: str, **detail: object) -> None:
    _hits[subsystem] = _hits.get(subsystem, 0) + 1
    if detail:
        detail_key = (subsystem, fn)
        _hit_detail[detail_key] = _hit_detail.get(detail_key, 0) + 1


def record_fallback(
    subsystem: str,
    fn: str,
    reason: FallbackReason | str,
    **detail: object,
) -> None:
    reason_value = reason.value if isinstance(reason, FallbackReason) else str(reason)
    key = (subsystem, fn, reason_value)
    _fallbacks[key] = _fallbacks.get(key, 0) + 1
    parts = [
        "[routing] fallback",
        f"subsystem={subsystem}",
        f"fn={fn}",
        f"reason={reason_value}",
    ]
    for name, value in detail.items():
        parts.append(f"{name}={value}")
    print(" ".join(parts), flush=True)


def snapshot() -> dict:
    """Process-wide routing stats: hits/fallbacks per subsystem and reason."""
    hits_by_subsystem: dict[str, int] = {}
    fallbacks_by_subsystem: dict[str, dict[str, int]] = {}
    fallbacks_by_reason: dict[str, int] = {}
    total_fallbacks = 0
    for (subsystem, _fn, reason), count in _fallbacks.items():
        fallbacks_by_subsystem.setdefault(subsystem, {})
        fallbacks_by_subsystem[subsystem][reason] = (
            fallbacks_by_subsystem[subsystem].get(reason, 0) + count
        )
        fallbacks_by_reason[reason] = fallbacks_by_reason.get(reason, 0) + count
        total_fallbacks += count
    for subsystem, count in _hits.items():
        hits_by_subsystem[subsystem] = hits_by_subsystem.get(subsystem, 0) + count
    return {
        "hits": hits_by_subsystem,
        "fallbacks": fallbacks_by_subsystem,
        "fallbacks_by_reason": fallbacks_by_reason,
        "total_fallbacks": total_fallbacks,
        "total_hits": sum(hits_by_subsystem.values()),
    }


def reset() -> None:
    _hits.clear()
    _fallbacks.clear()
    _hit_detail.clear()


def flush_to(artifacts_dir: Path | str | None) -> Path | None:
    """Write ``native_stats.json`` under the job artifacts dir (D5 hook).

    Safe no-op when the dir is absent so callers can flush unconditionally.
    """
    if artifacts_dir is None:
        return None
    directory = Path(artifacts_dir)
    if not directory.is_dir():
        return None
    target = directory / "native_stats.json"
    target.write_text(json.dumps(snapshot(), indent=2, sort_keys=True), encoding="utf-8")
    return target

"""Unified native-vs-python routing and observability for the rendering shims.

Every `_native.py` shim routes each call through :func:`native_eligible` with
its own module-level ``NATIVE`` constant passed in **at call time**, so the
differential smoke tests that monkeypatch ``shim.NATIVE`` at runtime keep
working. Fallbacks are recorded with a structured reason line plus process-wide
counters (``snapshot`` / ``flush_to``) consumed by the D5 metrics layer.

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
    STRATEGY_NOT_PORTED = "strategy_not_ported"
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
    "visual_profile",
    "source_cleanup_planning",
    "pdf_structure_profile",
    "analysis",
)

_GLOBAL_FLAG_ENV = "RETAIN_PDF_NATIVE"
_SUBSYSTEM_FLAG_PREFIX = "RETAIN_PDF_NATIVE_"

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
    ``IN_MEMORY_PAGE`` reason is recorded.
    """
    if not native_eligible(subsystem, module_native):
        record_fallback(subsystem, fn, FallbackReason.NATIVE_NOT_BUILT)
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

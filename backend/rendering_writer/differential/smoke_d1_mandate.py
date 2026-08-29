#!/usr/bin/env python3
"""D1 gate: native-mandatory governance for the wired rendering subsystems.

终态判据 4 (doc 15): production is native-mandatory — every routed call that
falls back to a Python dual must be documented in `_routing.ALLOWLIST` with a
reason, or `routed()` raises `NativeMandatoryError`. The static scan proves the
allowlist matches reality in both directions (no undocumented routed fn, no
stale allowlist entry); the runtime checks prove the mandate fires for unlisted
fns, permits allowlisted ones and the IN_MEMORY_PAGE capability boundary, and
honors the `RETAIN_PDF_NATIVE_MANDATE=0` off-valve.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python \
        ../rendering_writer/differential/smoke_d1_mandate.py
"""

import os
import re
import sys
from pathlib import Path

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering import _routing  # noqa: E402

_SERVICES_DIR = Path(_SCRIPTS_DIR) / "services" / "rendering"

# Matches `routed("subsystem", "fn", ...)` and `record_fallback("subsystem",
# "fn", ...)` call sites; \s spans newlines for multi-line call forms.
_CALL_RE = re.compile(r'(?:routed|record_fallback)\(\s*"(\w+)"\s*,\s*"(\w+)"')


def _scan_routed_fns() -> set[tuple[str, str]]:
    found: set[tuple[str, str]] = set()
    for path in _SERVICES_DIR.rglob("*.py"):
        text = path.read_text(encoding="utf-8")
        for subsystem, fn in _CALL_RE.findall(text):
            found.add((subsystem, fn))
    return found


def _check_allowlist_matches() -> None:
    found = _scan_routed_fns()
    unlisted = found - set(_routing.ALLOWLIST)
    stale = set(_routing.ALLOWLIST) - found
    assert not unlisted, f"routed fns missing from ALLOWLIST: {sorted(unlisted)}"
    assert not stale, f"ALLOWLIST entries with no routed call: {sorted(stale)}"
    print(
        f"allowlist matches: {len(_routing.ALLOWLIST)} entries, "
        f"{len(found)} call sites across {_SERVICES_DIR}"
    )


def _check_mandate_runtime() -> None:
    probe = ("source", "_d1_mandate_probe_unlisted")
    try:
        _routing.routed(*probe, False)
    except _routing.NativeMandatoryError:
        pass
    else:
        raise AssertionError("mandate did not raise for un-allowlisted fn")

    assert (
        _routing.routed("source", "save_optimized", False) is False
    ), "allowlisted fallback raised under mandate"
    assert (
        _routing.routed("source", "save_optimized", True) is True
    ), "native-eligible call routed False"
    assert (
        _routing.routed("source", "_d1_mandate_probe_unlisted", True, path="") is False
    ), "IN_MEMORY_PAGE blocked under mandate"

    os.environ["RETAIN_PDF_NATIVE_MANDATE"] = "0"
    try:
        assert _routing.routed(*probe, False) is False, "off-valve still raised"
    finally:
        del os.environ["RETAIN_PDF_NATIVE_MANDATE"]
    print(
        "mandate runtime: raises unlisted, permits allowlisted + IN_MEMORY_PAGE, "
        "off-valve works"
    )


def check_d1_mandate() -> None:
    _check_allowlist_matches()
    _check_mandate_runtime()
    print("all D1 mandate gates pass")


if __name__ == "__main__":
    check_d1_mandate()

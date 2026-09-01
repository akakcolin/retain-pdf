"""Console-script adapters for the RetainPDF Python worker entrypoints."""

from __future__ import annotations

from collections.abc import Callable

from foundation.shared.structured_errors import run_with_structured_failure


def _run_structured(
    main_fn: Callable[[], object],
    *,
    default_stage: str,
    provider: str,
) -> int:
    run_with_structured_failure(main_fn, default_stage=default_stage, provider=provider)
    return 0


def run_translate_only() -> int:
    from services.translation.entrypoints.translate_only_pipeline import main

    return _run_structured(main, default_stage="translation", provider="translation")

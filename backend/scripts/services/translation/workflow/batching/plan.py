from __future__ import annotations

from services.translation.workflow.batching.batching import build_translation_batches as _build_translation_batches_impl
from services.translation.workflow.batching.batching import classify_translation_batches
from services.translation.workflow.batching.batching import effective_translation_batch_size as _effective_translation_batch_size_impl
from services.translation.workflow.batching.batching import save_flush_interval
from services.translation.workflow.batching.batching import chunked
from services.translation.workflow.batching.dedupe import dedupe_pending_items
from services.translation.workflow.batching.dedupe import dedupe_signature
from services.translation.services.fast_path.keep_origin import fast_path_keep_origin_result
from services.translation.services.fast_path.keep_origin import is_fast_path_keep_origin_item
from services.translation.services.fast_path.keep_origin import normalized_text_without_placeholders
from services.translation.services.fast_path.keep_origin import plan_item_view
from services.translation.workflow.scheduling.allocation import adaptive_floor_limit
from services.translation.workflow.scheduling.allocation import adaptive_initial_limit
from services.translation.workflow.scheduling.allocation import allocate_translation_queue_workers
from services.translation.workflow.scheduling.allocation import provider_adaptive_initial_limit
from services.translation.workflow.scheduling.allocation import slow_worker_cap
from services.translation.workflow.scheduling.stats import TranslationBatchRunStats


def build_translation_batches(
    pending: list[dict],
    *,
    effective_batch_size: int,
    translation_context,
) -> tuple[list[list[dict]], list[dict[str, dict[str, str]]]]:
    return _build_translation_batches_impl(
        pending,
        effective_batch_size=effective_batch_size,
        translation_context=translation_context,
        is_fast_path_keep_origin_item_fn=is_fast_path_keep_origin_item,
        fast_path_keep_origin_result_fn=fast_path_keep_origin_result,
        plan_item_view_fn=plan_item_view,
    )


def effective_translation_batch_size(
    *,
    batch_size: int,
    model: str,
    base_url: str,
    translation_context,
) -> int:
    return _effective_translation_batch_size_impl(
        batch_size=batch_size,
        model=model,
        base_url=base_url,
        translation_context=translation_context,
    )


__all__ = [
    "chunked",
    "TranslationBatchRunStats",
    "adaptive_floor_limit",
    "adaptive_initial_limit",
    "provider_adaptive_initial_limit",
    "allocate_translation_queue_workers",
    "build_translation_batches",
    "classify_translation_batches",
    "dedupe_pending_items",
    "dedupe_signature",
    "effective_translation_batch_size",
    "fast_path_keep_origin_result",
    "is_fast_path_keep_origin_item",
    "normalized_text_without_placeholders",
    "plan_item_view",
    "save_flush_interval",
    "slow_worker_cap",
]

from __future__ import annotations

"""Compatibility exports for translation scheduling worker allocation."""

from services.translation.workflow.scheduling.allocation import adaptive_floor_limit
from services.translation.workflow.scheduling.allocation import adaptive_initial_limit
from services.translation.workflow.scheduling.allocation import allocate_translation_queue_workers
from services.translation.workflow.scheduling.allocation import distribute_extra_workers
from services.translation.workflow.scheduling.allocation import empty_worker_allocation
from services.translation.workflow.scheduling.allocation import fast_queue_targets
from services.translation.workflow.scheduling.allocation import single_worker_allocation
from services.translation.workflow.scheduling.allocation import slow_worker_cap
from services.translation.workflow.scheduling.allocation import weighted_fast_queue_targets
from services.translation.workflow.scheduling.stats import TranslationBatchRunStats

__all__ = [
    "TranslationBatchRunStats",
    "adaptive_floor_limit",
    "adaptive_initial_limit",
    "allocate_translation_queue_workers",
    "distribute_extra_workers",
    "empty_worker_allocation",
    "fast_queue_targets",
    "weighted_fast_queue_targets",
    "single_worker_allocation",
    "slow_worker_cap",
]

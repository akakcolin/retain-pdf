from __future__ import annotations

from services.translation.services.quality import TranslationQualityIssue
from services.translation.services.quality import TranslationQualityReport
from services.translation.services.quality import review_translation_batch
from services.translation.services.quality import review_translation_item
from services.translation.services.terms import GlossaryEntry
from services.translation.services.terms import normalize_glossary_entries


TranslationReviewIssue = TranslationQualityIssue
TranslationReviewResult = TranslationQualityReport


class ConsistencyReviewerAgent:
    name = "consistency_reviewer"

    def __init__(
        self,
        glossary_entries: list[GlossaryEntry | dict] | None = None,
        *,
        target_lang: str | None = None,
    ):
        self._glossary_entries = normalize_glossary_entries(glossary_entries)
        self._target_lang = target_lang

    def review_batch(
        self,
        batch: list[dict],
        result: dict[str, dict[str, str]],
    ) -> TranslationReviewResult:
        return review_translation_batch(
            batch,
            result,
            glossary_entries=self._glossary_entries,
            target_lang=self._target_lang,
        )

    def review_item(self, item: dict, translated_result: dict[str, str]) -> TranslationReviewResult:
        return review_translation_item(
            item,
            translated_result,
            glossary_entries=self._glossary_entries,
            target_lang=self._target_lang,
        )


__all__ = [
    "ConsistencyReviewerAgent",
    "TranslationReviewIssue",
    "TranslationReviewResult",
]

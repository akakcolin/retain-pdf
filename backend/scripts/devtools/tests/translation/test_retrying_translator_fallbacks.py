import importlib.util
import json
import sys
import types
import unittest
from dataclasses import replace
from pathlib import Path


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))

from services.translation.llm.result_payload import result_entry
from services.translation.llm.shared.orchestration.heavy_formula import heavy_formula_split_reason
from services.translation.llm.shared.orchestration.heavy_formula import translate_heavy_formula_block
from services.translation.llm.shared.orchestration.metadata import should_store_translation_result
from services.translation.llm.shared.orchestration.sentence_level import sentence_level_fallback
from services.translation.llm.shared.orchestration.transport import DeferredTransportRetry


def load_retrying_translator():
    sys.path.insert(0, str(REPO_SCRIPTS_ROOT))
    package_paths = {
        "services": REPO_SCRIPTS_ROOT / "services",
        "services.translation": REPO_SCRIPTS_ROOT / "services" / "translation",
        "services.translation.llm": REPO_SCRIPTS_ROOT / "services" / "translation" / "llm",
        "services.translation.llm.shared": REPO_SCRIPTS_ROOT / "services" / "translation" / "llm" / "shared",
        "services.translation.llm.shared.orchestration": REPO_SCRIPTS_ROOT / "services" / "translation" / "llm" / "shared" / "orchestration",
        "services.translation.llm.providers": REPO_SCRIPTS_ROOT / "services" / "translation" / "llm" / "providers",
        "services.translation.llm.providers.deepseek": REPO_SCRIPTS_ROOT / "services" / "translation" / "llm" / "providers" / "deepseek",
        "services.translation.services.policy": REPO_SCRIPTS_ROOT / "services" / "translation" / "policy",
        "services.document_schema": REPO_SCRIPTS_ROOT / "services" / "document_schema",
    }
    for name, path in package_paths.items():
        module = sys.modules.get(name)
        if module is None:
            module = types.ModuleType(name)
            module.__path__ = [str(path)]
            sys.modules[name] = module

    for module_name in (
        "services.translation.llm.shared.orchestration.retrying_translator",
        "services.translation.llm.shared.orchestration.fallbacks",
        "services.translation.llm.shared.orchestration.segment_routing",
        "services.translation.llm.providers.deepseek.client",
    ):
        sys.modules.pop(module_name, None)

    spec = importlib.util.spec_from_file_location(
        "services.translation.llm.shared.orchestration.retrying_translator",
        REPO_SCRIPTS_ROOT / "services" / "translation" / "llm" / "shared" / "orchestration" / "retrying_translator.py",
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def make_formula_item(formula_count: int) -> dict:
    parts = []
    for index in range(1, formula_count + 1):
        parts.append(f"clause {index} explaining the result")
        parts.append(f"[[FORMULA_{index}]]")
    parts.append("final discussion sentence")
    source = " ".join(parts)
    return {
        "item_id": f"formula-{formula_count}",
        "block_type": "text",
        "protected_source_text": source,
        "translation_unit_protected_source_text": source,
        "metadata": {"structure_role": "body"},
        "formula_map": {"dummy": "dummy"},
    }


def make_small_formula_inline_item() -> dict:
    source = (
        "The work function <f1-6a9/> which is also abbreviated as <f2-ef6/> "
        "of a catalyst can be defined as the minimum energy required to extract one electron."
    )
    return {
        "item_id": "small-inline-1",
        "block_type": "text",
        "protected_source_text": source,
        "translation_unit_protected_source_text": source,
        "metadata": {"structure_role": "body"},
        "formula_map": {"dummy": "dummy"},
    }


def make_fragmented_formula_item(formula_count: int = 5) -> dict:
    parts = []
    for index in range(1, formula_count + 1):
        parts.append(f"the catalyst <f{index}-a7c/> and")
    parts.append("shows stable activity in experiments.")
    source = " ".join(parts)
    return {
        "item_id": f"fragmented-{formula_count}",
        "block_type": "text",
        "protected_source_text": source,
        "translation_unit_protected_source_text": source,
        "metadata": {"structure_role": "body"},
        "formula_map": {"dummy": "dummy"},
    }


def make_prose_heavy_formula_item() -> dict:
    source = (
        "This discussion explains the catalytic pathway in prose and compares several prior studies while keeping "
        "only a few inline markers such as <f1-a7c/>, <f2-b2d/>, and <f3-c3e/> for notation. "
        "The surrounding paragraph remains long, narrative, and context heavy so the model should usually translate "
        "it as a normal body block instead of entering segmented formula mode."
    )
    return {
        "item_id": "prose-heavy-formula-1",
        "block_type": "text",
        "protected_source_text": source,
        "translation_unit_protected_source_text": source,
        "metadata": {"structure_role": "body"},
        "formula_map": {"dummy": "dummy"},
    }


def make_formula_dense_prose_item() -> dict:
    source = (
        "For the diffusion process, the transition matrix <f1-a11/> governs how tokens evolve in each step, "
        "while the marginal probability <f2-b22/> controls the expected corruption level. "
        "The hidden state <f3-c33/> is then related to the masking distribution <f4-d44/>, "
        "and the posterior estimator <f5-e55/> is combined with the score term <f6-f66/> to stabilize training. "
        "Although these markers appear frequently, the paragraph is still ordinary explanatory prose rather than a pure formula block."
    )
    return {
        "item_id": "formula-dense-prose-1",
        "block_type": "text",
        "protected_source_text": source,
        "translation_unit_protected_source_text": source,
        "metadata": {"structure_role": "body"},
        "formula_map": {"dummy": "dummy"},
    }

class RetryingTranslatorFallbacksTests(unittest.TestCase):
    def test_plain_text_retry_uses_raw_single_item_fallback_after_repeated_empty_translation(self):
        load_retrying_translator()
        import services.translation.llm.shared.orchestration.fallbacks as fallbacks
        from services.translation.llm.shared.control_context import build_translation_control_context

        item = {
            "item_id": "body-1",
            "block_type": "text",
            "protected_source_text": "This paragraph contains enough English prose to require translation into Chinese for the user.",
            "translation_unit_protected_source_text": "This paragraph contains enough English prose to require translation into Chinese for the user.",
            "metadata": {"structure_role": "body"},
        }
        calls: list[str] = []

        def fake_plain(*args, **kwargs):
            calls.append("structured")
            raise fallbacks.EmptyTranslationError(item["item_id"])

        def fake_raw(*args, **kwargs):
            calls.append("raw")
            return {item["item_id"]: result_entry("translate", "这段英文正文已经通过原始纯文本回退成功翻译。")}

        original_plain = fallbacks.translate_single_item_plain_text
        original_raw = fallbacks.translate_single_item_plain_text_unstructured
        try:
            fallbacks.translate_single_item_plain_text = fake_plain
            fallbacks.translate_single_item_plain_text_unstructured = fake_raw
            result = fallbacks.translate_single_item_plain_text_with_retries(
                item,
                request_label="unit",
                context=build_translation_control_context(),
                diagnostics=None,
            )
        finally:
            fallbacks.translate_single_item_plain_text = original_plain
            fallbacks.translate_single_item_plain_text_unstructured = original_raw

        self.assertEqual(calls[-1], "raw")
        self.assertEqual(result[item["item_id"]]["translated_text"], "这段英文正文已经通过原始纯文本回退成功翻译。")

    def test_sentence_fallback_chunks_long_group_when_no_sentence_split_exists(self):
        load_retrying_translator()
        from services.translation.llm.shared.control_context import build_translation_control_context
        item = {
            "item_id": "group-1",
            "block_type": "text",
            "metadata": {"structure_role": "body"},
            "continuation_group": "cg-001-001",
            "translation_unit_protected_source_text": " ".join(["word"] * 120),
            "protected_source_text": " ".join(["word"] * 120),
        }
        seen = []

        def fake_plain(*args, **kwargs):
            sentence_item = args[0]
            seen.append(sentence_item["translation_unit_protected_source_text"])
            # 合并后的整块结果会过 truncation 校验（源 ≥200 字符时 ratio<0.15 判为截断），
            # 固定返回短文本会被 TruncatedTranslationError 拒绝，故按块长成比例返回。
            chunk_source = str(sentence_item.get("translation_unit_protected_source_text") or "")
            translated = "已翻译片段" * max(1, len(chunk_source) // 8)
            return {item["item_id"]: result_entry("translate", translated)}

        result = sentence_level_fallback(
            item,
            api_key="",
            model="deepseek-chat",
            base_url="https://api.deepseek.com/v1",
            request_label="unit",
            context=build_translation_control_context(mode="sci"),
            diagnostics=None,
            translate_plain_fn=fake_plain,
        )

        self.assertGreaterEqual(len(seen), 2)
        self.assertEqual(result[item["item_id"]]["final_status"], "partially_translated")

    def test_sentence_fallback_rejects_merged_partial_output_with_long_english_residue(self):
        module = load_retrying_translator()
        import services.translation.llm.shared.orchestration.fallbacks as fallbacks
        from services.translation.llm.shared.control_context import build_translation_control_context

        item = {
            "item_id": "p009-b018",
            "block_type": "text",
            "metadata": {"structure_role": "body"},
            "translation_unit_protected_source_text": (
                "Electrochemical oxidations of nickel(II)(aryl)halide complexes result in irreversible oxidation waves "
                "ranging from <f1-2ff/> to <f2-05f/> (vs SCE). Perhaps more informative is looking at the study."
            ),
            "protected_source_text": (
                "Electrochemical oxidations of nickel(II)(aryl)halide complexes result in irreversible oxidation waves "
                "ranging from <f1-2ff/> to <f2-05f/> (vs SCE). Perhaps more informative is looking at the study."
            ),
            "formula_map": [{"placeholder": "<f1-2ff/>"}, {"placeholder": "<f2-05f/>"}],
            "translation_unit_formula_map": [{"placeholder": "<f1-2ff/>"}, {"placeholder": "<f2-05f/>"}],
        }

        seen = {"count": 0}

        def fake_plain(sentence_item, *args, **kwargs):
            seen["count"] += 1
            if seen["count"] == 1:
                raise fallbacks.EnglishResidueError(item["item_id"])
            return {item["item_id"]: result_entry("translate", "或许更具信息量的是查看该研究。")}

        def fake_raw(sentence_item, *args, **kwargs):
            return {
                item["item_id"]: result_entry(
                    "translate",
                    "Electrochemical oxidations of nickel(II)(aryl)halide complexes result in irreversible oxidation waves ranging from <f1-2ff/> to <f2-05f/> (vs SCE).",
                )
            }

        original_plain = fallbacks.translate_single_item_plain_text
        original_raw = fallbacks.translate_single_item_plain_text_unstructured
        try:
            fallbacks.translate_single_item_plain_text = fake_plain
            fallbacks.translate_single_item_plain_text_unstructured = fake_raw
            with self.assertRaises(fallbacks.EnglishResidueError):
                sentence_level_fallback(
                    item,
                    api_key="",
                    model="deepseek-chat",
                    base_url="https://api.deepseek.com/v1",
                    request_label="unit",
                    context=build_translation_control_context(mode="sci"),
                    diagnostics=None,
                    translate_plain_fn=fake_plain,
                    translate_unstructured_fn=fake_raw,
                )
        finally:
            fallbacks.translate_single_item_plain_text = original_plain
            fallbacks.translate_single_item_plain_text_unstructured = original_raw


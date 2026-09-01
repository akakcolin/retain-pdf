from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))


from services.translation.llm import placeholder_guard
from services.translation.llm.providers.deepseek import client as deepseek_client
from services.translation.llm.shared import structured_models
from services.translation.llm.shared import structured_output
from services.translation.llm.shared import structured_parsers
from services.translation.llm.shared.orchestration import segment_routing
from services.translation.llm.shared.response_parsing import strip_prompt_echo_markers
from services.translation.llm.shared.response_parsing import strip_prompt_prefix
from services.translation.llm.validation.english_residue import looks_like_source_echo_output
from services.translation.llm.validation.protocol_shell import looks_like_prompt_echo_output


def test_single_item_extractor_returns_plain_text_when_not_json() -> None:
    assert (
        deepseek_client.extract_single_item_translation_text("这是直接返回的中文译文。", "p001-b019")
        == "这是直接返回的中文译文。"
    )


def test_single_item_extractor_unwraps_nested_batch_json_shell() -> None:
    nested = {
        "translated_text": json.dumps(
            {
                "translations": [
                    {
                        "item_id": "p030-b010",
                        "translated_text": "计算效率、成本与精度。",
                    }
                ]
            },
            ensure_ascii=False,
        )
    }
    assert (
        deepseek_client.extract_single_item_translation_text(json.dumps(nested, ensure_ascii=False), "p030-b010")
        == "计算效率、成本与精度。"
    )


def test_single_item_extractor_rejects_broken_protocol_shell() -> None:
    broken = '{"translations":[{"item_id":"p001-b019","translated_text":"未闭合协议外壳"'

    with pytest.raises(Exception):
        deepseek_client.extract_single_item_translation_text(broken, "p001-b019")


def test_single_item_extractor_allows_plain_text_with_references() -> None:
    text = "该结论可由 Lehmann 表示推出 [94, 95]。"

    assert deepseek_client.extract_single_item_translation_text(text, "p001-b019") == text


def test_single_item_extractor_strips_prompt_echo_from_plain_output() -> None:
    # 弱模型把提示词定界符/指令/标签回显进译文，解析层必须回收真实译文
    polluted = (
        "翻译结果：\n"
        "【当前原文开始】\n"
        "将以下文本翻译成适合科研论文排版的简体中文，同时保持原意、术语精度和版面友好性。\n"
        "【当前原文结束】\n"
        "复杂计算机程序的发展。"
    )

    assert (
        deepseek_client.extract_single_item_translation_text(polluted, "p001-b019")
        == "复杂计算机程序的发展。"
    )


def test_single_item_extractor_strips_prompt_echo_inline_label() -> None:
    assert (
        deepseek_client.extract_single_item_translation_text("译文：复杂计算机程序的发展。", "p001-b019")
        == "复杂计算机程序的发展。"
    )


def test_single_item_extractor_keeps_clean_text_unchanged() -> None:
    text = "该结论可由 Lehmann 表示推出 [94, 95]。"

    assert deepseek_client.extract_single_item_translation_text(text, "p001-b019") == text


def test_strip_prompt_echo_markers_removes_markers_anywhere() -> None:
    text = "前半部分。\n【当前原文开始】\n将以下文本翻译成适合科研论文排版的简体中文。\n【当前原文结束】\n后半部分。"

    assert strip_prompt_echo_markers(text) == "前半部分。\n后半部分。"


def test_strip_prompt_echo_markers_keeps_clean_text() -> None:
    text = "复杂计算机程序的发展使得模拟精度显著提升。"

    assert strip_prompt_echo_markers(text) == text


def test_prompt_echo_detector_flags_markers_labels_and_instructions() -> None:
    assert looks_like_prompt_echo_output("【当前原文开始】将以下文本翻译成适合科研论文排版的简体中文。")
    assert looks_like_prompt_echo_output("翻译结果：\n将以下文本翻译成适合科研论文排版的简体中文。")
    assert looks_like_prompt_echo_output("请将下面的原文翻译成适合科研论文排版的简体中文，同时保持原意、术语精度和版面友好性。")
    assert looks_like_prompt_echo_output("只输出译文本身，不要重复指令。\n复杂计算机程序的发展。")
    assert not looks_like_prompt_echo_output("复杂计算机程序的发展使得模拟精度显著提升。")


def test_single_item_extractor_strips_leading_source_echo() -> None:
    # 极简兜底提示词无定界符：弱模型把原文整段回显到译文开头时，
    # 解析层拿原文做精确前缀剥离，回收其后的真实译文
    source = "The advancement of complex computer programs."
    polluted = "The advancement of complex computer programs.\n\n复杂计算机程序的发展。"

    assert (
        deepseek_client.extract_single_item_translation_text(polluted, "p001-b019", source_text=source)
        == "复杂计算机程序的发展。"
    )


def test_single_item_extractor_strips_pure_source_echo_to_empty() -> None:
    source = "The advancement of complex computer programs."

    assert deepseek_client.extract_single_item_translation_text(source, "p001-b019", source_text=source) == ""


def test_source_echo_detector_flags_echo_output_and_ignores_clean() -> None:
    item = {
        "item_id": "p001-b001",
        "block_type": "text",
        "metadata": {"structure_role": "body"},
        "translation_unit_protected_source_text": "The advancement of complex computer programs.",
    }

    assert looks_like_source_echo_output(
        item, "The advancement of complex computer programs.\n\n复杂计算机程序的发展。"
    )
    assert not looks_like_source_echo_output(item, "复杂计算机程序的发展。")


def test_english_residue_detector_only_blocks_copy_dominant_english_output() -> None:
    item = {
        "item_id": "p002-b001",
        "block_type": "text",
        "metadata": {"structure_role": "body"},
        "translation_unit_protected_source_text": (
            "The advancement of complex computer programs with faster computing power and material simulation methods "
            "has become an important tool for material researchers, because it explains many properties."
        ),
    }
    translated = (
        "The advancement of complex computer programs with faster computing power and material simulation methods "
        "remains important."
    )
    assert not placeholder_guard.looks_like_untranslated_english_output(item, translated)
    assert placeholder_guard.looks_like_predominantly_english_output(item, translated)


def test_english_residue_detector_only_warns_for_mixed_output_with_english_span() -> None:
    item = {
        "item_id": "p009-b067",
        "block_type": "text",
        "metadata": {"structure_role": "body"},
        "translation_unit_protected_source_text": (
            "Olefins offer the unique benefit of starting from prochiral carbons rather than preformed "
            "tetrasubstituted carbons like tertiary alkyl bromides, which can be laborious to synthesize or unstable."
        ),
    }
    translated = (
        "这是一个重要优势。 Olefins offer the unique benefit of starting from prochiral carbons rather than "
        "preformed tetrasubstituted carbons like tertiary alkyl bromides, which can be laborious to synthesize or unstable. "
        "后续底物也可以顺利偶联。"
    )
    assert not placeholder_guard.looks_like_untranslated_english_output(item, translated)
    assert placeholder_guard.looks_like_mixed_english_residue_output(item, translated)
    assert placeholder_guard.looks_like_predominantly_english_output(item, translated)


def test_english_residue_detector_ignores_author_name_list() -> None:
    item = {
        "item_id": "p001-b002",
        "block_type": "text",
        "metadata": {"structure_role": "body"},
        "translation_unit_protected_source_text": (
            "Samantha A. Green, Steven W. M. Crossley, Jeishla L. M. Matos, "
            "Suhelen Vásquez-Céspedes, Sophia L. Shevick, and Ryan A. Shenvi*"
        ),
    }
    assert not placeholder_guard.looks_like_untranslated_english_output(
        item,
        "Samantha A. Green, Steven W. M. Crossley, Jeishla L. M. Matos, "
        "Suhelen Vásquez-Céspedes, Sophia L. Shevick, and Ryan A. Shenvi*",
    )


def test_english_residue_guard_ignores_reference_like_entries() -> None:
    item = {
        "item_id": "p011-b009",
        "block_type": "text",
        "metadata": {
            "structure_role": "body",
            "source": {"raw_type": "ref_text"},
        },
        "translation_unit_protected_source_text": (
            "Gregor Bachmann and Vaishnavh Nagarajan. The pitfalls of next-token prediction. "
            "In Forty-first International Conference on Machine Learning, ICML, 2024."
        ),
    }
    translated = (
        "Gregor Bachmann and Vaishnavh Nagarajan. 下一个词预测的陷阱. "
        "In Forty-first International Conference on Machine Learning, ICML, 2024."
    )
    assert not placeholder_guard.looks_like_untranslated_english_output(item, translated)


def test_formula_dense_body_with_partial_chinese_is_not_treated_as_english_residue() -> None:
    item = {
        "item_id": "p003-b011",
        "block_type": "text",
        "metadata": {"structure_role": "body"},
        "translation_unit_protected_source_text": (
            "For the diffusion process <f1-a11/>, the transition matrix <f2-b22/> governs token updates, "
            "while the marginal probability <f3-c33/> controls the corruption level and the posterior estimator "
            "<f4-d44/> is combined with <f5-e55/> to stabilize training."
        ),
        "formula_map": [{"placeholder": "<f1-a11/>"}],
        "translation_unit_formula_map": [{"placeholder": "<f1-a11/>"}],
    }
    translated = (
        "对于扩散过程 <f1-a11/>，transition matrix <f2-b22/> 控制 token 更新，"
        "而 marginal probability <f3-c33/> 与 posterior estimator <f4-d44/> 共同稳定训练。"
    )
    assert not placeholder_guard.looks_like_predominantly_english_output(item, translated)
    assert not placeholder_guard.looks_like_mixed_english_residue_output(item, translated)


def test_term_preserving_formula_body_is_not_treated_as_english_residue() -> None:
    item = {
        "item_id": "p007-b014",
        "block_type": "text",
        "metadata": {"structure_role": "body"},
        "translation_unit_protected_source_text": (
            "The barrier from B3LYP/6-311G** is reported as <f1-a11/>, while GC-FID and MP2/6-311G "
            "measurements provide <f2-b22/> for comparison."
        ),
        "formula_map": [{"placeholder": "<f1-a11/>"}, {"placeholder": "<f2-b22/>"}],
        "translation_unit_formula_map": [{"placeholder": "<f1-a11/>"}, {"placeholder": "<f2-b22/>"}],
    }
    translated = (
        "B3LYP/6-311G** 计算给出的势垒为 <f1-a11/>，而 GC-FID 与 MP2/6-311G 测量结果提供了 "
        "<f2-b22/> 用于比较。"
    )
    assert not placeholder_guard.looks_like_predominantly_english_output(item, translated)
    assert not placeholder_guard.looks_like_mixed_english_residue_output(item, translated)


def test_structured_output_repairs_trailing_commas_and_unquoted_keys() -> None:
    payload = structured_output.parse_structured_json(
        """
        ```json
        {domain: "chemistry", summary: "ok", translation_guidance: "keep terms",}
        ```
        """
    )
    assert payload["domain"] == "chemistry"
    assert payload["summary"] == "ok"


def test_domain_context_parser_accepts_line_key_value_fallback() -> None:
    result = structured_parsers.parse_domain_context_response(
        "DOMAIN: materials science\nSUMMARY: photocatalysis paper\nTRANSLATION_GUIDANCE: preserve formulas",
        preview_text="preview",
    )
    assert result["domain"] == "materials science"
    assert result["summary"] == "photocatalysis paper"
    assert result["translation_guidance"] == "preserve formulas"


def test_domain_context_parser_salvages_fields_from_malformed_json() -> None:
    content = """
    Here is the result:
    {
      "domain": "computational chemistry",
      "summary": "A materials-modeling paper with equation-heavy prose."
      "translation_guidance": "保留术语、缩写和公式记号，不要意译。"
    }
    """
    result = structured_parsers.parse_domain_context_response(content, preview_text="preview")
    assert result["domain"] == "computational chemistry"
    assert result["summary"] == "A materials-modeling paper with equation-heavy prose."
    assert result["translation_guidance"] == "保留术语、缩写和公式记号，不要意译。"


def test_placeholder_guard_canonicalizes_nested_json_shell() -> None:
    result = placeholder_guard.canonicalize_batch_result(
        [{"item_id": "p030-b010", "translation_unit_protected_source_text": "Computational efficiency."}],
        {
            "p030-b010": {
                "decision": "translate",
                "translated_text": json.dumps(
                    {
                        "translations": [
                            {
                                "item_id": "p030-b010",
                                "translated_text": "计算效率、成本与精度。",
                            }
                        ]
                    },
                    ensure_ascii=False,
                ),
            }
        },
    )
    assert result["p030-b010"]["translated_text"] == "计算效率、成本与精度。"


def test_placeholder_guard_rejects_protocol_shell_output() -> None:
    with pytest.raises(placeholder_guard.TranslationProtocolError):
        placeholder_guard.validate_batch_result(
            [{"item_id": "p030-b010", "translation_unit_protected_source_text": "Computational efficiency."}],
            {
                "p030-b010": {
                    "decision": "translate",
                    "translated_text": '{ "translations": [{"item_id":"p030-b010","translated_text":"计算效率"}] }',
                }
            },
        )


def test_placeholder_guard_rejects_prompt_echo_output() -> None:
    # 解析层清理后仍残留提示词回显时，质检必须报协议错误走重试/降级
    with pytest.raises(placeholder_guard.TranslationProtocolError):
        placeholder_guard.validate_batch_result(
            [
                {
                    "item_id": "p014-b020",
                    "translation_unit_protected_source_text": "Computational efficiency improves simulation.",
                }
            ],
            {
                "p014-b020": {
                    "decision": "translate",
                    "translated_text": (
                        "翻译结果：\n将以下文本翻译成适合科研论文排版的简体中文，同时保持原意、术语精度和版面友好性。\n"
                        "复杂计算机程序的发展。"
                    ),
                }
            },
        )


def test_placeholder_guard_rejects_unbalanced_direct_typst_math_delimiters() -> None:
    with pytest.raises(placeholder_guard.MathDelimiterError):
        placeholder_guard.validate_batch_result(
            [
                {
                    "item_id": "p021-b005",
                    "math_mode": "direct_typst",
                    "translation_unit_protected_source_text": "Text with $ m' $ math.",
                }
            ],
            {
                "p021-b005": {
                    "decision": "translate",
                    "translated_text": "含有被破坏的 $ m' 数学片段。",
                }
            },
        )


def test_placeholder_guard_rejects_direct_typst_following_context_math_bleed() -> None:
    item = {
        "item_id": "p125-b018",
        "math_mode": "direct_typst",
        "translation_unit_protected_source_text": (
            r"For simplicity, consider a homonuclear neutral diatomic molecule AB. "
            r"We wish to prove that the binding energy"
        ),
        "translation_context_after": r"is positive for $ \lambda = 1 $. To do this we shall use",
    }

    with pytest.raises(placeholder_guard.TranslationProtocolError):
        placeholder_guard.validate_batch_result(
            [item],
            {
                "p125-b018": {
                    "decision": "translate",
                    "translated_text": r"为简化起见，考虑一个同核中性双原子分子AB。我们欲证明结合能在$ \lambda = 1 $时为正。",
                }
            },
        )


def test_placeholder_guard_rejects_model_request_prompt_output() -> None:
    with pytest.raises(placeholder_guard.TranslationProtocolError):
        placeholder_guard.validate_batch_result(
            [
                {
                    "item_id": "p014-b014",
                    "math_mode": "direct_typst",
                    "translation_unit_protected_source_text": "……",
                }
            ],
            {
                "p014-b014": {
                    "decision": "translate",
                    "translated_text": "请提供待翻译的原文。",
                }
            },
        )


def test_placeholder_guard_allows_legitimate_source_text_request_sentence() -> None:
    placeholder_guard.validate_batch_result(
        [
            {
                "item_id": "p014-b015",
                "math_mode": "direct_typst",
                "translation_unit_protected_source_text": "The form asks users to provide the source text before submitting.",
            }
        ],
        {
            "p014-b015": {
                "decision": "translate",
                "translated_text": "该表单要求用户在提交前提供原文。",
            }
        },
    )


def test_translation_and_formula_outputs_use_strict_json_schema_format() -> None:
    for schema in [
        structured_models.TRANSLATION_BATCH_RESPONSE_SCHEMA,
        structured_models.TRANSLATION_SINGLE_TEXT_RESPONSE_SCHEMA,
        structured_models.TRANSLATION_SINGLE_DECISION_RESPONSE_SCHEMA,
        structured_models.FORMULA_SEGMENT_RESPONSE_SCHEMA,
    ]:
        assert schema["type"] == "json_schema"
        assert schema["json_schema"]["strict"]


def test_formula_segment_parser_accepts_schema_json_payload() -> None:
    result = segment_routing.parse_segment_translation_payload(
        '{"segments":[{"segment_id":"1","translated_text":"第一段"},{"segment_id":"2","translated_text":"第二段"}]}',
        expected_segments=[
            {"segment_id": "1", "source_text": "first"},
            {"segment_id": "2", "source_text": "second"},
        ],
    )
    assert result == {"1": "第一段", "2": "第二段"}


def test_strip_prompt_prefix_strips_echoed_user_message() -> None:
    # 弱模型把 user 消息逐字回显到输出开头:按折叠空白后的精确前缀剥离,
    # 回收其后的真实译文。
    user_prompt = (
        "请把下面的原文翻译成简体中文。只输出译文本身，不要重复指令、不要加标签、不要解释。\n"
        "\n"
        "The advancement of complex computer programs."
    )
    polluted = f"{user_prompt}\n\n复杂计算机程序的发展。"

    assert strip_prompt_prefix(polluted, user_prompt) == "复杂计算机程序的发展。"


def test_strip_prompt_prefix_strips_echoed_guidance_block() -> None:
    # 动态领域指导被注入 system prompt 后也可能被逐字回显:带标签整块
    # 用标签前缀剥离。
    guidance = (
        "格式一致性：采用了符合学术论文格式的标题和段落结构。\n"
        "避免不必要的翻译：对于技术细节和公式，直接保留了原文的术语和格式。"
    )
    labeled = f"Document-specific translation guidance:\n{guidance}"
    polluted = f"{labeled}\n\n复杂计算机程序的发展。"

    assert strip_prompt_prefix(polluted, labeled) == "复杂计算机程序的发展。"


def test_strip_prompt_prefix_strips_bare_guidance_echo() -> None:
    # 不带标签、只回显领域指导内容本身时,用裸内容前缀剥离。
    guidance = "格式一致性：采用了符合学术论文格式的标题和段落结构。避免不必要的翻译：对于技术细节和公式，直接保留了原文的术语和格式。"
    polluted = f"{guidance}\n\n复杂计算机程序的发展。"

    assert strip_prompt_prefix(polluted, guidance) == "复杂计算机程序的发展。"


def test_strip_prompt_prefix_strips_multiple_echoed_messages() -> None:
    # system + user 两条消息都可能被逐字回显:循环剥离直到无可剥离项。
    system_prompt = (
        "You are a professional translation engine. Output only the translated text - "
        "no labels, no explanations, no repetition of the instruction."
    )
    user_prompt = "请把下面的原文翻译成简体中文。只输出译文本身。\n\nComputational efficiency."
    polluted = f"{system_prompt}\n\n{user_prompt}\n\n计算效率。"

    assert strip_prompt_prefix(polluted, system_prompt, user_prompt) == "计算效率。"


def test_strip_prompt_prefix_keeps_clean_text_unchanged() -> None:
    text = "复杂计算机程序的发展使得模拟精度显著提升。"
    assert strip_prompt_prefix(text, "请把下面的原文翻译成简体中文。") == text


def test_strip_prompt_prefix_skips_short_prefixes() -> None:
    # 短片段(<20 字符)不剥离,避免误伤译文开头合法保留的专名/术语。
    text = "格式一致性。计算自洽场循环收敛。"
    assert strip_prompt_prefix(text, "格式一致性") == text


def test_strip_prompt_prefix_matches_collapsed_whitespace() -> None:
    # 模型把提示词里的换行折叠成空格也能精确匹配。
    user_prompt = "请把下面的原文翻译成简体中文。\n\n只输出译文本身，不要加标签。\n\nComputational efficiency."
    polluted = (
        "请把下面的原文翻译成简体中文。 只输出译文本身，不要加标签。 Computational efficiency.\n"
        "\n"
        "计算效率、成本与精度。"
    )

    assert strip_prompt_prefix(polluted, user_prompt) == "计算效率、成本与精度。"


def test_prompt_echo_detector_flags_numbered_guidance_echo() -> None:
    # 弱模型把动态领域指导改写成编号列表回显:逐字前缀匹配失效,质检按行
    # 片段识别仍必须报错,让上层走重试/降级而不是渲染进 PDF。
    polluted = (
        "3. 格式一致性： - 采用了符合学术论文格式的标题和段落结构。\n"
        "4. 避免不必要的翻译： - 对于技术细节和公式，直接保留了原文的术语和格式。\n"
        "5. 保持原文意图\n"
        "复杂计算机程序的发展。"
    )
    assert looks_like_prompt_echo_output(polluted)


def test_prompt_echo_detector_ignores_legit_prose_with_guidance_phrases() -> None:
    # 短通用短语(格式一致性/避免不必要的翻译/保持原文意图等)在真实译文里
    # 也常见,裸子串匹配会误伤;只有"标签头"形态才算回显。
    legit = [
        "为了保证格式一致性，所有图表均采用相同的样式。",
        "保持原文意图是翻译的关键原则。",
        "本研究强调了避免不必要的翻译误差的重要性。",
        "作者讨论了格式一致性对可读性的影响。",
        "文中没有添加额外的解释，仅陈述实验结果。",
    ]
    for text in legit:
        assert not looks_like_prompt_echo_output(text), text


def test_strip_prompt_echo_markers_keeps_legit_prose_with_guidance_phrases() -> None:
    # 解析层头部剥离同样不能误伤以通用短语开头的合法句子(否则会把第一行删掉)。
    text = "保持原文意图是翻译的关键原则。\n复杂计算机程序的发展。"
    assert strip_prompt_echo_markers(text) == text


def test_prompt_echo_label_head_requires_echo_shape_not_bare_phrase() -> None:
    from services.translation.llm.shared.response_parsing import is_prompt_echo_label_head

    # 标签头形态(行首短语 + 冒号,或整行就是短语,可带列表编号前缀)算回显
    assert is_prompt_echo_label_head("格式一致性： - 采用了符合学术论文格式的标题。")
    assert is_prompt_echo_label_head("3. 避免不必要的翻译：对于技术细节和公式直接保留原文。")
    assert is_prompt_echo_label_head("5. 保持原文意图")
    assert is_prompt_echo_label_head("格式一致性")
    # 合法句子:短语嵌在句子里、行首但后接其他字、或行首带冒号之外的内容
    assert not is_prompt_echo_label_head("为了保证格式一致性，所有图表均采用相同的样式。")
    assert not is_prompt_echo_label_head("保持原文意图是翻译的关键原则。")
    assert not is_prompt_echo_label_head("研究了避免不必要的翻译误差的重要性。")
    assert not is_prompt_echo_label_head("格式一致性。计算自洽场循环收敛。")

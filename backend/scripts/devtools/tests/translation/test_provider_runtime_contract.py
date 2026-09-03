from __future__ import annotations

import sys
from pathlib import Path


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))

from services.translation.llm.shared.provider_runtime import ACTIVE_PROVIDER
from services.translation.llm.shared.provider_runtime import DEFAULT_BASE_URL
from services.translation.llm.shared.provider_runtime import DEFAULT_MODEL
from services.translation.llm.shared.provider_runtime import PROVIDER_CAPABILITIES
from services.translation.llm.shared.provider_registry import resolve_active_provider_runtime


def test_active_provider_runtime_uses_deepseek_v4_flash_default() -> None:
    runtime = resolve_active_provider_runtime()

    assert ACTIVE_PROVIDER == "deepseek"
    assert runtime.provider_id == "deepseek"
    assert DEFAULT_MODEL == "deepseek-v4-flash"
    assert runtime.default_model == "deepseek-v4-flash"
    assert DEFAULT_BASE_URL == "https://api.deepseek.com/v1"


def test_provider_runtime_declares_translation_capabilities() -> None:
    runtime = resolve_active_provider_runtime()

    assert runtime.capabilities == PROVIDER_CAPABILITIES
    assert runtime.capabilities.plain_text is True
    assert runtime.capabilities.unstructured_plain_text is True
    assert runtime.capabilities.tagged_text is True
    assert runtime.capabilities.structured_decision is True
    assert runtime.capabilities.batch_once is True


def test_offline_mode_with_local_llm_base_url_activates_local_runtime(monkeypatch) -> None:
    monkeypatch.setenv("RETAIN_OFFLINE", "1")
    monkeypatch.setenv("RETAIN_LOCAL_LLM_BASE_URL", "http://localhost:9999/v1")

    runtime = resolve_active_provider_runtime()

    assert runtime.provider_id == "local"
    assert runtime.provider_family == "other"
    assert runtime.default_api_key_env == ""


def test_offline_mode_without_local_llm_base_url_falls_back_to_deepseek(monkeypatch) -> None:
    monkeypatch.setenv("RETAIN_OFFLINE", "1")
    monkeypatch.delenv("RETAIN_LOCAL_LLM_BASE_URL", raising=False)

    runtime = resolve_active_provider_runtime()

    assert runtime.provider_id == "deepseek"


def test_offline_flag_ignores_non_truthy_values(monkeypatch) -> None:
    monkeypatch.setenv("RETAIN_OFFLINE", "2")
    monkeypatch.setenv("RETAIN_LOCAL_LLM_BASE_URL", "http://localhost:9999/v1")

    assert resolve_active_provider_runtime().provider_id == "deepseek"


def test_api_key_required_predicate_follows_provider_family() -> None:
    # translate_only_pipeline / diagnose_failure_with_ai 的 required= 判定:
    # 仅官方 DeepSeek 端点强制 API key,本地端点(含离线本地 LLM)不强制。
    from services.translation.artifacts import classify_provider_family

    assert (
        classify_provider_family(base_url="https://api.deepseek.com/v1", model="deepseek-chat")
        == "deepseek_official"
    )
    assert (
        classify_provider_family(base_url="http://localhost:11434/v1", model="qwen2.5:7b")
        != "deepseek_official"
    )


def test_explicit_provider_family_overrides_url_sniffing() -> None:
    # 走代理/网关时 URL 嗅探会失效;显式 provider_family 必须优先。
    from services.translation.llm.shared.provider_registry import infer_provider_capabilities

    proxied = infer_provider_capabilities(
        base_url="https://gateway.corp.example/v1",
        model="deepseek-v4-flash",
        provider_family="deepseek_official",
    )
    assert proxied.requires_api_key is True
    assert proxied.high_capacity is True

    sniffed = infer_provider_capabilities(
        base_url="https://gateway.corp.example/v1",
        model="deepseek-v4-flash",
    )
    # 兜底嗅探:模型名仍能识别出 deepseek_compatible,但不算官方端点
    assert sniffed.requires_api_key is False


def test_execution_plan_prefers_explicit_provider_family(tmp_path: Path) -> None:
    import json

    from services.translation.workflow.execution import TranslationExecutionRequest
    from services.translation.workflow.execution_plan import build_translation_execution_plan

    source_json = tmp_path / "document.v1.json"
    source_json.write_text(
        json.dumps(
            {
                "schema": "normalized_document_v1",
                "schema_version": "1.1",
                "document_id": "provider-family-test",
                "source": {"provider": "test", "provider_version": "test", "raw_files": {}},
                "page_count": 1,
                "pages": [
                    {
                        "page_index": 0,
                        "width": 200.0,
                        "height": 120.0,
                        "unit": "pt",
                        "blocks": [
                            {
                                "block_id": "p001-b0000",
                                "page_index": 0,
                                "order": 0,
                                "type": "text",
                                "sub_type": "",
                                "geometry": {"bbox": [0, 0, 150, 20]},
                                "content": {"kind": "text", "text": "Hello world."},
                                "bbox": [0, 0, 150, 20],
                                "text": "Hello world.",
                                "lines": [],
                                "segments": [],
                                "layout_role": "paragraph",
                                "semantic_role": "body",
                                "structure_role": "body",
                                "policy": {"translate": True, "translate_reason": "test"},
                                "provenance": {
                                    "provider": "test",
                                    "raw_label": "text",
                                    "raw_sub_type": "",
                                    "raw_bbox": [0, 0, 150, 20],
                                    "raw_path": "$.pages[0].blocks[0]",
                                },
                                "continuation_hint": {
                                    "source": "",
                                    "group_id": "",
                                    "role": "",
                                    "scope": "",
                                    "reading_order": -1,
                                    "confidence": 0.0,
                                },
                                "metadata": {},
                                "source": {"provider": "test", "raw_type": "text"},
                            }
                        ],
                    }
                ],
                "derived": {},
                "markers": {},
            },
            ensure_ascii=False,
        ),
        encoding="utf-8",
    )

    # 显式 family 优先:代理 URL 嗅探不出官方端点,但显式声明必须生效
    explicit = build_translation_execution_plan(
        TranslationExecutionRequest(
            source_json_path=source_json,
            output_dir=tmp_path / "translated-explicit",
            api_key="sk-test",
            base_url="https://gateway.corp.example/v1",
            model="deepseek-v4-flash",
            provider_family="deepseek_official",
        )
    )
    assert explicit.run_diagnostics.provider_family == "deepseek_official"

    # 留空时回退 URL/model 嗅探,行为与改造前一致
    fallback = build_translation_execution_plan(
        TranslationExecutionRequest(
            source_json_path=source_json,
            output_dir=tmp_path / "translated-fallback",
            api_key="sk-test",
            base_url="https://api.deepseek.com/v1",
            model="deepseek-v4-flash",
        )
    )
    assert fallback.run_diagnostics.provider_family == "deepseek_official"

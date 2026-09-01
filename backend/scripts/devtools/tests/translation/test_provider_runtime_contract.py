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

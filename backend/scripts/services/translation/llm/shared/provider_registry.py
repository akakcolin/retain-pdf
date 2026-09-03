from __future__ import annotations

import os
from dataclasses import dataclass

from services.translation.llm.providers.deepseek.client import DEFAULT_API_KEY_ENV as DEEPSEEK_DEFAULT_API_KEY_ENV
from services.translation.llm.providers.deepseek.client import DEFAULT_BASE_URL as DEEPSEEK_DEFAULT_BASE_URL
from services.translation.llm.providers.deepseek.client import DEFAULT_MODEL as DEEPSEEK_DEFAULT_MODEL
from services.translation.llm.providers.deepseek.client import build_headers as deepseek_build_headers
from services.translation.llm.providers.deepseek.client import chat_completions_url as deepseek_chat_completions_url
from services.translation.llm.providers.deepseek.client import get_api_key as deepseek_get_api_key
from services.translation.llm.providers.deepseek.client import get_session as deepseek_get_session
from services.translation.llm.providers.deepseek.client import is_transport_error as deepseek_is_transport_error
from services.translation.llm.providers.deepseek.client import normalize_base_url as deepseek_normalize_base_url
from services.translation.llm.providers.deepseek.client import request_chat_content as deepseek_request_chat_content
from services.translation.llm.providers.deepseek.translation_client import parse_translation_payload as deepseek_parse_translation_payload
from services.translation.llm.providers.deepseek.translation_client import translate_batch_once as deepseek_translate_batch_once
from services.translation.llm.providers.deepseek.translation_client import translate_single_item_plain_text as deepseek_translate_single_item_plain_text
from services.translation.llm.providers.deepseek.translation_client import (
    translate_single_item_plain_text_unstructured as deepseek_translate_single_item_plain_text_unstructured,
)
from services.translation.llm.providers.deepseek.translation_client import (
    translate_continuation_group_members as deepseek_translate_continuation_group_members,
)
from services.translation.llm.providers.deepseek.translation_client import translate_single_item_tagged_text as deepseek_translate_single_item_tagged_text
from services.translation.llm.providers.deepseek.translation_client import translate_single_item_with_decision as deepseek_translate_single_item_with_decision
from services.translation.artifacts import classify_provider_family
from services.translation.llm.shared.provider_protocol import ChatCompletionsUrlFn
from services.translation.llm.shared.provider_protocol import GetApiKeyFn
from services.translation.llm.shared.provider_protocol import HeadersBuilderFn
from services.translation.llm.shared.provider_protocol import NormalizeBaseUrlFn
from services.translation.llm.shared.provider_protocol import ParseTranslationPayloadFn
from services.translation.llm.shared.provider_protocol import SessionFactoryFn
from services.translation.llm.shared.provider_protocol import TranslationProviderCapabilities
from services.translation.llm.shared.provider_protocol import TranslationProviderRuntimeProtocol
from services.translation.llm.shared.provider_protocol import TranslateBatchFn
from services.translation.llm.shared.provider_protocol import TranslateSingleFn
from services.translation.llm.shared.provider_protocol import TransportErrorFn
from services.translation.llm.shared.provider_protocol import TransportRequestFn


@dataclass(frozen=True)
class TranslationProviderRuntime:
    provider_id: str
    provider_family: str
    default_api_key_env: str
    default_model: str
    default_base_url: str
    capabilities: TranslationProviderCapabilities
    build_headers: HeadersBuilderFn
    chat_completions_url: ChatCompletionsUrlFn
    get_api_key: GetApiKeyFn
    get_session: SessionFactoryFn
    is_transport_error: TransportErrorFn
    normalize_base_url: NormalizeBaseUrlFn
    request_chat_content: TransportRequestFn
    parse_translation_payload: ParseTranslationPayloadFn
    translate_batch_once: TranslateBatchFn
    translate_single_item_plain_text: TranslateSingleFn
    translate_single_item_plain_text_unstructured: TranslateSingleFn
    translate_continuation_group_members: TranslateSingleFn
    translate_single_item_tagged_text: TranslateSingleFn
    translate_single_item_with_decision: TranslateSingleFn


DEEPSEEK_RUNTIME = TranslationProviderRuntime(
    provider_id="deepseek",
    provider_family="deepseek_official",
    default_api_key_env=DEEPSEEK_DEFAULT_API_KEY_ENV,
    default_model=DEEPSEEK_DEFAULT_MODEL,
    default_base_url=DEEPSEEK_DEFAULT_BASE_URL,
    capabilities=TranslationProviderCapabilities(
        high_capacity=True,
        supports_prefix_cache=True,
        requires_api_key=True,
    ),
    build_headers=deepseek_build_headers,
    chat_completions_url=deepseek_chat_completions_url,
    get_api_key=deepseek_get_api_key,
    get_session=deepseek_get_session,
    is_transport_error=deepseek_is_transport_error,
    normalize_base_url=deepseek_normalize_base_url,
    request_chat_content=deepseek_request_chat_content,
    parse_translation_payload=deepseek_parse_translation_payload,
    translate_batch_once=deepseek_translate_batch_once,
    translate_single_item_plain_text=deepseek_translate_single_item_plain_text,
    translate_single_item_plain_text_unstructured=deepseek_translate_single_item_plain_text_unstructured,
    translate_continuation_group_members=deepseek_translate_continuation_group_members,
    translate_single_item_tagged_text=deepseek_translate_single_item_tagged_text,
    translate_single_item_with_decision=deepseek_translate_single_item_with_decision,
)


def _env(name: str, default: str) -> str:
    return os.environ.get(name, default)


def offline_mode() -> bool:
    value = os.environ.get("RETAIN_OFFLINE", "").strip().lower()
    return value in {"1", "true", "yes"}


# 本地 LLM 走 OpenAI 兼容端点(如 Ollama/自托管 vLLM)。与 DEEPSEEK_RUNTIME
# 复用同一组通用函数(request_chat_content 等),仅 base_url/model 不同;
# 本地端点通常无需 API key,default_api_key_env 留空、key 可选。
LOCAL_LLM_RUNTIME = TranslationProviderRuntime(
    provider_id="local",
    provider_family="other",
    default_api_key_env="",
    default_model=_env("RETAIN_LOCAL_LLM_MODEL", "qwen2.5:7b"),
    default_base_url=_env("RETAIN_LOCAL_LLM_BASE_URL", "http://localhost:11434/v1"),
    capabilities=TranslationProviderCapabilities(
        high_capacity=False,
        supports_prefix_cache=False,
        requires_api_key=False,
    ),
    build_headers=deepseek_build_headers,
    chat_completions_url=deepseek_chat_completions_url,
    get_api_key=deepseek_get_api_key,
    get_session=deepseek_get_session,
    is_transport_error=deepseek_is_transport_error,
    normalize_base_url=deepseek_normalize_base_url,
    request_chat_content=deepseek_request_chat_content,
    parse_translation_payload=deepseek_parse_translation_payload,
    translate_batch_once=deepseek_translate_batch_once,
    translate_single_item_plain_text=deepseek_translate_single_item_plain_text,
    translate_single_item_plain_text_unstructured=deepseek_translate_single_item_plain_text_unstructured,
    translate_continuation_group_members=deepseek_translate_continuation_group_members,
    translate_single_item_tagged_text=deepseek_translate_single_item_tagged_text,
    translate_single_item_with_decision=deepseek_translate_single_item_with_decision,
)


def resolve_active_provider_runtime() -> TranslationProviderRuntimeProtocol:
    # RETAIN_OFFLINE=1 且显式配置了本地 LLM 端点才偏好本地;否则回退 DeepSeek。
    if offline_mode() and os.environ.get("RETAIN_LOCAL_LLM_BASE_URL"):
        return LOCAL_LLM_RUNTIME
    return DEEPSEEK_RUNTIME


def infer_provider_capabilities(
    *,
    base_url: str,
    model: str,
    provider_family: str = "",
) -> TranslationProviderCapabilities:
    """将请求级 provider 标识映射为能力集合。

    这是整条代码库里唯一允许出现 ``"deepseek_official"`` 字符串比较、并据此
    推导能力的地方。所有上层调度/限流/鉴权决策都应读取返回的
    ``TranslationProviderCapabilities`` 布尔字段,而不是直接比较
    ``provider_family == "deepseek_official"``。

    ``provider_family`` 为请求显式声明的家族标识,非空时直接使用;
    空串时回退到 ``classify_provider_family`` 按 base_url/model 嗅探。
    """
    family = provider_family.strip() or classify_provider_family(base_url=base_url, model=model)
    if family == "deepseek_official":
        return TranslationProviderCapabilities(
            high_capacity=True,
            supports_prefix_cache=True,
            requires_api_key=True,
        )
    if family == "deepseek_compatible":
        return TranslationProviderCapabilities(
            high_capacity=False,
            supports_prefix_cache=False,
            requires_api_key=False,
        )
    return TranslationProviderCapabilities()


__all__ = [
    "DEEPSEEK_RUNTIME",
    "LOCAL_LLM_RUNTIME",
    "TranslationProviderRuntime",
    "TranslationProviderCapabilities",
    "TranslationProviderRuntimeProtocol",
    "infer_provider_capabilities",
    "offline_mode",
    "resolve_active_provider_runtime",
]

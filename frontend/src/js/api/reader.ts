import { buildApiHeaders, isMockMode } from "../config/runtime.js";
import { unwrapEnvelope } from "../job/core.js";
import { API_PREFIX } from "../config/api-constants.js";
import { getMockReaderRegions } from "../mock/documents.js";
import { buildApiEndpoint, buildJobDetailEndpoint, submitJson } from "./http.js";

export async function fetchReaderRegions(jobId, apiPrefix) {
  if (isMockMode()) {
    void jobId;
    void apiPrefix;
    return getMockReaderRegions();
  }
  const resp = await fetch(`${buildJobDetailEndpoint(jobId, apiPrefix)}/reader/regions`, {
    headers: buildApiHeaders(),
  });
  if (!resp.ok) {
    if (resp.status === 404) {
      return { items: [] };
    }
    throw new Error(`读取阅读区域失败，请稍后重试。(${resp.status})`);
  }
  return unwrapEnvelope(await resp.json());
}

export async function fetchReaderMetadata(jobId, apiPrefix) {
  if (isMockMode()) {
    void jobId;
    void apiPrefix;
    return null;
  }
  const resp = await fetch(`${buildJobDetailEndpoint(jobId, apiPrefix)}/reader/metadata`, {
    headers: buildApiHeaders(),
  });
  if (!resp.ok) {
    if (resp.status === 404) {
      return null;
    }
    throw new Error(`读取阅读元数据失败，请稍后重试。(${resp.status})`);
  }
  return unwrapEnvelope(await resp.json());
}

export async function fetchReaderAiChat(jobId, payload, apiPrefix) {
  if (isMockMode()) {
    void jobId;
    void apiPrefix;
    const message = `${payload?.message || ""}`.trim();
    return {
      answer: `这是 mock 阅读问答回复：${message || "请提出一个问题"}`,
      citations: [
        {
          title: "Mock Markdown",
          page: 1,
          snippet: "mock 模式下会返回固定引用，真实模式会调用后端 Reader AI Chat。",
        },
      ],
      used_context: {
        source: "mock",
        scope: payload?.scope || "document",
      },
    };
  }
  return submitJson(`${buildJobDetailEndpoint(jobId, apiPrefix)}/reader/ai/chat`, payload);
}

// 阅读器「选中文字翻译」:POST /api/v1/translate/text,模型凭据按请求携带(与 AI 问答同源)。
// submit 参数化便于测试注入,不触真实网络。
export async function translateText({
  text = "",
  targetLanguage = "简体中文",
  provider,
  model,
  apiKey,
  baseUrl,
  apiPrefix = API_PREFIX,
  submit = submitJson,
}: any = {}) {
  if (isMockMode()) {
    return {
      translated_text: `【mock 译文】${`${text || ""}`.replace(/\s+/g, " ").trim()}`,
      target_language: `${targetLanguage}`,
    };
  }
  const payload: Record<string, unknown> = {
    text: `${text}`,
    target_language: `${targetLanguage}`,
  };
  if (`${provider || ""}`.trim()) payload.provider = `${provider}`.trim();
  if (`${model || ""}`.trim()) payload.model = `${model}`.trim();
  const key = `${apiKey || ""}`.trim();
  if (key) payload.api_key = key.replace(/^Bearer\s+/i, "").trim();
  if (`${baseUrl || ""}`.trim()) payload.base_url = `${baseUrl}`.trim();
  return submit(buildApiEndpoint(apiPrefix, "translate/text"), payload);
}

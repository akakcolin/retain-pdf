// 概念图谱 HTTP 客户端（阅读器概念面板用）。
// 只经 pages/reader/external.ts 端口消费 src/js 纯逻辑层（防回弹门禁）。

import {
  API_PREFIX,
  buildApiEndpoint,
  buildApiHeaders,
  submitJson,
  unwrapEnvelope,
} from "../external.js";

export type EntitySummary = {
  entity_id: string;
  name: string;
  entity_type: string;
  aliases: string[];
  mention_count: number;
  document_count: number;
};

export type EntityRecord = EntitySummary & {
  name_norm: string;
  description: string;
  created_at: string;
  updated_at: string;
};

export type EntityMention = {
  document_id: string;
  job_id: string;
  page_idx: number;
  block_id: string;
  snippet: string;
};

export type RelatedEntity = {
  entity_id: string;
  name: string;
  entity_type: string;
  aliases: string[];
  mention_count: number;
  document_count: number;
  relation_type: string;
  /** out = 被查询实体 → 邻居；in = 邻居 → 被查询实体 */
  direction: "out" | "in" | string;
  confidence: number;
  explanation: string;
  source_document_id: string;
};

export type LinkGraphResult = { document_id: string; entities: number; mentions: number };
export type ExtractGraphResult = LinkGraphResult & { relations: number };

export type ExtractCredentials = { apiKey?: string; baseUrl?: string; model?: string };

/** 概念页正文 [n] 对应的证据锚点。 */
export type EntityPageCitation = {
  ref: number;
  document_id: string;
  document_title: string;
  job_id: string;
  page_idx: number;
  block_id: string;
  snippet: string;
};

/** 概念页正文 [[实体名]] 解析结果。surface = 正文原样文本（渲染时按它建索引）。 */
export type EntityPageLink = {
  surface: string;
  entity_id: string;
  name: string;
  entity_type: string;
  aliases: string[];
};

export type EntityPage = {
  entity_id: string;
  name: string;
  entity_type: string;
  /** false = 还没生成过（200，不是 404）。 */
  has_page: boolean;
  /** 证据签名与生成时不一致 = 内容可能过时。 */
  stale: boolean;
  generated_at: string;
  body_md: string;
  citations: EntityPageCitation[];
  /** 正文里能解析到实体的 [[...]]；解析不到的不在列表里。 */
  links: EntityPageLink[];
};

const MAX_LIMIT = 200;

async function getList<T>(path: string, params: URLSearchParams): Promise<T[]> {
  const query = params.toString();
  const url = `${buildApiEndpoint(API_PREFIX, path)}${query ? `?${query}` : ""}`;
  const resp = await fetch(url, { headers: buildApiHeaders() });
  if (!resp.ok) {
    throw new Error(`读取概念图谱失败，请稍后重试。(${resp.status})`);
  }
  const data = unwrapEnvelope<{ items?: T[] }>(await resp.json());
  return Array.isArray(data?.items) ? data.items : [];
}

async function getOne<T>(path: string): Promise<T> {
  const resp = await fetch(buildApiEndpoint(API_PREFIX, path), { headers: buildApiHeaders() });
  if (!resp.ok) {
    throw new Error(`读取概念图谱失败，请稍后重试。(${resp.status})`);
  }
  return unwrapEnvelope<T>(await resp.json());
}

/** 该文档已建链的实体概览（按提及数排序）。 */
export function listDocumentEntities(documentId: string, limit = 100): Promise<EntitySummary[]> {
  const params = new URLSearchParams();
  params.set("document_id", `${documentId || ""}`.trim());
  params.set("limit", String(Math.min(Math.max(1, limit), MAX_LIMIT)));
  return getList<EntitySummary>("entities", params);
}

export function listEntityMentions(
  entityId: string,
  { documentId = "", limit = 50 }: { documentId?: string; limit?: number } = {},
): Promise<EntityMention[]> {
  const params = new URLSearchParams();
  if (`${documentId || ""}`.trim()) {
    params.set("document_id", `${documentId}`.trim());
  }
  params.set("limit", String(Math.min(Math.max(1, limit), MAX_LIMIT)));
  return getList<EntityMention>(`entities/${encodeURIComponent(entityId)}/mentions`, params);
}

export function listEntityRelations(
  entityId: string,
  { relationType = "", limit = 50 }: { relationType?: string; limit?: number } = {},
): Promise<RelatedEntity[]> {
  const params = new URLSearchParams();
  if (`${relationType || ""}`.trim()) {
    params.set("relation_type", `${relationType}`.trim());
  }
  params.set("limit", String(Math.min(Math.max(1, limit), MAX_LIMIT)));
  return getList<RelatedEntity>(`entities/${encodeURIComponent(entityId)}/relations`, params);
}

/** 读该实体的概念页（未生成时 has_page=false，不报错）。 */
export function getEntityPage(entityId: string): Promise<EntityPage> {
  return getOne<EntityPage>(`entities/${encodeURIComponent(entityId)}/page`);
}

/** 生成/刷新概念页（一次 LLM 调用）；凭据留空由服务端回落启动配置。 */
export function generateEntityPage(
  entityId: string,
  credentials: ExtractCredentials = {},
): Promise<EntityPage> {
  const payload: Record<string, string> = {};
  const key = `${credentials.apiKey || ""}`.trim();
  if (key) payload.llm_api_key = key.replace(/^Bearer\s+/i, "").trim();
  const baseUrl = `${credentials.baseUrl || ""}`.trim();
  if (baseUrl) payload.llm_base_url = baseUrl;
  const model = `${credentials.model || ""}`.trim();
  if (model) payload.llm_model = model;
  return submitJson(
    buildApiEndpoint(API_PREFIX, `entities/${encodeURIComponent(entityId)}/page`),
    payload,
  ) as Promise<EntityPage>;
}

/** 零 LLM 成本：术语表种子 + 该文档全块字面扫描，可重复调用。 */
export function linkDocumentGraph(documentId: string): Promise<LinkGraphResult> {
  return submitJson(
    buildApiEndpoint(API_PREFIX, `documents/${encodeURIComponent(documentId)}/graph/link`),
    {},
  ) as Promise<LinkGraphResult>;
}

/** 一次 LLM 调用抽实体 + 关系；凭据留空由服务端回落启动配置。 */
export function extractDocumentGraph(
  documentId: string,
  credentials: ExtractCredentials = {},
): Promise<ExtractGraphResult> {
  const payload: Record<string, string> = {};
  const key = `${credentials.apiKey || ""}`.trim();
  if (key) payload.llm_api_key = key.replace(/^Bearer\s+/i, "").trim();
  const baseUrl = `${credentials.baseUrl || ""}`.trim();
  if (baseUrl) payload.llm_base_url = baseUrl;
  const model = `${credentials.model || ""}`.trim();
  if (model) payload.llm_model = model;
  return submitJson(
    buildApiEndpoint(API_PREFIX, `documents/${encodeURIComponent(documentId)}/graph/extract`),
    payload,
  ) as Promise<ExtractGraphResult>;
}

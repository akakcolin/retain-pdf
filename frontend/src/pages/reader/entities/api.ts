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

/** 子图里的一条有向边（方向由 from/to 表达，不返回相对 direction）。 */
export type NeighborhoodEdge = {
  from_entity_id: string;
  to_entity_id: string;
  relation_type: string;
  confidence: number;
  explanation: string;
  source_document_id: string;
};

/** 实体 N 跳关系子图：根 + 节点 + 有向边。 */
export type EntityNeighborhood = {
  root: string;
  nodes: EntitySummary[];
  edges: NeighborhoodEdge[];
};

export type LinkGraphResult = { document_id: string; entities: number; mentions: number };
export type RelinkGraphResult = LinkGraphResult & { removed: number };
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
  /** 生效正文（有修订用修订，否则模型原文）。 */
  body_md: string;
  citations: EntityPageCitation[];
  /** 正文里能解析到实体的 [[...]]；解析不到的不在列表里。 */
  links: EntityPageLink[];
  /** 是否存在人工修订。 */
  edited: boolean;
  /** 修订时间（空 = 无修订）。 */
  edited_at: string;
};

/** 反链：某个已生成的概念页正文里提到了本实体。snippet = 链接附近上下文。 */
export type EntityBacklink = {
  entity_id: string;
  name: string;
  entity_type: string;
  snippet: string;
};

/** 标注反查：某条收藏的引文/译文/备注提到了本实体（读取时现算）。 */
export type EntityFavorite = {
  favorite_id: string;
  document_id: string;
  document_title: string;
  job_id: string;
  page_idx: number;
  block_id: string;
  quote_text: string;
  translated_quote_text: string;
  note: string;
};

/** 待维护的概念页候选：缺页，或页的证据签名已变。 */
export type PendingEntityPage = {
  entity_id: string;
  name: string;
  entity_type: string;
  has_page: boolean;
  /** 有页但证据已更新 = 需要刷新。 */
  stale: boolean;
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

/** 实体 N 跳关系子图（depth 服务端 clamp 1..=2）。 */
export function getEntityNeighborhood(
  entityId: string,
  { depth = 2, limit = 50 }: { depth?: number; limit?: number } = {},
): Promise<EntityNeighborhood> {
  const params = new URLSearchParams();
  params.set("depth", String(Math.min(Math.max(1, depth), 2)));
  params.set("limit", String(Math.min(Math.max(1, limit), MAX_LIMIT)));
  return getOne<EntityNeighborhood>(
    `entities/${encodeURIComponent(entityId)}/neighborhood?${params.toString()}`,
  );
}

/** 哪些已生成的概念页提到了本实体（读取时现算）。 */
export function listEntityBacklinks(
  entityId: string,
  { limit = 50 }: { limit?: number } = {},
): Promise<EntityBacklink[]> {
  const params = new URLSearchParams();
  params.set("limit", String(Math.min(Math.max(1, limit), MAX_LIMIT)));
  return getList<EntityBacklink>(`entities/${encodeURIComponent(entityId)}/backlinks`, params);
}

/** 哪些收藏（引文/译文/备注）提到了本实体（读取时现算）。 */
export function listEntityFavorites(
  entityId: string,
  { limit = 50 }: { limit?: number } = {},
): Promise<EntityFavorite[]> {
  const params = new URLSearchParams();
  params.set("limit", String(Math.min(Math.max(1, limit), MAX_LIMIT)));
  return getList<EntityFavorite>(`entities/${encodeURIComponent(entityId)}/favorites`, params);
}

/** 该文档里缺页或页已陈旧的实体（批量维护用）。 */
export function listPendingEntityPages(
  documentId: string,
  { limit = 20 }: { limit?: number } = {},
): Promise<PendingEntityPage[]> {
  const params = new URLSearchParams();
  params.set("limit", String(Math.min(Math.max(1, limit), MAX_LIMIT)));
  return getList<PendingEntityPage>(
    `documents/${encodeURIComponent(documentId)}/graph/pending-pages`,
    params,
  );
}

/** PATCH 一个 JSON 端点并解包信封（submitJson 只支持 POST）。 */
async function patchJson<T>(path: string, payload: unknown): Promise<T> {
  const resp = await fetch(buildApiEndpoint(API_PREFIX, path), {
    method: "PATCH",
    headers: buildApiHeaders({ "Content-Type": "application/json" }),
    body: JSON.stringify(payload),
  });
  if (!resp.ok) {
    throw new Error(`保存概念页失败，请稍后重试。(${resp.status})`);
  }
  return unwrapEnvelope<T>(await resp.json());
}

/** 读该实体的概念页（未生成时 has_page=false，不报错）。 */
export function getEntityPage(entityId: string): Promise<EntityPage> {
  return getOne<EntityPage>(`entities/${encodeURIComponent(entityId)}/page`);
}

/** 生成/刷新概念页（一次 LLM 调用）；凭据留空由服务端回落启动配置。 */
export function generateEntityPage(
  entityId: string,
  credentials: ExtractCredentials = {},
  { overwriteManual = false }: { overwriteManual?: boolean } = {},
): Promise<EntityPage> {
  const payload: Record<string, unknown> = {};
  const key = `${credentials.apiKey || ""}`.trim();
  if (key) payload.llm_api_key = key.replace(/^Bearer\s+/i, "").trim();
  const baseUrl = `${credentials.baseUrl || ""}`.trim();
  if (baseUrl) payload.llm_base_url = baseUrl;
  const model = `${credentials.model || ""}`.trim();
  if (model) payload.llm_model = model;
  if (overwriteManual) payload.overwrite_manual = true;
  return submitJson(
    buildApiEndpoint(API_PREFIX, `entities/${encodeURIComponent(entityId)}/page`),
    payload,
  ) as Promise<EntityPage>;
}

/** 保存人工修订（不花 token）。 */
export function saveEntityPage(entityId: string, bodyMd: string): Promise<EntityPage> {
  return patchJson<EntityPage>(`entities/${encodeURIComponent(entityId)}/page`, {
    body_md: bodyMd,
  });
}

/** 撤销人工修订，回到模型原文（不花 token）。 */
export function revertEntityPage(entityId: string): Promise<EntityPage> {
  return patchJson<EntityPage>(`entities/${encodeURIComponent(entityId)}/page`, { revert: true });
}

/** 零 LLM 成本：术语表种子 + 该文档全块字面扫描，可重复调用。 */
export function linkDocumentGraph(documentId: string): Promise<LinkGraphResult> {
  return submitJson(
    buildApiEndpoint(API_PREFIX, `documents/${encodeURIComponent(documentId)}/graph/link`),
    {},
  ) as Promise<LinkGraphResult>;
}

/** 零 LLM 成本：用边界匹配器重扫本文档，差量修正误挂/漏挂。 */
export function relinkDocumentGraph(documentId: string): Promise<RelinkGraphResult> {
  return submitJson(
    buildApiEndpoint(API_PREFIX, `documents/${encodeURIComponent(documentId)}/graph/relink`),
    {},
  ) as Promise<RelinkGraphResult>;
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

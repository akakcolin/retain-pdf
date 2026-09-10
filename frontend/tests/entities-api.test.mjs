import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

// 概念图谱前端：api 模块（URL/查询/信封/错误/抽取凭据）+ 展示文案映射。
// 组件本身不在这里渲染（面板依赖阅读器运行时），契约由 api 层锁住。

const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/" });
for (const key of ["window", "document", "localStorage"]) {
  Object.defineProperty(globalThis, key, {
    value: dom.window[key] ?? dom.window,
    writable: true,
    configurable: true,
  });
}
globalThis.window = dom.window;

const {
  listDocumentEntities,
  listEntityMentions,
  listEntityRelations,
  listEntityBacklinks,
  listEntityFavorites,
  listPendingEntityPages,
  getEntityNeighborhood,
  linkDocumentGraph,
  relinkDocumentGraph,
  extractDocumentGraph,
  getEntityPage,
  generateEntityPage,
  saveEntityPage,
  revertEntityPage,
  searchEntities,
  renameEntity,
  mergeEntities,
} = await import("../src/pages/reader/entities/api.js");
const {
  entityTypeLabel,
  relationTypeLabel,
  directionArrow,
  mentionPageLabel,
} = await import("../src/pages/reader/entities/labels.js");

function jsonResponse(payload, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: () => "application/json" },
    json: async () => payload,
    text: async () => JSON.stringify(payload),
  };
}

function stubFetch(payload, status = 200) {
  const calls = [];
  globalThis.fetch = async (url, init = {}) => {
    calls.push({ url, init });
    return jsonResponse(payload, status);
  };
  return calls;
}

test("listDocumentEntities 带 document_id/limit 并解包 items", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: { items: [{ entity_id: "ent-1", name: "卤素", entity_type: "term" }] },
  });
  const items = await listDocumentEntities("doc-1", 20);
  assert.equal(items.length, 1);
  assert.equal(items[0].entity_id, "ent-1");
  const url = new URL(calls[0].url);
  assert.equal(url.pathname, "/api/v1/entities");
  assert.equal(url.searchParams.get("document_id"), "doc-1");
  assert.equal(url.searchParams.get("limit"), "20");
});

test("mentions/relations 端点与可选过滤参数", async () => {
  let calls = stubFetch({ code: 0, message: "ok", data: { items: [] } });
  await listEntityMentions("ent-9", { documentId: "doc-2" });
  assert.equal(new URL(calls[0].url).pathname, "/api/v1/entities/ent-9/mentions");
  assert.equal(new URL(calls[0].url).searchParams.get("document_id"), "doc-2");

  calls = stubFetch({ code: 0, message: "ok", data: { items: [] } });
  await listEntityRelations("ent-9", { relationType: "uses" });
  const url = new URL(calls[0].url);
  assert.equal(url.pathname, "/api/v1/entities/ent-9/relations");
  assert.equal(url.searchParams.get("relation_type"), "uses");
  assert.equal(url.searchParams.get("document_id"), null);
});

test("listEntityBacklinks 端点、limit 与解包", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: { items: [{ entity_id: "ent-3", name: "QM9", entity_type: "dataset", snippet: "…[[GNN]]…" }] },
  });
  const items = await listEntityBacklinks("ent-9", { limit: 10 });
  assert.equal(items.length, 1);
  assert.equal(items[0].name, "QM9");
  assert.equal(items[0].snippet, "…[[GNN]]…");
  const url = new URL(calls[0].url);
  assert.equal(url.pathname, "/api/v1/entities/ent-9/backlinks");
  assert.equal(url.searchParams.get("limit"), "10");
});

test("listEntityFavorites 端点、limit 与解包", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: {
      items: [
        {
          favorite_id: "fav-1",
          document_id: "doc-1",
          document_title: "化学",
          page_idx: 2,
          quote_text: "卤素是一类元素",
          note: "重点",
        },
      ],
    },
  });
  const items = await listEntityFavorites("ent-9", { limit: 10 });
  assert.equal(items.length, 1);
  assert.equal(items[0].favorite_id, "fav-1");
  assert.equal(items[0].document_title, "化学");
  assert.equal(items[0].note, "重点");
  const url = new URL(calls[0].url);
  assert.equal(url.pathname, "/api/v1/entities/ent-9/favorites");
  assert.equal(url.searchParams.get("limit"), "10");
});

test("listPendingEntityPages 端点、limit 与解包", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: {
      items: [
        { entity_id: "ent-2", name: "QM9", entity_type: "dataset", has_page: true, stale: true },
        { entity_id: "ent-1", name: "GNN", entity_type: "method", has_page: false, stale: false },
      ],
    },
  });
  const items = await listPendingEntityPages("doc-1");
  assert.equal(items.length, 2);
  assert.equal(items[0].entity_id, "ent-2");
  assert.equal(items[0].stale, true);
  assert.equal(items[1].has_page, false);
  const url = new URL(calls[0].url);
  assert.equal(url.pathname, "/api/v1/documents/doc-1/graph/pending-pages");
  assert.equal(url.searchParams.get("limit"), "20");
});

test("listPendingEntityPages 非 2xx 抛错", async () => {
  stubFetch({ code: 1, message: "boom" }, 500);
  await assert.rejects(() => listPendingEntityPages("doc-1"), /500/);
});

test("getEntityNeighborhood 带 depth/limit 并解包 root/nodes/edges", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: {
      root: "ent-1",
      nodes: [
        { entity_id: "ent-1", name: "GNN", entity_type: "method", aliases: [], mention_count: 5, document_count: 2 },
        { entity_id: "ent-2", name: "QM9", entity_type: "dataset", aliases: [], mention_count: 1, document_count: 1 },
      ],
      edges: [
        {
          from_entity_id: "ent-1",
          to_entity_id: "ent-2",
          relation_type: "evaluates",
          confidence: 0.8,
          explanation: "同一句",
          source_document_id: "doc-1",
        },
      ],
    },
  });
  const view = await getEntityNeighborhood("ent-1", { depth: 2, limit: 40 });
  assert.equal(view.root, "ent-1");
  assert.equal(view.nodes.length, 2);
  assert.equal(view.edges[0].relation_type, "evaluates");
  assert.equal(view.edges[0].to_entity_id, "ent-2");
  const url = new URL(calls[0].url);
  assert.equal(url.pathname, "/api/v1/entities/ent-1/neighborhood");
  assert.equal(url.searchParams.get("depth"), "2");
  assert.equal(url.searchParams.get("limit"), "40");
});

test("getEntityNeighborhood clamp depth/limit，非 2xx 抛错", async () => {
  let calls = stubFetch({ code: 0, message: "ok", data: { root: "ent-1", nodes: [], edges: [] } });
  await getEntityNeighborhood("ent-1", { depth: 9, limit: 9999 });
  const url = new URL(calls[0].url);
  assert.equal(url.searchParams.get("depth"), "2");
  assert.equal(url.searchParams.get("limit"), "200");

  calls = stubFetch({ code: 1, message: "boom" }, 500);
  await assert.rejects(() => getEntityNeighborhood("ent-1"), /500/);
});

test("非 2xx 抛带状态码的错误", async () => {
  stubFetch({ code: 1, message: "boom" }, 500);
  await assert.rejects(() => listDocumentEntities("doc-1"), /500/);
});

test("linkDocumentGraph POST 到 graph/link 并解包统计", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: { document_id: "doc-1", entities: 3, mentions: 7 },
  });
  const result = await linkDocumentGraph("doc-1");
  assert.equal(result.mentions, 7);
  assert.equal(calls[0].init.method, "POST");
  assert.equal(new URL(calls[0].url).pathname, "/api/v1/documents/doc-1/graph/link");
});

test("relinkDocumentGraph POST 到 graph/relink，空 body 并解包差量", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: { document_id: "doc-1", entities: 3, mentions: 2, removed: 1 },
  });
  const result = await relinkDocumentGraph("doc-1");
  assert.equal(result.mentions, 2);
  assert.equal(result.removed, 1);
  assert.equal(calls[0].init.method, "POST");
  assert.equal(new URL(calls[0].url).pathname, "/api/v1/documents/doc-1/graph/relink");
  assert.deepEqual(JSON.parse(calls[0].init.body), {}, "零 token 路径不带凭据");
});

test("extractDocumentGraph 上传凭据：Bearer 前缀剥掉、空字段不带", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: { document_id: "doc-1", entities: 2, mentions: 4, relations: 1 },
  });
  const result = await extractDocumentGraph("doc-1", {
    apiKey: "Bearer sk-test",
    baseUrl: "https://api.example.com",
    model: "",
  });
  assert.equal(result.relations, 1);
  const body = JSON.parse(calls[0].init.body);
  assert.equal(body.llm_api_key, "sk-test");
  assert.equal(body.llm_base_url, "https://api.example.com");
  assert.equal("llm_model" in body, false);
  assert.equal(new URL(calls[0].url).pathname, "/api/v1/documents/doc-1/graph/extract");
});

test("getEntityPage GET 并解包概念页", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: {
      entity_id: "ent-1",
      has_page: true,
      stale: false,
      body_md: "正文 [1]。",
      citations: [{ ref: 1, document_id: "doc-1", page_idx: 3 }],
      links: [{ surface: "图神经网络", entity_id: "ent-2", name: "GNN", entity_type: "method", aliases: [] }],
    },
  });
  const page = await getEntityPage("ent-1");
  assert.equal(page.has_page, true);
  assert.equal(page.citations[0].ref, 1);
  assert.equal(page.links[0].entity_id, "ent-2");
  assert.equal(new URL(calls[0].url).pathname, "/api/v1/entities/ent-1/page");
});

test("generateEntityPage POST 带凭据：Bearer 剥掉、空字段不带", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: { entity_id: "ent-1", has_page: true, citations: [] },
  });
  await generateEntityPage("ent-1", { apiKey: "Bearer sk-x", baseUrl: "", model: "m1" });
  const body = JSON.parse(calls[0].init.body);
  assert.equal(body.llm_api_key, "sk-x");
  assert.equal(body.llm_model, "m1");
  assert.equal("llm_base_url" in body, false);
  assert.equal(calls[0].init.method, "POST");
  assert.equal(new URL(calls[0].url).pathname, "/api/v1/entities/ent-1/page");
});

test("saveEntityPage PATCH 到 page 并带 body_md", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: { entity_id: "ent-1", has_page: true, edited: true, body_md: "人工修订" },
  });
  const page = await saveEntityPage("ent-1", "人工修订");
  assert.equal(page.edited, true);
  assert.equal(calls[0].init.method, "PATCH");
  assert.equal(new URL(calls[0].url).pathname, "/api/v1/entities/ent-1/page");
  assert.deepEqual(JSON.parse(calls[0].init.body), { body_md: "人工修订" });
});

test("revertEntityPage PATCH 并带 revert:true", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: { entity_id: "ent-1", has_page: true, edited: false, body_md: "模型原文" },
  });
  const page = await revertEntityPage("ent-1");
  assert.equal(page.edited, false);
  assert.equal(calls[0].init.method, "PATCH");
  assert.deepEqual(JSON.parse(calls[0].init.body), { revert: true });
});

test("saveEntityPage 非 2xx 抛带状态码的错误", async () => {
  stubFetch({ code: 1, message: "boom" }, 500);
  await assert.rejects(() => saveEntityPage("ent-1", "x"), /500/);
});

test("generateEntityPage 仅在要求时带 overwrite_manual", async () => {
  let calls = stubFetch({
    code: 0,
    message: "ok",
    data: { entity_id: "ent-1", has_page: true, edited: false, body_md: "x" },
  });
  await generateEntityPage("ent-1", { apiKey: "sk-test" });
  assert.equal("overwrite_manual" in JSON.parse(calls[0].init.body), false);

  calls = stubFetch({
    code: 0,
    message: "ok",
    data: { entity_id: "ent-1", has_page: true, edited: false, body_md: "x" },
  });
  await generateEntityPage("ent-1", { apiKey: "sk-test" }, { overwriteManual: true });
  assert.equal(JSON.parse(calls[0].init.body).overwrite_manual, true);
});

test("renameEntity PATCH 到 entities/:id 并带 name", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: {
      entity_id: "ent-1",
      name: "Graph Neural Network",
      name_norm: "graph neural network",
      entity_type: "method",
      aliases: ["GNN"],
      description: "",
      created_at: "",
      updated_at: "",
    },
  });
  const record = await renameEntity("ent-1", "Graph Neural Network");
  assert.equal(record.name, "Graph Neural Network");
  assert.equal(record.name_norm, "graph neural network");
  assert.equal(calls[0].init.method, "PATCH");
  assert.equal(new URL(calls[0].url).pathname, "/api/v1/entities/ent-1");
  assert.deepEqual(JSON.parse(calls[0].init.body), { name: "Graph Neural Network" });
});

test("renameEntity 撞名 409 带上服务端提示与状态码", async () => {
  stubFetch({ code: 40900, message: "已存在同名同类型实体「Transformer」,请改用合并", data: null }, 409);
  await assert.rejects(
    () => renameEntity("ent-1", "transformer"),
    /已存在同名同类型实体.*\(409\)/,
  );
});

test("mergeEntities POST 到 entities/:id/merge 并带 source_entity_ids", async () => {
  const calls = stubFetch({
    code: 0,
    message: "ok",
    data: {
      target: { entity_id: "ent-1", name: "GNN", entity_type: "method", aliases: ["图神经网络"] },
      merged: ["ent-2"],
      mentions: 5,
      relations: 2,
      page_adopted: true,
    },
  });
  const result = await mergeEntities("ent-1", ["ent-2"]);
  assert.equal(result.merged[0], "ent-2");
  assert.equal(result.page_adopted, true);
  assert.equal(calls[0].init.method, "POST");
  assert.equal(new URL(calls[0].url).pathname, "/api/v1/entities/ent-1/merge");
  assert.deepEqual(JSON.parse(calls[0].init.body), { source_entity_ids: ["ent-2"] });
});

test("searchEntities GET 且空 query/entity_type 省略", async () => {
  let calls = stubFetch({
    code: 0,
    message: "ok",
    data: { items: [{ entity_id: "ent-9", name: "QM9", entity_type: "dataset" }] },
  });
  const items = await searchEntities("", { limit: 20 });
  assert.equal(items.length, 1);
  let url = new URL(calls[0].url);
  assert.equal(url.pathname, "/api/v1/entities");
  assert.equal(url.searchParams.get("query"), null);
  assert.equal(url.searchParams.get("entity_type"), null);
  assert.equal(url.searchParams.get("limit"), "20");

  calls = stubFetch({ code: 0, message: "ok", data: { items: [] } });
  await searchEntities("  gnn ", { entityType: "method" });
  url = new URL(calls[0].url);
  assert.equal(url.searchParams.get("query"), "gnn");
  assert.equal(url.searchParams.get("entity_type"), "method");
});

test("展示文案：词表命中翻译，未知值原样回显", () => {
  assert.equal(entityTypeLabel("method"), "方法");
  assert.equal(entityTypeLabel("widget"), "widget");
  assert.equal(entityTypeLabel(""), "概念");
  assert.equal(relationTypeLabel("improves_on"), "改进");
  assert.equal(relationTypeLabel("bespoke"), "bespoke");
  assert.equal(directionArrow("in"), "←");
  assert.equal(directionArrow("out"), "→");
  assert.equal(mentionPageLabel(0), "第 1 页");
  assert.equal(mentionPageLabel(12), "第 13 页");
});

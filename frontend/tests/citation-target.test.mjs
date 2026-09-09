import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

// 引用跳转目标：同文档就地跳页；跨文档（另一篇 + 带 job_id）整页跳目标文献。
// 阅读器常态是 job 模式（session.documentId 为空），所以跨文档必须靠 job_id 判定。

const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/" });
globalThis.window = dom.window;
globalThis.document = dom.window.document;

const { resolveCitationTarget } = await import("../src/pages/reader/citation-target.ts");

test("job 模式跨文档：job_id 不同 → reader.html URL（带目标 job_id/page_idx/block_id）", () => {
  const target = resolveCitationTarget(
    { document_id: "doc-2", job_id: "job-2", page_idx: 4, block_id: "p005-b0001" },
    { currentJobId: "job-1", currentDocumentId: "" },
  );
  assert.equal(target.kind, "document");
  const url = new URL(target.url);
  assert.equal(url.pathname, "/reader.html");
  assert.equal(url.searchParams.get("job_id"), "job-2");
  assert.equal(url.searchParams.get("page_idx"), "4");
  assert.equal(url.searchParams.get("block_id"), "p005-b0001");
});

test("job 模式同文档：job_id 相同 → 就地跳页（page_idx 0 基 → 1 基）", () => {
  const target = resolveCitationTarget(
    { document_id: "doc-1", job_id: "job-1", page_idx: 2 },
    { currentJobId: "job-1", currentDocumentId: "" },
  );
  assert.deepEqual(target, { kind: "page", page1: 3 });
});

test("跨文档但缺 job_id → 回落页码（不跳错文档）", () => {
  const target = resolveCitationTarget(
    { document_id: "doc-2", page_idx: 2 },
    { currentJobId: "job-1", currentDocumentId: "" },
  );
  assert.deepEqual(target, { kind: "page", page1: 3 });
});

test("document 模式（无 job）按 document_id 判定跨文档", () => {
  const target = resolveCitationTarget(
    { document_id: "doc-2", job_id: "job-2", page_idx: 0 },
    { currentJobId: "", currentDocumentId: "doc-1" },
  );
  assert.equal(target.kind, "document");
  assert.equal(new URL(target.url).searchParams.get("page_idx"), "0");
});

test("document 模式同文档 → 就地跳页", () => {
  const target = resolveCitationTarget(
    { document_id: "doc-1", job_id: "job-2", page_idx: 1 },
    { currentJobId: "", currentDocumentId: "doc-1" },
  );
  assert.deepEqual(target, { kind: "page", page1: 2 });
});

test("判不出当前上下文 → 一律回落页码", () => {
  const target = resolveCitationTarget(
    { document_id: "doc-2", job_id: "job-2", page_idx: 0 },
    {},
  );
  assert.deepEqual(target, { kind: "page", page1: 1 });
});

test("跨文档且 page_idx=0 仍写进 URL（不被当空值丢掉）", () => {
  const target = resolveCitationTarget(
    { document_id: "doc-2", job_id: "job-2", page_idx: 0 },
    { currentJobId: "job-1" },
  );
  assert.equal(target.kind, "document");
  assert.equal(new URL(target.url).searchParams.get("page_idx"), "0");
});

test("只带 page 的引用按 1 基解析", () => {
  assert.deepEqual(
    resolveCitationTarget({ page: 5 }, { currentJobId: "job-1" }),
    { kind: "page", page1: 5 },
  );
});

test("只带 block_id 的引用从 p00N 解析页码", () => {
  assert.deepEqual(
    resolveCitationTarget({ block_id: "p009-b0010" }, { currentJobId: "job-1" }),
    { kind: "page", page1: 9 },
  );
});

test("空引用/无页码返回 null", () => {
  assert.equal(resolveCitationTarget(null, { currentJobId: "job-1" }), null);
  assert.equal(resolveCitationTarget(undefined, { currentJobId: "job-1" }), null);
  assert.equal(resolveCitationTarget({}, { currentJobId: "job-1" }), null);
  assert.equal(
    resolveCitationTarget({ document_id: "doc-1" }, { currentJobId: "job-1" }),
    null,
  );
});

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import {
  READING_STATUS_META,
  filterDocuments,
  highlightSegments,
  nextReadingStatus,
} from "../src/js/islands/library-search/view-model.js";

// 岛 buildPorts 需要 DOM + customElements(自定义元素在 import 时 define),
// 这里先铺好全局再动态 import 岛模块。
const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/" });
for (const key of ["window", "document", "HTMLElement", "CustomEvent", "Event", "Node", "MutationObserver", "customElements"]) {
  Object.defineProperty(globalThis, key, {
    value: dom.window[key] ?? dom.window,
    writable: true,
    configurable: true,
  });
}
globalThis.window = dom.window;

test("highlightSegments 把 [ ] 包裹的命中词拆成高亮分段", () => {
  assert.deepEqual(highlightSegments("…考察了[光谱]系列中的[交换]反应…"), [
    { text: "…考察了", hit: false },
    { text: "光谱", hit: true },
    { text: "系列中的", hit: false },
    { text: "交换", hit: true },
    { text: "反应…", hit: false },
  ]);
  assert.deepEqual(highlightSegments("无命中"), [{ text: "无命中", hit: false }]);
  assert.deepEqual(highlightSegments(""), []);
});

test("nextReadingStatus 按 未读→在读→读完 循环", () => {
  assert.equal(nextReadingStatus("unread"), "reading");
  assert.equal(nextReadingStatus("reading"), "done");
  assert.equal(nextReadingStatus("done"), "unread");
  assert.equal(nextReadingStatus("bogus"), "unread");
  assert.deepEqual(Object.keys(READING_STATUS_META), ["unread", "reading", "done"]);
});

test("filterDocuments 按标题/文件名/标签匹配并叠加状态过滤", () => {
  const documents = [
    { document_id: "a", title: "光谱分析", source_filename: "spec.pdf", tags: [], reading_status: "reading" },
    { document_id: "b", title: "Attention", source_filename: "attn.pdf", tags: ["机器学习"], reading_status: "done" },
    { document_id: "c", title: "Scaling", source_filename: "scaling.pdf", tags: [], reading_status: "unread" },
  ];
  assert.deepEqual(filterDocuments(documents, { query: "光谱" }).map((d) => d.document_id), ["a"]);
  assert.deepEqual(filterDocuments(documents, { query: "机器学习" }).map((d) => d.document_id), ["b"]);
  assert.deepEqual(filterDocuments(documents, { query: "pdf" }).map((d) => d.document_id), ["a", "b", "c"]);
  assert.deepEqual(filterDocuments(documents, { query: "pdf", readingStatus: "done" }).map((d) => d.document_id), ["b"]);
  assert.deepEqual(filterDocuments(documents, {}).length, 3);
});

test("库检索岛:仅馆藏文档(无 job_id)也派发打开阅读事件(回归)", async () => {
  // 回归覆盖:openReader 原来 `if (!jobId) return`,无 job 的馆藏文档点行没反应。
  // ReaderDialog 已支持 documentId-only 导航,这里必须把事件派出去。
  const { APP_EVENTS } = await import("../src/js/contracts/app-contract.js");
  await import("../src/js/islands/library-search/index.js");

  const el = dom.window.document.createElement("library-search-island");
  dom.window.document.body.appendChild(el);

  const seen = [];
  el.addEventListener(APP_EVENTS.openReaderRequested, (event) => seen.push(event.detail));
  const ports = el.buildPorts();

  ports.openReader({ document_id: "doc-lib-only", job_id: "" });
  assert.equal(seen.length, 1, "带 documentId 就应派发");
  assert.equal(seen[0].documentId, "doc-lib-only");
  assert.equal(seen[0].jobId, "");

  ports.openReader({ document_id: "", job_id: "" });
  assert.equal(seen.length, 1, "jobId 与 documentId 都空才丢弃");
});

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

// 概念图谱面板：本文档实体列表 → 实体详情（证据可跳页 / 关系可继续游走）。
// 网络层用桩 fetch 按 URL 路由，只验面板状态机与回调，不碰真实后端。

const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/reader.html" });
for (const key of [
  "window", "document", "HTMLElement", "MouseEvent", "Node", "localStorage",
  "getComputedStyle", "requestAnimationFrame", "cancelAnimationFrame",
]) {
  Object.defineProperty(globalThis, key, {
    value: dom.window[key] ?? dom.window,
    writable: true,
    configurable: true,
  });
}
globalThis.window = dom.window;
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const { act, createElement } = await import("react");
const { createRoot } = await import("react-dom/client");
const { ReaderEntitiesPanel } = await import("../src/pages/reader/components/react-pdf/index.js");

const ENTITY = {
  entity_id: "ent-1",
  name: "卤素",
  entity_type: "term",
  aliases: ["halogen"],
  mention_count: 2,
  document_count: 1,
};
const MENTION = {
  document_id: "doc-1",
  job_id: "job-1",
  page_idx: 2,
  block_id: "p003-b0000",
  snippet: "卤素是一类元素",
};
const RELATION = {
  entity_id: "ent-2",
  name: "置换反应",
  entity_type: "concept",
  aliases: [],
  mention_count: 1,
  document_count: 1,
  relation_type: "uses",
  direction: "out",
  confidence: 0.8,
  explanation: "卤素参与置换",
  source_document_id: "doc-1",
};

function stubFetch() {
  const calls = [];
  globalThis.fetch = async (url, init = {}) => {
    calls.push({ url, init });
    const path = new URL(url).pathname;
    let data = { items: [ENTITY] };
    if (path.endsWith("/mentions")) data = { items: [MENTION] };
    else if (path.endsWith("/relations")) data = { items: [RELATION] };
    return {
      ok: true,
      status: 200,
      headers: { get: () => "application/json" },
      json: async () => ({ code: 0, message: "ok", data }),
      text: async () => "",
    };
  };
  return calls;
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitFor(predicate, description) {
  const deadline = Date.now() + 3000;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await wait(15);
  }
  assert.fail(`等待超时：${description}`);
}

function findByText(host, text) {
  return [...host.querySelectorAll("button")].find((el) => el.textContent.includes(text));
}

test("面板：列表 → 详情 → 证据跳页", async () => {
  const calls = stubFetch();
  const jumps = [];
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  await act(async () => {
    root.render(createElement(ReaderEntitiesPanel, {
      open: true,
      jobId: "job-1",
      documentId: "doc-1",
      onClose() {},
      onJumpPage: (page) => jumps.push(page),
    }));
  });

  await waitFor(() => host.textContent.includes("卤素"), "实体列表渲染");
  assert.equal(new URL(calls[0].url).searchParams.get("document_id"), "doc-1");
  assert.ok(host.textContent.includes("术语"), "实体类型显示中文标签");

  await act(async () => {
    findByText(host, "卤素").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });

  await waitFor(() => host.textContent.includes("置换反应"), "关系渲染");
  assert.ok(host.textContent.includes("卤素是一类元素"), "证据片段渲染");
  assert.ok(host.textContent.includes("使用 →"), "关系类型 + 方向箭头");
  assert.ok(calls.some((c) => c.url.includes("/entities/ent-1/mentions")));
  assert.ok(calls.some((c) => c.url.includes("/entities/ent-1/relations")));

  await act(async () => {
    findByText(host, "第 3 页").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.deepEqual(jumps, [3]);

  await act(async () => {
    root.unmount();
  });
});

test("面板：无文档时给出提示，不发请求", async () => {
  const calls = stubFetch();
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  await act(async () => {
    root.render(createElement(ReaderEntitiesPanel, {
      open: true,
      jobId: "",
      documentId: "",
      onClose() {},
      onJumpPage() {},
    }));
  });

  await waitFor(() => host.textContent.includes("没有可关联的文档"), "空文档提示");
  assert.equal(calls.length, 0);

  await act(async () => {
    root.unmount();
  });
});

test("面板：解析文档失败时报错，不误报「没有文档」", async () => {
  globalThis.fetch = async (url) => {
    if (new URL(url).pathname.endsWith("/documents")) {
      return {
        ok: false,
        status: 500,
        headers: { get: () => "application/json" },
        json: async () => ({}),
        text: async () => "",
      };
    }
    throw new Error(`unexpected request: ${url}`);
  };
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  await act(async () => {
    root.render(createElement(ReaderEntitiesPanel, {
      open: true,
      jobId: "job-1",
      documentId: "",
      onClose() {},
      onJumpPage() {},
    }));
  });

  await waitFor(() => host.textContent.includes("解析文档失败"), "解析失败提示");
  assert.ok(!host.textContent.includes("没有可关联的文档"), "不把网络失败误报成无文档");

  await act(async () => {
    root.unmount();
  });
});

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
const BACKLINK = {
  entity_id: "ent-3",
  name: "元素周期表",
  entity_type: "concept",
  snippet: "…卤素属于 [[元素周期表]] 的第 17 族…",
};

function stubFetch() {
  const calls = [];
  globalThis.fetch = async (url, init = {}) => {
    calls.push({ url, init });
    const path = new URL(url).pathname;
    let data = { items: [ENTITY] };
    if (path.endsWith("/mentions")) data = { items: [MENTION] };
    else if (path.endsWith("/relations")) data = { items: [RELATION] };
    else if (path.endsWith("/backlinks")) data = { items: [] };
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

// 注入的 [n] 按钮会过滤 isTrusted=false 的合成事件（防幽灵点击）；真实用户点击不受影响。
// 测试里临时隐藏全局 MouseEvent，让 jsdom 的合成点击穿过这层守卫。
function clickCitation(el) {
  const saved = globalThis.MouseEvent;
  try {
    delete globalThis.MouseEvent;
    el.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  } finally {
    globalThis.MouseEvent = saved;
  }
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
  assert.ok(host.textContent.includes("还没有概念页提到它。"), "无反链时给空态提示");
  assert.ok(calls.some((c) => c.url.includes("/entities/ent-1/mentions")));
  assert.ok(calls.some((c) => c.url.includes("/entities/ent-1/relations")));
  assert.ok(calls.some((c) => c.url.includes("/entities/ent-1/backlinks")));

  await act(async () => {
    findByText(host, "第 3 页").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.deepEqual(jumps, [3]);

  await act(async () => {
    root.unmount();
  });
});

const PAGE = {
  entity_id: "ent-1",
  name: "卤素",
  entity_type: "term",
  has_page: true,
  stale: false,
  generated_at: "2026-09-09T00:00:00Z",
  body_md: "卤素是一类元素 [1]。",
  links: [],
  citations: [
    {
      ref: 1,
      document_id: "doc-1",
      document_title: "化学",
      job_id: "job-1",
      page_idx: 4,
      block_id: "p005-b0000",
      snippet: "卤素是一类元素",
    },
  ],
};

function stubFetchWithPage(page, backlinks = []) {
  const calls = [];
  globalThis.fetch = async (url, init = {}) => {
    calls.push({ url, init });
    const path = new URL(url).pathname;
    let data = { items: [ENTITY] };
    if (path.endsWith("/mentions")) data = { items: [MENTION] };
    else if (path.endsWith("/relations")) data = { items: [RELATION] };
    else if (path.endsWith("/backlinks")) data = { items: backlinks };
    else if (path.endsWith("/page")) data = page;
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

test("面板：概念页正文渲染 + [n] 跳页", async () => {
  const calls = stubFetchWithPage(PAGE);
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
  await act(async () => {
    findByText(host, "卤素").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });

  await waitFor(
    () => host.querySelector(".reader-entities-page-body")?.textContent.includes("卤素是一类元素"),
    "概念页正文渲染",
  );
  assert.ok(calls.some((c) => c.url.includes("/entities/ent-1/page")), "详情并行拉概念页");
  const citeBtn = host.querySelector("button.reader-ai-citation-ref");
  assert.ok(citeBtn, "正文 [1] 变成可点击引用");
  await act(async () => {
    clickCitation(citeBtn);
  });
  assert.deepEqual(jumps, [5]);

  await act(async () => {
    root.unmount();
  });
});

test("面板：跨文档引用不跳页，提示来源", async () => {
  stubFetchWithPage({
    ...PAGE,
    citations: [{ ...PAGE.citations[0], document_id: "doc-2", document_title: "其他文献" }],
  });
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
  await act(async () => {
    findByText(host, "卤素").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  await waitFor(
    () => host.querySelector("button.reader-ai-citation-ref"),
    "引用按钮渲染",
  );
  await act(async () => {
    clickCitation(host.querySelector("button.reader-ai-citation-ref"));
  });
  assert.deepEqual(jumps, [], "跨文档引用不跳当前文档");
  assert.ok(host.textContent.includes("其他文献"), "提示引用来源文档");

  await act(async () => {
    root.unmount();
  });
});

test("面板：wikilink 点击切到目标实体", async () => {
  const link = {
    surface: "图神经网络",
    entity_id: "ent-2",
    name: "图神经网络",
    entity_type: "method",
    aliases: [],
  };
  const calls = stubFetchWithPage({
    ...PAGE,
    body_md: "卤素与 [[图神经网络]] 有关 [1]。",
    links: [link],
  });
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  await act(async () => {
    root.render(createElement(ReaderEntitiesPanel, {
      open: true,
      jobId: "job-1",
      documentId: "doc-1",
      onClose() {},
      onJumpPage() {},
    }));
  });

  await waitFor(() => host.textContent.includes("卤素"), "实体列表渲染");
  await act(async () => {
    findByText(host, "卤素").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });

  await waitFor(
    () => host.querySelector("button.reader-entities-wikilink"),
    "wikilink 渲染成按钮",
  );
  await act(async () => {
    host.querySelector("button.reader-entities-wikilink").click();
  });
  await waitFor(
    () => calls.some((c) => c.url.includes("/entities/ent-2/mentions")),
    "切到目标实体",
  );
  assert.ok(host.textContent.includes("图神经网络"), "详情标题换成目标实体");

  await act(async () => {
    root.unmount();
  });
});

test("面板：全部未解析的 wikilink 不留括号", async () => {
  stubFetchWithPage({ ...PAGE, body_md: "卤素与 [[不存在]] 无关 [1]。", links: [] });
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  await act(async () => {
    root.render(createElement(ReaderEntitiesPanel, {
      open: true,
      jobId: "job-1",
      documentId: "doc-1",
      onClose() {},
      onJumpPage() {},
    }));
  });

  await waitFor(() => host.textContent.includes("卤素"), "实体列表渲染");
  await act(async () => {
    findByText(host, "卤素").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  await waitFor(
    () => host.querySelector(".reader-entities-page-body")?.textContent.includes("无关"),
    "概念页正文渲染",
  );
  assert.equal(host.querySelectorAll("button.reader-entities-wikilink").length, 0);
  assert.ok(!host.querySelector(".reader-entities-page-body").textContent.includes("[["), "括号不漏到界面");

  await act(async () => {
    root.unmount();
  });
});

test("面板：反链段渲染并点击切到来源实体", async () => {
  const calls = stubFetchWithPage(PAGE, [BACKLINK]);
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  await act(async () => {
    root.render(createElement(ReaderEntitiesPanel, {
      open: true,
      jobId: "job-1",
      documentId: "doc-1",
      onClose() {},
      onJumpPage() {},
    }));
  });

  await waitFor(() => host.textContent.includes("卤素"), "实体列表渲染");
  await act(async () => {
    findByText(host, "卤素").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });

  await waitFor(() => host.textContent.includes("元素周期表"), "反链行渲染");
  assert.ok(host.textContent.includes("被提及"), "反链段标题");
  assert.ok(host.textContent.includes("第 17 族"), "反链片段渲染");

  const backlinkBtn = [...host.querySelectorAll("button.reader-entities-relation-name")].find((el) =>
    el.textContent.includes("元素周期表"),
  );
  assert.ok(backlinkBtn, "反链名是可点按钮");
  await act(async () => {
    backlinkBtn.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  await waitFor(
    () => calls.some((c) => c.url.includes("/entities/ent-3/mentions")),
    "切到反链来源实体",
  );
  assert.ok(host.textContent.includes("元素周期表"), "详情标题换成来源实体");

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

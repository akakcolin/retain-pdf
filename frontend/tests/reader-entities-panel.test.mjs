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
const FAVORITE = {
  favorite_id: "fav-1",
  document_id: "doc-1",
  document_title: "化学",
  job_id: "job-1",
  page_idx: 2,
  block_id: "p003-b0000",
  quote_text: "卤素是一类元素",
  translated_quote_text: "",
  note: "重点",
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
    else if (path.endsWith("/favorites")) data = { items: [] };
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

function stubFetchWithPage(page, backlinks = [], favorites = []) {
  const calls = [];
  globalThis.fetch = async (url, init = {}) => {
    calls.push({ url, init });
    const path = new URL(url).pathname;
    let data = { items: [ENTITY] };
    if (path.endsWith("/mentions")) data = { items: [MENTION] };
    else if (path.endsWith("/relations")) data = { items: [RELATION] };
    else if (path.endsWith("/backlinks")) data = { items: backlinks };
    else if (path.endsWith("/favorites")) data = { items: favorites };
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

const CONFIG_KEY = "retainpdf.browser.config.v1";

function setChatKey() {
  dom.window.localStorage.setItem(
    CONFIG_KEY,
    JSON.stringify({ chatModelApiKey: "sk-test" }),
  );
}

function clearChatKey() {
  dom.window.localStorage.removeItem(CONFIG_KEY);
}

// jsdom 的 confirm 未实现（返回假值），批量动作前必须替换，否则静默 no-op。
async function withConfirm(answer, run) {
  const saved = dom.window.confirm;
  dom.window.confirm = () => answer;
  try {
    return await run();
  } finally {
    dom.window.confirm = saved;
  }
}

// 按方法 + 路径分流：GET 候选清单、POST 单实体概念页；gate 用于挂住第一个 POST。
function stubBatchFetch({ pending, failAt = -1, gate = null } = {}) {
  const calls = [];
  let postIndex = 0;
  globalThis.fetch = async (url, init = {}) => {
    const method = (init.method || "GET").toUpperCase();
    const path = new URL(url).pathname;
    calls.push({ url, method });
    let data = { items: [ENTITY] };
    let status = 200;
    if (path.endsWith("/graph/pending-pages")) {
      data = { items: pending };
    } else if (path.endsWith("/page") && method === "POST") {
      const index = postIndex++;
      if (gate && index === 0) await gate;
      if (index === failAt) {
        status = 500;
        data = {};
      } else {
        data = { entity_id: path, has_page: true, citations: [], links: [], body_md: "" };
      }
    } else if (path.endsWith("/page")) {
      data = { entity_id: "ent-1", has_page: false, stale: false, citations: [], links: [], body_md: "" };
    }
    return {
      ok: status >= 200 && status < 300,
      status,
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

test("面板：标注段渲染并跳页", async () => {
  stubFetchWithPage(PAGE, [], [FAVORITE]);
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

  await waitFor(() => host.textContent.includes("我的标注"), "标注段渲染");
  assert.ok(host.textContent.includes("重点"), "备注渲染");
  assert.ok(host.textContent.includes("化学"), "来源文档标题渲染");

  const favBtn = [...host.querySelectorAll(".reader-notes-item button.reader-notes-link")].find(
    (el) => el.textContent.includes("化学"),
  );
  assert.ok(favBtn, "标注跳页按钮渲染");
  await act(async () => {
    favBtn.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.deepEqual(jumps, [3]);

  await act(async () => {
    root.unmount();
  });
});

test("面板：跨文档标注不跳页，提示来源", async () => {
  stubFetchWithPage(PAGE, [], [
    { ...FAVORITE, document_id: "doc-2", document_title: "其他文献" },
  ]);
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
    () =>
      [...host.querySelectorAll(".reader-notes-item button.reader-notes-link")].some((el) =>
        el.textContent.includes("其他文献"),
      ),
    "标注跳页按钮渲染",
  );
  const favBtn = [...host.querySelectorAll(".reader-notes-item button.reader-notes-link")].find(
    (el) => el.textContent.includes("其他文献"),
  );
  await act(async () => {
    favBtn.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.deepEqual(jumps, [], "跨文档标注不跳当前文档");
  assert.ok(host.textContent.includes("其他文献"), "提示来源文档");

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

test("面板：批量概念页顺序生成并计数", async () => {
  const pending = [
    { entity_id: "ent-a", name: "A", entity_type: "term", has_page: false, stale: false },
    { entity_id: "ent-b", name: "B", entity_type: "concept", has_page: true, stale: true },
  ];
  const calls = stubBatchFetch({ pending });
  setChatKey();
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  try {
    await withConfirm(true, async () => {
      await act(async () => {
        root.render(createElement(ReaderEntitiesPanel, {
          open: true,
          jobId: "job-1",
          documentId: "doc-1",
          onClose() {},
          onJumpPage() {},
        }));
      });
      await waitFor(() => findByText(host, "批量概念页"), "工具栏渲染");
      await act(async () => {
        findByText(host, "批量概念页").dispatchEvent(
          new dom.window.MouseEvent("click", { bubbles: true }),
        );
      });
      await waitFor(() => host.textContent.includes("更新 2 个概念页"), "批量完成提示");
    });

    const posts = calls.filter((c) => c.method === "POST");
    assert.equal(posts.length, 2);
    assert.ok(posts[0].url.includes("/entities/ent-a/page"), "按候选顺序：先 A");
    assert.ok(posts[1].url.includes("/entities/ent-b/page"), "再 B");
    assert.equal(
      calls.filter((c) => c.url.includes("/graph/pending-pages")).length,
      1,
      "只取一次候选清单",
    );
  } finally {
    clearChatKey();
    await act(async () => {
      root.unmount();
    });
  }
});

test("面板：批量中单个失败不中断，结束时计数", async () => {
  const pending = [
    { entity_id: "ent-a", name: "A", entity_type: "term", has_page: false, stale: false },
    { entity_id: "ent-b", name: "B", entity_type: "concept", has_page: false, stale: false },
    { entity_id: "ent-c", name: "C", entity_type: "method", has_page: false, stale: false },
  ];
  const calls = stubBatchFetch({ pending, failAt: 1 });
  setChatKey();
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  try {
    await withConfirm(true, async () => {
      await act(async () => {
        root.render(createElement(ReaderEntitiesPanel, {
          open: true,
          jobId: "job-1",
          documentId: "doc-1",
          onClose() {},
          onJumpPage() {},
        }));
      });
      await waitFor(() => findByText(host, "批量概念页"), "工具栏渲染");
      await act(async () => {
        findByText(host, "批量概念页").dispatchEvent(
          new dom.window.MouseEvent("click", { bubbles: true }),
        );
      });
      await waitFor(() => host.textContent.includes("1 个失败"), "失败计数提示");
    });

    assert.equal(calls.filter((c) => c.method === "POST").length, 3, "失败后继续处理剩余实体");
    assert.ok(host.textContent.includes("更新 2 个概念页"), "成功数正确");
  } finally {
    clearChatKey();
    await act(async () => {
      root.unmount();
    });
  }
});

test("面板：没有待维护实体时不发 POST", async () => {
  const calls = stubBatchFetch({ pending: [] });
  setChatKey();
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  try {
    await withConfirm(true, async () => {
      await act(async () => {
        root.render(createElement(ReaderEntitiesPanel, {
          open: true,
          jobId: "job-1",
          documentId: "doc-1",
          onClose() {},
          onJumpPage() {},
        }));
      });
      await waitFor(() => findByText(host, "批量概念页"), "工具栏渲染");
      await act(async () => {
        findByText(host, "批量概念页").dispatchEvent(
          new dom.window.MouseEvent("click", { bubbles: true }),
        );
      });
      await waitFor(() => host.textContent.includes("都已有最新概念页"), "空候选提示");
    });
    assert.equal(calls.filter((c) => c.method === "POST").length, 0);
  } finally {
    clearChatKey();
    await act(async () => {
      root.unmount();
    });
  }
});

test("面板：确认框取消后不发 POST", async () => {
  const pending = [
    { entity_id: "ent-a", name: "A", entity_type: "term", has_page: false, stale: false },
  ];
  const calls = stubBatchFetch({ pending });
  setChatKey();
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  try {
    await withConfirm(false, async () => {
      await act(async () => {
        root.render(createElement(ReaderEntitiesPanel, {
          open: true,
          jobId: "job-1",
          documentId: "doc-1",
          onClose() {},
          onJumpPage() {},
        }));
      });
      await waitFor(() => findByText(host, "批量概念页"), "工具栏渲染");
      await act(async () => {
        findByText(host, "批量概念页").dispatchEvent(
          new dom.window.MouseEvent("click", { bubbles: true }),
        );
      });
      await wait(30);
    });
    assert.equal(
      calls.filter((c) => c.url.includes("/graph/pending-pages")).length,
      1,
      "已取候选清单，只是确认后被拦下",
    );
    assert.equal(calls.filter((c) => c.method === "POST").length, 0);
  } finally {
    clearChatKey();
    await act(async () => {
      root.unmount();
    });
  }
});

test("面板：缺模型 Key 时批量不发任何请求", async () => {
  const pending = [
    { entity_id: "ent-a", name: "A", entity_type: "term", has_page: false, stale: false },
  ];
  const calls = stubBatchFetch({ pending });
  clearChatKey();
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  try {
    await withConfirm(true, async () => {
      await act(async () => {
        root.render(createElement(ReaderEntitiesPanel, {
          open: true,
          jobId: "job-1",
          documentId: "doc-1",
          onClose() {},
          onJumpPage() {},
        }));
      });
      await waitFor(() => findByText(host, "批量概念页"), "工具栏渲染");
      await act(async () => {
        findByText(host, "批量概念页").dispatchEvent(
          new dom.window.MouseEvent("click", { bubbles: true }),
        );
      });
      await waitFor(() => host.textContent.includes("缺少模型 API Key"), "缺 Key 提示");
    });
    assert.equal(
      calls.filter((c) => c.url.includes("/graph/pending-pages")).length,
      0,
      "门禁在取候选之前",
    );
    assert.equal(calls.filter((c) => c.method === "POST").length, 0);
  } finally {
    clearChatKey();
    await act(async () => {
      root.unmount();
    });
  }
});

test("面板：停止在当前实体之后生效", async () => {
  const pending = [
    { entity_id: "ent-a", name: "A", entity_type: "term", has_page: false, stale: false },
    { entity_id: "ent-b", name: "B", entity_type: "concept", has_page: false, stale: false },
  ];
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const calls = stubBatchFetch({ pending, gate });
  setChatKey();
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  try {
    await withConfirm(true, async () => {
      await act(async () => {
        root.render(createElement(ReaderEntitiesPanel, {
          open: true,
          jobId: "job-1",
          documentId: "doc-1",
          onClose() {},
          onJumpPage() {},
        }));
      });
      await waitFor(() => findByText(host, "批量概念页"), "工具栏渲染");
      await act(async () => {
        findByText(host, "批量概念页").dispatchEvent(
          new dom.window.MouseEvent("click", { bubbles: true }),
        );
      });
      await waitFor(() => findByText(host, "停止"), "批量中按钮变停止");
      await act(async () => {
        findByText(host, "停止").dispatchEvent(
          new dom.window.MouseEvent("click", { bubbles: true }),
        );
      });
      await act(async () => {
        release();
        await gate;
      });
      await waitFor(() => host.textContent.includes("已停止"), "停止提示");
    });

    assert.equal(calls.filter((c) => c.method === "POST").length, 1, "停止后不再发下一个");
    assert.ok(host.textContent.includes("更新 1 个概念页"), "已完成的那个计入");
  } finally {
    clearChatKey();
    await act(async () => {
      root.unmount();
    });
  }
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

const EDITED_PAGE = {
  ...PAGE,
  edited: true,
  edited_at: "2026-09-09T01:00:00Z",
  body_md: "人工修订 [1]。",
};

// 按方法分流：GET 拉详情、PATCH 保存/撤销（返回更新后的页）。
function stubFetchWithEdit(page = PAGE, patchResult = null) {
  const calls = [];
  globalThis.fetch = async (url, init = {}) => {
    const method = (init.method || "GET").toUpperCase();
    const path = new URL(url).pathname;
    calls.push({ url, method, init });
    let data = { items: [ENTITY] };
    if (path.endsWith("/mentions")) data = { items: [MENTION] };
    else if (path.endsWith("/relations")) data = { items: [RELATION] };
    else if (path.endsWith("/backlinks")) data = { items: [] };
    else if (path.endsWith("/favorites")) data = { items: [] };
    else if (path.endsWith("/page") && method === "PATCH") {
      const body = JSON.parse(init.body);
      if (patchResult) data = patchResult(body);
      else if (body.revert) {
        data = { ...page, edited: false, edited_at: "", body_md: "模型原文。" };
      } else {
        data = {
          ...page,
          edited: true,
          edited_at: "2026-09-09T02:00:00Z",
          body_md: body.body_md,
        };
      }
    } else if (path.endsWith("/page")) {
      data = page;
    }
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

// React 用自己的 value tracker 判断变化：必须走原生 setter 再派发 input。
function setTextareaValue(ta, value) {
  const setter = Object.getOwnPropertyDescriptor(
    dom.window.HTMLTextAreaElement.prototype,
    "value",
  ).set;
  setter.call(ta, value);
  ta.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
}

async function openEntityDetail(host, root) {
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
}

test("面板：编辑保存后正文变人工版并提示", async () => {
  const calls = stubFetchWithEdit(PAGE);
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  await openEntityDetail(host, root);
  await waitFor(
    () => host.querySelector(".reader-entities-page-body")?.textContent.includes("卤素是一类元素"),
    "概念页正文渲染",
  );

  await act(async () => {
    findByText(host, "编辑").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  const ta = host.querySelector("textarea.reader-notes-textarea");
  assert.ok(ta, "编辑态出现 textarea");
  assert.equal(ta.value, "卤素是一类元素 [1]。", "草稿预填生效正文");

  await act(async () => {
    setTextareaValue(ta, "人工修订 [1]。");
  });
  await act(async () => {
    findByText(host, "保存").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });

  await waitFor(() => host.textContent.includes("概念页修订已保存"), "保存提示");
  await waitFor(
    () => host.querySelector(".reader-entities-page-body")?.textContent.includes("人工修订"),
    "正文换成人工版",
  );
  const patch = calls.find((c) => c.method === "PATCH");
  assert.ok(patch, "发出 PATCH");
  assert.deepEqual(JSON.parse(patch.init.body), { body_md: "人工修订 [1]。" });
  assert.ok(!host.querySelector("textarea.reader-notes-textarea"), "保存后退出编辑态");

  await act(async () => {
    root.unmount();
  });
});

test("面板：取消编辑后正文仍在（回归空白 div）", async () => {
  stubFetchWithEdit(PAGE);
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  await openEntityDetail(host, root);
  await waitFor(
    () => host.querySelector(".reader-entities-page-body")?.textContent.includes("卤素是一类元素"),
    "概念页正文渲染",
  );
  await act(async () => {
    findByText(host, "编辑").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.ok(host.querySelector("textarea.reader-notes-textarea"), "进入编辑态");
  await act(async () => {
    findByText(host, "取消").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  await waitFor(
    () => !host.querySelector("textarea.reader-notes-textarea"),
    "退出编辑态",
  );
  assert.ok(
    host.querySelector(".reader-entities-page-body")?.textContent.includes("卤素是一类元素"),
    "取消后已渲染正文还在，不空白",
  );

  await act(async () => {
    root.unmount();
  });
});

test("面板：有修订时显示徽章，撤销后回到模型原文", async () => {
  const calls = stubFetchWithEdit(EDITED_PAGE);
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  await openEntityDetail(host, root);
  await waitFor(() => host.textContent.includes("你的修订"), "修订徽章渲染");
  assert.ok(findByText(host, "撤销修订"), "撤销按钮渲染");

  await withConfirm(true, async () => {
    await act(async () => {
      findByText(host, "撤销修订").dispatchEvent(
        new dom.window.MouseEvent("click", { bubbles: true }),
      );
    });
    await waitFor(() => host.textContent.includes("已回到模型原文"), "撤销提示");
  });

  assert.ok(!host.textContent.includes("你的修订"), "徽章消失");
  assert.ok(
    host.querySelector(".reader-entities-page-body")?.textContent.includes("模型原文"),
    "正文回到模型原文",
  );
  const patch = calls.find((c) => c.method === "PATCH");
  assert.deepEqual(JSON.parse(patch.init.body), { revert: true });

  await act(async () => {
    root.unmount();
  });
});

test("面板：有修订时刷新确认并带 overwrite_manual", async () => {
  const calls = stubFetchWithEdit(EDITED_PAGE, () => ({
    ...PAGE,
    edited: false,
    edited_at: "",
    body_md: "模型新版 [1]。",
  }));
  setChatKey();
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  try {
    await openEntityDetail(host, root);
    await waitFor(() => host.textContent.includes("你的修订"), "修订徽章渲染");

    await withConfirm(true, async () => {
      await act(async () => {
        findByText(host, "刷新").dispatchEvent(
          new dom.window.MouseEvent("click", { bubbles: true }),
        );
      });
      await waitFor(() => host.textContent.includes("概念页已更新"), "刷新提示");
    });

    const post = calls.find((c) => c.method === "POST");
    assert.ok(post, "发出 POST");
    assert.equal(JSON.parse(post.init.body).overwrite_manual, true, "带覆盖标记");
    assert.ok(!host.textContent.includes("你的修订"), "覆盖后徽章消失");
  } finally {
    clearChatKey();
    await act(async () => {
      root.unmount();
    });
  }
});

test("面板：空草稿不发 PATCH", async () => {
  const calls = stubFetchWithEdit(PAGE);
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);

  await openEntityDetail(host, root);
  await waitFor(
    () => host.querySelector(".reader-entities-page-body")?.textContent.includes("卤素是一类元素"),
    "概念页正文渲染",
  );
  await act(async () => {
    findByText(host, "编辑").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  const ta = host.querySelector("textarea.reader-notes-textarea");
  await act(async () => {
    setTextareaValue(ta, "   ");
  });
  await act(async () => {
    findByText(host, "保存").dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  await waitFor(() => host.textContent.includes("正文不能为空"), "空正文报错");
  assert.equal(calls.filter((c) => c.method === "PATCH").length, 0, "空草稿不请求");
  assert.ok(host.querySelector("textarea.reader-notes-textarea"), "仍留在编辑态");

  await act(async () => {
    root.unmount();
  });
});

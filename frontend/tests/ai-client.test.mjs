import test from "node:test";
import assert from "node:assert/strict";

// 让 config/runtime.js 的 isMockMode()/apiBase() 在 node 下可用(无 jsdom 需求)
globalThis.window = globalThis.window || { location: { search: "", protocol: "http:", hostname: "127.0.0.1" } };

const { AiAskError, askLibraryAi, readAiAskStream } = await import("../src/js/api/ai.js");
const { setRuntimeConfig } = await import("../src/js/config/runtime.js");
const { buildScopedQuestion, createReaderAskAnswerer } = await import("../src/js/reader/ai/ask-answerer.js");

function sseStream(chunks = []) {
  const encoder = new TextEncoder();
  return new ReadableStream({
    start(controller) {
      for (const chunk of chunks) {
        controller.enqueue(encoder.encode(chunk));
      }
      controller.close();
    },
  });
}

// ===== SSE 行解析(/api/v1/ai/ask 流式契约) =====

test("readAiAskStream:tool 事件按序回调,done 事件返回归一化结果", async () => {
  const toolEvents = [];
  // 事件跨 chunk 截断 + CRLF + 非 data 行,验证缓冲逻辑
  const result = await readAiAskStream(sseStream([
    ': keep-alive\n',
    'data: {"type": "tool", "round": 1, "tool": "list_documents", "arguments": {"limit": 200}}\r\n\r\n',
    'data: {"type": "tool", "round": 2, "tool": "search_f',
    'ulltext", "arguments": {"query": "光谱"}}\n\n',
    'data: {"type": "done", "answer": "结论 [1]。", "citations": [{"ref": 1, "document_id": "doc-1", "job_id": "job-1", "page_idx": 3, "block_id": "p004-b0002", "snippet": "命中片段"}], "tool_trace": [{"round": 1, "tool": "list_documents"}], "rounds": 3}\n\n',
  ]), {
    onToolEvent: (event) => toolEvents.push(event),
  });

  assert.deepEqual(toolEvents.map((event) => [event.round, event.tool]), [
    [1, "list_documents"],
    [2, "search_fulltext"],
  ]);
  assert.equal(result.answer, "结论 [1]。");
  assert.equal(result.rounds, 3);
  assert.equal(result.citations.length, 1);
  assert.deepEqual(result.citations[0], {
    ref: 1,
    document_id: "doc-1",
    job_id: "job-1",
    page_idx: 3,
    block_id: "p004-b0002",
    snippet: "命中片段",
  });
  assert.equal(result.toolTrace.length, 1);
});

test("readAiAskStream:error 事件抛出 AiAskError", async () => {
  await assert.rejects(
    readAiAskStream(sseStream([
      'data: {"type": "tool", "round": 1, "tool": "search_fulltext", "arguments": {}}\n\n',
      'data: {"type": "error", "message": "上游模型超时"}\n\n',
    ])),
    (error) => error instanceof AiAskError && /上游模型超时/.test(error.message),
  );
});

test("readAiAskStream:流中断(无 done)抛出可重试错误", async () => {
  await assert.rejects(
    readAiAskStream(sseStream([
      'data: {"type": "tool", "round": 1, "tool": "read_blocks", "arguments": {}}\n\n',
    ])),
    (error) => error instanceof AiAskError && /中断/.test(error.message),
  );
});

test("readAiAskStream:末尾无换行的 done 行也能解析", async () => {
  const result = await readAiAskStream(sseStream([
    'data: {"type": "done", "answer": "ok", "citations": [], "tool_trace": [], "rounds": 1}',
  ]));
  assert.equal(result.answer, "ok");
});

// ===== askLibraryAi:请求构造与错误分级 =====

test("askLibraryAi:携带 X-API-Key,body 含 question/document_id/job_id/stream", async () => {
  setRuntimeConfig({ xApiKey: "test-key" });
  const calls = [];
  const result = await askLibraryAi({
    question: "这篇讲什么?",
    documentId: "doc-9",
    jobId: "job-9",
    fetchImpl: async (url, options) => {
      calls.push([url, options]);
      return {
        ok: true,
        headers: { get: () => "text/event-stream" },
        body: sseStream(['data: {"type": "done", "answer": "答", "citations": [], "tool_trace": [], "rounds": 1}\n\n']),
      };
    },
  });
  setRuntimeConfig({ xApiKey: "" });

  assert.equal(result.answer, "答");
  assert.equal(calls.length, 1);
  const [url, options] = calls[0];
  assert.match(url, /\/api\/v1\/ai\/ask$/);
  assert.equal(options.method, "POST");
  assert.equal(options.headers["X-API-Key"], "test-key");
  assert.equal(options.headers["Content-Type"], "application/json");
  assert.deepEqual(JSON.parse(options.body), {
    question: "这篇讲什么?",
    document_id: "doc-9",
    job_id: "job-9",
    stream: true,
  });
});

test("askLibraryAi:502 抛出带 status 的 AI 路由未响应错误", async () => {
  await assert.rejects(
    askLibraryAi({
      question: "hi",
      fetchImpl: async () => ({ ok: false, status: 502, text: async () => "" }),
    }),
    (error) => error instanceof AiAskError && error.status === 502 && /AI 问答路由未响应/.test(error.message),
  );
});

test("askLibraryAi:401 解析 AI 路由错误 detail 并提示 X-API-Key", async () => {
  await assert.rejects(
    askLibraryAi({
      question: "hi",
      fetchImpl: async () => ({
        ok: false,
        status: 401,
        text: async () => JSON.stringify({ detail: "invalid api key" }),
      }),
    }),
    (error) => error instanceof AiAskError
      && error.status === 401
      && /invalid api key/i.test(error.message),
  );
});

test("askLibraryAi:400 缺 LLM key 时透传/归并可读文案", async () => {
  await assert.rejects(
    askLibraryAi({
      question: "hi",
      fetchImpl: async () => ({
        ok: false,
        status: 400,
        text: async () => JSON.stringify({
          detail: "缺少 LLM API Key:请在前端凭据设置中填写模型 API Key。",
        }),
      }),
    }),
    (error) => error instanceof AiAskError
      && error.status === 400
      && /LLM API Key|模型 API Key|凭据/.test(error.message),
  );
});

test("askLibraryAi:非流式 JSON envelope 兜底解包", async () => {
  const result = await askLibraryAi({
    question: "hi",
    fetchImpl: async () => ({
      ok: true,
      headers: { get: () => "application/json" },
      json: async () => ({ code: 0, data: { answer: "非流式", citations: [], tool_trace: [], rounds: 2 } }),
    }),
  });
  assert.equal(result.answer, "非流式");
  assert.equal(result.rounds, 2);
});

// ===== ask-answerer:document_id 反查缓存与 scope 前缀 =====

test("buildScopedQuestion:页/选区范围以前缀写进 question 文本", () => {
  assert.equal(buildScopedQuestion({ question: "总结一下", scope: "document" }), "总结一下");
  assert.equal(
    buildScopedQuestion({ question: "总结一下", scope: "page", context: { page: 4 } }),
    "（当前第 4 页）总结一下",
  );
  assert.equal(
    buildScopedQuestion({
      question: "解释这段",
      scope: "selection",
      context: { page: 2, rect: {} },
      resolveQuote: () => ({ quoteText: "选中的  原文\n片段" }),
    }),
    "（针对选中的原文片段：「选中的 原文 片段」）解释这段",
  );
  assert.equal(
    buildScopedQuestion({ question: "解释这段", scope: "selection", context: { page: 2 }, resolveQuote: () => null }),
    "（针对第 2 页的选区内容）解释这段",
  );
});

// ===== chat 渲染:引用可点击、模型文本不注入 HTML =====

test("chat:agentic 回答渲染 [n] 可点击引用与脚注,模型文本 XSS 安全", async () => {
  // Phase 2b:AI 问答 UI 迁入 React(ReaderAiChat),本测试改为渲染组件、
  // 经 DOM 提交驱动;渲染语义断言(过程提示/XSS/引用按钮/脚注)保持不变。
  const { JSDOM } = await import("jsdom");
  const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/" });
  for (const k of ["window", "document", "HTMLElement", "CustomEvent", "Event", "Node", "MutationObserver"]) {
    Object.defineProperty(globalThis, k, { value: dom.window[k] ?? dom.window, writable: true, configurable: true });
  }
  globalThis.window = dom.window;
  globalThis.requestAnimationFrame = (cb) => setTimeout(() => cb(0), 0);
  // Radix Presence/Tabs(阶段 B 引入)在 jsdom 下需要 cancelAnimationFrame
  // (TabsContent 的 mount 动画计时器清理)和 getComputedStyle(Presence 读取
  // animation-name 判断退场动画是否结束)——jsdom 的 window 上有实现,只是没有
  // 像 requestAnimationFrame 一样被复制到裸 global 上,这里一并补上。
  globalThis.cancelAnimationFrame = (id) => clearTimeout(id);
  globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
  globalThis.IS_REACT_ACT_ENVIRONMENT = false;
  const documentRef = dom.window.document;
  const citation = {
    ref: 1,
    document_id: "doc-x",
    job_id: "job-x",
    page_idx: 3,
    block_id: "p004-b0002",
    snippet: "命中片段文本",
  };
  const jumps = [];
  const progressTexts = [];
  const { createRoot } = await import("react-dom/client");
  const React = await import("react");
  const { ReaderAiChat } = await import("../src/pages/reader/legacy/components/ReaderAiChat.jsx");
  const host = documentRef.createElement("div");
  documentRef.body.appendChild(host);
  createRoot(host).render(React.createElement(ReaderAiChat, {
    ports: {
      jobId: "job-x",
      historyStore: { load: () => ({ messages: [], history: [] }), save() {}, clear() {} },
      jumpToCitation: (target) => jumps.push(target),
      remoteAnswerer: {
        answer: async ({ onToolEvent }) => {
          onToolEvent?.({ type: "tool", round: 1, tool: "search_fulltext" });
          progressTexts.push(documentRef.querySelector(".reader-ai-message-assistant .reader-ai-message-body-el").textContent);
          return {
            answer: '答案见 [1]。<img src=x onerror="alert(1)">',
            citations: [citation],
            rounds: 2,
          };
        },
        ensureLoaded: async () => true,
      },
    },
  }));

  const waitFor = async (predicate, description) => {
    const deadline = Date.now() + 3000;
    while (Date.now() < deadline) {
      if (predicate()) {
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 15));
    }
    assert.fail(`等待超时:${description}`);
  };
  await waitFor(() => documentRef.getElementById("reader-ai-input"), "composer 挂载");
  const input = documentRef.getElementById("reader-ai-input");
  const setter = Object.getOwnPropertyDescriptor(dom.window.HTMLTextAreaElement.prototype, "value").set;
  setter.call(input, "Molassembler 是什么?");
  input.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
  documentRef.querySelector("[data-reader-ai-composer]")
    .dispatchEvent(new dom.window.Event("submit", { bubbles: true, cancelable: true }));
  await waitFor(
    () => documentRef.querySelector(".reader-ai-citations .reader-ai-citation-item"),
    "回答 finalize(脚注出现)",
  );

  const assistant = documentRef.querySelector(".reader-ai-message-assistant");
  assert.deepEqual(progressTexts, ["正在检索文档内容…"], "tool 事件应渲染为过程提示行");
  assert.ok(!assistant.className.includes("reader-ai-message-progress"), "完成后应移除过程态样式");
  assert.ok(!assistant.innerHTML.includes("<img"), "模型文本必须按纯文本插入");
  assert.match(assistant.textContent, /<img src=x/);

  const refButton = assistant.querySelector("button.reader-ai-citation-ref");
  assert.equal(refButton.textContent, "[1]");
  refButton.click();
  const footerButtons = assistant.querySelectorAll(".reader-ai-citations .reader-ai-citation-item");
  assert.equal(footerButtons.length, 1);
  assert.match(footerButtons[0].textContent, /^\[1\] 命中片段文本 · 第 4 页$/);
  footerButtons[0].click();
  assert.deepEqual(jumps, [citation, citation], "正文标记与脚注点击都跳同一引用");
});

test("ask answerer:按 job_id 直查 document_id 且只查一次", async () => {
  const listCalls = [];
  const askCalls = [];
  const answerer = createReaderAskAnswerer({
    jobId: "job-b",
    llmConfig: () => ({ apiKey: "sk-test", baseUrl: "", model: "" }),
    documentByJobId: async (apiPrefix, jobId) => {
      listCalls.push([apiPrefix, jobId]);
      // 后端直查:历史 run 也能解析到所属文档
      return { document_id: "doc-b", active_job_id: "job-a" };
    },
    ask: async (payload) => {
      askCalls.push(payload);
      payload.onToolEvent?.({ type: "tool", round: 1, tool: "search_fulltext" });
      return { answer: "回答", citations: [], toolTrace: [], rounds: 1 };
    },
  });

  const toolEvents = [];
  const first = await answerer.answer({
    question: "问题一",
    scope: "document",
    onToolEvent: (event) => toolEvents.push(event),
  });
  await answerer.answer({ question: "问题二", scope: "page", context: { page: 3 } });

  assert.equal(first.answer, "回答");
  assert.equal(first.scope, "document");
  assert.equal(listCalls.length, 1, "document_id 直查结果应被缓存");
  assert.deepEqual(listCalls[0][1], "job-b", "按 job_id 直查");
  assert.equal(askCalls[0].documentId, "doc-b");
  assert.equal(askCalls[0].jobId, "job-b");
  assert.equal(askCalls[1].question, "（当前第 3 页）问题二");
  assert.equal(toolEvents.length, 1);
});

test("ask answerer:反查不到 document 时 fail closed，不静默全库", async () => {
  const askCalls = [];
  const answerer = createReaderAskAnswerer({
    jobId: "job-orphan",
    llmConfig: () => ({ apiKey: "sk-test", baseUrl: "", model: "" }),
    documentByJobId: async () => null,
    ask: async (payload) => {
      askCalls.push(payload);
      return { answer: "不应调用", citations: [], toolTrace: [], rounds: 0 };
    },
  });
  await assert.rejects(
    answerer.answer({ question: "这篇讲什么?", scope: "document" }),
    /无法关联当前文档/,
  );
  assert.equal(askCalls.length, 0);
});

test("ask answerer:无模型 Key 时不查文档、不发 ask", async () => {
  const listCalls = [];
  const askCalls = [];
  const answerer = createReaderAskAnswerer({
    jobId: "job-x",
    llmConfig: () => ({ apiKey: "", baseUrl: "", model: "" }),
    documentByJobId: async () => {
      listCalls.push(1);
      return { document_id: "doc-x" };
    },
    ask: async (payload) => {
      askCalls.push(payload);
      return { answer: "不应调用", citations: [], toolTrace: [], rounds: 0 };
    },
  });
  await assert.rejects(
    answerer.answer({ question: "hi", scope: "document" }),
    /缺少模型 API Key/,
  );
  assert.equal(listCalls.length, 0, "缺 Key 不得先查文档");
  assert.equal(askCalls.length, 0, "缺 Key 不得发 ask");
});

test("askLibraryAi 携带 llm_api_key/base/model,仅非空字段进 payload", async () => {
  globalThis.window = { location: { search: "", protocol: "http:", hostname: "127.0.0.1" } };
  let sentBody = null;
  const fakeFetch = async (_url, options) => {
    sentBody = JSON.parse(options.body);
    return {
      ok: true,
      headers: { get: () => "application/json" },
      json: async () => ({ code: 0, message: "ok", data: { answer: "a", citations: [], tool_trace: [], rounds: 1 } }),
    };
  };
  await askLibraryAi({
    question: "问题",
    documentId: "doc-1",
    apiPrefix: "/api/v1",
    fetchImpl: fakeFetch,
    llmApiKey: "  sk-frontend  ",
    llmModel: "deepseek-v4-flash",
  });
  assert.equal(sentBody.llm_api_key, "sk-frontend");
  assert.equal(sentBody.llm_model, "deepseek-v4-flash");
  assert.equal("llm_base_url" in sentBody, false);
  assert.equal(sentBody.document_id, "doc-1");
});

test("askLibraryAi 无 llm key 时 payload 不含 llm_* 字段(后端回退 env)", async () => {
  globalThis.window = { location: { search: "", protocol: "http:", hostname: "127.0.0.1" } };
  let sentBody = null;
  const fakeFetch = async (_url, options) => {
    sentBody = JSON.parse(options.body);
    return {
      ok: true,
      headers: { get: () => "application/json" },
      json: async () => ({ code: 0, message: "ok", data: { answer: "a", citations: [] } }),
    };
  };
  await askLibraryAi({ question: "q", apiPrefix: "/api/v1", fetchImpl: fakeFetch });
  assert.equal("llm_api_key" in sentBody, false);
  assert.equal("llm_base_url" in sentBody, false);
  assert.equal("llm_model" in sentBody, false);
});

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

// 阅读器「选中文字翻译」回归：
// - translateText 端点封装（payload 组装 + /api/v1/translate/text）
// - useReaderTranslate：缺 Key 门禁 / pane→目标语言 / 成功 / 失败 / 重试 / 防串流
// - ReaderSelectionToolbar 渲染「翻译」按钮并回调
// - ReaderTranslatePopup 展示译文并支持复制/重译

const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/" });
for (const k of [
  "window", "document", "HTMLElement", "CustomEvent", "Event", "Node", "navigator",
  "getSelection", "localStorage", "sessionStorage", "getComputedStyle",
  "requestAnimationFrame", "cancelAnimationFrame",
]) {
  try {
    Object.defineProperty(globalThis, k, {
      value: k === "getSelection" ? dom.window.getSelection : (dom.window[k] ?? dom.window),
      writable: true,
      configurable: true,
    });
  } catch (_err) {
    // navigator 等只读时忽略
  }
}
globalThis.window = dom.window;
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const { act, createElement } = await import("react");
const { createRoot } = await import("react-dom/client");
const { translateText } = await import("../src/js/api/reader.js");
const {
  useReaderTranslate,
} = await import("../src/pages/reader/hooks/use-reader-translate.js");
const {
  ReaderSelectionToolbar,
  ReaderTranslatePopup,
} = await import("../src/pages/reader/components/react-pdf/index.js");
const {
  MISSING_MODEL_API_KEY_MESSAGE,
} = await import("../src/pages/reader/external.js");

function makeSelection(overrides = {}) {
  return {
    quote: "Attention is all you need",
    page: 3,
    pane: "source",
    rect: { left: 120, top: 240, width: 60, height: 18 },
    ...overrides,
  };
}

function mountTranslateHarness(options = {}) {
  const calls = [];
  const translateMock = options.translate || (async (payload) => {
    calls.push(payload);
    if (options.fail) {
      throw new Error(options.failMessage || "boom");
    }
    return { translated_text: `译:${payload.text}`, target_language: payload.targetLanguage };
  });
  const hasKey = options.hasKey || (() => true);
  const resolveConfig = options.resolveConfig || (() => ({
    apiKey: "sk-test",
    baseUrl: "https://api.deepseek.com/v1",
    model: "deepseek-chat",
    provider: "deepseek",
  }));
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);
  let tr;
  act(() => {
    root.render(createElement(function Harness() {
      tr = useReaderTranslate({ translate: translateMock, hasKey, resolveConfig, language: options.language });
      return null;
    }));
  });
  return {
    calls,
    getTr: () => tr,
    unmount: () => act(() => root.unmount()),
  };
}

test("translateText 组装 payload 并 POST 到 /api/v1/translate/text", async () => {
  const requests = [];
  const fakeSubmit = async (url, payload) => {
    requests.push({ url, payload });
    return { translated_text: "x", target_language: "简体中文" };
  };
  const res = await translateText({
    text: "hello",
    targetLanguage: "简体中文",
    provider: "deepseek",
    model: "deepseek-chat",
    apiKey: " Bearer sk-1 ",
    baseUrl: "https://api.deepseek.com/v1/",
    submit: fakeSubmit,
  });
  assert.equal(requests.length, 1);
  assert.ok(requests[0].url.endsWith("/translate/text"), `url=${requests[0].url}`);
  assert.deepEqual(requests[0].payload, {
    text: "hello",
    target_language: "简体中文",
    provider: "deepseek",
    model: "deepseek-chat",
    api_key: "sk-1",
    base_url: "https://api.deepseek.com/v1/",
  });
  assert.deepEqual(res, { translated_text: "x", target_language: "简体中文" });
});

test("useReaderTranslate：缺模型 Key 时提示门禁文案且不发请求", async () => {
  const { calls, getTr, unmount } = mountTranslateHarness({ hasKey: () => false });
  await act(async () => {
    await getTr().translateSelection(makeSelection());
  });
  assert.equal(getTr().open, true);
  assert.equal(getTr().loading, false);
  assert.equal(getTr().error, MISSING_MODEL_API_KEY_MESSAGE);
  assert.equal(calls.length, 0);
  unmount();
});

test("useReaderTranslate：原文栏选中 → 简体中文并展示译文", async () => {
  const { calls, getTr, unmount } = mountTranslateHarness();
  await act(async () => {
    await getTr().translateSelection(makeSelection({ quote: "hello world", pane: "source" }));
  });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].targetLanguage, "简体中文");
  assert.equal(calls[0].apiKey, "sk-test");
  assert.equal(getTr().targetLanguage, "简体中文");
  assert.equal(getTr().result, "译:hello world");
  assert.equal(getTr().loading, false);
  assert.equal(getTr().error, "");
  unmount();
});

test("useReaderTranslate：译文栏选中 → English", async () => {
  const { calls, getTr, unmount } = mountTranslateHarness();
  await act(async () => {
    await getTr().translateSelection(makeSelection({ quote: "你好", pane: "translated" }));
  });
  assert.equal(calls[0].targetLanguage, "English");
  assert.equal(getTr().targetLanguage, "English");
  unmount();
});

test("useReaderTranslate：有任务语言元数据时原文栏译成该任务译文语言", async () => {
  const { calls, getTr, unmount } = mountTranslateHarness({
    language: { source_lang: "en", target_lang: "ja", target_language_name: "日本語" },
  });
  await act(async () => {
    await getTr().translateSelection(makeSelection({ quote: "hello", pane: "source" }));
  });
  assert.equal(calls[0].targetLanguage, "日本語");
  assert.equal(getTr().targetLanguage, "日本語");
  unmount();
});

test("useReaderTranslate：有任务语言元数据时译文栏译回原文语言", async () => {
  const { calls, getTr, unmount } = mountTranslateHarness({
    language: { source_lang: "zh-CN", target_lang: "ja", target_language_name: "日本語" },
  });
  await act(async () => {
    await getTr().translateSelection(makeSelection({ quote: "日本語のテキスト", pane: "translated" }));
  });
  assert.equal(calls[0].targetLanguage, "简体中文");
  assert.equal(getTr().targetLanguage, "简体中文");
  unmount();
});

test("useReaderTranslate：译文栏原文语言未知(auto/空)时回退 English", async () => {
  const { calls, getTr, unmount } = mountTranslateHarness({
    language: { source_lang: "auto", target_lang: "zh-CN", target_language_name: "简体中文" },
  });
  await act(async () => {
    await getTr().translateSelection(makeSelection({ quote: "你好", pane: "translated" }));
  });
  assert.equal(calls[0].targetLanguage, "English");
  assert.equal(getTr().targetLanguage, "English");
  unmount();
});

test("useReaderTranslate：翻译失败 → 设置错误信息", async () => {
  const { getTr, unmount } = mountTranslateHarness({ fail: true, failMessage: "网络错误" });
  await act(async () => {
    await getTr().translateSelection(makeSelection());
  });
  assert.equal(getTr().loading, false);
  assert.equal(getTr().error, "网络错误");
  assert.equal(getTr().result, "");
  unmount();
});

test("useReaderTranslate：retry 复用上一次选区重新翻译", async () => {
  const { calls, getTr, unmount } = mountTranslateHarness();
  await act(async () => {
    await getTr().translateSelection(makeSelection({ quote: "first" }));
  });
  assert.equal(getTr().result, "译:first");
  await act(async () => {
    await getTr().retry();
  });
  assert.equal(calls.length, 2);
  assert.equal(calls[1].text, "first");
  assert.equal(getTr().result, "译:first");
  unmount();
});

test("useReaderTranslate：换选区后旧 in-flight 结果被 seq 丢弃（防串流）", async () => {
  let resolveFirst;
  const translateMock = async (payload) => {
    if (payload.text === "first") {
      return new Promise((resolve) => {
        resolveFirst = resolve;
      });
    }
    return { translated_text: `译:${payload.text}`, target_language: payload.targetLanguage };
  };
  const { getTr, unmount } = mountTranslateHarness({ translate: translateMock });
  let firstPromise;
  act(() => {
    firstPromise = getTr().translateSelection(makeSelection({ quote: "first" }));
  });
  await act(async () => {
    await getTr().translateSelection(makeSelection({ quote: "second" }));
  });
  assert.equal(getTr().result, "译:second");
  await act(async () => {
    resolveFirst({ translated_text: "译:first", target_language: "简体中文" });
    await firstPromise;
  });
  // 旧 seq 结果被丢弃，不覆盖最新译文
  assert.equal(getTr().result, "译:second");
  unmount();
});

test("useReaderTranslate：超长 quote 在弹窗内截断", async () => {
  const longQuote = "x".repeat(300);
  const { getTr, unmount } = mountTranslateHarness();
  await act(async () => {
    await getTr().translateSelection(makeSelection({ quote: longQuote }));
  });
  assert.ok(getTr().quote.length <= 241, `quote=${getTr().quote.length}`);
  assert.ok(getTr().quote.endsWith("…"));
  unmount();
});

test("ReaderSelectionToolbar 渲染「翻译」按钮并回调 onTranslate", () => {
  const selection = makeSelection();
  const notes = [];
  const translates = [];
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);
  act(() => {
    root.render(createElement(ReaderSelectionToolbar, {
      selection,
      onAddNote: (s) => notes.push(s),
      onTranslate: (s) => translates.push(s),
      onDismiss: () => {},
    }));
  });
  const buttons = [...host.querySelectorAll("button")];
  const translateBtn = buttons.find((b) => b.textContent.includes("翻译"));
  assert.ok(translateBtn, "存在「翻译」按钮");
  act(() => {
    translateBtn.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.equal(translates.length, 1);
  assert.equal(translates[0], selection);
  const noteBtn = buttons.find((b) => b.textContent.includes("添加批注"));
  assert.ok(noteBtn, "仍保留「添加批注」按钮");
  act(() => {
    noteBtn.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.equal(notes.length, 1);
  act(() => root.unmount());
});

test("ReaderTranslatePopup 展示译文并支持复制/重译", async () => {
  const closed = [];
  const retried = [];
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);
  act(() => {
    root.render(createElement(ReaderTranslatePopup, {
      open: true,
      quote: "hello",
      targetLanguage: "简体中文",
      result: "你好",
      loading: false,
      error: "",
      onClose: () => closed.push(true),
      onRetry: () => retried.push(true),
    }));
  });
  const body = host.textContent;
  assert.ok(body.includes("hello"), "展示原文摘录");
  assert.ok(body.includes("你好"), "展示译文");
  const buttons = [...host.querySelectorAll("button")];
  const retryBtn = buttons.find((b) => b.textContent.includes("重新翻译"));
  assert.ok(retryBtn);
  act(() => {
    retryBtn.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.equal(retried.length, 1);
  act(() => root.unmount());
});

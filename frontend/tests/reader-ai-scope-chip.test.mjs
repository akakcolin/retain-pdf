import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

// 阅读器 AI 面板「检索范围」chip：当前文档 ⇄ 全库。
// 面板依赖阅读器运行时，这里只挂线程 + 最小 assistant-ui runtime，
// 锁住 chip 文案/激活态与回调（请求体契约由 ask-answerer 测试锁住）。

const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/" });
for (const k of ["window", "document", "HTMLElement", "CustomEvent", "Event", "Node", "MutationObserver"]) {
  Object.defineProperty(globalThis, k, { value: dom.window[k] ?? dom.window, writable: true, configurable: true });
}
globalThis.window = dom.window;
globalThis.requestAnimationFrame = (cb) => setTimeout(() => cb(0), 0);
globalThis.cancelAnimationFrame = (id) => clearTimeout(id);
globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
globalThis.IS_REACT_ACT_ENVIRONMENT = false;
// assistant-ui 的 top-anchor reserve 用 ResizeObserver；jsdom 没有，给个空壳
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const { createRoot } = await import("react-dom/client");
const React = await import("react");
const { AssistantRuntimeProvider, useExternalStoreRuntime } = await import("@assistant-ui/react");
const { ReaderAssistantThread } = await import(
  "../src/pages/reader/components/react-pdf/assistant/ReaderAssistantThread.js"
);
const { applyDefaultCredentialInputs } = await import(
  "../src/js/features/credentials/default-state-port.js"
);

// 无模型 Key 时面板渲染的是「缺 Key」占位，chip 根本不挂载——先注入内存凭据。
applyDefaultCredentialInputs({ chatModelApiKey: "sk-test" });

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitFor(predicate, description) {
  const deadline = Date.now() + 3000;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await wait(15);
  }
  assert.fail(`等待超时:${description}`);
}

function Probe({ retrievalScope, onRetrievalScopeChange }) {
  const runtime = useExternalStoreRuntime({
    isRunning: false,
    messages: [],
    onNew: async () => {},
  });
  return React.createElement(
    AssistantRuntimeProvider,
    { runtime },
    React.createElement(ReaderAssistantThread, {
      retrievalScope,
      onRetrievalScopeChange,
    }),
  );
}

test("scope chip：当前文档 → 点击切全库；激活态显示「全库」", async () => {
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);
  const changes = [];

  await new Promise((resolve) => {
    root.render(
      React.createElement(Probe, {
        retrievalScope: "document",
        onRetrievalScopeChange: (scope) => changes.push(scope),
      }),
    );
    setTimeout(resolve, 0);
  });

  await waitFor(
    () => host.querySelector(".aui-composer-chip--scope"),
    "chip 挂载",
  );
  const chip = host.querySelector(".aui-composer-chip--scope");
  assert.equal(chip.textContent.trim(), "当前文档");
  assert.equal(chip.classList.contains("is-library"), false);

  chip.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  await waitFor(() => changes.length === 1, "点击回调");
  assert.deepEqual(changes, ["library"]);

  await new Promise((resolve) => {
    root.render(
      React.createElement(Probe, {
        retrievalScope: "library",
        onRetrievalScopeChange: (scope) => changes.push(scope),
      }),
    );
    setTimeout(resolve, 0);
  });
  await waitFor(
    () => host.querySelector(".aui-composer-chip--scope")?.textContent.trim() === "全库",
    "切到全库文案",
  );
  const active = host.querySelector(".aui-composer-chip--scope");
  assert.equal(active.classList.contains("is-library"), true);
  assert.equal(active.getAttribute("aria-pressed"), "true");

  active.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  await waitFor(() => changes.length === 2, "第二次点击回调");
  assert.deepEqual(changes, ["library", "document"]);
});

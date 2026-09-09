import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

// switchSession 失败回滚回归。session-operations 是从 use-reader-ask-runtime
// 拆出的纯工厂(React 状态经 getter/setter 注入),可以直接实例化,不用挂
// 整个 reader。

const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/index.html" });
for (const key of ["window", "document", "HTMLElement", "Element", "Event", "MouseEvent", "CustomEvent", "KeyboardEvent", "Node", "MutationObserver", "NodeFilter"]) {
  Object.defineProperty(globalThis, key, {
    value: dom.window[key] ?? dom.window,
    writable: true,
    configurable: true,
  });
}
globalThis.window = dom.window;
globalThis.requestAnimationFrame = (callback) => setTimeout(() => callback(0), 0);
globalThis.cancelAnimationFrame = (id) => clearTimeout(id);
globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);

const { createReaderAskSessionOps } = await import(
  "../src/pages/reader/components/react-pdf/assistant/session-operations.js"
);

test("switchSession：加载对话失败时回滚 activeConversationId(回归)", async () => {
  // 回归覆盖:失败 catch 只清 items/headId,没恢复 activeConversationId——
  // 会话条仍高亮失败的会话,而 send 读 activeConversationIdRef,下一问会
  // 发进一个 UI 从未加载过的线程。
  const refs = {
    runAbort: { current: null },
    running: { current: false },
    switchToken: { current: 0 },
    documentId: { current: "doc-1" },
    activeConversationId: { current: "conv-old" },
    items: { current: [] },
    headId: { current: null },
  };
  const seen = { activeConversationId: "conv-old", error: "", busy: false };
  const ops = createReaderAskSessionOps({
    jobId: "job-1",
    remoteAnswerer: null,
    getSessionBusy: () => false,
    getSessions: () => [],
    setSessionBusy: (busy) => { seen.busy = busy; },
    setSessionError: (message) => { seen.error = message; },
    setIsRunning: () => {},
    setSessions: () => {},
    setItems: () => {},
    setHeadId: () => {},
    setActiveConversationId: (id) => { seen.activeConversationId = id; },
    refreshSessions: () => {},
    applyConversationTree: () => {},
    refs,
  });

  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => ({
    ok: false,
    status: 500,
    statusText: "Server Error",
    json: async () => ({ message: "boom" }),
  });
  try {
    await ops.switchSession("conv-new");
  } finally {
    globalThis.fetch = originalFetch;
  }

  assert.ok(seen.error.includes("加载该对话失败"), `应给出失败提示，实际:${seen.error}`);
  assert.equal(seen.activeConversationId, "conv-old", "失败后选中态回滚到旧会话");
  assert.equal(
    refs.activeConversationId.current,
    "conv-old",
    "ref 也要回滚,否则 send 仍发往加载失败的会话",
  );
  assert.equal(seen.busy, false, "失败后 busy 必须复位");
});

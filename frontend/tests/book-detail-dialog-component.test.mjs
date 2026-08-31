import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

// 书籍详情弹窗(参考 PDF_MD_lib 的 BookDetailModal)组件级测试:点卡片打开、
// 元数据渲染、阅读状态切换走 patchDocument、馆藏/已翻译的动作集不同。
//
// 每个 test 一份全新 JSDOM(同一个 jsdom 第二次 createRoot 会停摆)。

function makeDom(search = "") {
  const dom = new JSDOM("<!doctype html><html><body></body></html>", {
    url: `http://localhost/index.html${search}`,
  });
  for (const key of ["window", "document", "HTMLElement", "HTMLInputElement", "CustomEvent", "Event", "KeyboardEvent", "MouseEvent", "Node", "MutationObserver", "NodeFilter"]) {
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
  globalThis.IS_REACT_ACT_ENVIRONMENT = false;
  return dom;
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitFor(predicate, description) {
  const deadline = Date.now() + 3000;
  while (Date.now() < deadline) {
    const value = predicate();
    if (value) {
      return value;
    }
    await wait(15);
  }
  assert.fail(`等待超时：${description}`);
}

function click(dom, element) {
  // Radix Tabs Trigger 挂在 mousedown 上，只 dispatch click 不会切 tab
  element.dispatchEvent(new dom.window.MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0 }));
  element.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true, cancelable: true }));
}

async function bootHomeApp(dom) {
  const { createRoot } = await import("react-dom/client");
  const React = await import("react");
  const { createHomeComposition } = await import("../src/pages/home/composition.js");
  const { HomeApp } = await import("../src/pages/home/HomeApp.jsx");

  const host = dom.window.document.createElement("div");
  host.id = "home-root";
  dom.window.document.body.appendChild(host);

  const services = createHomeComposition({
    fetchGlossaries: async () => ({ items: [] }),
    loadPersistedDeveloperConfig: () => ({}),
    loadPersistedBrowserConfig: () => ({}),
  });
  services.initialize();

  const root = createRoot(host);
  root.render(React.createElement(HomeApp, { services }));
  await waitFor(() => dom.window.document.getElementById("app-shell"), "HomeApp 首帧渲染");
  await wait(0);
  return { services, root, host };
}

test("馆藏卡打开书籍详情:元数据 + 阅读状态切换 + 翻译/读原文动作,无对照阅读", async () => {
  const dom = makeDom("?mock=parallel");
  const byId = (id) => dom.window.document.getElementById(id);
  const { services, root, host } = await bootHomeApp(dom);

  const card = await waitFor(
    () => dom.window.document.querySelector('#recent-jobs-list .recent-job-item[data-library-only="true"]'),
    "馆藏卡就位",
  );
  const documentId = card.getAttribute("data-document-id");
  click(dom, card);

  const dlg = await waitFor(() => byId("book-detail-dialog"), "书籍详情弹窗打开");
  // 标题默认是只读大标题(不是常驻输入框),编辑才出现输入框
  await waitFor(() => dlg.querySelector(".book-detail-title")?.textContent?.trim(), "标题就位");
  assert.equal(byId("book-detail-title-input"), null, "默认只读,无标题输入框");
  assert.ok(dlg.querySelector(".book-detail-status")?.textContent.includes("未翻译"), "馆藏显示未翻译");
  // 未翻译：轻量空态 + StageFlow 预览（尚无真实 job，不嵌完整 StatusCard）
  assert.ok(byId("book-detail-translate-progress"), "馆藏有翻译进度面板");
  assert.ok(byId("book-detail-stage-flow"), "未翻译进度区有 StageFlow 预览");
  assert.equal(byId("book-detail-job-status-card"), null, "未翻译不嵌 StatusCard");
  // 馆藏:有翻译 + 读原文,无对照阅读
  assert.ok(byId("book-detail-translate-btn"), "馆藏有翻译按钮");
  assert.ok(byId("book-detail-read-source-btn"), "有读原文");
  assert.equal(byId("book-detail-compare-btn"), null, "馆藏没有对照阅读");
  // 点"编辑"进入标题/标签编辑
  click(dom, byId("book-detail-edit-btn"));
  await waitFor(() => byId("book-detail-title-input"), "点编辑出现标题输入框");

  // 阅读状态切换 → patchDocument(mock),按钮变激活
  const { getMockDocument } = await import("../src/js/mock/documents.js");
  const readBtns = dlg.querySelectorAll(".book-detail-reading-btn");
  const doneBtn = Array.from(readBtns).find((b) => b.textContent === "读完");
  click(dom, doneBtn);
  await waitFor(() => doneBtn.classList.contains("is-active"), "读完变激活");
  await waitFor(() => getMockDocument(documentId).reading_status === "done", "patchDocument 落库 reading_status=done");

  root.unmount();
  services.dispose();
  host.remove();
});

test("已翻译卡打开书籍详情:有对照阅读,可再次发起翻译", async () => {
  const dom = makeDom("?mock=parallel");
  const byId = (id) => dom.window.document.getElementById(id);
  const { services, root, host } = await bootHomeApp(dom);

  // mock 里 att-001/scl-002 等合成 book 是 succeeded 的已翻译文档
  const card = await waitFor(
    () => dom.window.document.querySelector('#recent-jobs-list .recent-job-item[data-library-only="false"][data-status="succeeded"]'),
    "已翻译卡就位",
  );
  click(dom, card);

  const dlg = await waitFor(() => byId("book-detail-dialog"), "书籍详情弹窗打开");
  // 默认在「简介」：不应弹出工作流对话框
  assert.equal(
    services.stores.dialog.getSnapshot().open,
    false,
    "打开书籍详情不得自动打开工作流弹窗",
  );
  // 已翻译书默认落在「翻译」Tab；进度卡应立刻在 DOM
  await waitFor(
    () => dlg.querySelector(".book-detail-status")?.textContent?.includes("已完成"),
    "显示已完成",
  );
  const statusCard = await waitFor(() => byId("book-detail-job-status-card"), "翻译 Tab 内嵌 StatusCard");
  assert.ok(statusCard.classList.contains("bd-job-status-card"), "详情专用进度卡");
  assert.equal(statusCard.getAttribute("data-embedded"), "true", "embedded 模式");
  assert.ok(
    statusCard.closest("#book-detail-panel-translate"),
    "StatusCard 在翻译 Tab 面板内",
  );
  // 书籍详情专用内部结构（bd-job-status-*），固定高度
  assert.ok(statusCard.classList.contains("bd-job-status-card"), "bd-job-status-card 根类");
  assert.ok(statusCard.querySelector(".bd-job-status-inner"), "独立 inner，非 status-card-shell");
  assert.ok(statusCard.querySelector(".bd-job-status-main"), "固定高度主区");
  assert.ok(
    statusCard.querySelector(".status-stage-flow .status-stage-step"),
    "含阶段流",
  );
  assert.equal(statusCard.querySelector(".status-card-shell"), null, "不使用主流程 shell");
  assert.equal(statusCard.querySelector(".status-progress-hero"), null, "不使用主流程 hero");
  await waitFor(
    () => `${statusCard.getAttribute("data-status") || ""}` === "succeeded"
      || statusCard.querySelector(".status-stage-step.is-active, .status-stage-step.is-done"),
    "StatusCard 进入完成/有阶段高亮",
  );
  const doneStep = statusCard.querySelector(
    '.status-stage-flow .status-stage-step[data-stage-key="done"]',
  );
  assert.ok(
    doneStep?.classList.contains("is-active")
      || doneStep?.classList.contains("is-selected")
      || doneStep?.classList.contains("is-done"),
    "完成阶段高亮",
  );
  const valueText = statusCard.querySelector(".bd-job-status-value")?.textContent?.trim();
  assert.ok(valueText && valueText !== "准备中", `完成态应有进度文案，实际: ${valueText}`);
  // 详情进度卡已从 ring 改为 bar 布局（StatusCardEmbedded：.bd-job-status-percent）
  const pct = statusCard.querySelector(".bd-job-status-percent")?.textContent?.trim();
  assert.equal(pct, "100%", "完成态进度条 100%");
  assert.ok(
    statusCard.querySelector(".bd-job-status-bar.is-done"),
    "完成态进度条 is-done",
  );
  // 仍然不得弹工作流
  assert.equal(
    services.stores.dialog.getSnapshot().open,
    false,
    "切换翻译 Tab / 加载进度后仍不打开工作流弹窗",
  );
  assert.ok(byId("book-detail-compare-btn"), "已翻译有对照阅读");
  // 已翻译文档可再次发起翻译：翻译 Tab 表单出现「重新翻译整本」
  const translateBtn = byId("book-detail-translate-btn");
  assert.ok(translateBtn, "已翻译仍有翻译按钮(重新翻译)");
  assert.ok(
    translateBtn.textContent.includes("重新翻译"),
    `按钮文案应为重新翻译，实际: ${translateBtn.textContent}`,
  );
  assert.ok(byId("book-detail-read-source-btn"), "仍可读原文");

  root.unmount();
  services.dispose();
  host.remove();
});

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

// 书籍详情翻译 Tab 的页码范围映射回归:
// OCR page_ranges 是 1-based 字符串("1-3"),而后端 translation 的
// start_page/end_page 是 0-based 索引(end=-1 表到末页)——此前详情 Tab 把
// 1-based 直接透传,单页文档(1-1)会算出 start=1 > stop=0 而失败,多页则
// 漏翻首页。这里断言映射关系,防止回归。

const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/" });
for (const k of ["window", "document", "HTMLElement", "CustomEvent", "Event", "Node", "navigator"]) {
  try {
    Object.defineProperty(globalThis, k, { value: dom.window[k] ?? dom.window, writable: true, configurable: true });
  } catch (_err) {
    // navigator 只读时忽略
  }
}
globalThis.window = dom.window;
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const { act, createElement } = await import("react");
const { createRoot } = await import("react-dom/client");
const { useBookDetailTranslate } = await import("../src/pages/home/features/library/detail/use-book-detail-translate.js");

function mountHarness(props) {
  const calls = [];
  const setErrorCalls = [];
  const withBusy = async (_key, fn) => {
    await fn();
  };
  const actions = {
    translateDocument: async (documentId, payload) => {
      calls.push({ documentId, payload });
    },
  };
  const host = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(host);
  const root = createRoot(host);
  let tr;
  act(() => {
    root.render(
      createElement(
        function Harness() {
          tr = useBookDetailTranslate({
            open: true,
            documentId: "doc-1",
            pageCount: props.pageCount,
            actions,
            withBusy,
            setError: (message) => setErrorCalls.push(message),
            onTranslateStarted: () => {},
            ...props,
          });
          return null;
        },
      ),
    );
  });
  // 每次渲染 hook 都返回新对象,测试须经 getTr() 拿「当前渲染」的实例,
  // 避免持有上一次渲染的过期闭包(rangeOn 仍为旧值)。
  return {
    calls,
    setErrorCalls,
    getTr: () => tr,
    unmount: () => act(() => root.unmount()),
  };
}

test("整本(不开页码范围):不发送 translation.start_page/end_page,后端走整本默认", async () => {
  const { calls, unmount, getTr } = mountHarness({ pageCount: 5 });
  await act(async () => {
    await getTr().handleTranslate();
  });
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0].payload, {});
  unmount();
});

test("单页文档选 1-1(回归:此前 1-based 透传导致 Invalid page range)", async () => {
  const { calls, unmount, getTr } = mountHarness({ pageCount: 1 });
  act(() => {
    getTr().setRangeOn(true);
    getTr().setStartPage("1");
    getTr().setEndPage("1");
  });
  await act(async () => {
    await getTr().handleTranslate();
  });
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0].payload, {
    ocr: { page_ranges: "1-1" },
    translation: { start_page: 0, end_page: 0 },
  });
  unmount();
});

test("多页文档选 1-3:OCR 保持 1-based,translation 转 0-based", async () => {
  const { calls, unmount, getTr } = mountHarness({ pageCount: 5 });
  act(() => {
    getTr().setRangeOn(true);
    getTr().setStartPage("1");
    getTr().setEndPage("3");
  });
  await act(async () => {
    await getTr().handleTranslate();
  });
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0].payload, {
    ocr: { page_ranges: "1-3" },
    translation: { start_page: 0, end_page: 2 },
  });
  unmount();
});

test("多页文档选 2-5:OCR 裁剪后 translation 覆盖整段选中页(0..e-s)", async () => {
  const { calls, unmount, getTr } = mountHarness({ pageCount: 5 });
  act(() => {
    getTr().setRangeOn(true);
    getTr().setStartPage("2");
    getTr().setEndPage("5");
  });
  await act(async () => {
    await getTr().handleTranslate();
  });
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0].payload, {
    ocr: { page_ranges: "2-5" },
    translation: { start_page: 0, end_page: 3 },
  });
  unmount();
});

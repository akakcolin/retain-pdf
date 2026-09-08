import { $ } from "../../../shared/dom/query.js";
import {
  loadPdfDocument,
  resolveReaderArtifactUrl,
} from "./pdf-document.js";
import {
  bindReaderRegionHover,
  scheduleRegionOverlayRender,
} from "./region-interactions.js";
import { bindPrimaryViewer } from "./primary-viewer.js";
import {
  mountManualPages,
  scheduleVisibleManualPages,
} from "./pdf-renderer.js";
import {
  applyViewerScale,
  schedulePageRowSync as scheduleReaderPageRowSync,
  scheduleScaleRefresh as scheduleReaderScaleRefresh,
} from "./pdf-layout.js";
import {
  showReaderPaneEmpty,
  showReaderPaneReady,
} from "./view.js";

const viewerControllers = new Map();

export { resolveReaderArtifactUrl };

function getViewerController(key) {
  return viewerControllers.get(key) || null;
}

function renderCallbacks() {
  return {
    onPageRendered: () => {
      schedulePageRowSync();
      scheduleRegionOverlayRender();
    },
    onScaleChanged: () => schedulePageRowSync(),
    onScaleRefresh: () => schedulePageRowSync(),
  };
}

function schedulePageRowSync() {
  scheduleReaderPageRowSync(viewerControllers);
}

export function scheduleScaleRefresh() {
  scheduleReaderScaleRefresh(viewerControllers, renderCallbacks());
}

export { bindPrimaryViewer };

function createViewerController(key) {
  const scrollShell = $("reader-scroll-shell");
  const viewerHost = $(`${key}-viewer-host`);
  const viewerElement = $(`${key}-viewer`);
  if (!scrollShell || !viewerHost || !viewerElement) {
    return null;
  }

  const controller = {
    key,
    scrollShell,
    viewerHost,
    viewerElement,
    basePageWidth: 0,
    currentScale: 0,
    pdfDocument: null,
    pageViewports: new Map(),
    renderedPages: new Set(),
    renderTasks: new Map(),
    visiblePages: new Set(),
    pageObserver: null,
    primaryScrollHandler: null,
  };
  viewerControllers.set(key, controller);
  return controller;
}

export async function mountPdfViewer({
  key,
  itemOrUrl,
  label,
  emptyId,
  fetchProtected = null,
}) {
  const viewerWrap = $(`${key}-wrap`);
  const empty = $(emptyId);
  const controller = getViewerController(key) || createViewerController(key);
  if (!viewerWrap || !empty || !controller) {
    return null;
  }

  let pdfDocument = null;
  try {
    pdfDocument = await loadPdfDocument({ itemOrUrl, fetchProtected });
  } catch (error) {
    // loadPdfDocument 内部对 pdfjsLib.getDocument(...).promise 没有兜底——
    // 404/CORS/损坏的 PDF 会在这里 reject,原来直接向上抛,最终被
    // mountReaderPdfPair 的 Promise.allSettled 吞掉,控制台不留任何痕迹,
    // 用户只会看到"这块 PDF 不显示"却无从判断是哪一种原因。这里补上日志,
    // 不改变外部可见行为(仍然落到下面的空状态展示)。
    console.error(`[reader] ${label || key} 加载失败`, error);
    showReaderPaneEmpty(key, emptyId);
    return null;
  }
  if (!pdfDocument) {
    // loadPdfDocument 在"没有可用 URL"时静默返回 null(没有 URL 可解析,
    // 或 itemOrUrl 本身是空字符串)——同样补一条日志,方便区分"没有 URL"
    // 和上面 catch 到的"有 URL 但加载失败"这两种不同原因。
    console.warn(`[reader] ${label || key} 没有可用的资源地址,跳过挂载`, { itemOrUrl });
    showReaderPaneEmpty(key, emptyId);
    return null;
  }

  const firstPage = await pdfDocument.getPage(1);
  const firstViewport = firstPage.getViewport({ scale: 1 });
  controller.basePageWidth = firstViewport.width;
  mountManualPages(controller, pdfDocument, firstViewport, renderCallbacks());
  applyViewerScale(controller, renderCallbacks());
  controller.visiblePages.add(1);
  if (pdfDocument.numPages > 1) {
    controller.visiblePages.add(2);
  }
  scheduleVisibleManualPages(controller, renderCallbacks());

  showReaderPaneReady(key, emptyId);

  return {
    key,
    pagesCount: pdfDocument.numPages,
    controller,
  };
}

export function bindResizeRefresh() {
  window.addEventListener("resize", scheduleScaleRefresh);
}

export { bindReaderRegionHover };

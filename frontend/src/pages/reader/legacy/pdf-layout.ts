import {
  scheduleVisibleManualPages,
  updateManualPageSizes,
} from "./pdf-renderer.js";

let resizeTicking = false;
let pageRowSyncTicking = false;

function resetSyncedPageHeights(viewerControllers) {
  viewerControllers.forEach((controller) => {
    controller.viewerElement.querySelectorAll(".page").forEach((page) => {
      page.style.minHeight = "";
    });
  });
}

function syncReaderPageRows(viewerControllers) {
  const rows = new Map();
  resetSyncedPageHeights(viewerControllers);
  viewerControllers.forEach((controller) => {
    controller.viewerElement.querySelectorAll(".page[data-page-number]").forEach((page) => {
      const pageNumber = page.getAttribute("data-page-number") || "";
      if (!pageNumber) {
        return;
      }
      const height = page.getBoundingClientRect().height;
      if (!Number.isFinite(height) || height <= 0) {
        return;
      }
      const row = rows.get(pageNumber) || { height: 0, pages: [] };
      row.height = Math.max(row.height, height);
      row.pages.push(page);
      rows.set(pageNumber, row);
    });
  });
  rows.forEach((row) => {
    if (row.pages.length < 2 || row.height <= 0) {
      return;
    }
    const height = `${Math.ceil(row.height)}px`;
    row.pages.forEach((page) => {
      page.style.minHeight = height;
    });
  });
}

export function schedulePageRowSync(viewerControllers) {
  if (pageRowSyncTicking) {
    return;
  }
  pageRowSyncTicking = true;
  window.requestAnimationFrame(() => {
    pageRowSyncTicking = false;
    syncReaderPageRows(viewerControllers);
  });
}

export function applyViewerScale(controller, callbacks: any = {}) {
  if (!controller?.pdfDocument || !controller.basePageWidth) {
    return;
  }
  const hostWidth = Math.max(320, controller.viewerHost.clientWidth || 0);
  const availableWidth = Math.max(280, hostWidth - 12);
  const scale = Math.max(0.35, Math.min(2.4, availableWidth / controller.basePageWidth));
  if (Math.abs((controller.currentScale || 0) - scale) < 0.005) {
    return;
  }
  controller.currentScale = scale;
  updateManualPageSizes(controller);
  controller.renderTasks?.forEach((task) => task?.cancel?.());
  controller.renderTasks?.clear();
  controller.renderedPages?.clear();
  scheduleVisibleManualPages(controller, callbacks);
  callbacks.onScaleChanged?.();
}

export function scheduleScaleRefresh(viewerControllers, callbacks: any = {}) {
  if (resizeTicking) {
    return;
  }
  resizeTicking = true;
  window.requestAnimationFrame(() => {
    resizeTicking = false;
    viewerControllers.forEach((controller) => {
      applyViewerScale(controller, callbacks);
    });
    callbacks.onScaleRefresh?.();
  });
}

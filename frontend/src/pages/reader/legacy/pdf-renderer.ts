import { pageNumberOfElement } from "./page-geometry.js";
import type { ManualPageRenderCallbacks } from "./types.js";

const MAX_READER_CANVAS_PIXELS = 8192 * 8192;
const MAX_READER_OUTPUT_SCALE = 2.5;

function outputScaleForPage(width, height) {
  const dpr = Math.max(1, Math.min(window.devicePixelRatio || 1, MAX_READER_OUTPUT_SCALE));
  const pixels = width * height * dpr * dpr;
  if (pixels <= MAX_READER_CANVAS_PIXELS) {
    return dpr;
  }
  return Math.max(1, Math.sqrt(MAX_READER_CANVAS_PIXELS / Math.max(1, width * height)));
}

function pageElementFor(controller, pageNumber) {
  return controller?.viewerElement.querySelector(`.page[data-page-number="${pageNumber}"]`) || null;
}

function setManualPageSize(controller, pageElement, pageNumber) {
  const viewport = controller.pageViewports.get(Number(pageNumber));
  if (!viewport || !pageElement) {
    return;
  }
  const width = Math.floor(viewport.width * controller.currentScale);
  const height = Math.floor(viewport.height * controller.currentScale);
  pageElement.style.width = `${width}px`;
  pageElement.style.height = `${height}px`;
  const canvasWrapper = pageElement.querySelector(".canvasWrapper");
  if (canvasWrapper) {
    canvasWrapper.style.width = `${width}px`;
    canvasWrapper.style.height = `${height}px`;
  }
}

export function updateManualPageSizes(controller) {
  controller?.viewerElement.querySelectorAll(".page[data-page-number]").forEach((pageElement) => {
    setManualPageSize(controller, pageElement, pageNumberOfElement(pageElement));
  });
}

async function renderManualPage(controller, pageNumber, callbacks: ManualPageRenderCallbacks = {}) {
  if (
    !controller?.pdfDocument
    || controller.renderedPages.has(pageNumber)
    || controller.renderTasks.has(pageNumber)
  ) {
    return;
  }
  const pageElement = pageElementFor(controller, pageNumber);
  const canvas = pageElement?.querySelector("canvas");
  if (!pageElement || !canvas) {
    return;
  }
  try {
    const pdfPage = await controller.pdfDocument.getPage(pageNumber);
    const baseViewport = pdfPage.getViewport({ scale: 1 });
    controller.pageViewports.set(pageNumber, {
      width: baseViewport.width,
      height: baseViewport.height,
    });
    setManualPageSize(controller, pageElement, pageNumber);
    const viewport = pdfPage.getViewport({ scale: controller.currentScale });
    const outputScale = outputScaleForPage(viewport.width, viewport.height);
    const context = canvas.getContext("2d", { alpha: false });
    if (!context) {
      return;
    }
    canvas.width = Math.floor(viewport.width * outputScale);
    canvas.height = Math.floor(viewport.height * outputScale);
    canvas.style.width = `${Math.floor(viewport.width)}px`;
    canvas.style.height = `${Math.floor(viewport.height)}px`;
    context.setTransform(outputScale, 0, 0, outputScale, 0, 0);
    const renderTask = pdfPage.render({ canvas, canvasContext: context, viewport });
    controller.renderTasks.set(pageNumber, renderTask);
    await renderTask.promise;
    controller.renderedPages.add(pageNumber);
  } catch (error) {
    if (error?.name !== "RenderingCancelledException") {
      pageElement.dataset.renderError = "1";
    }
  } finally {
    controller.renderTasks.delete(pageNumber);
    callbacks.onPageRendered?.();
  }
}

export function scheduleVisibleManualPages(controller, callbacks: ManualPageRenderCallbacks = {}) {
  const visiblePages = [...(controller?.visiblePages || [])].sort((a, b) => a - b);
  visiblePages.slice(0, 8).forEach((pageNumber) => {
    void renderManualPage(controller, pageNumber, callbacks);
  });
}

function createManualPageElement(controller, pageNumber, fallbackViewport) {
  const pageElement = document.createElement("div");
  pageElement.className = "page";
  pageElement.dataset.pageNumber = `${pageNumber}`;
  pageElement.setAttribute("role", "region");
  const canvasWrapper = document.createElement("div");
  canvasWrapper.className = "canvasWrapper";
  const canvas = document.createElement("canvas");
  canvasWrapper.appendChild(canvas);
  pageElement.appendChild(canvasWrapper);
  controller.viewerElement.appendChild(pageElement);
  controller.pageViewports.set(pageNumber, {
    width: fallbackViewport.width,
    height: fallbackViewport.height,
  });
  setManualPageSize(controller, pageElement, pageNumber);
  controller.pageObserver.observe(pageElement);
}

export function mountManualPages(controller, pdfDocument, firstViewport, callbacks: ManualPageRenderCallbacks = {}) {
  controller.pageObserver?.disconnect?.();
  controller.renderTasks.forEach((task) => task?.cancel?.());
  controller.viewerElement.innerHTML = "";
  controller.pdfDocument = pdfDocument;
  controller.pageViewports.clear();
  controller.renderedPages.clear();
  controller.renderTasks.clear();
  controller.visiblePages.clear();
  controller.pageObserver = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      const pageNumber = pageNumberOfElement(entry.target);
      if (!pageNumber) {
        return;
      }
      if (entry.isIntersecting) {
        controller.visiblePages.add(pageNumber);
      } else {
        controller.visiblePages.delete(pageNumber);
      }
    });
    scheduleVisibleManualPages(controller, callbacks);
  }, {
    root: controller.scrollShell,
    rootMargin: "900px 0px",
    threshold: 0.01,
  });
  for (let pageNumber = 1; pageNumber <= pdfDocument.numPages; pageNumber += 1) {
    createManualPageElement(controller, pageNumber, firstViewport);
  }
}

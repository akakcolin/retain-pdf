export function pageNumberOfElement(pageElement) {
  return Number(pageElement?.getAttribute?.("data-page-number") || 0);
}

function getPageCanvasBox(pageElement) {
  const canvas = pageElement?.querySelector?.("canvas");
  const pageRect = pageElement?.getBoundingClientRect?.();
  const rect = canvas?.getBoundingClientRect?.();
  if (!rect || !pageRect || rect.width <= 0 || rect.height <= 0) {
    return null;
  }
  return {
    left: rect.left - pageRect.left,
    top: rect.top - pageRect.top,
    width: rect.width,
    height: rect.height,
    pdfWidth: 0,
    pdfHeight: 0,
  };
}

function getPdfPageView(controller, pageNumber) {
  const viewport = controller?.pageViewports?.get(Number(pageNumber));
  if (!viewport) {
    return null;
  }
  return {
    pdfPage: {
      getViewport: ({ scale = 1 } = {}) => ({
        width: viewport.width * scale,
        height: viewport.height * scale,
      }),
    },
  };
}

export function getPageCanvasBoxWithPdfSize(controller, pageElement, pageNumber) {
  const canvasBox = getPageCanvasBox(pageElement);
  const pageView = getPdfPageView(controller, pageNumber);
  const viewport = pageView?.pdfPage?.getViewport?.({ scale: 1 });
  if (!canvasBox || !viewport?.width || !viewport?.height) {
    return canvasBox;
  }
  return {
    ...canvasBox,
    pdfWidth: viewport.width,
    pdfHeight: viewport.height,
  };
}

export function ensureRegionLayer(pageElement, className) {
  let layer = pageElement.querySelector(`.${className}`);
  if (!layer) {
    layer = document.createElement("div");
    layer.className = className;
    pageElement.appendChild(layer);
  }
  return layer;
}

export function clearRegionLayers(controller, className) {
  controller?.viewerElement.querySelectorAll(`.${className}`).forEach((layer) => {
    layer.innerHTML = "";
  });
}

export function placeRegionBox(element, bbox, canvasBox) {
  if (!element || !canvasBox) {
    return false;
  }
  const [x0, y0, x1, y1] = bbox;
  const pageWidth = Number(canvasBox.pdfWidth || 0);
  const pageHeight = Number(canvasBox.pdfHeight || 0);
  if (!pageWidth || !pageHeight) {
    return false;
  }
  const widthScale = canvasBox.width / pageWidth;
  const heightScale = canvasBox.height / pageHeight;
  const left = x0 * widthScale;
  const top = y0 * heightScale;
  const width = (x1 - x0) * widthScale;
  const height = (y1 - y0) * heightScale;
  if (![left, top, width, height].every(Number.isFinite) || width <= 0 || height <= 0) {
    return false;
  }
  element.style.left = `${canvasBox.left + left}px`;
  element.style.top = `${canvasBox.top + top}px`;
  element.style.width = `${Math.max(1, width)}px`;
  element.style.height = `${Math.max(1, height)}px`;
  return true;
}

export function regionRectFromBox(bbox, canvasBox) {
  if (!canvasBox) {
    return null;
  }
  const [x0, y0, x1, y1] = bbox;
  const pageWidth = Number(canvasBox.pdfWidth || 0);
  const pageHeight = Number(canvasBox.pdfHeight || 0);
  if (!pageWidth || !pageHeight) {
    return null;
  }
  const widthScale = canvasBox.width / pageWidth;
  const heightScale = canvasBox.height / pageHeight;
  const left = canvasBox.left + x0 * widthScale;
  const top = canvasBox.top + y0 * heightScale;
  const width = (x1 - x0) * widthScale;
  const height = (y1 - y0) * heightScale;
  if (![left, top, width, height].every(Number.isFinite) || width <= 0 || height <= 0) {
    return null;
  }
  return { left, top, right: left + width, bottom: top + height };
}

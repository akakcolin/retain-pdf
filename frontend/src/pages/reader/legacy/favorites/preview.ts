import type { ReaderSelection } from "../types.js";

export function captureSelectionPreview(selection: Partial<ReaderSelection> = {}) {
  const canvas = selection.pageElement?.querySelector?.("canvas") as HTMLCanvasElement | null;
  const rect = selection.sourceRect || selection.rect;
  if (!canvas?.getContext || !rect) {
    return "";
  }
  const pageRect = selection.pageElement?.getBoundingClientRect?.() || { left: 0, top: 0 };
  const canvasRect = canvas.getBoundingClientRect?.() || {
    left: pageRect.left,
    top: pageRect.top,
    width: canvas.clientWidth || canvas.width || 1,
    height: canvas.clientHeight || canvas.height || 1,
  };
  const canvasClientWidth = canvasRect.width || canvas.clientWidth || canvas.width || 1;
  const canvasClientHeight = canvasRect.height || canvas.clientHeight || canvas.height || 1;
  const scaleX = canvas.width / canvasClientWidth;
  const scaleY = canvas.height / canvasClientHeight;
  const rectViewportLeft = pageRect.left + rect.left;
  const rectViewportTop = pageRect.top + rect.top;
  const sourceX = Math.max(0, Math.round((rectViewportLeft - canvasRect.left) * scaleX));
  const sourceY = Math.max(0, Math.round((rectViewportTop - canvasRect.top) * scaleY));
  const sourceWidth = Math.max(1, Math.round(rect.width * scaleX));
  const sourceHeight = Math.max(1, Math.round(rect.height * scaleY));
  const documentRef = selection.pageElement?.ownerDocument || globalThis.document;
  const preview = documentRef?.createElement?.("canvas");
  if (!preview?.getContext) {
    return "";
  }
  const maxWidth = 1200;
  const ratio = Math.min(1, maxWidth / sourceWidth);
  preview.width = Math.max(1, Math.round(sourceWidth * ratio));
  preview.height = Math.max(1, Math.round(sourceHeight * ratio));
  try {
    const context = preview.getContext("2d");
    if (context) {
      context.imageSmoothingEnabled = true;
      context.imageSmoothingQuality = "high";
    }
    context?.drawImage(
      canvas,
      sourceX,
      sourceY,
      sourceWidth,
      sourceHeight,
      0,
      0,
      preview.width,
      preview.height,
    );
    return preview.toDataURL?.("image/png") || "";
  } catch {
    return "";
  }
}

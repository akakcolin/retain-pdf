import type { DragRectInput, PixelRect, RelativeRect } from "../types.js";

export function clampRect(rect: DragRectInput): PixelRect {
  const left = Math.min(rect.startX, rect.endX);
  const top = Math.min(rect.startY, rect.endY);
  const width = Math.abs(rect.endX - rect.startX);
  const height = Math.abs(rect.endY - rect.startY);
  return { left, top, width, height };
}

export function formatRect(rect: Partial<PixelRect> = {}) {
  return [
    Math.round(rect.left),
    Math.round(rect.top),
    Math.round(rect.width),
    Math.round(rect.height),
  ].join(", ");
}

export function relativeRect(rect: Partial<PixelRect> = {}, pageElement?: Element | null): RelativeRect {
  const pageRect = pageElement?.getBoundingClientRect?.();
  const width = Number(pageRect?.width) || 1;
  const height = Number(pageRect?.height) || 1;
  return {
    x: Math.max(0, Math.min(1, (Number(rect.left) || 0) / width)),
    y: Math.max(0, Math.min(1, (Number(rect.top) || 0) / height)),
    width: Math.max(0, Math.min(1, (Number(rect.width) || 0) / width)),
    height: Math.max(0, Math.min(1, (Number(rect.height) || 0) / height)),
  };
}

export function viewportRelativeRect(rect: Partial<PixelRect> = {}, rootElement?: Element | null): RelativeRect {
  const viewportWidth = Number(globalThis.window?.innerWidth) || 0;
  const viewportHeight = Number(globalThis.window?.innerHeight) || 0;
  const rootRect = rootElement?.getBoundingClientRect?.();
  const width = viewportWidth || Number(rootRect?.width) || 1;
  const height = viewportHeight || Number(rootRect?.height) || 1;
  return {
    x: Math.max(0, Math.min(1, (Number(rect.left) || 0) / width)),
    y: Math.max(0, Math.min(1, (Number(rect.top) || 0) / height)),
    width: Math.max(0, Math.min(1, (Number(rect.width) || 0) / width)),
    height: Math.max(0, Math.min(1, (Number(rect.height) || 0) / height)),
  };
}

export function rectFromViewportRelative(relative: Partial<RelativeRect> = {}, rootElement?: Element | null): PixelRect | null {
  const viewportWidth = Number(globalThis.window?.innerWidth) || 0;
  const viewportHeight = Number(globalThis.window?.innerHeight) || 0;
  const rootRect = rootElement?.getBoundingClientRect?.();
  const width = viewportWidth || Number(rootRect?.width) || 0;
  const height = viewportHeight || Number(rootRect?.height) || 0;
  if (!width || !height) {
    return null;
  }
  return {
    left: (Number(relative.x) || 0) * width,
    top: (Number(relative.y) || 0) * height,
    width: (Number(relative.width) || 0) * width,
    height: (Number(relative.height) || 0) * height,
  };
}

export function clampScale(value: number | string | null | undefined) {
  return Math.max(1 / 3, Math.min(3, Number(value) || 1));
}

export function rectFromRelative(relative: Partial<RelativeRect> = {}, pageElement?: Element | null): PixelRect | null {
  const pageRect = pageElement?.getBoundingClientRect?.();
  const width = Number(pageRect?.width) || 0;
  const height = Number(pageRect?.height) || 0;
  if (!width || !height) {
    return null;
  }
  return {
    left: (Number(relative.x) || 0) * width,
    top: (Number(relative.y) || 0) * height,
    width: (Number(relative.width) || 0) * width,
    height: (Number(relative.height) || 0) * height,
  };
}

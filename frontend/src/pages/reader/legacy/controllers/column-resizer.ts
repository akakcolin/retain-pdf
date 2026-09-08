// 三栏骨架的左右栏拖拽调宽:分隔条 pointer 拖动实时改 CSS 变量,松手持久化。
// 纯宽度计算(clampColumnWidth)与持久化(load/save)抽出便于单测;DOM 拖拽为薄封装。

export const READER_COLUMN_LIMITS = {
  left: { min: 180, max: 460, default: 248 },
  right: { min: 300, max: 620, default: 384 },
};

const STORAGE_KEY = "retainpdf-reader-cols-v1";

// 夹取到 [min, max] 的整数像素;非法输入回退到 default。
export function clampColumnWidth(px, { min, max, default: fallback }) {
  const value = Number(px);
  if (!Number.isFinite(value)) {
    return fallback;
  }
  return Math.max(min, Math.min(max, Math.round(value)));
}

export function loadColumnWidths(storage = globalThis.localStorage || null) {
  const defaults = {
    left: READER_COLUMN_LIMITS.left.default,
    right: READER_COLUMN_LIMITS.right.default,
  };
  if (!storage) {
    return defaults;
  }
  try {
    const parsed = JSON.parse(storage.getItem(STORAGE_KEY) || "null");
    if (!parsed || typeof parsed !== "object") {
      return defaults;
    }
    return {
      left: clampColumnWidth(parsed.left ?? defaults.left, READER_COLUMN_LIMITS.left),
      right: clampColumnWidth(parsed.right ?? defaults.right, READER_COLUMN_LIMITS.right),
    };
  } catch (_err) {
    return defaults;
  }
}

export function saveColumnWidths(widths, storage = globalThis.localStorage || null) {
  if (!storage) {
    return;
  }
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify({ left: widths.left, right: widths.right }));
  } catch (_err) {
    // 配额满/隐私模式:静默失败
  }
}

export function createReaderColumnResizer({
  documentRef = globalThis.document,
  storage = globalThis.localStorage || null,
  onResize = null,
} = {}) {
  const body = documentRef?.body || documentRef?.querySelector?.("body");
  const leftHandle = documentRef?.getElementById?.("reader-col-resizer-left");
  const rightHandle = documentRef?.getElementById?.("reader-col-resizer-right");
  const widths = loadColumnWidths(storage);

  function setVar(name, value) {
    body?.style?.setProperty?.(name, `${value}px`);
  }

  // 左栏"展开宽度":applied 宽 --reader-left-w 由 CSS 从它派生,折叠时被类覆盖为 0
  function applyLeft(px) {
    widths.left = clampColumnWidth(px, READER_COLUMN_LIMITS.left);
    setVar("--reader-left-col", widths.left);
    return widths.left;
  }

  // 右栏"展开宽度"::has(.is-open) 时右栏取用它,收起时右栏为 0(见 CSS)
  function applyRight(px) {
    widths.right = clampColumnWidth(px, READER_COLUMN_LIMITS.right);
    setVar("--reader-right-col", widths.right);
    return widths.right;
  }

  function viewportWidth() {
    return documentRef?.defaultView?.innerWidth
      || globalThis.window?.innerWidth
      || 1280;
  }

  function bindHandle(handle, side) {
    if (!handle?.addEventListener) {
      return;
    }
    let dragging = false;

    function onMove(event) {
      if (!dragging) {
        return;
      }
      const clientX = Number(event.clientX) || 0;
      if (side === "left") {
        applyLeft(clientX);
      } else {
        applyRight(viewportWidth() - clientX);
      }
      onResize?.();
    }

    function onUp(event) {
      if (!dragging) {
        return;
      }
      dragging = false;
      body?.classList?.remove?.("reader-resizing");
      handle.releasePointerCapture?.(event.pointerId);
      documentRef?.removeEventListener?.("pointermove", onMove);
      documentRef?.removeEventListener?.("pointerup", onUp);
      saveColumnWidths(widths, storage);
      onResize?.();
    }

    handle.addEventListener("pointerdown", (event) => {
      dragging = true;
      body?.classList?.add?.("reader-resizing");
      handle.setPointerCapture?.(event.pointerId);
      documentRef?.addEventListener?.("pointermove", onMove);
      documentRef?.addEventListener?.("pointerup", onUp);
      event.preventDefault?.();
    });

    // 双击分隔条:恢复该栏默认宽度
    handle.addEventListener("dblclick", () => {
      if (side === "left") {
        applyLeft(READER_COLUMN_LIMITS.left.default);
      } else {
        applyRight(READER_COLUMN_LIMITS.right.default);
      }
      saveColumnWidths(widths, storage);
      onResize?.();
    });
  }

  function bindEvents() {
    applyLeft(widths.left);
    applyRight(widths.right);
    bindHandle(leftHandle, "left");
    bindHandle(rightHandle, "right");
  }

  return { bindEvents, applyLeft, applyRight, widths: () => ({ ...widths }) };
}

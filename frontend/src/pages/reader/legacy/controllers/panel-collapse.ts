// 三栏骨架的左右栏折叠:把手切换 body 上的折叠类,收起该栏(宽度归零),状态持久化。
// 折叠是纯视觉(不改右栏的 active 抽屉),展开后原内容仍在。

const STORAGE_KEY = "retainpdf-reader-collapse-v1";

export function loadCollapseState(storage = globalThis.localStorage || null) {
  const blank = { left: false, right: false };
  if (!storage) {
    return blank;
  }
  try {
    const parsed = JSON.parse(storage.getItem(STORAGE_KEY) || "null");
    if (!parsed || typeof parsed !== "object") {
      return blank;
    }
    return { left: Boolean(parsed.left), right: Boolean(parsed.right) };
  } catch (_err) {
    return blank;
  }
}

export function saveCollapseState(state, storage = globalThis.localStorage || null) {
  if (!storage) {
    return;
  }
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify({ left: Boolean(state.left), right: Boolean(state.right) }));
  } catch (_err) {
    // 配额满/隐私模式:静默失败
  }
}

export function createReaderPanelCollapse({
  documentRef = globalThis.document,
  storage = globalThis.localStorage || null,
  onChange = null,
} = {}) {
  const body = documentRef?.body || documentRef?.querySelector?.("body");
  const leftBtn = documentRef?.getElementById?.("reader-left-collapse-btn");
  const rightBtn = documentRef?.getElementById?.("reader-right-collapse-btn");
  const state = loadCollapseState(storage);

  function apply() {
    body?.classList?.toggle?.("reader-left-collapsed", state.left);
    body?.classList?.toggle?.("reader-right-collapsed", state.right);
    // aria-expanded 反映"栏是否展开";把手图标随 is-collapsed 翻转(CSS)
    leftBtn?.setAttribute?.("aria-expanded", state.left ? "false" : "true");
    rightBtn?.setAttribute?.("aria-expanded", state.right ? "false" : "true");
    leftBtn?.classList?.toggle?.("is-collapsed", state.left);
    rightBtn?.classList?.toggle?.("is-collapsed", state.right);
    onChange?.();
  }

  function toggleLeft() {
    state.left = !state.left;
    saveCollapseState(state, storage);
    apply();
  }

  function toggleRight() {
    state.right = !state.right;
    saveCollapseState(state, storage);
    apply();
  }

  // 顶栏点开某工具时,右栏若处于折叠态则自动展开亮出内容
  function expandRight() {
    if (state.right) {
      state.right = false;
      saveCollapseState(state, storage);
      apply();
    }
  }

  function bindEvents() {
    leftBtn?.addEventListener?.("click", toggleLeft);
    rightBtn?.addEventListener?.("click", toggleRight);
    apply();
  }

  return { bindEvents, toggleLeft, toggleRight, expandRight, state: () => ({ ...state }) };
}

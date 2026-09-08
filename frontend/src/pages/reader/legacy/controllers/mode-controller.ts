const READER_MODES = new Set(["source", "translated", "compare"]);
const READER_MODE_ORDER = ["source", "translated", "compare"];

function normalizeMode(mode) {
  return READER_MODES.has(mode) ? mode : "compare";
}

function updateModeClasses(root, mode) {
  if (!root?.classList) {
    return;
  }
  root.classList.toggle("reader-mode-source", mode === "source");
  root.classList.toggle("reader-mode-translated", mode === "translated");
  root.classList.toggle("reader-mode-compare", mode === "compare");
}

function updateTabs(documentRef, mode) {
  documentRef?.querySelectorAll?.("[data-reader-mode]").forEach((button) => {
    const active = button.dataset.readerMode === mode;
    button.classList.toggle("is-active", active);
    button.setAttribute("aria-selected", active ? "true" : "false");
    button.setAttribute("tabindex", active ? "0" : "-1");
  });
}

function updatePanels(documentRef, mode) {
  documentRef?.querySelectorAll?.("[data-reader-pane]").forEach((panel) => {
    const pane = panel.dataset.readerPane;
    const visible = mode === "compare" || pane === mode;
    panel.hidden = !visible;
    panel.inert = !visible;
  });
}

export function createReaderModeController({
  documentRef = globalThis.document,
  root = documentRef?.body,
  onModeChanged = null,
  onModeHudChanged = null,
}: any = {}) {
  let currentMode = normalizeMode(root?.dataset?.readerMode || "compare");

  function setMode(nextMode) {
    const mode = normalizeMode(nextMode);
    currentMode = mode;
    if (root?.dataset) {
      root.dataset.readerMode = mode;
    }
    updateModeClasses(root, mode);
    updateTabs(documentRef, mode);
    updatePanels(documentRef, mode);
    onModeHudChanged?.(mode);
    onModeChanged?.(mode);
    return mode;
  }

  function bindEvents() {
    const buttons: any[] = Array.from(documentRef?.querySelectorAll?.("[data-reader-mode]") || []);
    buttons.forEach((button) => {
      button.addEventListener("click", () => setMode(button.dataset.readerMode));
      button.addEventListener("keydown", (event) => {
        if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
          return;
        }
        event.preventDefault();
        const currentIndex = READER_MODE_ORDER.indexOf(currentMode);
        const lastIndex = READER_MODE_ORDER.length - 1;
        const nextIndex = event.key === "Home"
          ? 0
          : event.key === "End"
            ? lastIndex
            : event.key === "ArrowLeft"
              ? (currentIndex + lastIndex) % READER_MODE_ORDER.length
              : (currentIndex + 1) % READER_MODE_ORDER.length;
        const nextMode = setMode(READER_MODE_ORDER[nextIndex]);
        buttons.find((item) => item.dataset.readerMode === nextMode)?.focus?.();
      });
    });
    setMode(currentMode);
  }

  return {
    bindEvents,
    currentMode: () => currentMode,
    setMode,
  };
}

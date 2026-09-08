const DEFAULT_IDLE_DELAY = 1600;

function hasOpenDrawer(documentRef) {
  return Boolean(documentRef?.querySelector?.(".reader-side-drawer.is-open"));
}

export function createReaderChromeController({
  documentRef = globalThis.document,
  root = documentRef?.body,
  idleDelay = DEFAULT_IDLE_DELAY,
  setTimeoutFn = globalThis.setTimeout,
  clearTimeoutFn = globalThis.clearTimeout,
}: any = {}) {
  let hideTimer = 0;
  let bound = false;

  function setVisible(visible) {
    root?.classList?.toggle("reader-chrome-muted", !visible);
  }

  function scheduleHide() {
    if (!root?.classList || hasOpenDrawer(documentRef)) {
      return;
    }
    if (hideTimer) {
      clearTimeoutFn(hideTimer);
    }
    hideTimer = setTimeoutFn(() => {
      hideTimer = 0;
      setVisible(false);
    }, idleDelay);
  }

  function wake() {
    if (hideTimer) {
      clearTimeoutFn(hideTimer);
      hideTimer = 0;
    }
    setVisible(true);
    scheduleHide();
  }

  function keepVisible() {
    if (hideTimer) {
      clearTimeoutFn(hideTimer);
      hideTimer = 0;
    }
    setVisible(true);
  }

  function bindEvents() {
    if (bound) {
      return;
    }
    bound = true;
    const scrollShell = documentRef?.getElementById?.("reader-scroll-shell");
    const topbar = documentRef?.querySelector?.(".reader-topbar");
    const pageIndicator = documentRef?.getElementById?.("reader-page-indicator");

    scrollShell?.addEventListener?.("scroll", wake, { passive: true });
    documentRef?.addEventListener?.("mousemove", wake, { passive: true });
    documentRef?.addEventListener?.("keydown", wake);
    [topbar, pageIndicator].forEach((element) => {
      element?.addEventListener?.("mouseenter", keepVisible);
      element?.addEventListener?.("focusin", keepVisible);
      element?.addEventListener?.("mouseleave", scheduleHide);
      element?.addEventListener?.("focusout", scheduleHide);
    });
    wake();
  }

  return {
    bindEvents,
    keepVisible,
    scheduleHide,
    wake,
  };
}

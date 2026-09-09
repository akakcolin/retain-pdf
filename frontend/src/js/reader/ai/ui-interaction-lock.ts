// AI 会话切换 / 分支时的短时隔离：防列表收起后点击穿透到 PDF/链接。
// 只在 lock 窗口内生效，绝不永久拦截阅读入口或正常导航。

let lockUntil = 0;
let shieldCleanup: (() => void) | null = null;
let overlayEl: HTMLDivElement | null = null;
let openGuardInstalled = false;

export function isReaderAiNavigationLocked(now = Date.now()): boolean {
  return now < lockUntil;
}

export function lockReaderAiNavigation(durationMs = 700): void {
  const until = Date.now() + Math.max(0, durationMs);
  if (until > lockUntil) lockUntil = until;
}

/** 强制解除隔离（进页/异常时兜底，避免遮罩残留导致点不了） */
export function clearReaderAiNavigationLock(): void {
  lockUntil = 0;
  shieldCleanup?.();
  shieldCleanup = null;
  removeOverlay();
}

function ensureOverlay(): HTMLDivElement | null {
  if (typeof document === "undefined") return null;
  if (overlayEl && overlayEl.isConnected) return overlayEl;
  const el = document.createElement("div");
  el.setAttribute("data-reader-ai-pointer-shield", "1");
  el.setAttribute("aria-hidden", "true");
  Object.assign(el.style, {
    position: "fixed",
    inset: "0",
    zIndex: "2147483000",
    cursor: "progress",
    background: "transparent",
    pointerEvents: "auto",
    touchAction: "none",
  } as CSSStyleDeclaration);
  const block = (event: Event) => {
    event.preventDefault();
    event.stopPropagation();
    (event as { stopImmediatePropagation?: () => void }).stopImmediatePropagation?.();
  };
  for (const type of [
    "pointerdown",
    "pointerup",
    "mousedown",
    "mouseup",
    "click",
    "auxclick",
    "dblclick",
    "contextmenu",
    "touchstart",
    "touchend",
  ] as const) {
    el.addEventListener(type, block, { capture: true, passive: false });
  }
  document.documentElement.appendChild(el);
  overlayEl = el;
  return el;
}

function removeOverlay(): void {
  if (!overlayEl) return;
  try {
    overlayEl.remove();
  } catch {
    // ignore
  }
  overlayEl = null;
}

/**
 * 短时全屏吞指针 + 禁止跳页/开链。
 * 仅用于 AI 会话条切换 / 分支，时长应尽量短。
 */
export function armReaderAiClickShield(
  durationMs = 700,
  options: { overlayDelayMs?: number } = {},
): void {
  lockReaderAiNavigation(durationMs);
  if (typeof document === "undefined") return;

  shieldCleanup?.();
  shieldCleanup = null;

  const until = Date.now() + Math.max(0, durationMs);
  const overlayDelay = Math.max(0, Number(options.overlayDelayMs) || 0);
  let overlayTimer: ReturnType<typeof setTimeout> | null = null;
  let safetyTimer: ReturnType<typeof setTimeout> | null = null;

  if (overlayDelay === 0) {
    ensureOverlay();
  } else {
    overlayTimer = setTimeout(() => {
      overlayTimer = null;
      if (Date.now() < until) ensureOverlay();
    }, overlayDelay);
  }

  const swallow = (event: Event) => {
    if (Date.now() >= until) {
      teardown();
      return;
    }
    const target = event.target;
    // 会话条 / 答案操作条（开新对话按钮）放行，避免 pointerdown 上锁后点不中
    if (
      target instanceof Element
      && target.closest(
        "[data-reader-ai-sessions], [data-reader-ai-actions], .aui-action-btn-branch",
      )
    ) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    (event as { stopImmediatePropagation?: () => void }).stopImmediatePropagation?.();
  };

  const opts: AddEventListenerOptions = { capture: true, passive: false };
  const types = ["click", "auxclick", "dblclick", "pointerup", "mouseup"] as const;

  const teardown = () => {
    if (overlayTimer != null) {
      clearTimeout(overlayTimer);
      overlayTimer = null;
    }
    if (safetyTimer != null) {
      clearTimeout(safetyTimer);
      safetyTimer = null;
    }
    for (const type of types) {
      document.removeEventListener(type, swallow, opts);
    }
    removeOverlay();
    if (shieldCleanup === teardown) shieldCleanup = null;
  };

  for (const type of types) {
    document.addEventListener(type, swallow, opts);
  }
  shieldCleanup = teardown;
  // 存住兜底定时器:重新 arm 时 shieldCleanup() 会跑旧 teardown,若不清掉
  // 这个定时器,它稍后仍会 removeOverlay(),把新 shield 的遮罩提前摘掉。
  safetyTimer = setTimeout(teardown, Math.max(0, durationMs) + 48);
}

export function shouldIgnoreReaderAiNavEvent(event: Event | null | undefined): boolean {
  if (isReaderAiNavigationLocked()) return true;
  if (!event) return false;
  // typeof 守卫：node/jsdom 没有全局 MouseEvent，裸 instanceof 会抛
  // ReferenceError 且在事件 listener 里被静默吞掉（answer-enhance 测试
  // 曾因此假失败——按钮注入成功但 onJump 永不触发）。
  if (typeof MouseEvent !== "undefined" && event instanceof MouseEvent && event.isTrusted === false) {
    return true;
  }
  return false;
}

/**
 * 仅在 AI 导航锁定期拦截 window.open / 链接默认行为。
 * 不永久 ban 同源导航，避免破坏正常打开阅读。
 */
export function installReaderWindowOpenGuard(): () => void {
  if (typeof window === "undefined" || typeof window.open !== "function") {
    return () => {};
  }
  if (openGuardInstalled) return () => {};
  openGuardInstalled = true;

  // 进页先清残留遮罩
  clearReaderAiNavigationLock();

  const original = window.open.bind(window);
  window.open = ((url?: string | URL, target?: string, features?: string) => {
    // 只在锁定期拒绝（会话切换误触）；平时不拦
    if (isReaderAiNavigationLocked()) {
      return null;
    }
    return original(url as string, target, features);
  }) as typeof window.open;

  const onClickCapture = (event: Event) => {
    if (!isReaderAiNavigationLocked()) return;
    const t = event.target;
    if (!(t instanceof Element)) return;
    // 会话条自身放行
    if (t.closest("[data-reader-ai-sessions]")) return;
    const a = t.closest("a[href]");
    if (a) {
      event.preventDefault();
      event.stopPropagation();
    }
  };
  document.addEventListener("click", onClickCapture, true);

  return () => {
    window.open = original;
    document.removeEventListener("click", onClickCapture, true);
    openGuardInstalled = false;
    clearReaderAiNavigationLock();
  };
}

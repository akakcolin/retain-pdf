// 模式切换 / 跳页时的阅读锚点锁定与恢复。
// 关键规则：切换前锁定 progress，恢复期间禁止 scroll 写回锚点，且绝不 re-measure。

import { useCallback, useEffect, useRef } from "react";
import type { RefObject } from "react";
import {
  alignShellToPage,
  alignShellToProgress,
  applyPageScrollProgress,
  clampPageNumber,
  cloneProgress,
  measurePageScrollProgress,
  scrollShellToPage,
  type PageScrollProgress,
} from "./scroll-to-page.js";

export type ReadingAnchorPane = "source" | "translated";

// ── 时序常数（评审 P2-7；工程经验值，改动前先读注释）────────────────
// 模式切换后恢复滚动锚点：分多拍重试，覆盖字体/图片加载造成的 layout 漂移。
// 失效后果：拍数太少→锚点恢复不到位；末拍太大→用户看到"跳一下"。
const MODE_RESTORE_DELAYS_MS = [0, 48, 140, 320, 560];
// 兜底安全窗：超过该时长仍未稳定就放弃恢复，避免长时间劫持滚动。
const MODE_RESTORE_SAFETY_MS = 700;
// goto 页码对齐重试：三拍，等待目标页渲染出真实高度。
const GOTO_ALIGN_DELAYS_MS = [80, 200, 400];
// goto 兜底安全窗，语义同 MODE_RESTORE_SAFETY_MS。
const GOTO_SAFETY_MS = 500;
// 恢复完成后解除滚动冻结前的缓冲：等浏览器把最后一拍 layout 画完。
const UNFREEZE_DELAY_MS = 50;
// 滚动壳 ref 尚未挂载时的重绑间隔（attach 重试）。
const ATTACH_RETRY_MS = 50;

export function useReadingAnchor(
  shellRef: RefObject<HTMLElement | null>,
  options: {
    primaryPane: ReadingAnchorPane;
    /** when mode changes, hook restores locked progress */
    mode: string;
    /** false while boot loading */
    enabled?: boolean;
  },
): {
  /** measure shell progress (HUD / fallback); does not freeze restore */
  lockFromShell: () => PageScrollProgress;
  /** call before setMode; freezes restore and locks progress */
  beginModeSwitch: () => PageScrollProgress;
  /** jump to page top; freezes briefly */
  goToPage: (page: number, numPages: number) => void;
  getAnchor: () => PageScrollProgress;
  isRestoring: () => boolean;
  /** call when layout settles (rowHeights/shellWidth) while restoring — re-pin locked only */
  repinIfRestoring: () => void;
} {
  const { primaryPane, mode, enabled = true } = options;

  /** 用户真实阅读锚点（仅用户滚动 / 跳转 / 恢复完成后更新） */
  const anchorRef = useRef<PageScrollProgress>({ page: 1, fraction: 0 });
  /** 本次恢复锁定的锚点（不被中间 scroll 事件污染） */
  const pendingRestoreRef = useRef<PageScrollProgress | null>(null);
  /** 恢复中：禁止 scroll 写回 anchor */
  const restoringRef = useRef(false);
  const prevModeRef = useRef(mode);
  const cancelRestoreRef = useRef<(() => void) | null>(null);
  const safetyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const unfreezeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const primaryPaneRef = useRef(primaryPane);
  primaryPaneRef.current = primaryPane;

  const clearRestoreTimers = useCallback(() => {
    cancelRestoreRef.current?.();
    cancelRestoreRef.current = null;
    if (safetyTimerRef.current != null) {
      clearTimeout(safetyTimerRef.current);
      safetyTimerRef.current = null;
    }
  }, []);

  const finishRestore = useCallback((locked: PageScrollProgress) => {
    // 恢复完成：锚点钉回锁定值，再允许滚动更新
    anchorRef.current = cloneProgress(locked);
    pendingRestoreRef.current = null;
    if (unfreezeTimerRef.current != null) {
      clearTimeout(unfreezeTimerRef.current);
    }
    // 稍后再解冻，避免最后一次程序化 scroll 事件写脏锚点
    unfreezeTimerRef.current = setTimeout(() => {
      unfreezeTimerRef.current = null;
      restoringRef.current = false;
    }, UNFREEZE_DELAY_MS);
  }, []);

  // 仅用户滚动时更新锚点；恢复期间一律忽略
  useEffect(() => {
    if (!enabled) {
      return;
    }

    let cancelled = false;
    let root: HTMLElement | null = null;
    let onScroll: (() => void) | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;

    const attach = () => {
      if (cancelled) return;
      const el = shellRef.current;
      if (!el) {
        retryTimer = setTimeout(attach, ATTACH_RETRY_MS);
        return;
      }
      root = el;
      onScroll = () => {
        if (restoringRef.current) {
          return;
        }
        const progress = measurePageScrollProgress(root, primaryPaneRef.current);
        if (progress) {
          anchorRef.current = progress;
        }
      };
      root.addEventListener("scroll", onScroll, { passive: true });
      if (!restoringRef.current) {
        onScroll();
      }
    };

    attach();
    return () => {
      cancelled = true;
      if (retryTimer != null) {
        clearTimeout(retryTimer);
      }
      if (root && onScroll) {
        root.removeEventListener("scroll", onScroll);
      }
    };
  }, [enabled, mode, primaryPane, shellRef]);

  // 模式切换后：只用 pending 锁定锚点恢复，绝不重新 measure
  useEffect(() => {
    if (prevModeRef.current === mode) {
      return;
    }
    prevModeRef.current = mode;

    if (!enabled) {
      restoringRef.current = false;
      pendingRestoreRef.current = null;
      clearRestoreTimers();
      return;
    }

    const locked = pendingRestoreRef.current
      ? cloneProgress(pendingRestoreRef.current)
      : cloneProgress(anchorRef.current);

    // 再次确保冻结（应对严格模式下 effect 重跑）
    restoringRef.current = true;
    pendingRestoreRef.current = locked;
    anchorRef.current = locked;

    clearRestoreTimers();
    cancelRestoreRef.current = alignShellToProgress(
      () => shellRef.current,
      locked,
      {
        behavior: "auto",
        pane: primaryPane,
        // 等页宽/行高同步后再钉；同一 locked 幂等，不会越滚越远
        delaysMs: MODE_RESTORE_DELAYS_MS,
        onDone: () => finishRestore(locked),
      },
    );

    // 兜底解冻
    safetyTimerRef.current = setTimeout(() => {
      safetyTimerRef.current = null;
      finishRestore(locked);
    }, MODE_RESTORE_SAFETY_MS);

    return () => {
      clearRestoreTimers();
    };
  }, [mode, enabled, primaryPane, shellRef, finishRestore, clearRestoreTimers]);

  useEffect(() => {
    return () => {
      clearRestoreTimers();
      if (unfreezeTimerRef.current != null) {
        clearTimeout(unfreezeTimerRef.current);
        unfreezeTimerRef.current = null;
      }
    };
  }, [clearRestoreTimers]);

  const lockFromShell = useCallback((): PageScrollProgress => {
    const measured = measurePageScrollProgress(
      shellRef.current,
      primaryPaneRef.current,
    );
    if (measured) {
      return cloneProgress(measured);
    }
    return cloneProgress(anchorRef.current);
  }, [shellRef]);

  const beginModeSwitch = useCallback((): PageScrollProgress => {
    // 1) 先冻结，防止 setMode 后布局钳位 scrollTop 触发的 scroll 写脏锚点
    restoringRef.current = true;
    // 2) 在布局变化前锁定当前位置
    const measured = measurePageScrollProgress(
      shellRef.current,
      primaryPaneRef.current,
    );
    const locked = cloneProgress(measured ?? anchorRef.current);
    anchorRef.current = locked;
    pendingRestoreRef.current = locked;
    return locked;
  }, [shellRef]);

  const goToPage = useCallback((page: number, numPages: number) => {
    const target = clampPageNumber(page, numPages || 1);
    const locked: PageScrollProgress = { page: target, fraction: 0 };
    anchorRef.current = locked;
    restoringRef.current = true;
    pendingRestoreRef.current = locked;

    clearRestoreTimers();
    const pane = primaryPaneRef.current;
    scrollShellToPage(shellRef.current, target, "smooth", pane);
    cancelRestoreRef.current = alignShellToPage(
      () => shellRef.current,
      target,
      {
        behavior: "auto",
        pane,
        delaysMs: GOTO_ALIGN_DELAYS_MS,
        onDone: () => finishRestore(locked),
      },
    );
    safetyTimerRef.current = setTimeout(() => {
      safetyTimerRef.current = null;
      finishRestore(locked);
    }, GOTO_SAFETY_MS);
  }, [shellRef, finishRestore, clearRestoreTimers]);

  const getAnchor = useCallback((): PageScrollProgress => {
    return cloneProgress(anchorRef.current);
  }, []);

  const isRestoring = useCallback((): boolean => {
    return restoringRef.current;
  }, []);

  const repinIfRestoring = useCallback(() => {
    if (!restoringRef.current || !pendingRestoreRef.current) {
      return;
    }
    const locked = cloneProgress(pendingRestoreRef.current);
    applyPageScrollProgress(
      shellRef.current,
      locked,
      "auto",
      primaryPaneRef.current,
    );
  }, [shellRef]);

  return {
    lockFromShell,
    beginModeSwitch,
    goToPage,
    getAnchor,
    isRestoring,
    repinIfRestoring,
  };
}

// 根据共享滚动壳内阅读焦点线估算当前页（1-based）。
// 与 measurePageScrollProgress / 滚动锚点使用同一 pickPageAtFocus 规则。

import { useEffect, useState } from "react";
import type { RefObject } from "react";
import {
  pageSelector,
  type ReaderPaneId,
} from "./reader-dom-contract.js";
import {
  pickPageAtFocus,
  readingFocusY,
} from "./scroll-to-page.js";

// ── 时序常数（评审 P2-7）───────────────────────────────────────────
// 页节点尚未渲染完成时的重绑间隔（attach 重试）。
const PAGE_LIST_ATTACH_RETRY_MS = 120;

export function useCurrentPage(
  scrollRef: RefObject<HTMLElement | null>,
  numPages: number,
  enabled = true,
  /** 缩放 / 模式导致节点变化时重绑 */
  observeKey: string | number = "",
  /** 只看某一栏的页；空则看全部 */
  pane?: ReaderPaneId | null,
): number {
  const [currentPage, setCurrentPage] = useState(1);

  useEffect(() => {
    if (!enabled || numPages <= 0) {
      setCurrentPage(1);
      return;
    }
    const root = scrollRef.current;
    if (!root) {
      return;
    }

    let cancelled = false;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let rafId = 0;

    const selector = pageSelector(undefined, pane);

    const measure = () => {
      if (cancelled) return;
      const pages = Array.from(root.querySelectorAll<HTMLElement>(selector));
      if (!pages.length) {
        return;
      }
      const focusY = readingFocusY(root);
      const picked = pickPageAtFocus(pages, focusY);
      if (picked) {
        setCurrentPage(picked.page);
      }
    };

    const scheduleMeasure = () => {
      if (cancelled) return;
      if (rafId) {
        cancelAnimationFrame(rafId);
      }
      rafId = requestAnimationFrame(() => {
        rafId = 0;
        measure();
      });
    };

    const attach = () => {
      if (cancelled) return;
      const pages = Array.from(root.querySelectorAll<HTMLElement>(selector));
      if (!pages.length) {
        retryTimer = setTimeout(attach, PAGE_LIST_ATTACH_RETRY_MS);
        return;
      }
      measure();
      root.addEventListener("scroll", scheduleMeasure, { passive: true });
    };

    attach();

    return () => {
      cancelled = true;
      if (retryTimer) {
        clearTimeout(retryTimer);
      }
      if (rafId) {
        cancelAnimationFrame(rafId);
      }
      root.removeEventListener("scroll", scheduleMeasure);
    };
  }, [scrollRef, numPages, enabled, observeKey, pane]);

  return currentPage;
}

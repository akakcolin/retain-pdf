import { pageNumberOfElement } from "./page-geometry.js";

export function bindPrimaryViewer(controller, onPageChange) {
  if (!controller) {
    return;
  }
  if (controller.primaryScrollHandler) {
    controller.scrollShell.removeEventListener("scroll", controller.primaryScrollHandler);
  }
  let ticking = false;
  let lastPage = 0;
  controller.primaryScrollHandler = () => {
    if (ticking) {
      return;
    }
    ticking = true;
    window.requestAnimationFrame(() => {
      ticking = false;
      const containerRect = controller.scrollShell.getBoundingClientRect();
      const focusY = containerRect.top + Math.min(containerRect.height * 0.35, 320);
      const keepCurrentTolerance = 28;
      let bestPage = 1;
      let bestDistance = Number.POSITIVE_INFINITY;
      let containingPage = 0;
      controller.viewerElement.querySelectorAll(".page[data-page-number]").forEach((pageElement) => {
        const rect = pageElement.getBoundingClientRect();
        const pageNumber = pageNumberOfElement(pageElement);
        if (!pageNumber) {
          return;
        }
        if (
          pageNumber === lastPage
          && rect.top - keepCurrentTolerance <= focusY
          && rect.bottom + keepCurrentTolerance >= focusY
        ) {
          containingPage = lastPage;
        }
        if (!containingPage && rect.top <= focusY && rect.bottom >= focusY) {
          containingPage = pageNumber;
        }
        const pageFocus = rect.top + Math.min(rect.height * 0.35, 320);
        const distance = Math.abs(pageFocus - focusY);
        if (distance < bestDistance) {
          bestDistance = distance;
          bestPage = pageNumber;
        }
      });
      const nextPage = containingPage || bestPage;
      if (nextPage && nextPage !== lastPage) {
        lastPage = nextPage;
        onPageChange?.(nextPage);
      }
    });
  };
  controller.scrollShell.addEventListener("scroll", controller.primaryScrollHandler, { passive: true });
  controller.primaryScrollHandler();
}

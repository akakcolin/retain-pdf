import {
  bindReaderRegionHover,
  bindPrimaryViewer,
  scheduleScaleRefresh,
} from "./pdf-controller.js";
import { setPageIndicator } from "./view.js";
import type {
  BindReaderInteractionsOptions,
  ReaderMetadata,
  ReaderMetadataSide,
  RegionsPayload,
  ViewerPageState,
  ViewerReady,
} from "./types.js";

function resolveTotalPagesForReady(ready: ViewerReady | null | undefined, metadata?: ReaderMetadataSide | Record<string, unknown> | null) {
  if (!ready) {
    return 0;
  }
  return Number(metadata?.page_count) || ready.pagesCount || 0;
}

function resolveActiveViewerKey(
  mode: string,
  sourceReady: ViewerReady | null | undefined,
  translatedReady: ViewerReady | null | undefined,
) {
  if (mode === "translated" && translatedReady) {
    return translatedReady.key;
  }
  if (mode === "source" && sourceReady) {
    return sourceReady.key;
  }
  return sourceReady?.key || translatedReady?.key || "";
}

export function bindReaderInteractions({
  apiPrefix,
  bindPrimary = bindPrimaryViewer,
  bindRegions = bindReaderRegionHover,
  fetchTranslationItem,
  getReaderMode = () => globalThis.document?.body?.dataset?.readerMode || "compare",
  jobId,
  pageState,
  readerMetadata,
  regionsPayload,
  scheduleScale = scheduleScaleRefresh,
  setIndicator = setPageIndicator,
  sourceReady,
  translatedReady,
}: BindReaderInteractionsOptions = {}) {
  const primary = sourceReady || translatedReady;
  if (!primary || !pageState?.reader) {
    return null;
  }

  const metadata = (readerMetadata || {}) as ReaderMetadata;
  const regions = (regionsPayload || {}) as RegionsPayload;
  const viewerState: Record<string, ViewerPageState> = {
    [sourceReady?.key || ""]: {
      currentPage: 1,
      totalPages: resolveTotalPagesForReady(sourceReady, metadata.source),
    },
    [translatedReady?.key || ""]: {
      currentPage: 1,
      totalPages: resolveTotalPagesForReady(translatedReady, metadata.translated),
    },
  };
  delete viewerState[""];

  function syncIndicatorForMode(mode = getReaderMode()) {
    const activeKey = resolveActiveViewerKey(mode, sourceReady, translatedReady);
    const activeState: ViewerPageState = viewerState[activeKey] || viewerState[primary.key] || {
      currentPage: 1,
      totalPages: 0,
    };
    pageState.reader.primaryViewerKey = activeKey || primary.key;
    pageState.reader.currentPage = activeState.currentPage || 1;
    pageState.reader.totalPages = activeState.totalPages || primary.pagesCount || 0;
    setIndicator(pageState.reader.currentPage, pageState.reader.totalPages);
  }

  [sourceReady, translatedReady].filter(Boolean).forEach((ready) => {
    bindPrimary(ready.controller, (pageNumber) => {
      if (!viewerState[ready.key]) {
        return;
      }
      viewerState[ready.key].currentPage = pageNumber || 1;
      if (resolveActiveViewerKey(getReaderMode(), sourceReady, translatedReady) === ready.key) {
        syncIndicatorForMode();
      }
    });
  });
  syncIndicatorForMode();

  bindRegions({
    regions: regions.items || [],
    sourceController: sourceReady?.controller,
    translatedController: translatedReady?.controller,
    jobId,
    apiPrefix,
    fetchTranslationItem,
  });
  scheduleScale();

  return {
    primary,
    syncIndicatorForMode,
    totalPages: pageState.reader.totalPages,
    viewerState,
  };
}

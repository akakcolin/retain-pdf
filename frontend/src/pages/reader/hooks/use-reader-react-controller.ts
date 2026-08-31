// Composes full react-pdf reader logic (session → shell → panes → tools → HUD).

import { useCallback, useEffect, useMemo, type RefObject } from "react";
import { useReaderSession } from "./use-reader-session.js";
import { useReaderKeyboard } from "./use-reader-keyboard.js";
import { useReaderShell } from "./use-reader-shell.js";
import { useReaderPaneModel } from "./use-reader-pane-model.js";
import { useReaderZoom } from "./use-reader-zoom.js";
import { useReaderModeNavigation } from "./use-reader-mode-navigation.js";
import { useReaderAnnotations } from "./use-reader-annotations.js";
import { useReaderTextSelection } from "./use-reader-text-selection.js";
import { useReaderTranslate } from "./use-reader-translate.js";
import { useReaderTools, type ReaderToolsApi } from "./use-reader-tools.js";
import { useCurrentPage } from "../pdf/useCurrentPage.js";
import { usePageRowSync } from "../pdf/usePageRowSync.js";
import { useReadingAnchor } from "../pdf/useReadingAnchor.js";
import { useUrlAnchorJump } from "./use-url-anchor-jump.js";
import type { PageRowHeights } from "../pdf/usePageRowSync.js";
import type { ReaderMode, ReaderSessionState } from "./use-reader-session.js";
import type { ProtectedPdfFile } from "../pdf/useProtectedPdfFile.js";
import type { ReaderPaneModel } from "./use-reader-pane-model.js";
import type { ReaderAnnotationsApi } from "./use-reader-annotations.js";
import type { ReaderTextSelection } from "./use-reader-text-selection.js";
import type { ReaderNote } from "../annotations/types.js";

export type ReaderReactController = {
  session: ReaderSessionState;
  boot: ReaderSessionState["boot"];
  sourceOnly: boolean;
  mode: ReaderMode;
  userZoom: number;
  onZoomChange: (zoom: number) => void;
  shell: {
    bindShell: (node: HTMLDivElement | null) => void;
    shellEl: HTMLElement | null;
    shellWidth: number;
    compareColWidth: number;
    shellRef: RefObject<HTMLDivElement | null>;
  };
  panes: ReaderPaneModel;
  sessionFiles: {
    sourceUrl: string;
    translatedUrl: string;
    sourceFile: ProtectedPdfFile | null;
    translatedFile: ProtectedPdfFile | null;
  };
  rowHeights: PageRowHeights;
  currentPage: number;
  goToPage: (page: number) => void;
  setModeKeepingPage: (next: ReaderMode) => void;
  showHud: boolean;
  tools: ReaderToolsApi;
  notes: ReaderAnnotationsApi;
  selection: ReaderTextSelection | null;
  clearSelection: () => void;
  addNoteFromSelection: (selection: ReaderTextSelection) => void;
  translate: ReturnType<typeof useReaderTranslate>;
  translateSelection: (selection: ReaderTextSelection) => void;
  jumpToNote: (note: ReaderNote) => void;
  documentTitle: string;
  download: ReaderSessionState["download"];
};


export function useReaderReactController(): ReaderReactController {
  const session = useReaderSession();
  const tools = useReaderTools();
  const { shellRef, shellEl, shellWidth, compareColWidth, bindShell } = useReaderShell();
  const { userZoom, onZoomChange } = useReaderZoom(session.mode, shellRef);

  const panes = useReaderPaneModel(
    {
      mode: session.mode,
      sourceOnly: session.sourceOnly,
      assetsReady: session.assetsReady,
      sourceUrl: session.sourceUrl,
      translatedUrl: session.translatedUrl,
      sourceFile: session.sourceFile,
      translatedFile: session.translatedFile,
    },
    { userZoom, shellWidth },
  );

  const {
    beginModeSwitch,
    goToPage: goToPageWithTotal,
    repinIfRestoring,
  } = useReadingAnchor(shellRef, {
    primaryPane: panes.primaryPane,
    mode: session.mode,
    enabled: !session.boot.loading,
  });

  useEffect(() => {
    repinIfRestoring();
  }, [shellWidth, repinIfRestoring]);

  const rowHeights = usePageRowSync(
    shellRef,
    panes.compareMode,
    panes.rowSyncRevision,
    repinIfRestoring,
  );

  const currentPage = useCurrentPage(
    shellRef,
    panes.primaryNumPages,
    !session.boot.loading,
    `${session.mode}-${userZoom}-${panes.metricsTick}`,
    panes.primaryPane,
  );

  const goToPage = useCallback((page: number) => {
    // 取已加载栏的最大页数；未知时传 0，由 clampPageNumber 放行目标页
    const total = Math.max(
      Number(panes.hudNumPages) || 0,
      Number(panes.primaryNumPages) || 0,
      Number(panes.numPagesByPane?.source) || 0,
      Number(panes.numPagesByPane?.translated) || 0,
    );
    goToPageWithTotal(page, total);
  }, [goToPageWithTotal, panes.hudNumPages, panes.primaryNumPages, panes.numPagesByPane]);

  // 收藏 / 搜索回跳：URL ?page_idx= → 页码（0 基 → 1 基）
  useUrlAnchorJump({
    enabled: !session.boot.loading && !session.boot.failed && session.assetsReady,
    numPages: panes.hudNumPages || 0,
    goToPage,
  });

  const { setModeKeepingPage } = useReaderModeNavigation({
    mode: session.mode,
    setMode: session.setMode,
    beginModeSwitch,
  });

  const openNotes = useCallback(() => {
    tools.open("notes");
  }, [tools]);

  const notes = useReaderAnnotations(
    {
      jobId: session.jobId,
      documentId: session.documentId,
    },
    { onAfterAdd: openNotes },
  );

  const { selection, clearSelection } = useReaderTextSelection(
    shellRef,
    !session.boot.loading && !session.boot.failed,
  );

  const translate = useReaderTranslate();

  const addNoteFromSelection = useCallback((sel: ReaderTextSelection) => {
    notes.addFromQuote({
      page: sel.page,
      pane: sel.pane,
      quote: sel.quote,
    });
    clearSelection();
  }, [notes, clearSelection]);

  const translateSelection = useCallback((sel: ReaderTextSelection) => {
    // 翻译后收起选文浮条，译文在独立浮窗展示
    translate.translateSelection(sel);
    clearSelection();
  }, [translate, clearSelection]);

  const jumpToNote = useCallback((note: ReaderNote) => {
    // 若批注在译文/原文栏，尽量切到对应单栏或对照
    if (note.pane === "translated" && session.mode === "source") {
      beginModeSwitch();
      session.setMode("compare");
    } else if (note.pane === "source" && session.mode === "translated") {
      beginModeSwitch();
      session.setMode("compare");
    }
    goToPage(note.page);
  }, [session, beginModeSwitch, goToPage]);

  const showHud = !session.boot.loading && !session.boot.failed;

  useReaderKeyboard({
    mode: session.mode,
    sourceOnly: session.sourceOnly,
    setMode: setModeKeepingPage,
    userZoom,
    onZoomChange,
    currentPage,
    numPages: panes.hudNumPages,
    goToPage,
    enabled: showHud,
  });

  // tools 对象引用稳定到 active 变化时
  const toolsApi = useMemo(() => tools, [tools.active, tools.open, tools.close, tools.toggle, tools.isOpen]);

  return {
    session,
    boot: session.boot,
    sourceOnly: session.sourceOnly,
    mode: session.mode,
    userZoom,
    onZoomChange,
    shell: { bindShell, shellEl, shellWidth, compareColWidth, shellRef },
    panes,
    sessionFiles: {
      sourceUrl: session.sourceUrl,
      translatedUrl: session.translatedUrl,
      sourceFile: session.sourceFile,
      translatedFile: session.translatedFile,
    },
    rowHeights,
    currentPage,
    goToPage,
    setModeKeepingPage,
    download: session.download,
    showHud,
    tools: toolsApi,
    notes,
    selection,
    clearSelection,
    addNoteFromSelection,
    translate,
    translateSelection,
    jumpToNote,
    documentTitle: session.title || "",
  };
}

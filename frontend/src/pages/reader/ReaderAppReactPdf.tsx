// React-pdf 阅读器视图：逻辑在 useReaderReactController；工具为悬浮窗（对齐 legacy 四件套）。

import { useCallback } from "react";
import { useReaderReactController } from "./hooks/use-reader-react-controller.js";
import {
  ReaderCloseHome,
  ReaderModeTabs,
  ReaderReactBoot,
  ReaderCompareGrid,
  ReaderZoomHud,
  ReaderFab,
  ReaderNotesPanel,
  ReaderFavoritesPanel,
  ReaderEntitiesPanel,
  ReaderMarkdownPanel,
  ReaderAiPanel,
  ReaderSelectionToolbar,
  ReaderTranslatePopup,
} from "./components/react-pdf/index.js";
import { DownloadToastHost } from "../../shared/react/DownloadToastHost.jsx";
import { isReaderAiNavigationLocked, type AiCitationLike } from "./external.js";
import { resolveCitationTarget } from "./citation-target.js";

export function ReaderAppReactPdf() {
  const c = useReaderReactController();
  const { boot, panes, shell, sessionFiles, notes, tools, session } = c;

  const closeTool = useCallback(() => {
    tools.close();
  }, [tools]);

  // citation.page_idx 0 基；兼容 page / block_id(p00N)；阅读器页码 1 基。
  // 跨文档引用（属于另一篇文献且带 job_id）整页跳到目标文献的对应页。
  const jumpCitation = useCallback((citation: AiCitationLike | number) => {
    // 分支/切会话锁定期：绝不跳 PDF（体感像整页刷新+跳转）
    if (isReaderAiNavigationLocked()) return;
    if (typeof citation === "number") {
      // 约定：直接传数字时为 0 基 idx
      if (Number.isFinite(citation) && citation >= 0) {
        c.goToPage(Math.floor(citation) + 1);
      }
      return;
    }
    const target = resolveCitationTarget(citation, {
      currentJobId: session.jobId,
      currentDocumentId: session.documentId,
    });
    if (!target) return;
    if (target.kind === "document") {
      window.location.assign(target.url);
      return;
    }
    c.goToPage(target.page1);
  }, [c.goToPage, session.documentId, session.jobId]);

  return (
    <div className="reader-react-root" data-reader-engine="react-pdf">
      <ReaderReactBoot
        loading={boot.loading}
        failed={boot.failed}
        text={boot.text}
        percent={boot.percent}
      />

      {/* 整页阅读器：右上角关闭 → 回主页（替代旧 iframe 宿主关闭钮） */}
      <ReaderCloseHome />

      <ReaderModeTabs
        mode={c.mode}
        sourceOnly={c.sourceOnly}
        onModeChange={c.setModeKeepingPage}
      />

      {c.showHud ? (
        <ReaderFab
          activeTool={tools.active}
          notesCount={notes.count}
          sourceOnly={c.sourceOnly}
          onToggleTool={tools.toggle}
          download={c.download}
        />
      ) : null}

      <ReaderCompareGrid
        mode={c.mode}
        bindShell={shell.bindShell}
        shellEl={shell.shellEl}
        userZoom={c.userZoom}
        compareMode={panes.compareMode}
        shellWidth={shell.shellWidth}
        compareColWidth={shell.compareColWidth}
        rowHeights={c.rowHeights}
        mountSource={panes.mountSource}
        mountTranslated={panes.mountTranslated}
        showSource={panes.showSource}
        showTranslated={panes.showTranslated}
        sourceOnly={c.sourceOnly}
        sourceUrl={sessionFiles.sourceUrl}
        translatedUrl={sessionFiles.translatedUrl}
        sourceFile={sessionFiles.sourceFile}
        translatedFile={sessionFiles.translatedFile}
        onMetrics={panes.onMetrics}
        onNumPagesChange={panes.onNumPages}
      />

      {c.showHud ? (
        <ReaderZoomHud
          userZoom={c.userZoom}
          onZoomChange={c.onZoomChange}
          currentPage={c.currentPage}
          numPages={panes.hudNumPages}
          mode={c.mode}
          onGoToPage={c.goToPage}
        />
      ) : null}

      <ReaderNotesPanel
        open={tools.isOpen("notes")}
        groups={notes.groups}
        count={notes.count}
        onClose={closeTool}
        onJump={c.jumpToNote}
        onUpdateNote={notes.updateNote}
        onRemove={notes.remove}
        onExport={() => notes.exportMarkdown(c.documentTitle)}
      />

      <ReaderFavoritesPanel
        open={tools.isOpen("favorites")}
        jobId={session.jobId}
        documentId={session.documentId}
        onClose={closeTool}
        onJumpPage={c.goToPage}
      />

      <ReaderEntitiesPanel
        open={tools.isOpen("entities")}
        jobId={session.jobId}
        documentId={session.documentId}
        onClose={closeTool}
        onJumpPage={c.goToPage}
      />

      <ReaderMarkdownPanel
        open={tools.isOpen("markdown")}
        jobId={session.jobId}
        sourceOnly={c.sourceOnly}
        onClose={closeTool}
      />

      <ReaderAiPanel
        open={tools.isOpen("ai")}
        jobId={session.jobId}
        sourceOnly={c.sourceOnly}
        onClose={closeTool}
        onJumpCitation={jumpCitation}
      />

      <ReaderSelectionToolbar
        selection={c.selection}
        onAddNote={c.addNoteFromSelection}
        onTranslate={c.translateSelection}
        onDismiss={c.clearSelection}
      />

      <ReaderTranslatePopup
        open={c.translate.open}
        quote={c.translate.quote}
        targetLanguage={c.translate.targetLanguage}
        result={c.translate.result}
        loading={c.translate.loading}
        error={c.translate.error}
        onClose={c.translate.close}
        onRetry={c.translate.retry}
      />

      <DownloadToastHost />
    </div>
  );
}

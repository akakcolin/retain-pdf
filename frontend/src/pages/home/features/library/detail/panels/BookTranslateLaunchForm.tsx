// 详情「翻译」Tab：发起 / 重新翻译表单。
// 从原 TranslateWorkspacePanel 抽出；书已在馆，无需 WorkflowPanel 上传瓦片。

import { btn, IconLanguages } from "./ui.jsx";

export type BookTranslateLaunchFormProps = {
  canTranslate: boolean;
  readerAvailable?: boolean;
  isActive?: boolean;
  statusTone?: string;
  rangeOn: boolean;
  startPage: string | number;
  endPage: string | number;
  pageCount?: number;
  busy?: string;
  error?: string;
  onRangeOnChange: (value: boolean) => void;
  onStartPageChange: (value: string) => void;
  onEndPageChange: (value: string) => void;
  onTranslate: () => void;
};

export function BookTranslateLaunchForm({
  canTranslate,
  readerAvailable = false,
  isActive = false,
  statusTone = "",
  rangeOn,
  startPage,
  endPage,
  pageCount,
  busy = "",
  error = "",
  onRangeOnChange,
  onStartPageChange,
  onEndPageChange,
  onTranslate,
}: BookTranslateLaunchFormProps) {
  // failed / done(已完成) 都是「再次发起」，标题与按钮用「重新翻译」
  const retranslate = statusTone === "failed" || statusTone === "done";
  return (
    <div className="book-translate-launch-form space-y-2.5">
      {error ? (
        <p
          id="book-detail-translate-error"
          className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive"
          role="alert"
        >
          {error}
        </p>
      ) : null}

      {canTranslate ? (
        <div className="space-y-2.5 rounded-lg border border-border/60 bg-muted/15 px-3.5 py-3">
          <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            {retranslate ? "重新翻译" : "发起翻译"}
          </p>
          <label className="flex cursor-pointer select-none items-center gap-2 text-sm text-muted-foreground">
            <input
              type="checkbox"
              className="h-4 w-4 rounded border-muted-foreground/40"
              checked={rangeOn}
              onChange={(e) => onRangeOnChange(e.target.checked)}
            />
            指定页码范围（不勾选 = 整本翻译）
          </label>
          {rangeOn ? (
            <div className="flex items-center gap-2">
              <input
                type="number"
                min="1"
                value={startPage}
                aria-label="起始页"
                onChange={(e) => onStartPageChange(e.target.value)}
                className="h-8 w-20 rounded-md border border-input bg-background px-2 text-sm"
              />
              <span className="text-xs text-muted-foreground">–</span>
              <input
                type="number"
                min="1"
                value={endPage}
                aria-label="结束页"
                onChange={(e) => onEndPageChange(e.target.value)}
                className="h-8 w-20 rounded-md border border-input bg-background px-2 text-sm"
              />
              <span className="text-[11px] text-muted-foreground/70">
                共 {pageCount || "?"} 页
              </span>
            </div>
          ) : null}
          <button
            id="book-detail-translate-btn"
            type="button"
            className={btn("default")}
            disabled={Boolean(busy)}
            onClick={onTranslate}
          >
            <IconLanguages className="mr-1" />
            {busy === "translate"
              ? "提交中…"
              : rangeOn
                ? "翻译选定页码"
                : retranslate
                  ? "重新翻译整本"
                  : "翻译整本"}
          </button>
        </div>
      ) : readerAvailable ? (
        <p className="text-xs text-muted-foreground">
          已翻译完成。上方为本书任务进度；左侧可「对照阅读」。
        </p>
      ) : isActive ? (
        <p className="text-xs text-muted-foreground">
          翻译进行中，进度在本 Tab 内自动刷新。完成后可在左侧对照阅读。
        </p>
      ) : null}
    </div>
  );
}

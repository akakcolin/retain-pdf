// 选中文字翻译结果浮窗：原文摘录 + 译文 + 复制/重新翻译。

import { useCallback } from "react";
import { Copy, Languages, RefreshCw } from "lucide-react";
import { ReaderFloatShell } from "./ReaderFloatShell.js";

export type ReaderTranslatePopupProps = {
  open: boolean;
  quote: string;
  targetLanguage: string;
  result: string;
  loading: boolean;
  error: string;
  onClose: () => void;
  onRetry: () => void;
};

export function ReaderTranslatePopup({
  open,
  quote,
  targetLanguage,
  result,
  loading,
  error,
  onClose,
  onRetry,
}: ReaderTranslatePopupProps) {
  const copy = useCallback(async () => {
    try {
      await navigator.clipboard?.writeText?.(result);
    } catch {
      /* 剪贴板被拒/无权限时静默 */
    }
  }, [result]);

  return (
    <ReaderFloatShell
      id="reader-translate-pop"
      open={open}
      title="译文"
      subtitle={`译成 ${targetLanguage}`}
      titleIcon={<Languages size={14} strokeWidth={2.1} aria-hidden />}
      storageKey="retainpdf.reader.translate-float.pos.v1"
      ariaLabel="选中文字翻译"
      width={360}
      className="reader-float-translate"
      onClose={onClose}
    >
      <div className="reader-translate-body">
        {quote ? (
          <blockquote className="reader-translate-quote" title={quote}>
            “{quote}”
          </blockquote>
        ) : null}

        {loading ? (
          <p className="reader-translate-status" role="status">
            正在翻译…
          </p>
        ) : null}

        {error ? (
          <div className="reader-translate-error" role="alert">
            <p>{error}</p>
            <button type="button" onClick={onRetry}>
              重试
            </button>
          </div>
        ) : null}

        {!loading && !error && result ? (
          <p className="reader-translate-result">{result}</p>
        ) : null}

        <div className="reader-translate-actions">
          <button type="button" disabled={loading || !result} onClick={copy}>
            <Copy size={14} aria-hidden />
            <span>复制译文</span>
          </button>
          <button type="button" disabled={loading} onClick={onRetry}>
            <RefreshCw size={14} aria-hidden />
            <span>重新翻译</span>
          </button>
        </div>
      </div>
    </ReaderFloatShell>
  );
}

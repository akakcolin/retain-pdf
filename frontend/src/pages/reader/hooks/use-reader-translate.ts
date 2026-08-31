// 阅读器「选中文字翻译」：浮窗状态 + 发起翻译。目标语言跟随选中栏自动选
// （原文栏→简体中文，译文栏→English），不暴露手动切换。

import { useCallback, useRef, useState } from "react";
import {
  hasModelApiKey,
  MISSING_MODEL_API_KEY_MESSAGE,
  resolveReaderAiConfig,
  translateText,
} from "../external.js";
import type { ReaderTextSelection } from "./use-reader-text-selection.js";

const QUOTE_MAX_LENGTH = 240;

const TARGET_BY_PANE: Record<string, string> = {
  source: "简体中文",
  translated: "English",
};

function clipQuoteText(text = "", maxLength = QUOTE_MAX_LENGTH) {
  const normalized = `${text}`.replace(/\s+/g, " ").trim();
  if (normalized.length <= maxLength) {
    return normalized;
  }
  return `${normalized.slice(0, maxLength).trim()}…`;
}

/**
 * @param {object} [options]
 * @param {typeof translateText} [options.translate] 测试可注入
 * @param {typeof hasModelApiKey} [options.hasKey] 测试可注入
 * @param {typeof resolveReaderAiConfig} [options.resolveConfig] 测试可注入
 */
export function useReaderTranslate({
  translate = translateText,
  hasKey = hasModelApiKey,
  resolveConfig = resolveReaderAiConfig,
}: any = {}) {
  const [open, setOpen] = useState(false);
  const [quote, setQuote] = useState("");
  const [targetLanguage, setTargetLanguage] = useState("简体中文");
  const [result, setResult] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  // 请求序号：换选区/关闭/重试后，旧 in-flight 结果按 seq 丢弃，防串流
  const seqRef = useRef(0);
  const lastSelectionRef = useRef<ReaderTextSelection | null>(null);

  const translateSelection = useCallback(async (selection: ReaderTextSelection) => {
    const seq = ++seqRef.current;
    lastSelectionRef.current = selection;
    setQuote(clipQuoteText(selection.quote));
    setResult("");
    setError("");
    setOpen(true);
    const language = TARGET_BY_PANE[selection.pane] || "简体中文";
    setTargetLanguage(language);
    if (!hasKey()) {
      setLoading(false);
      setError(MISSING_MODEL_API_KEY_MESSAGE);
      return;
    }
    setLoading(true);
    try {
      const config = resolveConfig();
      const res = await translate({
        text: selection.quote,
        targetLanguage: language,
        provider: config?.provider,
        model: config?.model,
        apiKey: config?.apiKey,
        baseUrl: config?.baseUrl,
      });
      if (seq !== seqRef.current) {
        return;
      }
      setResult(`${(res as { translated_text?: string })?.translated_text || ""}`);
      setLoading(false);
    } catch (err) {
      if (seq !== seqRef.current) {
        return;
      }
      setError(err instanceof Error ? err.message : "翻译失败，请重试。");
      setLoading(false);
    }
  }, [translate, hasKey, resolveConfig]);

  const close = useCallback(() => {
    seqRef.current += 1;
    setError("");
    setOpen(false);
  }, []);

  const retry = useCallback(() => {
    const last = lastSelectionRef.current;
    if (last) {
      void translateSelection(last);
    }
  }, [translateSelection]);

  return {
    open,
    quote,
    targetLanguage,
    result,
    loading,
    error,
    translateSelection,
    close,
    retry,
  };
}

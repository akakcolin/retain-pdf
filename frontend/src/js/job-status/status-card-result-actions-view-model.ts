import {
  buildReaderPageUrl,
  isReaderActionEnabled,
} from "../job/action-model.js";
import {
  isJobTerminal,
} from "../job/core.js";
import {
  resolveJobActions,
  resolveJobMarkdownBundleAction,
  resolveJobSourcePdfAction,
  resolveJobTranslatedMarkdownBundleAction,
} from "../job/actions.js";

export function buildStatusCardResultActions({
  job = null,
  manifest = null,
}: any = {}) {
  const actions = resolveJobActions(job);
  const succeeded = isJobTerminal(job) && job?.status === "succeeded";
  const readerEnabled = isReaderActionEnabled(job, manifest);
  const sourcePdfAction = resolveJobSourcePdfAction(job, manifest);
  const markdownBundleAction = resolveJobMarkdownBundleAction(job, manifest);
  const translatedMarkdownBundleAction = resolveJobTranslatedMarkdownBundleAction(job, manifest);
  // OCR 完成（md/full.md 就绪）即可下载原文 markdown zip，无需等任务终态；
  // 渲染完成后译文 markdown zip 就绪，按钮自动切到译文（URL + 标签跟随）。
  const markdownBundleTranslated = Boolean(
    translatedMarkdownBundleAction.ready && translatedMarkdownBundleAction.url,
  );
  const markdownBundleUrl = markdownBundleTranslated
    ? translatedMarkdownBundleAction.url
    : markdownBundleAction.url;
  const markdownBundleReady = Boolean(markdownBundleUrl);
  return {
    pdfReady: actions.pdfEnabled && Boolean(actions.pdf) && succeeded,
    pdfUrl: actions.pdf,
    markdownBundleReady,
    markdownBundleUrl,
    markdownBundleTranslated,
    readerReady: readerEnabled && succeeded,
    readerUrl: buildReaderPageUrl(job?.job_id),
    sourcePdfReady: sourcePdfAction.ready && Boolean(sourcePdfAction.url) && succeeded,
    sourcePdfUrl: sourcePdfAction.url,
  };
}

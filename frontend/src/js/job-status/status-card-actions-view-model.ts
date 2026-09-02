export function buildStatusCardPrimaryActions({
  selectedStageKey = "",
  snapshot = null,
}: any = {}) {
  const selected = `${selectedStageKey || ""}`.trim();
  const showResultActions = selected === "done";
  return {
    pdfReady: showResultActions && Boolean(snapshot?.pdfReady),
    pdfUrl: snapshot?.pdfUrl || "",
    markdownBundleReady: showResultActions && Boolean(snapshot?.markdownBundleReady),
    markdownBundleUrl: snapshot?.markdownBundleUrl || "",
    markdownBundleTranslated: showResultActions && Boolean(snapshot?.markdownBundleTranslated),
    readerReady: showResultActions && Boolean(snapshot?.readerReady),
    readerUrl: snapshot?.readerUrl || "",
    sourcePdfReady: showResultActions && Boolean(snapshot?.sourcePdfReady),
    sourcePdfUrl: snapshot?.sourcePdfUrl || "",
  };
}

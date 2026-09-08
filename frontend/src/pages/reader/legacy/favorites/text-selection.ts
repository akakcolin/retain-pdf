export function clearTextSelection(documentRef = globalThis.document) {
  try {
    documentRef?.getSelection?.()?.removeAllRanges?.();
    globalThis.window?.getSelection?.()?.removeAllRanges?.();
  } catch {
    // Ignore selection cleanup failures; this is only interaction polish.
  }
}

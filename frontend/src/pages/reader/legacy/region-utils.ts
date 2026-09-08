export function normalizeReaderRegions(regions) {
  return (Array.isArray(regions) ? regions : [])
    .map((region) => {
      const sourcePage = Number(region?.source?.page || 0);
      const translatedPage = Number(region?.translated?.page || 0);
      const sourceBox = Array.isArray(region?.source?.bbox) ? region.source.bbox.map(Number) : [];
      const translatedBox = Array.isArray(region?.translated?.bbox) ? region.translated.bbox.map(Number) : [];
      if (
        !sourcePage
        || !translatedPage
        || sourceBox.length !== 4
        || translatedBox.length !== 4
        || !sourceBox.every(Number.isFinite)
        || !translatedBox.every(Number.isFinite)
      ) {
        return null;
      }
      return {
        itemId: `${region?.item_id || ""}`,
        source: {
          page: sourcePage,
          bbox: sourceBox,
          text: `${region?.source?.text || region?.source_text || ""}`,
        },
        translated: {
          page: translatedPage,
          bbox: translatedBox,
          text: `${region?.translated?.text || region?.translated_text || ""}`,
        },
        markdown: `${region?.markdown || region?.markdown_text || ""}`,
        regionType: `${region?.region_type || ""}`,
        status: `${region?.status || ""}`,
      };
    })
    .filter(Boolean);
}

export function escapeHtml(value) {
  return `${value ?? ""}`
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

export function firstText(...values) {
  for (const value of values) {
    const text = `${value ?? ""}`.trim();
    if (text) {
      return text;
    }
  }
  return "";
}

export async function copyTextToClipboard(text) {
  if (navigator.clipboard?.writeText && window.isSecureContext) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  textarea.style.top = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const copied = document.execCommand("copy");
  textarea.remove();
  if (!copied) {
    throw new Error("copy command failed");
  }
}

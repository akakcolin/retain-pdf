import { showDeleteConfirmation } from "./overlays.js";

const SERVER_FAVORITE_KIND_LABELS = {
  block: "段落",
  page: "页面",
  sentence: "句子",
};

function formatServerFavoriteDate(createdAt = "") {
  const timestamp = Date.parse(createdAt);
  if (!Number.isFinite(timestamp)) {
    return "";
  }
  const date = new Date(timestamp);
  const month = `${date.getMonth() + 1}`.padStart(2, "0");
  const day = `${date.getDate()}`.padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

// 云端收藏区渲染:与本地截图摘录分区展示。
// 服务端文本一律走 textContent(禁止 innerHTML),防止引文内容注入。
export function renderServerFavorites(sectionEl, records = [], {
  onOpenFavorite = null,
  onRemoveFavorite = null,
} = {}) {
  if (!sectionEl) {
    return;
  }
  const documentRef = sectionEl.ownerDocument || globalThis.document;
  sectionEl.replaceChildren();
  sectionEl.classList.toggle("is-empty", !records.length);
  if (!records.length) {
    return;
  }

  const head = documentRef.createElement("div");
  head.className = "reader-favorite-server-head";
  head.textContent = "云端收藏";
  sectionEl.appendChild(head);

  records.forEach((record) => {
    const card = documentRef.createElement("div");
    card.className = "reader-favorite-server-card";
    card.dataset.readerServerFavoriteId = record.favoriteId || "";
    card.setAttribute("role", "button");
    card.tabIndex = 0;
    const openFavorite = (event) => {
      event.preventDefault?.();
      onOpenFavorite?.(record);
    };
    card.addEventListener("click", openFavorite);
    card.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") {
        return;
      }
      openFavorite(event);
    });

    const topRow = documentRef.createElement("span");
    topRow.className = "reader-favorite-server-top";

    const kindBadge = documentRef.createElement("span");
    kindBadge.className = "reader-favorite-server-kind";
    kindBadge.textContent = SERVER_FAVORITE_KIND_LABELS[record.kind] || record.kind || "句子";

    const meta = documentRef.createElement("span");
    meta.className = "reader-favorite-server-meta";
    const dateText = formatServerFavoriteDate(record.createdAt);
    const pageText = Number.isFinite(Number(record.pageIdx)) ? `第 ${Number(record.pageIdx) + 1} 页` : "";
    meta.textContent = [pageText, dateText].filter(Boolean).join(" · ");

    const removeButton = documentRef.createElement("span");
    removeButton.className = "reader-favorite-server-remove";
    removeButton.setAttribute("role", "button");
    removeButton.tabIndex = 0;
    removeButton.setAttribute("aria-label", "删除云端收藏");
    removeButton.textContent = "×";
    const confirmRemove = (event) => {
      event.preventDefault?.();
      event.stopPropagation?.();
      showDeleteConfirmation(removeButton, () => onRemoveFavorite?.(record), documentRef);
    };
    removeButton.addEventListener("click", confirmRemove);
    removeButton.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") {
        return;
      }
      confirmRemove(event);
    });

    topRow.append(kindBadge, meta, removeButton);
    card.appendChild(topRow);

    const quote = documentRef.createElement("div");
    quote.className = "reader-favorite-server-quote";
    quote.textContent = record.quoteText || "";
    card.appendChild(quote);

    if (record.translatedQuoteText) {
      const translated = documentRef.createElement("div");
      translated.className = "reader-favorite-server-quote-translated";
      translated.textContent = record.translatedQuoteText;
      card.appendChild(translated);
    }

    sectionEl.appendChild(card);
  });
}

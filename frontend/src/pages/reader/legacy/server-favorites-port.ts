import { API_PREFIX } from "../../../js/config/api-constants.js";
import { fetchDocumentByJobId } from "../../../js/api/documents.js";
import { createFavorite, deleteFavorite, fetchFavorites } from "../../../js/api/favorites.js";
import type {
  CreateServerFavoritesPortOptions,
  FavoriteItem,
  SelectionQuote,
  ServerFavorite,
  ServerFavoriteRaw,
} from "./types.js";

// 服务端收藏 → 阅读器视图记录:snake_case 转 camelCase,
// page_idx 与 jumpToReaderAnchor 的 pageIdx 同为 0 基。
// 缺 favorite_id 或 quote_text 的脏数据直接丢弃(返回 null)。
export function normalizeServerFavorite(raw: ServerFavoriteRaw = {}): ServerFavorite | null {
  const favoriteId = `${raw?.favorite_id || ""}`.trim();
  const quoteText = `${raw?.quote_text || ""}`.trim();
  if (!favoriteId || !quoteText) {
    return null;
  }
  const pageIdx = Number(raw.page_idx);
  return {
    favoriteId,
    documentId: `${raw.document_id || ""}`.trim(),
    jobId: `${raw.job_id || ""}`.trim(),
    pageIdx: Number.isFinite(pageIdx) && pageIdx >= 0 ? pageIdx : 0,
    blockId: `${raw.block_id || ""}`.trim(),
    kind: `${raw.kind || ""}`.trim() || "sentence",
    quoteText,
    translatedQuoteText: `${raw.translated_quote_text || ""}`.trim(),
    note: `${raw.note || ""}`.trim(),
    createdAt: `${raw.created_at || ""}`.trim(),
  };
}

// 本地记录同步成功后带 serverFavoriteId;云端区不重复展示这些收藏。
export function dedupeServerFavorites(
  serverFavorites: ServerFavorite[] = [],
  localItems: FavoriteItem[] = [],
): ServerFavorite[] {
  const syncedIds = new Set(
    (Array.isArray(localItems) ? localItems : [])
      .map((item) => `${item?.serverFavoriteId || ""}`.trim())
      .filter(Boolean),
  );
  return (Array.isArray(serverFavorites) ? serverFavorites : [])
    .filter((favorite) => favorite?.favoriteId && !syncedIds.has(favorite.favoriteId));
}

// 把阅读器收藏同步到后端 favorites。
// document_id 经后端 GET /documents?job_id= 直查(含历史 run),前端不再扫列表反查。
// 所有服务端调用尽力而为:失败仅记录日志,阅读器本地功能不受影响。
export function createReaderServerFavoritesPort({
  jobId = "",
  apiPrefix = API_PREFIX,
  documentByJobId = fetchDocumentByJobId,
  submitFavorite = createFavorite,
  loadFavorites = fetchFavorites,
  removeFavorite = deleteFavorite,
}: CreateServerFavoritesPortOptions = {}) {
  let documentIdPromise: Promise<string> | null = null;

  function resolveDocumentId() {
    if (!documentIdPromise) {
      documentIdPromise = (async () => {
        try {
          const document = await documentByJobId(apiPrefix, jobId);
          return `${document?.document_id || ""}`.trim();
        } catch (_err) {
          return "";
        }
      })();
    }
    return documentIdPromise;
  }

  async function syncFavorite(quote: SelectionQuote = {}) {
    const blockId = `${quote.blockId || ""}`.trim();
    const quoteText = `${quote.quoteText || ""}`.trim();
    if (!blockId || !quoteText) {
      return null;
    }
    try {
      // 写路径只给 job_id,后端解析所属文档(历史 run 也能收藏)
      const favorite = await submitFavorite(apiPrefix, {
        job_id: jobId,
        page_idx: Number(quote.pageIdx) || 0,
        block_id: blockId,
        quote_text: quoteText,
        translated_quote_text: `${quote.translatedQuoteText || ""}`,
        kind: "sentence",
      });
      console.info("收藏已同步到服务端", favorite?.favorite_id || "");
      return favorite;
    } catch (error) {
      console.error("同步收藏到服务端失败", error);
      return null;
    }
  }

  // 拉取当前文档的服务端收藏并归一化;离线/解析不到文档时静默返回空。
  // mock 模式不短路:api 层自带 mock 分支,基线与 e2e 依赖 mock 全流程可用。
  async function loadServerFavorites(): Promise<ServerFavorite[]> {
    const documentId = await resolveDocumentId();
    if (!documentId) {
      return [];
    }
    try {
      const { favorites = [] } = await loadFavorites(apiPrefix, { documentId });
      return (Array.isArray(favorites) ? favorites : [])
        .map(normalizeServerFavorite)
        .filter(Boolean);
    } catch (error) {
      console.warn("读取服务端收藏失败", error);
      return [];
    }
  }

  // 删除服务端收藏,成功返回 true;失败仅记录日志返回 false(不阻塞本地流程)。
  async function removeServerFavorite(favoriteId: string) {
    const normalized = `${favoriteId || ""}`.trim();
    if (!normalized) {
      return false;
    }
    try {
      await removeFavorite(apiPrefix, normalized);
      return true;
    } catch (error) {
      console.error("删除服务端收藏失败", error);
      return false;
    }
  }

  // 规范没有收藏 PATCH:改笔记 = 同锚点重建 + 删旧。先建后删,失败不丢数据。
  // 写路径只给 job_id,后端解析所属文档。
  async function recreateFavoriteNote(annotation: Partial<ServerFavorite> = {}, note = "") {
    if (!annotation?.favoriteId) {
      return null;
    }
    try {
      const created = await submitFavorite(apiPrefix, {
        job_id: `${annotation.jobId || jobId || ""}`.trim() || undefined,
        page_idx: Number(annotation.pageIdx) || 0,
        block_id: `${annotation.blockId || ""}`.trim(),
        quote_text: `${annotation.quoteText || ""}`,
        translated_quote_text: `${annotation.translatedQuoteText || ""}`,
        kind: `${annotation.kind || "sentence"}`,
        note: `${note || ""}`,
      });
      await removeServerFavorite(annotation.favoriteId);
      return normalizeServerFavorite(created);
    } catch (error) {
      console.error("更新批注笔记失败", error);
      return null;
    }
  }

  return Object.freeze({
    loadServerFavorites,
    recreateFavoriteNote,
    removeServerFavorite,
    resolveDocumentId,
    syncFavorite,
  });
}

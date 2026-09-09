// 阅读器「新引擎 / 共享层」对 src/js/* 的出口。
//
// 仅供 pages/reader 非 legacy 路径使用：
//   hooks/、pdf/、annotations/、components/react-pdf/、ReaderAppReactPdf
// 缺符号只改本文件。
//
// legacy/** 与 ?engine=legacy 继续直接 import js/reader 命令式引擎
// （pdf-controller / selection-favorites / regions…）——不要把它们塞进这里。

// —— config / mock / messaging ——
export { isMockMode } from "../../js/config/runtime.js";
export { sourceLanguageName } from "../../js/config/languages.js";
export { MOCK_DOCUMENT_SOURCE_PDF_URL } from "../../js/mock/documents.js";
export { READER_DIALOG_MESSAGES } from "../../js/features/reader-dialog/contract.js";

// —— job / http / vendor ——
export { resolveResourceUrl } from "../../js/job/artifacts.js";
export { fetchProtected } from "../../js/api/http.js";
export {
  resolvePdfjsVendorUrl,
  resolveMarkedVendorUrl,
} from "../../js/runtime/vendor-url.js";

// —— js/reader 共享 ports（新引擎允许依赖的子集）——
export { defaultReaderDataPort } from "./legacy/data-port.js";
export {
  defaultReaderPageConfigPort,
  resolveReaderAnchor,
  resolveReaderDocumentId,
  resolveReaderJobId,
} from "./legacy/config-port.js";
export { resolveReaderArtifactUrl } from "./legacy/pdf-document.js";
export {
  resolveReaderSourcePdf,
  resolveReaderTranslatedPdfUrl,
} from "./legacy/resource-resolver.js";
export { READER_PROGRESS_COPY } from "./legacy/page-state.js";

// —— 下载（与 legacy 共用解析 / 受保护下载）——
export {
  READER_DOWNLOAD_ACTIONS,
  disabledReason as readerDownloadDisabledReason,
  resolveReaderDownloadName,
  resolveReaderDownloadUrls,
  trimString as trimReaderDownloadString,
} from "../../js/reader/downloads/resolve.js";
export { downloadProtectedResource } from "../../js/features/reader-dialog/downloads.js";
export { failDownloadToast } from "../../js/utils/download-feedback.js";

// —— markdown 面板 ——
export { resolveMarkdownAssetUrl } from "../../js/job/artifacts.js";
export { parseMarkdownWithMath } from "./legacy/markdown-math.js";

// —— AI 追问（react-pdf assistant）——
export {
  CREDENTIALS_CHANGED_EVENT,
  hasChatModelApiKey,
  hasModelApiKey,
  MISSING_MODEL_API_KEY_MESSAGE,
  resolveReaderAiConfig,
  resolveReaderChatConfig,
} from "../../js/reader/ai/config.js";
export { translateText } from "../../js/api/reader.js";
export { createReaderAskAnswerer } from "../../js/reader/ai/ask-answerer.js";
export { createReaderMarkdownAnswerer } from "../../js/reader/ai/markdown-answerer.js";
export {
  hydrateProtectedImages,
  injectCitationMarkers,
  isAgenticCitation,
  neutralizeMarkdownAnchors,
  renderCitationFooter,
  revokeHydratedImageUrls,
} from "../../js/reader/ai/answer-enhance.js";
export type { AiCitationLike } from "../../js/reader/ai/answer-enhance.js";
export {
  armReaderAiClickShield,
  clearReaderAiNavigationLock,
  installReaderWindowOpenGuard,
  isReaderAiNavigationLocked,
  lockReaderAiNavigation,
  shouldIgnoreReaderAiNavEvent,
} from "../../js/reader/ai/ui-interaction-lock.js";
export {
  peekFinalAnswerHtmlCache,
  renderFinalAnswerHtml,
  renderStreamingPreviewHtml,
} from "../../js/reader/ai/render-answer-html.js";
export { sanitizeAssistantAnswer } from "../../js/reader/ai/sanitize-answer.js";
export {
  clearThreadBranchSnapshot,
  loadThreadBranchSnapshot,
  saveThreadBranchSnapshot,
  threadBranchStorageKey,
  visiblePathFromSnapshot,
} from "../../js/reader/ai/thread-branch-store.js";
export type {
  ThreadBranchCitation,
  ThreadBranchItem,
  ThreadBranchMessage,
  ThreadBranchSnapshot,
} from "../../js/reader/ai/thread-branch-store.js";
export {
  appendConversationMessage,
  baseConversationTitle,
  createConversation,
  deleteConversation,
  forkConversationFromPath,
  getConversation,
  listConversations,
  messagesToBranchItems,
  nextForkConversationTitle,
  patchConversation,
} from "../../js/api/conversations.js";
export type {
  ConversationDetail,
  ConversationRecord,
  MessageRecord,
} from "../../js/api/conversations.js";
export {
  loadStoredConversationId,
  saveStoredConversationId,
  clearStoredConversationId,
} from "../../js/reader/ai/conversation-store.js";

// —— 服务端收藏面板 ——
export { API_PREFIX } from "../../js/config/api-constants.js";
export { fetchFavorites } from "../../js/api/favorites.js";
export {
  createReaderServerFavoritesPort,
  normalizeServerFavorite,
} from "./legacy/server-favorites-port.js";
export type { ServerFavorite } from "./legacy/types.js";

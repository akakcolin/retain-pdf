import { API_PREFIX } from "../../../js/config/api-constants.js";
import {
  fetchJobPayload,
} from "../../../js/api/jobs-query.js";
import {
  fetchJobArtifactsManifest,
  fetchJobMarkdown,
  fetchJobMarkdownDocument,
} from "../../../js/api/jobs-artifacts.js";
import { fetchProtected } from "../../../js/api/http.js";
import {
  fetchReaderAiChat,
  fetchReaderMetadata,
  fetchReaderRegions,
} from "../../../js/api/reader.js";
import {
  fetchTranslationItem,
} from "../../../js/api/translation-debug.js";

export function createReaderDataPort({
  apiPrefix = API_PREFIX,
  loadJob = fetchJobPayload,
  loadManifest = fetchJobArtifactsManifest,
  loadMarkdown = fetchJobMarkdown,
  loadMarkdownDocument = fetchJobMarkdownDocument,
  loadAiChat = fetchReaderAiChat,
  loadRegions = fetchReaderRegions,
  loadMetadata = fetchReaderMetadata,
  loadTranslationItem = fetchTranslationItem,
  fetchProtectedResource = fetchProtected,
} = {}) {
  async function loadReaderPayload(jobId) {
    const [jobPayload, manifestPayload, regionsPayload, readerMetadata] = await Promise.all([
      loadJob(jobId, apiPrefix),
      loadManifest(jobId, apiPrefix),
      loadRegions(jobId, apiPrefix).catch(() => ({ items: [] })),
      loadMetadata(jobId, apiPrefix).catch(() => null),
    ]);
    return {
      jobPayload,
      manifestPayload,
      readerMetadata,
      regionsPayload,
    };
  }

  function fetchRegionTranslationItem(jobId, itemId) {
    return loadTranslationItem(jobId, itemId, apiPrefix);
  }

  // 优先 /markdown/document：带 content_with_absolute_image_urls，图片可按 API 地址鉴权拉取。
  // /markdown 仅有相对 images/...，在 reader 页上会解析到静态源 404 →「图片暂不可用」。
  async function loadMarkdownPayload(jobId) {
    try {
      const documentPayload = await loadMarkdownDocument(jobId, apiPrefix);
      if (documentPayload) {
        return documentPayload;
      }
    } catch (_err) {
      /* fall through */
    }
    return loadMarkdown(jobId, apiPrefix);
  }

  function submitAiChat(jobId, payload) {
    return loadAiChat(jobId, payload, apiPrefix);
  }

  return Object.freeze({
    apiPrefix,
    fetchProtected: fetchProtectedResource,
    fetchRegionTranslationItem,
    loadMarkdownPayload,
    loadReaderPayload,
    submitAiChat,
  });
}

export const defaultReaderDataPort = createReaderDataPort();

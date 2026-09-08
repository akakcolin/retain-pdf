import {
  isMockMode,
  readerMessageTargetOrigin,
} from "../../../js/config/runtime.js";
import { getMockJobId } from "../../../js/mock/index.js";

function defaultSearch() {
  return globalThis.window?.location?.search || "";
}

export function resolveReaderJobId({
  search = defaultSearch(),
  isMock = isMockMode,
  mockJobId = getMockJobId,
} = {}) {
  const jobId = new URLSearchParams(search).get("job_id")?.trim() || "";
  if (jobId) {
    return jobId;
  }
  // ?document_id= 是馆藏文档"读原文"入口(F4):此时没有 job,不应回退到 mock job,
  // 否则源文档阅读器会误挂 mock 任务。
  const documentId = new URLSearchParams(search).get("document_id")?.trim() || "";
  if (documentId) {
    return "";
  }
  return isMock() ? mockJobId() : "";
}

// 馆藏文档"读原文"(F4):无 job、仅 document_id 时,阅读器走只读源文档分支。
export function resolveReaderDocumentId({ search = defaultSearch() } = {}) {
  return new URLSearchParams(search).get("document_id")?.trim() || "";
}

// 锚点 (page_idx, block_id) 来自搜索命中/收藏回跳的 URL 透传
export function resolveReaderAnchor({ search = defaultSearch() } = {}) {
  const params = new URLSearchParams(search);
  const rawPageIdx = `${params.get("page_idx") ?? ""}`.trim();
  const blockId = `${params.get("block_id") || ""}`.trim();
  const pageIdx = rawPageIdx === "" ? NaN : Number(rawPageIdx);
  if (!Number.isFinite(pageIdx) && !blockId) {
    return null;
  }
  return {
    pageIdx: Number.isFinite(pageIdx) ? pageIdx : null,
    blockId,
  };
}

export function createReaderPageConfigPort({
  messageTargetOrigin = readerMessageTargetOrigin,
  isMock = isMockMode,
  mockJobId = getMockJobId,
  search = defaultSearch,
} = {}) {
  function readerJobId() {
    return resolveReaderJobId({
      search: search(),
      isMock,
      mockJobId,
    });
  }

  return Object.freeze({
    messageTargetOrigin,
    readerJobId,
  });
}

export const defaultReaderPageConfigPort = createReaderPageConfigPort();

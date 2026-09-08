// pages/detail 对 src/js/* 的唯一出口。
// DetailApp / components 禁止直接 import ../../js/**；缺符号只改本文件。

// —— job ——
export { normalizeJobPayload } from "../../js/job/normalize.js";
export { isJobTerminal } from "../../js/job/core.js";
export {
  formatEventTimestamp,
  formatRuntimeDuration,
} from "../../js/job/formatters.js";
export { stageHistoryDisplay } from "../../js/job/stage-history.js";

// —— job-detail ——
export { getJobIdFromQuery } from "./legacy/routing.js";
export { defaultJobDetailConfigPort } from "./legacy/config-port.js";
export { defaultJobDetailDataPort } from "./legacy/data-port.js";
export { defaultJobDetailResumePort } from "./legacy/resume-port.js";
export { bindRerunButton } from "./legacy/resume.js";
export { renderJobDetailOverview } from "./legacy/overview-renderer.js";
export { loadAndRenderMarkdownFlow } from "./legacy/markdown-flow.js";
export {
  createJobDetailPageState,
  revokeJobDetailMarkdownImageUrls,
} from "./legacy/page-state.js";
export { buildJobDetailEventViewModel } from "./legacy/status-view-model.js";

// —— downloads ——
export {
  fileNameFromDisposition,
  prepareDownloadTarget,
  saveResponseDownload,
} from "../../js/utils/downloads.js";
export {
  completeDownloadToast,
  failDownloadToast,
  showDownloadPreparing,
  updateDownloadProgress,
} from "../../js/utils/download-feedback.js";

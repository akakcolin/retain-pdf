// features/status 对 src/js/* 纯逻辑层的本地端口（ADR 0009 双轨收敛）。
// 自 js/job-status 迁入的纯 VM 经此消费旧逻辑层；本文件是
// pages/home/features/status 下唯一允许直接 import src/js/* 的位置。
// 缺符号只改本文件。

// —— app-framework ——
export { createSelector } from "../../../../js/app-framework/selector.js";

// —— job-runtime ——
export {
  currentDisplayedStagePin,
  keepDisplayedStageForward,
  pinnedStagePresentation,
  resetDisplayedStagePin,
  resolvePinnedStagePresentation,
  setDisplayedStagePin,
} from "../../../../js/features/job-runtime/stage-pin-state.js";

// —— job ——
export { buildReaderPageUrl, isReaderActionEnabled } from "../../../../js/job/action-model.js";
export {
  resolveJobActions,
  resolveJobMarkdownBundleAction,
  resolveJobSourcePdfAction,
  resolveJobTranslatedMarkdownBundleAction,
} from "../../../../js/job/actions.js";
export { firstNonEmpty, isJobTerminal } from "../../../../js/job/core.js";
export { summarizePublicError, summarizeStatus } from "../../../../js/job/diagnostics.js";
export { resolveLiveDurations } from "../../../../js/job/durations.js";
export { formatJobFinishedAt } from "../../../../js/job/formatters.js";
export type {
  JobLane,
  JobLike,
  JobPayload,
  JobProgress,
  JobStatus,
  ManifestPayload,
  ProgressUnit,
  PublicStage,
  StageKey,
  StageSnapshot,
} from "../../../../js/job/types.js";

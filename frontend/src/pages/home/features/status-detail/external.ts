// features/status-detail 对 src/js/* 纯逻辑层的本地端口（ADR 0009 双轨收敛）。
// 自 js/status-detail 迁入的纯 VM 经此消费旧逻辑层；本文件是
// pages/home/features/status-detail 下唯一允许直接 import src/js/* 的位置。
// 缺符号只改本文件。

export { resolveJobActions } from "../../../../js/job/actions.js";
export { isJobTerminal, isTerminalStatus } from "../../../../js/job/core.js";
export { clampPositiveMs, parseIsoTime, resolveLiveDurations } from "../../../../js/job/durations.js";
export {
  formatEventTimestamp,
  formatRuntimeDuration,
  summarizeInvocationProtocol,
  summarizeInvocationSchemaVersion,
  summarizeRuntimeField,
} from "../../../../js/job/formatters.js";
export {
  resolveStageHistory,
  resolveStageHistoryDuration,
  stageHistoryDisplay,
  summarizeStageName,
} from "../../../../js/job/stage-history.js";
export type { JobLike, JobPayload } from "../../../../js/job/types.js";

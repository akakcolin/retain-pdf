import { resolveDisplayedStagePresentation } from "../status/job-stage-presentation.js";
import { buildEventsPresentation } from "./events.js";
import { buildStageHistoryPresentation } from "./history.js";
import { isJobTerminal } from "./external.js";
import {
  resolveJobActions,
} from "./external.js";
import {
  summarizeInvocationProtocol,
  summarizeInvocationSchemaVersion,
  summarizeRuntimeField,
} from "./external.js";
import {
  formatEventTimestamp,
  resolveLiveDurations,
} from "./utils.js";
import { summarizeStageName } from "./external.js";
import type { JobLike, JobPayload } from "./external.js";

/** Options forwarded to resolveLiveDurations / stage history. */
export interface StatusDetailDurationOptions {
  finishedAtFallback?: string;
  now?: string | Date | null;
}

export interface StatusDetailSnapshotOptions {
  durationOptions?: StatusDetailDurationOptions;
}

/** Stage presentation fields consumed by the status-detail headline/runtime. */
interface StagePresentationLike {
  detail?: string;
  progressText?: string;
  [key: string]: unknown;
}

type StatusDetailJob = JobLike | JobPayload | null | undefined;

function stageIconMarkup(job: StatusDetailJob, stageText: string | undefined): string {
  const text = `${stageText || ""}`.toLowerCase();
  const status = `${job?.status || ""}`.trim();
  if (status === "succeeded" && isJobTerminal(job)) {
    return '<svg viewBox="0 0 24 24" fill="none"><path d="M5 13l4 4L19 7" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  }
  if (status === "failed") {
    return '<svg viewBox="0 0 24 24" fill="none"><path d="M15 9l-6 6M9 9l6 6M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  }
  if (text.includes("排队")) {
    return '<svg viewBox="0 0 24 24" fill="none"><path d="M8 7h8M8 12h8M8 17h5M6 4h12a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/></svg>';
  }
  if (text.includes("翻译")) {
    return '<svg viewBox="0 0 24 24" fill="none"><path d="M4 6h8M8 6c0 6-2 10-5 12M8 6c1 3 3.5 6.5 7 9M14 6h6M17 6v12M14 18h6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  }
  if (text.includes("解析") || text.includes("ocr")) {
    return '<svg viewBox="0 0 24 24" fill="none"><path d="M7 4h7l5 5v11a1 1 0 0 1-1 1H7a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/><path d="M14 4v5h5" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/></svg>';
  }
  return '<svg viewBox="0 0 24 24" fill="none"><path d="M12 7v5l3 2M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>';
}

function statusDetailNote(job: StatusDetailJob = {}): string {
  return job.status === "failed"
    ? "查看失败原因、建议与事件流"
    : job.status === "succeeded" && isJobTerminal(job)
      ? "任务已完成，可查看概览与事件流"
      : "查看任务概览、失败原因与事件流";
}

function buildHeadline(job: StatusDetailJob, stageText: string | undefined) {
  return {
    iconMarkup: stageIconMarkup(job, stageText),
    jobId: job?.job_id || "-",
    note: statusDetailNote(job),
  };
}

function summarizeMathMode(job: StatusDetailJob): string {
  const mathMode = `${(job as JobPayload)?.request_payload_math_mode || ""}`.trim();
  if (mathMode === "placeholder") {
    return "placeholder - 公式占位保护";
  }
  if (mathMode === "direct_typst") {
    return "direct_typst - 模型直出公式";
  }
  return mathMode || "-";
}

function publicStageForRuntime(job: StatusDetailJob = {}): string {
  return `${job?.stage_snapshot?.publicStage || job?.display_stage || ""}`;
}

function runtimeStageDetail(presentation: StagePresentationLike = {}): string {
  return `${presentation.detail || presentation.progressText || ""}`;
}

function buildRuntimeDetails(
  job: StatusDetailJob,
  eventsPayload: unknown,
  durationOptions: StatusDetailDurationOptions = {},
) {
  const durations = resolveLiveDurations(job, durationOptions);
  const presentation = resolveDisplayedStagePresentation(job, eventsPayload) as StagePresentationLike;
  return {
    currentStage: summarizeStageName(publicStageForRuntime(job), runtimeStageDetail(presentation)),
    stageElapsed: durations.stageElapsedText,
    totalElapsed: durations.totalElapsedText,
    retryCount: `${job?.retry_count ?? 0}`,
    lastTransition: job?.last_stage_transition_at ? formatEventTimestamp(job.last_stage_transition_at) : "-",
    terminalReason: summarizeRuntimeField(job?.terminal_reason),
    inputProtocol: summarizeInvocationProtocol(job),
    stageSpecVersion: summarizeInvocationSchemaVersion(job),
    mathMode: summarizeMathMode(job),
  };
}

function buildFailureDetails(job: StatusDetailJob) {
  const failure = (job?.failure || {}) as Record<string, unknown>;
  const failureDiagnostic = (job?.failure_diagnostic || {}) as Record<string, unknown>;
  const diagnostics = (job?.diagnostics || job?.failure_diagnostics || {}) as Record<string, unknown>;
  const logTail = Array.isArray(job?.log_tail) ? job.log_tail : [];
  const failureLastLogLine = failure.last_log_line
    || failureDiagnostic.last_log_line
    || failure.raw_excerpt
    || failure.raw_exception_message
    || (logTail.length ? logTail[logTail.length - 1] : "");
  const retryable = failure.retryable ?? failureDiagnostic.retryable;
  return {
    summary: summarizeRuntimeField(
      diagnostics.summary || diagnostics.detail || failure.summary || failure.detail || job?.final_failure_summary || failureDiagnostic.summary || failureDiagnostic.detail || failure.raw_excerpt,
    ),
    category: summarizeRuntimeField(
      diagnostics.failure_category || diagnostics.category || diagnostics.error_type || failure.category || failure.failure_category || job?.final_failure_category || failureDiagnostic.type || failureDiagnostic.error_kind || failure.error_type || failure.failure_code,
    ),
    stage: summarizeRuntimeField(
      diagnostics.failed_stage || diagnostics.stage || failure.stage || failure.failed_stage || failure.provider_stage || failureDiagnostic.stage || failureDiagnostic.failed_stage,
    ),
    rootCause: summarizeRuntimeField(
      diagnostics.root_cause || diagnostics.raw_exception_type || failure.root_cause || failureDiagnostic.root_cause || failure.raw_exception_type || failure.upstream_host,
    ),
    suggestion: summarizeRuntimeField(
      diagnostics.suggestion || failure.suggestion || failureDiagnostic.suggestion || failure.failure_code,
    ),
    lastLogLine: summarizeRuntimeField(
      diagnostics.raw_excerpt || diagnostics.detail || failureLastLogLine,
    ),
    retryable: typeof (diagnostics.retryable ?? retryable) === "boolean" ? ((diagnostics.retryable ?? retryable) ? "是" : "否") : "-",
  };
}

export function buildStatusDetailSnapshot(
  job: StatusDetailJob,
  eventsPayload: unknown,
  {
    durationOptions = {},
  }: StatusDetailSnapshotOptions = {},
) {
  const presentation = resolveDisplayedStagePresentation(job, eventsPayload) as StagePresentationLike;
  const actions = resolveJobActions(job);
  const rerunEnabled = Boolean(actions.rerunEnabled && actions.rerun);

  return {
    headline: buildHeadline(job, presentation.detail),
    runtime: buildRuntimeDetails(job, eventsPayload, durationOptions),
    failure: buildFailureDetails(job),
    stageHistory: buildStageHistoryPresentation(job, durationOptions),
    events: buildEventsPresentation(eventsPayload),
    rerun: {
      enabled: rerunEnabled,
      status: rerunEnabled
        ? "后端支持从当前任务产物创建恢复任务。"
        : "当前任务暂不可从断点恢复。",
    },
  };
}

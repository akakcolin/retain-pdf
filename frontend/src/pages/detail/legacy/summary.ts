import { TEXT_KEYS } from "../../../shared/dom/text-keys.js";
import {
  formatEventTimestamp,
  formatJobFinishedAt,
  summarizeInvocationProtocol,
  summarizeInvocationSchemaVersion,
  summarizeRuntimeField,
} from "../../../js/job/formatters.js";
import {
  summarizePublicError,
  summarizeStatus,
} from "../../../js/job/diagnostics.js";
import { firstNonEmptyText } from "./routing.js";

export function summarizeMathMode(job) {
  const mathMode = `${job?.request_payload_math_mode || ""}`.trim();
  if (mathMode === "placeholder") {
    return "placeholder - 公式占位保护";
  }
  if (mathMode === "direct_typst") {
    return "direct_typst - 模型直出公式";
  }
  return mathMode || "-";
}

export function renderJobDetailRuntimeSummary({
  durations,
  job,
  setText,
  statusViewModel,
}) {
  setText(TEXT_KEYS.detailStatusSummary, summarizeStatus(job.status || "idle"));
  setText(TEXT_KEYS.detailStageDetail, statusViewModel.stageDetail);
  setText(TEXT_KEYS.detailFinishedAt, formatJobFinishedAt(job));
  setText(TEXT_KEYS.detailRuntimeCurrentStage, statusViewModel.runtimeCurrentStage);
  setText(TEXT_KEYS.detailRuntimeStageElapsed, durations.stageElapsedText);
  setText(TEXT_KEYS.detailRuntimeTotalElapsed, durations.totalElapsedText);
  setText(TEXT_KEYS.detailRuntimeRetryCount, `${job.retry_count ?? 0}`);
  setText(TEXT_KEYS.detailRuntimeLastTransition, job.last_stage_transition_at ? formatEventTimestamp(job.last_stage_transition_at) : "-");
  setText(TEXT_KEYS.detailRuntimeTerminalReason, summarizeRuntimeField(job.terminal_reason));
  setText(TEXT_KEYS.detailRuntimeInputProtocol, summarizeInvocationProtocol(job));
  setText(TEXT_KEYS.detailRuntimeStageSpecVersion, summarizeInvocationSchemaVersion(job));
  setText(TEXT_KEYS.detailRuntimeMathMode, summarizeMathMode(job));
}

export function renderJobDetailFailureSummary({ job, setText }) {
  const failure = job.failure || {};
  const failureDiagnostic = job.failure_diagnostic || {};
  const retryable = failure.retryable ?? failureDiagnostic.retryable;
  const failureLastLogLine = firstNonEmptyText(
    failure.last_log_line,
    failureDiagnostic.last_log_line,
    Array.isArray(job.log_tail) && job.log_tail.length ? job.log_tail[job.log_tail.length - 1] : "",
  );

  setText(TEXT_KEYS.detailFailureSummary, summarizeRuntimeField(failure.summary || job.final_failure_summary || failureDiagnostic.summary || failure.raw_excerpt));
  setText(TEXT_KEYS.detailFailureCategory, summarizeRuntimeField(
    failure.category
    || failure.failure_category
    || job.final_failure_category
    || failureDiagnostic.type
    || failureDiagnostic.error_kind,
  ));
  setText(TEXT_KEYS.detailFailureStage, summarizeRuntimeField(
    failure.stage
    || failure.failed_stage
    || failure.provider_stage
    || failureDiagnostic.stage
    || failureDiagnostic.failed_stage,
  ));
  setText(TEXT_KEYS.detailFailureRootCause, summarizeRuntimeField(failure.root_cause || failureDiagnostic.root_cause || failure.upstream_host));
  setText(TEXT_KEYS.detailFailureSuggestion, summarizeRuntimeField(failure.suggestion || failureDiagnostic.suggestion || failure.failure_code));
  setText(TEXT_KEYS.detailFailureLastLogLine, summarizeRuntimeField(failureLastLogLine));
  setText(TEXT_KEYS.detailFailureRetryable, typeof retryable === "boolean" ? (retryable ? "是" : "否") : "-");
}

export function renderJobDetailPublicError({ job, setText }) {
  setText(TEXT_KEYS.detailErrorBox, summarizePublicError(job));
}

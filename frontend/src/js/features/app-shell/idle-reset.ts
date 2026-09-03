import { TEXT_KEYS } from "../../dom/text-keys.js";

export function resetStatusDetailRuntimeView({ setText, resetEventsList, activateDetailTab }: any) {
  setText(TEXT_KEYS.runtimeCurrentStage, "-");
  setText(TEXT_KEYS.runtimeStageElapsed, "-");
  setText(TEXT_KEYS.runtimeTotalElapsed, "-");
  setText(TEXT_KEYS.runtimeRetryCount, "0");
  setText(TEXT_KEYS.runtimeLastTransition, "-");
  setText(TEXT_KEYS.runtimeTerminalReason, "-");
  setText(TEXT_KEYS.runtimeInputProtocol, "-");
  setText(TEXT_KEYS.runtimeStageSpecVersion, "-");
  setText(TEXT_KEYS.runtimeMathMode, "-");
  setText(TEXT_KEYS.statusDetailJobId, "-");
  setText(TEXT_KEYS.failureSummary, "-");
  setText(TEXT_KEYS.failureCategory, "-");
  setText(TEXT_KEYS.failureStage, "-");
  setText(TEXT_KEYS.failureRootCause, "-");
  setText(TEXT_KEYS.failureSuggestion, "-");
  setText(TEXT_KEYS.failureLastLogLine, "-");
  setText(TEXT_KEYS.failureRetryable, "-");
  setText(TEXT_KEYS.eventsStatus, "全部事件");
  resetEventsList();
  activateDetailTab("overview");
}

export function initializeIdleAppView({
  configPort,
  jobPresentationPort = {},
  setText,
  setWorkflowSections,
  setLinearProgress,
  updateActionButtons,
  renderPageRangeSummary,
  resetUploadProgress,
  resetUploadedFile,
  applyWorkflowMode,
  updateJobWarning,
  resetEventsList,
  activateDetailTab,
}: any) {
  const normalizeJobPayload = jobPresentationPort.normalizeJobPayload || ((payload) => payload);
  const summarizeStatus = jobPresentationPort.summarizeStatus || ((status) => status);

  updateActionButtons(normalizeJobPayload({}));
  setWorkflowSections(null);
  setLinearProgress("job-progress-bar", "job-progress-text", NaN, NaN, "-");
  setText(TEXT_KEYS.jobSummary, summarizeStatus("idle"));
  setText(TEXT_KEYS.jobStageDetail, "-");
  setText(TEXT_KEYS.queryJobDuration, "-");
  resetStatusDetailRuntimeView({ setText, resetEventsList, activateDetailTab });
  if (configPort?.isMock?.()) {
    setText(TEXT_KEYS.errorBox, "-");
  }
  renderPageRangeSummary();
  resetUploadProgress();
  resetUploadedFile();
  applyWorkflowMode();
  updateJobWarning("idle");
}

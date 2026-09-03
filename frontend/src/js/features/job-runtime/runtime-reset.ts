import { TEXT_KEYS } from "../../dom/text-keys.js";
import { resetStatusDetailRuntimeView } from "../app-shell/idle-reset.js";
import { clearActiveJobId } from "./active-job-storage.js";
import { createJobRuntimeShellViewPort } from "./shell-view-port.js";
import { createJobRuntimeResetStatePort } from "./reset-state-port.js";
import {
  currentJobId,
} from "./current-job-state.js";
import {
  invalidateJobPolls,
} from "./runtime-polling-state.js";

export function returnJobRuntimeToHome({
  state,
  onReaderDialogClose,
  setWorkflowSections,
  resetUploadProgress,
  resetUploadedFile,
  applyWorkflowMode,
  clearPageRanges,
  setText,
  updateJobWarning,
  activateDetailTab,
  uploadStatePort,
  resetStatePort,
  shellViewPort = createJobRuntimeShellViewPort(),
  jobPresentationPort = {},
}: any) {
  const summarizeStatus = jobPresentationPort.summarizeStatus || ((status) => status);
  const resetState = resetStatePort || createJobRuntimeResetStatePort(state);
  clearActiveJobId(currentJobId(state));
  // invalidate = stop + generation 前移：在途 poll 返回后不再写回渲染/书架。
  invalidateJobPolls(state);
  shellViewPort.closeDialogs();
  onReaderDialogClose?.();
  resetState.resetJob();
  if (uploadStatePort?.clearAppliedPageRange) {
    uploadStatePort.clearAppliedPageRange();
  } else {
    resetState.clearAppliedPageRange?.();
  }
  setWorkflowSections(null);
  resetUploadProgress();
  resetUploadedFile();
  applyWorkflowMode();
  setText(TEXT_KEYS.jobSummary, summarizeStatus("idle"));
  setText(TEXT_KEYS.jobStageDetail, "-");
  setText(TEXT_KEYS.jobId, "-");
  setText(TEXT_KEYS.queryJobDuration, "-");
  setText(TEXT_KEYS.jobFinishedAt, "-");
  clearPageRanges();
  resetStatusDetailRuntimeView({
    setText,
    resetEventsList: shellViewPort.resetEvents,
    activateDetailTab,
  });
  updateJobWarning("idle");
}

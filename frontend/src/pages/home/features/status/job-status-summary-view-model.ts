import {
  formatJobFinishedAt,
} from "./external.js";
import {
  summarizePublicError,
  summarizeStatus,
} from "./external.js";

export function buildJobStatusSummaryViewModel(job: any = {}, stagePresentation: any = {}) {
  const jobId = job.job_id || "-";
  const finishedAt = formatJobFinishedAt(job);
  const publicErrorText = summarizePublicError(job);
  return {
    errorText: publicErrorText,
    fields: {
      jobId,
      jobIdInput: job.job_id || "",
      stageDetail: stagePresentation.detail || "-",
      statusSummary: summarizeStatus(job.status || "idle"),
      finishedAt,
      queryFinishedAt: finishedAt,
    },
    publicErrorText,
  };
}

import { defaultJobDetailConfigPort } from "./config-port.js";

export function getJobIdFromQuery() {
  return new URLSearchParams(window.location.search).get("job_id")?.trim() || "";
}

export function firstNonEmptyText(...values) {
  for (const value of values) {
    if (typeof value === "string" && value.trim()) {
      return value.trim();
    }
  }
  return "";
}

export function firstJobIdFromPayload(payload) {
  return firstNonEmptyText(
    payload?.job_id,
    payload?.data?.job_id,
    payload?.job?.job_id,
    payload?.job?.id,
    payload?.id,
  );
}

export function buildReaderPageUrl(jobId) {
  return defaultJobDetailConfigPort.buildReaderPageUrl(jobId);
}

export function buildDetailPageUrl(jobId) {
  return defaultJobDetailConfigPort.buildDetailPageUrl(jobId);
}

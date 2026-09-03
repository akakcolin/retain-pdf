import { buildApiHeaders, isMockMode } from "../config/runtime.js";
import { unwrapEnvelope } from "../job/core.js";
import {
  getMockJobList,
  getMockJobPayload,
} from "../mock/index.js";
import { buildJobDetailEndpoint, buildJobsEndpoint } from "./http.js";

/** 轮询单次请求超时：超时后本轮按失败处理，轮询拍继续滚动（不再静默假死）。 */
export const JOB_FETCH_TIMEOUT_MS = 10000;

function jobFetchSignal() {
  if (typeof AbortSignal !== "undefined" && typeof AbortSignal.timeout === "function") {
    return AbortSignal.timeout(JOB_FETCH_TIMEOUT_MS);
  }
  return undefined;
}

function isTimeoutLikeError(error: unknown): boolean {
  const name = `${(error as { name?: string })?.name || ""}`;
  return name === "TimeoutError" || name === "AbortError";
}

export async function fetchJobPayload(jobId, apiPrefix) {
  if (isMockMode()) {
    void apiPrefix;
    return getMockJobPayload(jobId);
  }
  let resp;
  try {
    resp = await fetch(buildJobDetailEndpoint(jobId, apiPrefix), {
      headers: buildApiHeaders(),
      signal: jobFetchSignal(),
    });
  } catch (error) {
    if (isTimeoutLikeError(error)) {
      throw new Error(`读取任务状态超时（>${Math.round(JOB_FETCH_TIMEOUT_MS / 1000)}秒），请确认后端服务仍在响应。`);
    }
    throw error;
  }
  if (!resp.ok) {
    if (resp.status === 404) {
      throw new Error("未找到该任务，请检查 job_id 是否正确。");
    }
    throw new Error(`读取任务失败，请稍后重试。(${resp.status})`);
  }
  return unwrapEnvelope(await resp.json());
}

export async function fetchJobList(
  apiPrefix,
  {
    limit = 20,
    offset = 0,
    status = "",
    workflow = "",
    provider = "",
    scope = "jobs",
    q = "",
  } = {},
) {
  if (isMockMode()) {
    void apiPrefix;
    return getMockJobList();
  }
  const params = new URLSearchParams();
  params.set("limit", `${limit}`);
  params.set("offset", `${offset}`);
  if (status) {
    params.set("status", status);
  }
  if (workflow) {
    params.set("workflow", workflow);
  }
  if (provider) {
    params.set("provider", provider);
  }
  if (`${q || ""}`.trim()) {
    params.set("q", `${q || ""}`.trim());
  }
  const resp = await fetch(`${buildJobsEndpoint(apiPrefix, scope)}?${params.toString()}`, {
    headers: buildApiHeaders(),
  });
  if (!resp.ok) {
    throw new Error(`读取最近任务失败，请稍后重试。(${resp.status})`);
  }
  return unwrapEnvelope(await resp.json());
}

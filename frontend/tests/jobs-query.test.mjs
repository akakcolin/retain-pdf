import test from "node:test";
import assert from "node:assert/strict";

// 让 config/runtime.js 的 isMockMode()/apiBase() 在 node 下可用(无 jsdom 需求)
globalThis.window = globalThis.window || { location: { search: "", protocol: "http:", hostname: "127.0.0.1" } };

const { fetchJobPayload, JOB_FETCH_TIMEOUT_MS } = await import("../src/js/api/jobs-query.js");

function jsonResponse(payload) {
  return { ok: true, status: 200, json: async () => payload };
}

test("fetchJobPayload 携带超时 AbortSignal 发请求(防后端 hang 静默假死)", async () => {
  const previousFetch = globalThis.fetch;
  let seen = null;
  globalThis.fetch = async (url, options = {}) => {
    seen = { url, options };
    return jsonResponse({ job_id: "job-timeout-signal", status: "running" });
  };
  try {
    const payload = await fetchJobPayload("job-timeout-signal", "/api/v1");
    assert.equal(payload.job_id, "job-timeout-signal");
    assert.match(seen.url, /\/api\/v1\/jobs\/job-timeout-signal$/);
    assert.ok(seen.options.signal instanceof AbortSignal, "轮询请求必须携带 AbortSignal");
    assert.equal(seen.options.signal.aborted, false);
    assert.ok(JOB_FETCH_TIMEOUT_MS >= 5000 && JOB_FETCH_TIMEOUT_MS <= 30000);
  } finally {
    globalThis.fetch = previousFetch;
  }
});

test("fetchJobPayload 把超时错误翻译成用户可见的中文提示", async () => {
  const previousFetch = globalThis.fetch;
  globalThis.fetch = async () => {
    const error = new Error("The operation timed out");
    error.name = "TimeoutError";
    throw error;
  };
  try {
    await assert.rejects(
      fetchJobPayload("job-hang", "/api/v1"),
      (error) => {
        assert.match(error.message, /超时/);
        return true;
      },
    );
  } finally {
    globalThis.fetch = previousFetch;
  }
});

test("fetchJobPayload 对非超时网络错误原样抛出(不误报为超时)", async () => {
  const previousFetch = globalThis.fetch;
  globalThis.fetch = async () => {
    throw new TypeError("fetch failed");
  };
  try {
    await assert.rejects(fetchJobPayload("job-down", "/api/v1"), TypeError);
  } finally {
    globalThis.fetch = previousFetch;
  }
});

test("fetchJobPayload 保留 404 语义", async () => {
  const previousFetch = globalThis.fetch;
  globalThis.fetch = async () => ({ ok: false, status: 404, json: async () => ({}) });
  try {
    await assert.rejects(fetchJobPayload("job-missing", "/api/v1"), /未找到该任务/);
  } finally {
    globalThis.fetch = previousFetch;
  }
});

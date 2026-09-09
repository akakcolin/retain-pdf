import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

// 取消语义锁（审计 P0-2/P0-4 回归锁,ask-answerer 侧）：
// 1. answer() 把 AbortSignal 透传给 ask（askLibraryAi → fetch）——断流是真的
// 2. aborted 的旧流即便带回 conversation_id,也禁止回写会话粘性
//    （否则"生成中切会话"会被旧 done 拽回旧会话,下一问落错线程）

const dom = new JSDOM("<!doctype html><body></body>", { url: "http://localhost/" });
globalThis.document = dom.window.document;
globalThis.localStorage = dom.window.localStorage;

const { createReaderAskAnswerer } = await import("../src/js/reader/ai/ask-answerer.ts");
const { loadStoredConversationId } = await import("../src/js/reader/ai/conversation-store.ts");

function makeAnswerer(fakeAsk, jobId) {
  return createReaderAskAnswerer({
    jobId,
    ask: fakeAsk,
    documentByJobId: async () => ({ document_id: `doc-${jobId}` }),
    llmConfig: () => ({ apiKey: "test-model-key" }),
  });
}

test("signal 透传到 ask,正常完成时回写会话粘性", async () => {
  const seen = {};
  const answerer = makeAnswerer(async (args) => {
    seen.signal = args.signal;
    return { answer: "答 [1]", citations: [], conversationId: "conv-live" };
  }, "job-cancel-a");

  const controller = new AbortController();
  const result = await answerer.answer({ question: "问", signal: controller.signal });
  assert.equal(seen.signal, controller.signal, "signal 必须透传给 ask");
  assert.equal(result.conversationId, "conv-live");
  assert.equal(answerer.getConversationId(), "conv-live", "正常完成回写内存粘性");
  assert.equal(
    loadStoredConversationId({ jobId: "job-cancel-a" }),
    "conv-live",
    "正常完成回写 storage 粘性",
  );
});

test("aborted 的旧流禁止回写会话粘性(P0-4)", async () => {
  const answerer = makeAnswerer(async () => {
    // 模拟：abort 发生在流进行中,但 done 仍带回旧会话 id
    return { answer: "迟到的旧答案", citations: [], conversationId: "conv-stale" };
  }, "job-cancel-b");

  const controller = new AbortController();
  controller.abort();
  await answerer.answer({ question: "问", signal: controller.signal });
  assert.notEqual(answerer.getConversationId(), "conv-stale", "aborted 不得改内存粘性");
  assert.notEqual(
    loadStoredConversationId({ jobId: "job-cancel-b" }),
    "conv-stale",
    "aborted 不得写 storage 粘性",
  );
});

// 检索范围（retrievalScope）：library 必须同时清空 documentId 与 jobId，
// 否则后端 resolve_document_id 会由 job_id 反查回当前文档，又变回单文档检索。

test("全库模式：documentId/jobId 皆空且不反查文档", async () => {
  const seen = {};
  let lookupCalls = 0;
  const answerer = createReaderAskAnswerer({
    jobId: "job-library",
    ask: async (args) => {
      seen.documentId = args.documentId;
      seen.jobId = args.jobId;
      return { answer: "答", citations: [], conversationId: "conv-lib" };
    },
    documentByJobId: async () => {
      lookupCalls += 1;
      return { document_id: "doc-library" };
    },
    llmConfig: () => ({ apiKey: "test-model-key" }),
  });

  await answerer.answer({ question: "问", retrievalScope: "library" });
  assert.equal(seen.documentId, "", "全库模式 document_id 必须为空");
  assert.equal(seen.jobId, "", "全库模式必须清空 job_id");
  assert.equal(lookupCalls, 0, "全库模式不应反查文档");
});

test("全库模式：文档反查失败也不 fail closed", async () => {
  let called = false;
  const answerer = createReaderAskAnswerer({
    jobId: "job-library-2",
    ask: async () => {
      called = true;
      return { answer: "答", citations: [], conversationId: "conv-lib-2" };
    },
    documentByJobId: async () => {
      throw new Error("boom");
    },
    llmConfig: () => ({ apiKey: "test-model-key" }),
  });

  await answerer.answer({ question: "问", retrievalScope: "library" });
  assert.equal(called, true, "全库模式不得因文档反查失败抛错");
});

test("单文档模式：仍传 documentId/jobId", async () => {
  const seen = {};
  const answerer = createReaderAskAnswerer({
    jobId: "job-doc",
    ask: async (args) => {
      seen.documentId = args.documentId;
      seen.jobId = args.jobId;
      return { answer: "答", citations: [], conversationId: "conv-doc" };
    },
    documentByJobId: async () => ({ document_id: "doc-doc" }),
    llmConfig: () => ({ apiKey: "test-model-key" }),
  });

  await answerer.answer({ question: "问" });
  assert.equal(seen.documentId, "doc-doc");
  assert.equal(seen.jobId, "job-doc");
});

test("单文档模式：反查不到文档仍 fail closed", async () => {
  const answerer = createReaderAskAnswerer({
    jobId: "job-doc-miss",
    ask: async () => {
      throw new Error("不应调用 ask");
    },
    documentByJobId: async () => null,
    llmConfig: () => ({ apiKey: "test-model-key" }),
  });

  await assert.rejects(
    () => answerer.answer({ question: "问" }),
    /无法关联当前文档/,
  );
});

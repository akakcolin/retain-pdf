import { buildApiHeaders, isMockMode } from "../config/runtime.js";
import { API_PREFIX } from "../config/api-constants.js";
import { unwrapEnvelope } from "../job/core.js";
import { buildApiEndpoint } from "./http.js";

// 图书馆 AI 问答(POST /api/v1/ai/ask,SSE 流式)。
// 用流式 fetch 而不是 EventSource:EventSource 无法携带 X-API-Key 请求头。

export class AiAskError extends Error {
  status: number;
  constructor(message, status = 0) {
    super(message);
    this.name = "AiAskError";
    this.status = status;
  }
}

function normalizeDonePayload(payload: any = {}) {
  return {
    answer: `${payload?.answer || ""}`,
    citations: Array.isArray(payload?.citations) ? payload.citations : [],
    toolTrace: Array.isArray(payload?.tool_trace) ? payload.tool_trace : [],
    rounds: Number(payload?.rounds) || 0,
    conversationId: `${payload?.conversation_id || payload?.conversationId || ""}`.trim(),
    // 审计 C2:后端历史回写失败时 done.persisted=false,上层提示"未存入历史"。
    // 旧后端无此字段 → 视为已持久化(不误报)。
    persisted: payload?.persisted !== false,
  };
}

function parseSseEvent(line = "") {
  const trimmed = `${line}`.replace(/\r$/, "");
  if (!trimmed.startsWith("data:")) {
    return null;
  }
  const jsonText = trimmed.slice("data:".length).trim();
  if (!jsonText) {
    return null;
  }
  try {
    return JSON.parse(jsonText);
  } catch (_err) {
    return null;
  }
}

// 消费 /ai/ask 的 SSE body:按行切分 `data: {json}`,tool 事件回调,
// done 事件返回最终结果,error 事件抛 AiAskError。
export async function readAiAskStream(body, { onToolEvent = null, onAnswerDelta = null } = {}) {
  if (!body || typeof body.getReader !== "function") {
    throw new AiAskError("AI 服务响应格式异常,请重试。");
  }
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let result = null;
  let streamedAnswer = "";

  function handleLine(line) {
    const event = parseSseEvent(line);
    if (!event || typeof event !== "object") {
      return;
    }
    if (event.type === "tool") {
      onToolEvent?.(event);
      return;
    }
    if (event.type === "answer_delta") {
      // 最终回答轮的逐 token 增量:累积并回调,前端据此增量渲染
      const chunk = `${event.text || ""}`;
      if (chunk) {
        streamedAnswer += chunk;
        onAnswerDelta?.(streamedAnswer, chunk);
      }
      return;
    }
    if (event.type === "done") {
      // done.answer 为权威全文;后端未回 answer 时用累积的流式文本兜底
      result = normalizeDonePayload({
        ...event,
        answer: event.answer || streamedAnswer,
      });
      return;
    }
    if (event.type === "error") {
      throw new AiAskError(`${event.message || "AI 服务返回错误。"}`);
    }
  }

  try {
    while (!result) {
      const { done, value } = await reader.read();
      if (done) {
        break;
      }
      buffer += decoder.decode(value, { stream: true });
      let newlineIndex = buffer.indexOf("\n");
      while (newlineIndex >= 0) {
        handleLine(buffer.slice(0, newlineIndex));
        buffer = buffer.slice(newlineIndex + 1);
        newlineIndex = buffer.indexOf("\n");
      }
    }
    if (!result) {
      buffer += decoder.decode();
      if (buffer.trim()) {
        handleLine(buffer);
      }
    }
  } finally {
    reader.cancel?.().catch?.(() => {});
    reader.releaseLock?.();
  }
  if (!result) {
    throw new AiAskError("AI 服务响应中断,请重试。");
  }
  return result;
}

async function extractErrorMessage(resp) {
  const text = await resp.text().catch(() => "");
  try {
    const envelope = JSON.parse(text);
    // Rust envelope: { code, message }
    const message = `${envelope?.message || ""}`.trim();
    if (message) {
      return message;
    }
    // AI route error envelope: { detail: string | [{msg}] }
    const detail = envelope?.detail;
    if (typeof detail === "string" && detail.trim()) {
      return detail.trim();
    }
    if (Array.isArray(detail)) {
      const parts = detail
        .map((item) => {
          if (typeof item === "string") {
            return item.trim();
          }
          if (item && typeof item === "object") {
            return `${(item as { msg?: string }).msg || (item as { message?: string }).message || ""}`.trim();
          }
          return "";
        })
        .filter(Boolean);
      if (parts.length) {
        return parts.join("; ");
      }
    }
    return "";
  } catch (_err) {
    // 非 JSON 时截一段原文，避免整页 HTML 糊到聊天气泡
    return `${text || ""}`.replace(/\s+/g, " ").trim().slice(0, 240);
  }
}

// mock 模式的 SSE 流:忠实复刻真实后端事件序列(tool → answer_delta → done)。
// 引用 block_id 对齐 mock 阅读区域(b-intro-3),使引用跳转可端到端验证。
function buildMockAskStream(question = "") {
  const encoder = new TextEncoder();
  const answer = [
    `关于「${question}」,检索到以下要点:\n\n`,
    "- **卤素-锂交换**在共轭体系中表现出显著选择性 [1]\n",
    "- 该效应源于锂原子与芳环的有效共轭 [1]\n\n",
    "### 结论\n\n",
    "四重卤素交换未表现出配位倾向,量子化学计算支持这一解释。原始 HTML 如 <img src=x> 会以文本显示。\n",
  ];
  const events = [
    { type: "tool", round: 1, tool: "search_fulltext", arguments: { query: question } },
    { type: "tool", round: 1, tool: "read_blocks", arguments: {} },
    ...answer.map((text) => ({ type: "answer_delta", text })),
    {
      type: "done",
      answer: answer.join(""),
      citations: [
        {
          ref: 1,
          document_id: "doc-9f2a41c8e77b",
          job_id: "mock-job-20260415",
          page_idx: 0,
          block_id: "b-intro-3",
          snippet: "现代有机合成已达到极高的精密水平",
        },
      ],
      tool_trace: [{ round: 1, tool: "search_fulltext" }],
      rounds: 1,
      conversation_id: "mock-conv-reader",
    },
  ];
  return new ReadableStream({
    start(controller) {
      for (const event of events) {
        controller.enqueue(encoder.encode(`data: ${JSON.stringify(event)}\n\n`));
      }
      controller.close();
    },
  });
}

// 图书馆 agentic 问答。documentId 传入时限定单文档,不传全库检索。
// jobId 一并上传:服务端可反查 document,历史 run 更稳。
// conversationId 传入时走多轮;缺省服务端可 auto-create 并在 done.conversation_id 回传。
// 返回 { answer, citations, toolTrace, rounds, conversationId };失败抛 AiAskError。
export async function askLibraryAi({
  question = "",
  documentId = "",
  jobId = "",
  conversationId = "",
  /** 新 user 的 parent / regenerate 时的 user 消息 id */
  parentId = "",
  /** 重新生成:只追加 assistant 兄弟分支 */
  regenerate = false,
  userMessageId = "",
  assistantMessageId = "",
  onToolEvent = null,
  onAnswerDelta = null,
  signal = null,
  apiPrefix = API_PREFIX,
  fetchImpl = fetch,
  llmApiKey = "",
  llmBaseUrl = "",
  llmModel = "",
} = {}) {
  const trimmed = `${question}`.trim();
  if (!trimmed) {
    throw new AiAskError("请输入问题。", 400);
  }
  if (isMockMode()) {
    // 忠实模拟真实 SSE 流:tool 事件 → answer_delta 逐块 → done 带引用,
    // 让 markdown 渲染 / 流式 / 引用跳转三条链路都能在 mock 下端到端复现。
    return readAiAskStream(buildMockAskStream(trimmed), { onToolEvent, onAnswerDelta });
  }
  const payload: Record<string, any> = { question: trimmed, stream: true };
  const normalizedDocumentId = `${documentId || ""}`.trim();
  const normalizedJobId = `${jobId || ""}`.trim();
  const normalizedConversationId = `${conversationId || ""}`.trim();
  if (normalizedDocumentId) {
    payload.document_id = normalizedDocumentId;
  }
  if (normalizedJobId) {
    payload.job_id = normalizedJobId;
  }
  if (normalizedConversationId) {
    payload.conversation_id = normalizedConversationId;
  }
  const normalizedParentId = `${parentId || ""}`.trim();
  if (normalizedParentId) {
    payload.parent_id = normalizedParentId;
  }
  if (regenerate) {
    payload.regenerate = true;
  }
  const uid = `${userMessageId || ""}`.trim();
  const aid = `${assistantMessageId || ""}`.trim();
  if (uid) payload.user_message_id = uid;
  if (aid) payload.assistant_message_id = aid;
  // 按请求携带 LLM 凭据:必须非空,禁止带出空 Authorization: Bearer
  const key = `${llmApiKey || ""}`.trim();
  if (key) {
    // 若用户误把 "Bearer xxx" 整段粘进设置,剥掉前缀
    payload.llm_api_key = key.replace(/^Bearer\s+/i, "").trim();
  }
  if (`${llmBaseUrl || ""}`.trim()) {
    payload.llm_base_url = `${llmBaseUrl}`.trim();
  }
  if (`${llmModel || ""}`.trim()) {
    payload.llm_model = `${llmModel}`.trim();
  }
  const resp = await fetchImpl(buildApiEndpoint(apiPrefix, "ai/ask"), {
    method: "POST",
    headers: buildApiHeaders({ "Content-Type": "application/json" }),
    body: JSON.stringify(payload),
    signal,
  });
  if (!resp.ok) {
    if (resp.status === 502) {
      throw new AiAskError("AI 问答路由未响应(502),请检查后端 Rust API 是否在运行。", 502);
    }
    const message = await extractErrorMessage(resp);
    // 401：多半是服务入口 X-API-Key（runtime xApiKey），不是模型 Key
    if (resp.status === 401) {
      const hint = /X-API-Key|api key|invalid api key|Unauthorized/i.test(message)
        ? message
        : "服务鉴权失败：X-API-Key 无效或未配置（检查 runtime-config 的 xApiKey / 后端 auth 配置）。";
      throw new AiAskError(`${hint}(${resp.status})`, 401);
    }
    // 400 缺 LLM key：明确指向「设置 → 凭据」的模型 API Key
    if (resp.status === 400 && /LLM|模型\s*API\s*Key|api key/i.test(message)) {
      throw new AiAskError(
        message.includes("凭据") || message.includes("设置")
          ? `${message}(${resp.status})`
          : `缺少模型 API Key：请到设置 → API 设置填写后再提问。(${resp.status})`,
        400,
      );
    }
    throw new AiAskError(`${message || "AI 问答请求失败,请稍后重试。"}(${resp.status})`, resp.status);
  }
  const contentType = `${resp.headers?.get?.("content-type") || ""}`.toLowerCase();
  if (contentType.includes("application/json")) {
    // 后端未按流式返回时,兼容一次性 JSON envelope
    return normalizeDonePayload(unwrapEnvelope(await resp.json()));
  }
  return readAiAskStream(resp.body, { onToolEvent, onAnswerDelta });
}

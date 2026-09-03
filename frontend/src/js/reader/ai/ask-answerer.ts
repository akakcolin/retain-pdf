import { API_PREFIX } from "../../config/api-constants.js";
import { askLibraryAi } from "../../api/ai.js";
import { fetchDocumentByJobId } from "../../api/documents.js";
import {
  hasModelApiKey,
  MISSING_MODEL_API_KEY_MESSAGE,
  resolveReaderAiConfig,
} from "./config.js";
import {
  clearStoredConversationId,
  loadStoredConversationId,
  saveStoredConversationId,
} from "./conversation-store.js";

// 阅读器问答的 agentic 应答器:走 /api/v1/ai/ask(带 SSE 过程事件与可跳转引用)。
// document_id 经后端 GET /documents?job_id= 直查(含历史 run),查不到时 fail closed。
// conversation_id 本地粘性 + 服务端 auto-create / done 回传,实现多轮。

const QUOTE_MAX_LENGTH = 240;

function clipQuoteText(text = "", maxLength = QUOTE_MAX_LENGTH) {
  const normalized = `${text}`.replace(/\s+/g, " ").trim();
  if (normalized.length <= maxLength) {
    return normalized;
  }
  return `${normalized.slice(0, maxLength).trim()}…`;
}

export function buildScopedQuestion({ question = "", scope = "document", context = null, resolveQuote = null } = {}) {
  const trimmed = `${question}`.trim();
  if (!trimmed) {
    return "";
  }
  if (scope === "selection") {
    const quote = typeof resolveQuote === "function" && context ? resolveQuote(context) : null;
    const quoteText = clipQuoteText(quote?.quoteText || "");
    if (quoteText) {
      return `（针对选中的原文片段：「${quoteText}」）${trimmed}`;
    }
    if (context?.page) {
      return `（针对第 ${Number(context.page)} 页的选区内容）${trimmed}`;
    }
  }
  if (scope === "page" && context?.page) {
    return `（当前第 ${Number(context.page)} 页）${trimmed}`;
  }
  return trimmed;
}

export function createReaderAskAnswerer({
  jobId = "",
  apiPrefix = API_PREFIX,
  ask = askLibraryAi,
  documentByJobId = fetchDocumentByJobId,
  resolveQuote = null,
  // 前端凭据设置里的模型 API Key(与翻译流程同源),按请求随问答一起传给后端
  llmConfig = resolveReaderAiConfig,
} = {}) {
  let documentIdPromise = null;
  // 内存优先,localStorage 兜底(跨刷新)
  let conversationId = loadStoredConversationId({ jobId });

  function resolveDocumentId() {
    if (!documentIdPromise) {
      documentIdPromise = (async () => {
        try {
          const document = await documentByJobId(apiPrefix, jobId) as { document_id?: string } | null | undefined;
          return `${document?.document_id || ""}`.trim();
        } catch (_err) {
          return "";
        }
      })();
    }
    return documentIdPromise;
  }

  function rememberConversationId(nextId: string, documentId = "") {
    const id = `${nextId || ""}`.trim();
    if (!id) {
      return;
    }
    conversationId = id;
    saveStoredConversationId({ jobId, documentId }, id);
  }

  async function answer({
    question = "",
    scope = "document",
    context = null,
    onToolEvent = null,
    onAnswerDelta = null,
    parentId = "",
    regenerate = false,
    userMessageId = "",
    assistantMessageId = "",
    /** 取消信号：中止 SSE；aborted 后不回写会话粘性（防旧流污染新会话） */
    signal = null,
  } = {}) {
    const scopedQuestion = buildScopedQuestion({ context, question, resolveQuote, scope });
    if (!scopedQuestion) {
      throw new Error("请输入问题。");
    }
    // 凭据门禁必须在任何网络请求之前：否则用户会先看到「检索中」再报缺 Key
    const config = typeof llmConfig === "function" ? llmConfig() : (llmConfig || {});
    const apiKey = `${config.apiKey || ""}`.trim();
    if (!apiKey) {
      throw new Error(MISSING_MODEL_API_KEY_MESSAGE);
    }
    const documentId = await resolveDocumentId();
    // 阅读器默认整本问答:反查不到文档时 fail closed,禁止静默变全库检索
    if (!documentId && `${jobId || ""}`.trim()) {
      throw new Error("无法关联当前文档，暂不能做整本问答。请确认任务已绑定文档后重试。");
    }
    // document 解析后若 storage 里只有 job key,再补写一份 doc key
    if (!conversationId) {
      conversationId = loadStoredConversationId({ jobId, documentId });
    }
    const result = await ask({
      question: scopedQuestion,
      documentId,
      jobId: `${jobId || ""}`.trim(),
      conversationId,
      parentId: `${parentId || ""}`.trim(),
      regenerate: Boolean(regenerate),
      userMessageId: `${userMessageId || ""}`.trim(),
      assistantMessageId: `${assistantMessageId || ""}`.trim(),
      onToolEvent,
      onAnswerDelta,
      llmApiKey: apiKey,
      llmBaseUrl: `${config.baseUrl || ""}`.trim(),
      llmModel: `${config.model || ""}`.trim(),
      signal,
    });
    const nextConversationId = `${(result as { conversationId?: string })?.conversationId || ""}`.trim();
    // aborted 的旧流禁止回写粘性：否则"生成中切会话"会被 done 事件把
    // conversation_id 拽回旧会话，下一问落错线程（审计 P0-4）
    if (nextConversationId && !(signal as AbortSignal | null)?.aborted) {
      rememberConversationId(nextConversationId, documentId);
    }
    return {
      ...result,
      conversationId: nextConversationId || conversationId,
      scope,
    };
  }

  return {
    answer,
    getConversationId: () => conversationId,
    setConversationId: (nextId: string, documentId = "") => {
      rememberConversationId(nextId, documentId);
    },
    clearConversationId: (documentId = "") => {
      conversationId = "";
      clearStoredConversationId({ jobId, documentId });
      if (documentId) {
        clearStoredConversationId({ documentId });
      }
      clearStoredConversationId({ jobId });
    },
    getDocumentId: () => resolveDocumentId(),
    ensureLoaded: async (_jobId?: string) => {
      // 预热 document_id;失败在 answer 时再报错
      // （与 markdown-answerer 的 ensureLoaded(jobId) 保持同型，参数可忽略）
      const documentId = await resolveDocumentId();
      return Boolean(documentId);
    },
  };
}

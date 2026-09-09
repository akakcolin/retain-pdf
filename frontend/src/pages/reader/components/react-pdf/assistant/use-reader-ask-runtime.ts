// assistant-ui ExternalStore：进度/正文分离 + 消息分支树 + 本地快照。
// 树/消息/citations 纯函数已拆到 ./thread-tree.ts（评审 P1-5）；本文件只做编排。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ExportedMessageRepository,
  useExternalStoreRuntime,
  type AppendMessage,
  type ThreadMessage,
} from "@assistant-ui/react";
import { describeToolEvent } from "../../../legacy/ai/answer-view.js";
import {
  armReaderAiClickShield,
  clearThreadBranchSnapshot,
  createReaderAskAnswerer,
  createReaderMarkdownAnswerer,
  defaultReaderDataPort,
  getConversation,
  listConversations,
  loadStoredConversationId,
  loadThreadBranchSnapshot,
  lockReaderAiNavigation,
  messagesToBranchItems,
  patchConversation,
  sanitizeAssistantAnswer,
  saveThreadBranchSnapshot,
  type AiCitationLike,
  type ConversationRecord,
} from "../../../external.js";
import {
  findMessage,
  makeReaderAskId as makeId,
  normalizeAiCitations as normalizeCitations,
  READER_ASK_SUGGESTIONS as SUGGESTIONS,
  shouldFallbackToLocal,
  snapshotFromTree,
  textFromAppend,
  toThreadMessageLike,
  treeFromSnapshot,
  treeItemsFromBranchItems,
  visibleMessages,
  type ReaderAskStoreMessage,
  type ReaderAskTreeItem,
} from "./thread-tree.js";
import { createReaderAskSessionOps } from "./session-operations.js";

type TreeItem = ReaderAskTreeItem;

export type ReaderAskSessionSummary = {
  id: string;
  title: string;
  updatedAt: string;
  messageCount: number;
  active: boolean;
};

export function useReaderAskRuntime(options: {
  jobId: string;
  enabled: boolean;
  /** 检索范围：document=仅当前文档；library=全库 */
  retrievalScope?: "document" | "library";
}) {
  const { jobId, enabled, retrievalScope = "document" } = options;
  const [items, setItems] = useState<TreeItem[]>([]);
  const [headId, setHeadId] = useState<string | null>(null);
  const [isRunning, setIsRunning] = useState(false);
  const [sessions, setSessions] = useState<ConversationRecord[]>([]);
  const [activeConversationId, setActiveConversationId] = useState("");
  const [sessionBusy, setSessionBusy] = useState(false);
  const [sessionError, setSessionError] = useState("");
  const runningRef = useRef(false);
  // 贯穿单次 runAssistant 的取消把手：onCancel/切会话/新会话/连发前 abort。
  // 审计 P0-2/3/4 的共同根因就是此前没有它——停止不断流、完成流复活已取消
  // 消息、旧流回写会话粘性，全靠这一个 controller 消掉。
  const runAbortRef = useRef<AbortController | null>(null);
  const itemsRef = useRef(items);
  const headIdRef = useRef(headId);
  const activeConversationIdRef = useRef(activeConversationId);
  const streamRafRef = useRef<number | null>(null);
  const pendingContentRef = useRef("");
  const streamAssistantIdRef = useRef("");
  /** 流式正文旁路（不依赖 aui Parts / status），保证 token 到即刷 */
  const streamContentRef = useRef<Record<string, string>>({});
  const [streamEpoch, setStreamEpoch] = useState(0);
  const [streamingAssistantId, setStreamingAssistantId] = useState("");
  const answerStartedRef = useRef(false);
  const persistReadyRef = useRef(false);
  const lastJobRef = useRef("");
  const documentIdRef = useRef("");
  const switchTokenRef = useRef(0);

  itemsRef.current = items;
  headIdRef.current = headId;
  activeConversationIdRef.current = activeConversationId;

  const remoteAnswerer = useMemo(() => {
    if (!enabled || !jobId) return null;
    return createReaderAskAnswerer({ jobId });
  }, [enabled, jobId]);

  const refreshSessions = useCallback(async (documentId = "") => {
    const doc = `${documentId || documentIdRef.current || ""}`.trim();
    if (!doc) {
      setSessions([]);
      return;
    }
    try {
      const res = await listConversations({ document_id: doc, limit: 50 });
      setSessions(res.conversations || []);
    } catch {
      // 列表失败不挡主流程
    }
  }, []);

  const applyConversationTree = useCallback((
    branchItems: ReturnType<typeof messagesToBranchItems>,
    head?: string | null,
  ) => {
    const tree = treeItemsFromBranchItems(branchItems);
    setItems(tree);
    setHeadId(
      `${head || ""}`.trim()
      || tree[tree.length - 1]?.message.id
      || null,
    );
  }, []);

  // job 切换 / 面板打开：拉会话列表 + hydrate 消息树
  // 注意：面板关闭时 enabled=false 也会跑 effect；不能用 lastJobRef 在 enabled 翻转时直接 return，
  // 否则「打开面板」永远不会 refreshSessions（用户只能新对话后才看到列表）。
  useEffect(() => {
    if (!jobId) {
      setItems([]);
      setHeadId(null);
      setSessions([]);
      setActiveConversationId("");
      activeConversationIdRef.current = "";
      lastJobRef.current = "";
      documentIdRef.current = "";
      persistReadyRef.current = false;
      return;
    }

    const jobChanged = lastJobRef.current !== jobId;
    if (jobChanged) {
      lastJobRef.current = jobId;
      persistReadyRef.current = false;
      runningRef.current = false;
      setIsRunning(false);
      setItems([]);
      setHeadId(null);
      setSessions([]);
      setActiveConversationId("");
      activeConversationIdRef.current = "";
      documentIdRef.current = "";
    }

    // 面板未打开：只记 job，等 enabled 再拉网
    if (!enabled || !remoteAnswerer) return;

    let cancelled = false;
    void (async () => {
      // 1) 解析 document_id（列表按文档过滤）
      let docId = `${documentIdRef.current || ""}`.trim();
      if (!docId) {
        try {
          docId = `${(await remoteAnswerer.getDocumentId?.()) || ""}`.trim();
        } catch {
          docId = "";
        }
        if (docId) documentIdRef.current = docId;
      }

      // 2) 始终刷新会话列表（打开面板 / 切 job 都要）
      if (!cancelled && docId) {
        await refreshSessions(docId);
      }

      // 3) 消息树：job 刚切换，或当前还是空树时再 hydrate
      const needHydrate = jobChanged || !itemsRef.current.length;
      if (!needHydrate || cancelled) {
        if (!cancelled) persistReadyRef.current = true;
        return;
      }

      const convId =
        loadStoredConversationId({ jobId, documentId: docId })
        || `${remoteAnswerer.getConversationId?.() || ""}`.trim();

      if (convId) {
        setActiveConversationId(convId);
        activeConversationIdRef.current = convId;
        remoteAnswerer.setConversationId?.(convId, docId);
        try {
          const detail = await getConversation(convId);
          if (cancelled) return;
          const branchItems = messagesToBranchItems(detail.messages || []);
          if (branchItems.length) {
            applyConversationTree(branchItems, detail.head_id);
            requestAnimationFrame(() => {
              if (!cancelled) persistReadyRef.current = true;
            });
            return;
          }
        } catch {
          // 网络/404 → 本地快照
        }
      }

      // 无粘性会话：若列表里已有对话，自动挂最近一条，便于直接切换
      if (!cancelled && docId) {
        try {
          const listed = await listConversations({ document_id: docId, limit: 50 });
          if (cancelled) return;
          const rows = listed.conversations || [];
          setSessions(rows);
          const latest = rows[0];
          if (latest?.conversation_id) {
            const latestId = latest.conversation_id;
            setActiveConversationId(latestId);
            activeConversationIdRef.current = latestId;
            remoteAnswerer.setConversationId?.(latestId, docId);
            try {
              const detail = await getConversation(latestId);
              if (cancelled) return;
              applyConversationTree(
                messagesToBranchItems(detail.messages || []),
                detail.head_id,
              );
              requestAnimationFrame(() => {
                if (!cancelled) persistReadyRef.current = true;
              });
              return;
            } catch {
              // fall through to snapshot
            }
          }
        } catch {
          // ignore list failure
        }
      }

      if (cancelled) return;
      const saved = loadThreadBranchSnapshot(jobId, convId);
      if (saved?.items.length) {
        const tree = treeFromSnapshot(saved);
        setItems(tree.items);
        setHeadId(tree.headId);
      } else {
        setItems([]);
        setHeadId(null);
      }
      requestAnimationFrame(() => {
        if (!cancelled) persistReadyRef.current = true;
      });
    })();

    return () => {
      cancelled = true;
    };
  }, [jobId, enabled, remoteAnswerer, refreshSessions, applyConversationTree]);

  // 防抖持久化全量树（按会话隔离）
  useEffect(() => {
    if (!jobId || !persistReadyRef.current) return;
    const convId = activeConversationId;
    const timer = window.setTimeout(() => {
      if (!items.length) {
        clearThreadBranchSnapshot(jobId, convId);
        return;
      }
      saveThreadBranchSnapshot(jobId, snapshotFromTree(items, headId), convId);
    }, 280);
    return () => window.clearTimeout(timer);
  }, [jobId, items, headId, activeConversationId]);

  const localAnswerer = useMemo(() => {
    if (!enabled || !jobId) return null;
    return createReaderMarkdownAnswerer({
      loadMarkdownPayload: defaultReaderDataPort.loadMarkdownPayload,
    });
  }, [enabled, jobId]);

  const messages = useMemo(
    () => visibleMessages(items, headId),
    [items, headId],
  );

  const citationsByMessageId = useMemo(() => {
    const map: Record<string, AiCitationLike[]> = {};
    for (const item of items) {
      const m = item.message;
      if (m.role === "assistant" && m.citations?.length) {
        map[m.id] = m.citations as AiCitationLike[];
      }
    }
    return map;
  }, [items]);

  const progressByMessageId = useMemo(() => {
    const map: Record<string, string> = {};
    for (const item of items) {
      const m = item.message;
      if (m.role === "assistant" && m.progress) {
        map[m.id] = m.progress;
      }
    }
    return map;
  }, [items]);

  /** 流式正文旁路：store + 进行中的 streamContent（streamEpoch 强制刷新） */
  const contentByMessageId = useMemo(() => {
    const map: Record<string, string> = {};
    for (const item of items) {
      const m = item.message;
      if (m.content) map[m.id] = m.content;
    }
    // 覆盖为最新流式缓冲（可能比 items 里多半帧）
    for (const [id, text] of Object.entries(streamContentRef.current)) {
      if (text) map[id] = text;
    }
    void streamEpoch;
    return map;
  }, [items, streamEpoch]);

  const messageRepository = useMemo(
    () =>
      ExportedMessageRepository.fromBranchableArray(
        items.map((item) => ({
          message: toThreadMessageLike(item.message),
          parentId: item.parentId,
        })),
        { headId },
      ),
    [items, headId],
  );

  const patchAssistant = useCallback((
    assistantId: string,
    patch: Partial<ReaderAskStoreMessage>,
  ) => {
    setItems((prev) =>
      prev.map((item) =>
        item.message.id === assistantId
          ? { ...item, message: { ...item.message, ...patch } }
          : item,
      ),
    );
  }, []);

  // 流式清洗降频：sanitize 从"每 token 一次"移到"每帧一次"（rAF flush 内），
  // 长答案不再对全文反复跑 5 条正则（审计 P1-7 O(n²)）。
  // 语义不变：清洗后为空则回退原文，保证可见增量。
  const sanitizeStreamText = useCallback((raw: string) => {
    const cleaned = sanitizeAssistantAnswer(raw || "", []);
    return cleaned.trim() ? cleaned : `${raw || ""}`;
  }, []);

  const scheduleAnswerText = useCallback((assistantId: string, text: string) => {
    const next = `${text || ""}`;
    pendingContentRef.current = next;
    answerStartedRef.current = true;
    streamAssistantIdRef.current = assistantId;

    // 首包：同步上屏（旁路 + store），避免等 rAF / aui status
    const cur = itemsRef.current.find((i) => i.message.id === assistantId);
    if (!cur?.message.content?.trim()) {
      if (streamRafRef.current != null) {
        cancelAnimationFrame(streamRafRef.current);
        streamRafRef.current = null;
      }
      const shown = sanitizeStreamText(next);
      streamContentRef.current[assistantId] = shown;
      setStreamEpoch((n) => n + 1);
      patchAssistant(assistantId, {
        content: shown,
        progress: "",
        status: { type: "running" },
      });
      return;
    }

    // 后续：每帧最多 flush 一次旁路；store 也更新以免切消息丢字
    if (streamRafRef.current != null) return;
    streamRafRef.current = requestAnimationFrame(() => {
      streamRafRef.current = null;
      // 陈旧守卫：取消/连发后 pendingContentRef 已属于新流，
      // 旧 rAF 不许把它写进旧气泡（审计 P0-3 串流）
      if (streamAssistantIdRef.current !== assistantId) return;
      const latest = sanitizeStreamText(pendingContentRef.current);
      streamContentRef.current[assistantId] = latest;
      setStreamEpoch((n) => n + 1);
      patchAssistant(assistantId, {
        content: latest,
        progress: "",
        status: { type: "running" },
      });
    });
  }, [patchAssistant, sanitizeStreamText]);

  // 卸载（关浮窗/离开页面）时断掉在飞请求，不留幽灵 SSE
  useEffect(() => () => {
    runAbortRef.current?.abort();
    runAbortRef.current = null;
  }, []);

  const runAssistant = useCallback(async (
    assistantId: string,
    question: string,
    opts: {
      parentId?: string | null;
      userMessageId?: string;
      regenerate?: boolean;
    } = {},
  ) => {
    if (!remoteAnswerer && !localAnswerer) {
      patchAssistant(assistantId, {
        content: "问答暂不可用：请确认已打开任务阅读器。",
        progress: "",
        status: { type: "incomplete", reason: "error" },
      });
      return;
    }

    // 连发保护：终止上一在飞请求，再开新 controller
    runAbortRef.current?.abort();
    const controller = new AbortController();
    runAbortRef.current = controller;

    runningRef.current = true;
    answerStartedRef.current = false;
    pendingContentRef.current = "";
    streamAssistantIdRef.current = assistantId;
    streamContentRef.current[assistantId] = "";
    setStreamingAssistantId(assistantId);
    setStreamEpoch((n) => n + 1);
    setIsRunning(true);
    // 占位 running，确保 UI 走流式路径
    patchAssistant(assistantId, {
      content: "",
      progress: "正在检索文档…",
      status: { type: "running" },
    });

    try {
      await remoteAnswerer?.ensureLoaded?.(jobId);
      let usedFallback = false;
      let result: { answer?: string; citations?: unknown[] };
      try {
        result = await remoteAnswerer!.answer({
          question,
          scope: "document",
          retrievalScope,
          parentId: opts.parentId || "",
          regenerate: Boolean(opts.regenerate),
          userMessageId: opts.userMessageId || "",
          assistantMessageId: assistantId,
          onToolEvent: (event) => {
            const line = describeToolEvent(event);
            if (!line || answerStartedRef.current) return;
            patchAssistant(assistantId, {
              progress: line,
              status: { type: "running" },
            });
          },
          onAnswerDelta: (fullText: string) => {
            if (controller.signal.aborted) return;
            // 原文直接入队，清洗在 scheduleAnswerText 的帧级 flush 里做
            //（每 token 全文 sanitize 是 O(n²)，审计 P1-7）
            if (fullText) scheduleAnswerText(assistantId, fullText);
          },
          signal: controller.signal,
        });
      } catch (error) {
        // 主动取消：不降级、不报错，气泡状态已由 onCancel 定格
        if (controller.signal.aborted) return;
        // 全库模式不降级：本地检索是文档级，会答非所问
        if (retrievalScope === "library" || !localAnswerer || !shouldFallbackToLocal(error)) {
          throw error;
        }
        usedFallback = true;
        if (!answerStartedRef.current) {
          patchAssistant(assistantId, {
            progress: "在线服务暂不可用，改用本地检索…",
            status: { type: "running" },
          });
        }
        await localAnswerer.ensureLoaded?.(jobId);
        result = await localAnswerer.answer({ question, scope: "document" });
      }

      // 完成前最后一道闸：已取消的运行禁止用完整答案覆盖"已取消"气泡
      if (controller.signal.aborted) return;
      if (streamRafRef.current != null) {
        cancelAnimationFrame(streamRafRef.current);
        streamRafRef.current = null;
      }
      const citations = normalizeCitations(result?.citations);
      let answer = sanitizeAssistantAnswer(
        `${result?.answer || pendingContentRef.current || ""}`.trim() || "没有找到可用回答。",
        citations,
      );
      if (usedFallback) {
        answer = `${answer}\n\n_在线服务暂不可用，以上来自本地文档检索。_`;
      }
      if ((result as { persisted?: boolean })?.persisted === false) {
        // 审计 C2:回写失败不再静默——用户至少知道该复制保存
        answer = `${answer}\n\n_⚠️ 本轮回答未能写入历史记录（存储暂时不可用），刷新后可能丢失。_`;
      }
      delete streamContentRef.current[assistantId];
      streamAssistantIdRef.current = "";
      setStreamingAssistantId("");
      setStreamEpoch((n) => n + 1);
      patchAssistant(assistantId, {
        content: answer,
        progress: "",
        citations,
        status: { type: "complete", reason: "stop" },
      });
    } catch (error) {
      // 主动取消抛出的 AbortError 不算失败，气泡已由 onCancel 定格
      if (controller.signal.aborted) return;
      if (streamRafRef.current != null) {
        cancelAnimationFrame(streamRafRef.current);
        streamRafRef.current = null;
      }
      delete streamContentRef.current[assistantId];
      streamAssistantIdRef.current = "";
      setStreamingAssistantId("");
      setStreamEpoch((n) => n + 1);
      const msg = error instanceof Error ? error.message : "生成回答失败，请重试。";
      patchAssistant(assistantId, {
        content: msg,
        progress: "",
        citations: [],
        status: { type: "incomplete", reason: "error" },
      });
    } finally {
      // 只有"仍是当前运行"才复位全局标志：旧 run 的 finally 不许踩新 run
      if (runAbortRef.current === controller) {
        runAbortRef.current = null;
        runningRef.current = false;
        setIsRunning(false);
      }
    }
  }, [jobId, localAnswerer, patchAssistant, remoteAnswerer, retrievalScope, scheduleAnswerText]);

  const onNew = useCallback(async (message: AppendMessage) => {
    if (runningRef.current) return;
    const question = textFromAppend(message);
    if (!question) return;

    // 优先 append 自带的 parentId：从某条答案「分支」时 parent = 该 assistant id
    const parentId = message.parentId ?? headIdRef.current;
    const userId = makeId("u");
    const assistantId = makeId("a");

    setItems((prev) => [
      ...prev,
      { parentId, message: { id: userId, role: "user", content: question } },
      {
        parentId: userId,
        message: {
          id: assistantId,
          role: "assistant",
          content: "",
          progress: "正在检索文档…",
          status: { type: "running" },
          citations: [],
        },
      },
    ]);
    setHeadId(assistantId);
    await runAssistant(assistantId, question, {
      parentId,
      userMessageId: userId,
      regenerate: false,
    });
  }, [runAssistant]);

  /** 重新生成：parentId 为被替换助手消息的父节点（通常是 user）。 */
  const onReload = useCallback(async (parentId: string | null) => {
    if (runningRef.current) return;
    const tree = itemsRef.current;
    const parent = parentId ? findMessage(tree, parentId) : null;
    let question = "";
    let userId = parentId;
    if (parent?.role === "user") {
      question = parent.content.trim();
    } else {
      // 兜底：沿可见路径找最后一个 user
      const path = visibleMessages(tree, parentId ?? headIdRef.current);
      for (let i = path.length - 1; i >= 0; i -= 1) {
        if (path[i].role === "user") {
          question = path[i].content.trim();
          userId = path[i].id;
          break;
        }
      }
    }
    if (!question) return;

    const assistantId = makeId("a");
    const branchParent = userId || parentId;
    setItems((prev) => [
      ...prev,
      {
        parentId: branchParent,
        message: {
          id: assistantId,
          role: "assistant",
          content: "",
          progress: "正在重新生成…",
          status: { type: "running" },
          citations: [],
        },
      },
    ]);
    setHeadId(assistantId);
    await runAssistant(assistantId, question, {
      parentId: branchParent,
      regenerate: true,
    });
  }, [runAssistant]);

  /** 编辑用户消息：在 parentId 下长出新 user 兄弟分支并重跑。 */
  const onEdit = useCallback(async (message: AppendMessage) => {
    if (runningRef.current) return;
    const question = textFromAppend(message);
    if (!question) return;

    const parentId = message.parentId ?? null;
    const userId = makeId("u");
    const assistantId = makeId("a");

    setItems((prev) => [
      ...prev,
      { parentId, message: { id: userId, role: "user", content: question } },
      {
        parentId: userId,
        message: {
          id: assistantId,
          role: "assistant",
          content: "",
          progress: "正在检索文档…",
          status: { type: "running" },
          citations: [],
        },
      },
    ]);
    setHeadId(assistantId);
    await runAssistant(assistantId, question, {
      parentId,
      userMessageId: userId,
      regenerate: false,
    });
  }, [runAssistant]);

  const onCancel = useCallback(async () => {
    // 真取消：断 SSE（省网络/token），并掐灭 pending rAF 防止把状态打回 running
    runAbortRef.current?.abort();
    runAbortRef.current = null;
    if (streamRafRef.current != null) {
      cancelAnimationFrame(streamRafRef.current);
      streamRafRef.current = null;
    }
    streamAssistantIdRef.current = "";
    setStreamingAssistantId("");
    setStreamEpoch((n) => n + 1);
    runningRef.current = false;
    setIsRunning(false);
    setItems((prev) =>
      prev.map((item) =>
        item.message.status?.type === "running"
          ? {
            ...item,
            message: {
              ...item.message,
              status: { type: "incomplete", reason: "cancelled" as const },
              progress: "",
              content: item.message.content.trim() || "已取消",
            },
          }
          : item,
      ),
    );
  }, []);

  /** 分支切换：runtime 传入当前可见 ThreadMessage 路径，只改 head。 */
  const setMessages = useCallback((next: readonly ThreadMessage[]) => {
    const last = next[next.length - 1];
    setHeadId(last?.id ?? null);
  }, []);

  const unstable_onBranchChange = useCallback((
    event: { headId: string | null },
  ) => {
    setHeadId(event.headId);
    // 同步服务端 head，跨端/刷新保持可见分支
    const convId =
      remoteAnswerer?.getConversationId?.()
      || loadStoredConversationId({ jobId });
    const head = `${event.headId || ""}`.trim();
    if (convId && head) {
      void patchConversation(convId, { head_id: head }).catch(() => {
        // 离线/404 忽略，本地树仍可用
      });
    }
  }, [jobId, remoteAnswerer]);

  const onImport = useCallback((imported: readonly ThreadMessage[]) => {
    const last = imported[imported.length - 1];
    if (last?.id) setHeadId(last.id);
  }, []);

  const sessionBusyRef = useRef(sessionBusy);
  sessionBusyRef.current = sessionBusy;
  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;

  // 会话操作（新建/切换/删除/重命名/分支）拆到 ./session-operations.ts；
  // 状态经 getter/setter、可变引用经 refs 袋注入，行为与内联实现一致。
  const {
    branchFromAnswer,
    newSession,
    removeSession,
    renameSession,
    switchSession,
  } = useMemo(() => createReaderAskSessionOps({
    jobId,
    remoteAnswerer,
    getSessionBusy: () => sessionBusyRef.current,
    getSessions: () => sessionsRef.current,
    setSessionBusy,
    setSessionError,
    setIsRunning,
    setSessions,
    setItems,
    setHeadId,
    setActiveConversationId,
    refreshSessions,
    applyConversationTree,
    refs: {
      runAbort: runAbortRef,
      running: runningRef,
      switchToken: switchTokenRef,
      documentId: documentIdRef,
      activeConversationId: activeConversationIdRef,
      items: itemsRef,
      headId: headIdRef,
    },
  }), [applyConversationTree, jobId, refreshSessions, remoteAnswerer]);

  // 问答完成后刷新会话列表标题/排序
  const prevRunning = useRef(false);
  useEffect(() => {
    if (prevRunning.current && !isRunning) {
      const docId = documentIdRef.current;
      if (docId) void refreshSessions(docId);
      const id = remoteAnswerer?.getConversationId?.() || "";
      if (id) setActiveConversationId(id);
    }
    prevRunning.current = isRunning;
  }, [isRunning, remoteAnswerer, refreshSessions]);

  const runtime = useExternalStoreRuntime({
    isRunning,
    // 切会话/分支时不要整线程 isDisabled（会闪禁用态像刷新）；按钮侧用 branchBusy 锁
    isDisabled: !enabled,
    messageRepository,
    setMessages,
    unstable_onBranchChange,
    onNew,
    onReload,
    onEdit,
    onCancel,
    onImport,
    suggestions: messages.length === 0 ? SUGGESTIONS : [],
  });

  const sessionSummaries: ReaderAskSessionSummary[] = useMemo(() => {
    const active = activeConversationId
      || remoteAnswerer?.getConversationId?.()
      || "";
    return (sessions || []).map((s) => ({
      id: s.conversation_id,
      title: `${s.title || ""}`.trim() || "未命名对话",
      updatedAt: s.updated_at || "",
      messageCount: Number(s.message_count) || 0,
      active: s.conversation_id === active,
    }));
  }, [sessions, activeConversationId, remoteAnswerer]);

  return {
    runtime,
    citationsByMessageId,
    progressByMessageId,
    contentByMessageId,
    streamingAssistantId,
    isRunning,
    messages,
    sessions: sessionSummaries,
    activeConversationId: activeConversationId
      || remoteAnswerer?.getConversationId?.()
      || "",
    sessionBusy,
    sessionError,
    newSession,
    switchSession,
    removeSession,
    renameSession,
    branchFromAnswer,
  };
}

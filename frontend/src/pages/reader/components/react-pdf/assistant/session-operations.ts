// 阅读器 AI 问答的会话操作：新建/切换/删除/重命名/分支。
// 从 use-reader-ask-runtime.ts 拆出（评审 P1-5）为纯工厂：React 状态经
// getter/setter 注入，可变引用经 refs 袋注入，API 走 external.js。
// 行为与拆分前逐行一致；时序（40/80ms 隔离、switchToken 防串、rAF 滚动）不变。

import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import {
  armReaderAiClickShield,
  clearThreadBranchSnapshot,
  deleteConversation,
  forkConversationFromPath,
  getConversation,
  listConversations,
  lockReaderAiNavigation,
  messagesToBranchItems,
  nextForkConversationTitle,
  patchConversation,
  saveThreadBranchSnapshot,
  type ConversationRecord,
  type ThreadBranchItem,
  createReaderAskAnswerer,
} from "../../../external.js";
import {
  pathForBranch,
  snapshotFromTree,
  treeItemsFromBranchItems,
  type ReaderAskTreeItem,
} from "./thread-tree.js";

// ── 时序常数（评审 P2-7；工程经验值，改动前先读注释）────────────────
// 新建/分支会话前，等被 abort 的旧流尾事件与 React commit 落定的隔离窗。
// 失效后果：太短→旧流 done 串扰新会话；太长→「点了没反应」。
const SESSION_SETTLE_MS = 40;
// 切换会话的隔离窗：比新建长——UI 选中态已先切，且要抑制自动滚动。
const SESSION_SWITCH_SETTLE_MS = 80;
// 切会话后手动滚到底，期间抑制自动滚动，该延迟后释放抑制。
const AUTOSCROLL_SUPPRESS_RELEASE_MS = 200;
// 新建会话的点击盾/导航锁：覆盖 refreshSessions 网络往返的典型耗时。
const NEW_SESSION_SHIELD_MS = 900;
// 切换会话的点击盾/导航锁：链路更长（清场 + 加载 + 滚动）。
const SWITCH_SESSION_SHIELD_MS = 1200;
// 切换完成后的短盾：挡住用户立刻连点引用/会话项。
const POST_SWITCH_SHIELD_MS = 350;

export type ReaderAskAnswerer = ReturnType<typeof createReaderAskAnswerer>;

export interface ReaderAskSessionOpsRefs {
  /** 贯穿单次 runAssistant 的取消把手（与流式运行共享）。 */
  runAbort: MutableRefObject<AbortController | null>;
  running: MutableRefObject<boolean>;
  /** 会话切换令牌：旧异步收尾不许踩新会话。 */
  switchToken: MutableRefObject<number>;
  documentId: MutableRefObject<string>;
  activeConversationId: MutableRefObject<string>;
  items: MutableRefObject<ReaderAskTreeItem[]>;
  headId: MutableRefObject<string | null>;
}

export interface ReaderAskSessionOpsDeps {
  jobId: string;
  remoteAnswerer: ReaderAskAnswerer | null;
  getSessionBusy: () => boolean;
  getSessions: () => ConversationRecord[];
  setSessionBusy: (busy: boolean) => void;
  setSessionError: (message: string) => void;
  setIsRunning: (running: boolean) => void;
  setSessions: Dispatch<SetStateAction<ConversationRecord[]>>;
  setItems: (items: ReaderAskTreeItem[]) => void;
  setHeadId: (headId: string | null) => void;
  setActiveConversationId: (id: string) => void;
  refreshSessions: (documentId?: string) => Promise<void> | void;
  applyConversationTree: (
    branchItems: ReturnType<typeof messagesToBranchItems>,
    head?: string | null,
  ) => void;
  refs: ReaderAskSessionOpsRefs;
}

export function createReaderAskSessionOps({
  jobId,
  remoteAnswerer,
  getSessionBusy,
  getSessions,
  setSessionBusy,
  setSessionError,
  setIsRunning,
  setSessions,
  setItems,
  setHeadId,
  setActiveConversationId,
  refreshSessions,
  applyConversationTree,
  refs,
}: ReaderAskSessionOpsDeps) {
  const {
    runAbort: runAbortRef,
    running: runningRef,
    switchToken: switchTokenRef,
    documentId: documentIdRef,
    activeConversationId: activeConversationIdRef,
    items: itemsRef,
    headId: headIdRef,
  } = refs;

  /** 新对话窗口：清空气泡，下次 ask 会 auto-create 新 conversation。 */
  async function newSession() {
    if (getSessionBusy()) return;
    // 生成中也允许开新窗：真 abort 在飞请求，防旧流写进新窗口
    runAbortRef.current?.abort();
    runAbortRef.current = null;
    runningRef.current = false;
    setIsRunning(false);
    armReaderAiClickShield(NEW_SESSION_SHIELD_MS);
    lockReaderAiNavigation(NEW_SESSION_SHIELD_MS);
    setSessionBusy(true);
    setSessionError("");
    const token = ++switchTokenRef.current;
    try {
      await new Promise<void>((r) => {
        window.setTimeout(r, SESSION_SETTLE_MS);
      });
      if (token !== switchTokenRef.current) return;
      const docId = documentIdRef.current
        || `${(await remoteAnswerer?.getDocumentId?.()) || ""}`.trim();
      documentIdRef.current = docId;
      remoteAnswerer?.clearConversationId?.(docId);
      setActiveConversationId("");
      activeConversationIdRef.current = "";
      setItems([]);
      setHeadId(null);
      clearThreadBranchSnapshot(jobId);
      if (docId) await refreshSessions(docId);
    } catch (error) {
      console.warn("[reader-ai] new session failed", error);
      setSessionError("无法创建新对话，请重试。");
    } finally {
      if (token === switchTokenRef.current) setSessionBusy(false);
    }
  }

  /** 切换已有会话窗口。 */
  async function switchSession(conversationId: string) {
    const id = `${conversationId || ""}`.trim();
    const current =
      activeConversationIdRef.current
      || remoteAnswerer?.getConversationId?.()
      || "";
    if (!id || id === current || getSessionBusy()) return;

    // 生成中也允许切走：真 abort 在飞请求——旧流的 done 若继续，会把
    // conversation_id 粘回旧会话、下一问落错线程（审计 P0-4）
    runAbortRef.current?.abort();
    runAbortRef.current = null;
    runningRef.current = false;
    setIsRunning(false);

    // 短时隔离即可；过长会像「点了没反应 / 乱跳」
    armReaderAiClickShield(SWITCH_SESSION_SHIELD_MS);
    lockReaderAiNavigation(SWITCH_SESSION_SHIELD_MS);
    setSessionBusy(true);
    setSessionError("");
    const token = ++switchTokenRef.current;

    // 先切 UI 选中态 + 清空，避免仍显示上一会话内容
    setActiveConversationId(id);
    activeConversationIdRef.current = id;
    setItems([]);
    setHeadId(null);

    const viewport = globalThis.document?.querySelector?.(
      "[data-reader-ai-viewport]",
    ) as HTMLElement | null;
    if (viewport) viewport.dataset.suppressAutoscroll = "1";

    try {
      await new Promise<void>((r) => {
        window.setTimeout(r, SESSION_SWITCH_SETTLE_MS);
      });
      if (token !== switchTokenRef.current) return;

      try {
        (globalThis.document?.activeElement as HTMLElement | null)?.blur?.();
      } catch {
        // ignore
      }

      const docId = documentIdRef.current
        || `${(await remoteAnswerer?.getDocumentId?.()) || ""}`.trim();
      documentIdRef.current = docId;

      const detail = await getConversation(id);
      if (token !== switchTokenRef.current) return;

      armReaderAiClickShield(800);
      lockReaderAiNavigation(800);

      const branchItems = messagesToBranchItems(detail.messages || []);
      applyConversationTree(branchItems, detail.head_id);
      remoteAnswerer?.setConversationId?.(id, docId);

      // 本地快照与服务端对齐（按会话隔离）
      if (branchItems.length) {
        saveThreadBranchSnapshot(
          jobId,
          {
            version: 1,
            headId: `${detail.head_id || ""}`.trim()
              || branchItems[branchItems.length - 1]?.message.id
              || null,
            items: branchItems as ThreadBranchItem[],
          },
          id,
        );
      } else {
        clearThreadBranchSnapshot(jobId, id);
      }

      if (docId) await refreshSessions(docId);

      // 只滚 AI 面板，不碰 PDF
      requestAnimationFrame(() => {
        const vp = globalThis.document?.querySelector?.(
          "[data-reader-ai-viewport]",
        ) as HTMLElement | null;
        if (vp) {
          vp.scrollTop = vp.scrollHeight;
          window.setTimeout(() => {
            delete vp.dataset.suppressAutoscroll;
          }, AUTOSCROLL_SUPPRESS_RELEASE_MS);
        }
        armReaderAiClickShield(POST_SWITCH_SHIELD_MS);
        lockReaderAiNavigation(POST_SWITCH_SHIELD_MS);
      });
    } catch (error) {
      console.warn("[reader-ai] switch session failed", error);
      if (token === switchTokenRef.current) {
        setSessionError("加载该对话失败，请检查网络后重试。");
        // 失败时不要假装已切换：恢复为空，避免展示错会话
        setItems([]);
        setHeadId(null);
      }
    } finally {
      if (token === switchTokenRef.current) setSessionBusy(false);
    }
  }

  /**
   * 从某条助手答案「开新对话」：
   * 复制 root→该答案 的历史到新 conversation，原会话原样保留。
   * 之后提问只带新会话上下文，避免原线程被续写污染（ChatGPT Branch in new chat）。
   * @returns 是否成功
   */
  async function branchFromAnswer(assistantMessageId: string): Promise<boolean> {
    const forkId = `${assistantMessageId || ""}`.trim();
    // 允许在 busy 时排队失败要有提示；生成中也可 fork（先停本地 running）
    if (!forkId) {
      setSessionError("无法分支：消息 id 无效。");
      return false;
    }
    if (getSessionBusy()) {
      setSessionError("请稍候，当前有会话操作进行中。");
      return false;
    }
    if (runningRef.current) {
      runningRef.current = false;
      setIsRunning(false);
    }

    const path = pathForBranch(itemsRef.current, forkId, headIdRef.current);
    if (!path.length) {
      setSessionError("无法分支：找不到到此答案的对话路径。");
      return false;
    }
    const last = path[path.length - 1];
    if (last.message.role !== "assistant") {
      setSessionError("只能从助手答案处开新对话。");
      return false;
    }

    setSessionBusy(true);
    setSessionError("");
    try {
      await new Promise<void>((resolve) => {
        window.setTimeout(resolve, SESSION_SETTLE_MS);
      });

      let docId = documentIdRef.current
        || `${(await remoteAnswerer?.getDocumentId?.()) || ""}`.trim();
      documentIdRef.current = docId;
      if (!docId) {
        // 再试一次解析
        try {
          docId = `${(await remoteAnswerer?.getDocumentId?.()) || ""}`.trim();
          documentIdRef.current = docId;
        } catch {
          docId = "";
        }
      }
      if (!docId) {
        setSessionError("无法分支：文档未就绪，请稍后重试。");
        return false;
      }

      // 线性化 parent，保证 fork 写入时父子链完整（不依赖可能断裂的旧 parentId）
      const pathPayload = path.map((item, i) => ({
        id: item.message.id,
        role: item.message.role as "user" | "assistant",
        content: item.message.content,
        citations: item.message.citations,
        parentId: i === 0 ? null : path[i - 1].message.id,
      }));

      // 标题：fork-n-xxx（xxx = 当前/原始对话名）
      const sessions = getSessions();
      const currentId =
        activeConversationIdRef.current
        || remoteAnswerer?.getConversationId?.()
        || "";
      const currentRow = (sessions || []).find((s) => s.conversation_id === currentId);
      const firstUser = pathPayload.find((p) => p.role === "user");
      const sourceTitle =
        `${currentRow?.title || ""}`.trim()
        || `${firstUser?.content || ""}`.replace(/\s+/g, " ").trim()
        || "未命名对话";
      const existingTitles = (sessions || []).map((s) => s.title || "");
      const branchTitle = nextForkConversationTitle(sourceTitle, existingTitles);

      // 必须完整 fork 到服务端（含消息），禁止只建空会话
      const forked = await forkConversationFromPath({
        documentId: docId,
        title: branchTitle,
        path: pathPayload,
      });
      const nextItems = treeItemsFromBranchItems(forked.items);
      const nextHead = nextItems[nextItems.length - 1]?.message.id || null;
      const nextConvId = forked.conversation.conversation_id;
      if (!nextConvId || !nextItems.length) {
        throw new Error("fork returned empty conversation");
      }

      armReaderAiClickShield(600);
      lockReaderAiNavigation(600);

      // 切到新会话：原会话仍在列表里可切回
      setItems(nextItems);
      setHeadId(nextHead);
      setActiveConversationId(nextConvId);
      activeConversationIdRef.current = nextConvId;
      remoteAnswerer?.setConversationId?.(nextConvId, docId);

      // 乐观插入列表（带正确标题与消息数），再 refresh 对齐服务端
      setSessions((prev) => {
        const row: ConversationRecord = {
          conversation_id: nextConvId,
          title: branchTitle,
          document_id: docId,
          created_at: forked.conversation.created_at || new Date().toISOString(),
          updated_at: forked.conversation.updated_at || new Date().toISOString(),
          message_count: nextItems.length,
          head_id: nextHead || "",
        };
        const without = prev.filter((s) => s.conversation_id !== nextConvId);
        return [row, ...without];
      });

      saveThreadBranchSnapshot(
        jobId,
        snapshotFromTree(nextItems, nextHead),
        nextConvId,
      );
      await refreshSessions(docId);

      // 新对话：滚到末尾，方便接着问
      requestAnimationFrame(() => {
        const vp = globalThis.document?.querySelector?.(
          "[data-reader-ai-viewport]",
        ) as HTMLElement | null;
        if (vp) {
          delete vp.dataset.suppressAutoscroll;
          vp.scrollTop = vp.scrollHeight;
        }
      });
      return true;
    } catch (error) {
      console.warn("[reader-ai] branch from answer failed", error);
      setSessionError("分支失败：未能复制上文到新对话。请检查网络后重试。");
      return false;
    } finally {
      setSessionBusy(false);
    }
  }

  /** 删除会话（服务端 + 本地快照）；删当前则切到最近一条或空窗。 */
  async function removeSession(conversationId: string) {
    const id = `${conversationId || ""}`.trim();
    if (!id || getSessionBusy()) return;
    runningRef.current = false;
    setIsRunning(false);
    setSessionBusy(true);
    setSessionError("");
    const token = ++switchTokenRef.current;
    try {
      const docId = documentIdRef.current
        || `${(await remoteAnswerer?.getDocumentId?.()) || ""}`.trim();
      documentIdRef.current = docId;

      try {
        await deleteConversation(id);
      } catch (error) {
        const status = Number((error as { status?: number })?.status) || 0;
        if (status !== 404) throw error;
      }
      clearThreadBranchSnapshot(jobId, id);

      const current =
        activeConversationIdRef.current
        || remoteAnswerer?.getConversationId?.()
        || "";
      const deletingActive = current === id;

      setSessions((prev) => prev.filter((s) => s.conversation_id !== id));

      if (deletingActive) {
        remoteAnswerer?.clearConversationId?.(docId);
        setActiveConversationId("");
        activeConversationIdRef.current = "";
        setItems([]);
        setHeadId(null);
        clearThreadBranchSnapshot(jobId);

        const list = docId
          ? ((await listConversations({ document_id: docId, limit: 50 }).catch(
            () => ({ conversations: [] as ConversationRecord[] }),
          )).conversations || [])
          : [];
        if (token !== switchTokenRef.current) return;
        setSessions(list);

        const next = list[0];
        if (next?.conversation_id) {
          const nextId = next.conversation_id;
          setActiveConversationId(nextId);
          activeConversationIdRef.current = nextId;
          try {
            const detail = await getConversation(nextId);
            if (token !== switchTokenRef.current) return;
            applyConversationTree(
              messagesToBranchItems(detail.messages || []),
              detail.head_id,
            );
            remoteAnswerer?.setConversationId?.(nextId, docId);
          } catch {
            setItems([]);
            setHeadId(null);
          }
        }
      } else if (docId) {
        await refreshSessions(docId);
      }
    } catch (error) {
      console.warn("[reader-ai] delete session failed", error);
      setSessionError("删除对话失败，请重试。");
    } finally {
      if (token === switchTokenRef.current) setSessionBusy(false);
    }
  }

  /** 重命名会话标题。 */
  async function renameSession(conversationId: string, title: string) {
    const id = `${conversationId || ""}`.trim();
    const nextTitle = `${title || ""}`.replace(/\s+/g, " ").trim();
    if (!id || !nextTitle || getSessionBusy()) return;
    setSessionBusy(true);
    setSessionError("");
    try {
      const clipped = nextTitle.slice(0, 80);
      await patchConversation(id, { title: clipped });
      setSessions((prev) =>
        prev.map((s) =>
          s.conversation_id === id ? { ...s, title: clipped } : s,
        ),
      );
      const docId = documentIdRef.current;
      if (docId) await refreshSessions(docId);
    } catch (error) {
      console.warn("[reader-ai] rename session failed", error);
      setSessionError("重命名失败，请重试。");
    } finally {
      setSessionBusy(false);
    }
  }

  return {
    branchFromAnswer,
    newSession,
    removeSession,
    renameSession,
    switchSession,
  };
}

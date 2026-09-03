// 阅读器 AI 问答的消息分支树：纯函数，无 React / 无副作用。
// 从 use-reader-ask-runtime.ts 拆出（评审 P1-5），行为逐行保持不变。
// 单测：tests/reader-ask-thread-tree.test.mjs

import type {
  AppendMessage,
  ThreadMessageLike,
} from "@assistant-ui/react";
import {
  messagesToBranchItems,
  type AiCitationLike,
  type ThreadBranchItem,
  type ThreadBranchMessage,
  type ThreadBranchSnapshot,
} from "../../../external.js";

export type ReaderAskStoreMessage = ThreadBranchMessage & {
  citations?: AiCitationLike[];
  status?: ThreadMessageLike["status"];
};

export type ReaderAskTreeItem = {
  parentId: string | null;
  message: ReaderAskStoreMessage;
};

export const READER_ASK_SUGGESTIONS = [
  { prompt: "这篇文献的主要结论是什么？" },
  { prompt: "作者用了什么方法或模型？" },
  { prompt: "有哪些关键结果或数据？" },
];

export function textFromAppend(message: AppendMessage): string {
  const content = message.content as unknown;
  if (typeof content === "string") return content.trim();
  if (!Array.isArray(content)) return "";
  return content
    .map((part) => {
      if (part && typeof part === "object" && (part as { type?: string }).type === "text") {
        return `${(part as { text?: string }).text || ""}`;
      }
      return "";
    })
    .join("")
    .trim();
}

export function toThreadMessageLike(message: ReaderAskStoreMessage): ThreadMessageLike {
  // 注意：fromThreadMessageLike 会丢掉 trim 为空的 text part；
  // 流式占位用可见点，避免 content=[] 时气泡不挂载 Parts。
  const raw = message.content;
  const isAssistant = message.role === "assistant";
  const content = raw.trim()
    ? raw
    : isAssistant && message.status?.type === "running"
      ? "…"
      : raw;
  // assistant-ui fromBranchableArray：status 只能出现在 assistant，否则
  // Uncaught Error: status is only supported for assistant messages
  return {
    id: message.id,
    role: message.role,
    content: [{ type: "text", text: content || "" }],
    ...(isAssistant && message.status ? { status: message.status } : {}),
    metadata: {
      custom: {
        citations: message.citations || [],
        progress: message.progress || "",
        storeId: message.id,
      },
    },
  };
}

export function shouldFallbackToLocal(error: unknown): boolean {
  const status = Number((error as { status?: number })?.status) || 0;
  const msg = `${(error as Error)?.message || ""}`;
  return status === 502 || /\b502\b/.test(msg);
}

export function makeReaderAskId(prefix: string) {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

export function normalizeAiCitations(raw: unknown): AiCitationLike[] {
  if (!Array.isArray(raw)) return [];
  const out: AiCitationLike[] = [];
  for (const item of raw) {
    if (!item || typeof item !== "object") continue;
    const c = item as AiCitationLike;
    const blockId = `${c.block_id || ""}`.trim();
    if (!blockId) continue;
    let pageIdx: number | undefined;
    const rawPage = c.page_idx ?? c.page;
    if (rawPage !== undefined && rawPage !== null && `${rawPage}`.trim() !== "") {
      const n = Number(rawPage);
      if (Number.isFinite(n) && n >= 0) pageIdx = Math.floor(n);
    }
    if (pageIdx === undefined) {
      const m = blockId.match(/(?:^|[^0-9])p0*([1-9]\d*)(?:-|_|\b)/i);
      if (m) pageIdx = Math.max(0, Number(m[1]) - 1);
    }
    out.push({
      ...c,
      block_id: blockId,
      ref: c.ref,
      page_idx: pageIdx,
      job_id: `${c.job_id || ""}`.trim(),
      document_id: `${c.document_id || ""}`.trim(),
      snippet: `${c.snippet || ""}`.trim(),
    });
  }
  return out;
}

export function snapshotFromTree(
  items: readonly ReaderAskTreeItem[],
  headId: string | null,
): ThreadBranchSnapshot {
  return {
    version: 1,
    headId,
    items: items.map((item) => ({
      parentId: item.parentId,
      message: {
        id: item.message.id,
        role: item.message.role,
        content: item.message.content,
        ...(item.message.progress ? { progress: item.message.progress } : {}),
        ...(item.message.citations?.length
          ? { citations: item.message.citations }
          : {}),
        ...(item.message.status
          ? {
            status: {
              type: item.message.status.type,
              ...("reason" in item.message.status && item.message.status.reason
                ? { reason: `${item.message.status.reason}` }
                : {}),
            },
          }
          : {}),
      },
    })) as ThreadBranchItem[],
  };
}

export function treeFromSnapshot(snapshot: ThreadBranchSnapshot): {
  items: ReaderAskTreeItem[];
  headId: string | null;
} {
  const items: ReaderAskTreeItem[] = snapshot.items.map((item) => ({
    parentId: item.parentId,
    message: {
      ...item.message,
      citations: (item.message.citations || []) as AiCitationLike[],
      status: item.message.status as ReaderAskStoreMessage["status"],
    },
  }));
  return { items, headId: snapshot.headId };
}

export function visibleMessages(
  items: readonly ReaderAskTreeItem[],
  headId: string | null,
): ReaderAskStoreMessage[] {
  if (!items.length) return [];
  const byId = new Map(items.map((i) => [i.message.id, i]));
  const head = (headId && byId.get(headId)) || items[items.length - 1];
  if (!head) return [];
  const chain: ReaderAskStoreMessage[] = [];
  let cur: ReaderAskTreeItem | undefined = head;
  const guard = new Set<string>();
  while (cur && !guard.has(cur.message.id)) {
    guard.add(cur.message.id);
    chain.push(cur.message);
    cur = cur.parentId ? byId.get(cur.parentId) : undefined;
  }
  return chain.reverse();
}

export function findMessage(
  items: readonly ReaderAskTreeItem[],
  id: string | null | undefined,
): ReaderAskStoreMessage | null {
  if (!id) return null;
  return items.find((i) => i.message.id === id)?.message ?? null;
}

/** 从任意消息沿 parent 回溯到根，得到路径上的 TreeItem 序列（根→叶）。 */
export function pathItemsToMessage(
  items: readonly ReaderAskTreeItem[],
  targetId: string,
): ReaderAskTreeItem[] {
  const byId = new Map(items.map((i) => [i.message.id, i]));
  let cur = byId.get(targetId);
  if (!cur) return [];
  const chain: ReaderAskTreeItem[] = [];
  const guard = new Set<string>();
  while (cur && !guard.has(cur.message.id)) {
    guard.add(cur.message.id);
    chain.push(cur);
    cur = cur.parentId ? byId.get(cur.parentId) : undefined;
  }
  return chain.reverse();
}

/**
 * 分支用路径：优先 parent 链；链断/缺 parent 时退化为「可见路径截到目标答案」。
 * 避免只 fork 出半截（甚至只有一条 assistant）导致「分支不像新对话/无上文」。
 */
export function pathForBranch(
  items: readonly ReaderAskTreeItem[],
  targetId: string,
  headId: string | null,
): ReaderAskTreeItem[] {
  const tid = `${targetId || ""}`.trim();
  if (!tid || !items.length) return [];

  // aui 有时用自己的 id；优先精确匹配，再回退到当前 head / 最近 assistant
  let resolvedId = tid;
  if (!items.some((i) => i.message.id === resolvedId)) {
    if (headId && items.some((i) => i.message.id === headId)) {
      resolvedId = headId;
    } else {
      const lastAssist = [...items].reverse().find((i) => i.message.role === "assistant");
      if (lastAssist) resolvedId = lastAssist.message.id;
    }
  }

  let path = pathItemsToMessage(items, resolvedId);
  // parent 链完整：至少含 user+assistant
  if (path.length >= 2 && path[path.length - 1].message.role === "assistant") {
    return path;
  }
  if (path.length === 1 && path[0].message.role === "user") {
    path = [];
  }

  // Fallback：按当前可见线性顺序，从根截到 target（含）
  const visible = visibleMessages(items, headId || resolvedId);
  let idx = visible.findIndex((m) => m.id === resolvedId);
  if (idx < 0) {
    // 再退：整条可见路径（以 head 为叶）
    idx = visible.length - 1;
  }
  if (idx < 0) return path;
  const byId = new Map(items.map((i) => [i.message.id, i]));
  const linear: ReaderAskTreeItem[] = [];
  for (let i = 0; i <= idx; i += 1) {
    const row = byId.get(visible[i].id);
    if (row) linear.push(row);
  }
  // 确保以 assistant 结尾
  while (linear.length && linear[linear.length - 1].message.role !== "assistant") {
    linear.pop();
  }
  return linear.length ? linear : path;
}

export function treeItemsFromBranchItems(
  branchItems: ReturnType<typeof messagesToBranchItems>,
): ReaderAskTreeItem[] {
  return branchItems.map((item) => ({
    parentId: item.parentId,
    message: {
      ...item.message,
      citations: (item.message.citations || []) as AiCitationLike[],
      status: item.message.status as ReaderAskStoreMessage["status"],
    },
  }));
}

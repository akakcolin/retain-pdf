// AI 问答悬浮窗：assistant-ui 线程 + 会话窗口 + 分支开窗

import { useCallback, useState } from "react";
import { AssistantRuntimeProvider } from "@assistant-ui/react";
import { Sparkles } from "lucide-react";
import {
  isReaderAiNavigationLocked,
  type AiCitationLike,
} from "../../external.js";
import { ReaderFloatShell } from "./ReaderFloatShell.js";
import { ReaderAssistantThread } from "./assistant/ReaderAssistantThread.js";
import { ReaderConversationBar } from "./assistant/ReaderConversationBar.js";
import { useReaderAskRuntime } from "./assistant/use-reader-ask-runtime.js";

// 分支成功提示的展示时长：文案较长，给足用户读完的时间（评审 P2-7，工程经验值）。
const BRANCH_NOTICE_DISMISS_MS = 6000;

export type ReaderAiPanelProps = {
  open: boolean;
  jobId: string;
  sourceOnly: boolean;
  onClose: () => void;
  /** page_idx 为 0 基；由阅读器 goToPage(page_idx+1) */
  onJumpCitation: (citation: AiCitationLike) => void;
};

export function ReaderAiPanel({
  open,
  jobId,
  sourceOnly,
  onClose,
  onJumpCitation,
}: ReaderAiPanelProps) {
  const enabled = open && !sourceOnly && Boolean(jobId);
  const [retrievalScope, setRetrievalScope] = useState<"document" | "library">("document");
  const {
    runtime,
    citationsByMessageId,
    progressByMessageId,
    contentByMessageId,
    streamingAssistantId,
    isRunning,
    sessions,
    activeConversationId,
    sessionBusy,
    sessionError,
    newSession,
    switchSession,
    removeSession,
    renameSession,
    branchFromAnswer,
  } = useReaderAskRuntime({
    jobId,
    enabled,
    retrievalScope,
  });

  const [branchNotice, setBranchNotice] = useState("");

  const handleBranch = useCallback(async (assistantId: string) => {
    setBranchNotice("");
    const ok = await branchFromAnswer(assistantId);
    if (ok) {
      setBranchNotice(
        "已保存新对话（fork-n-原名）：复制了到此答案的上文，原对话不变。顶部列表可切换。",
      );
      window.setTimeout(() => setBranchNotice(""), BRANCH_NOTICE_DISMISS_MS);
    }
  }, [branchFromAnswer]);

  // 分支/切会话锁定期内不跳 PDF，避免误触引用
  const safeJumpCitation = useCallback((citation: AiCitationLike) => {
    if (isReaderAiNavigationLocked()) return;
    onJumpCitation(citation);
  }, [onJumpCitation]);

  return (
    <ReaderFloatShell
      id="reader-ai-panel"
      open={open}
      title="AI 问答"
      subtitle={retrievalScope === "library" ? "全库检索" : "基于当前文档"}
      titleIcon={<Sparkles size={14} strokeWidth={2.1} aria-hidden />}
      storageKey="retainpdf.reader.ai-float.pos.v1"
      ariaLabel="阅读问答"
      width={400}
      className={`reader-float-ai${sessionBusy ? " is-session-busy" : ""}`}
      onClose={onClose}
    >
      {sourceOnly || !jobId ? (
        <div className="reader-float-ai-empty">
          <Sparkles size={22} strokeWidth={1.75} aria-hidden />
          <p>源文档只读模式不提供 AI 问答</p>
          <span>请从任务入口打开阅读器后再试</span>
        </div>
      ) : (
        <div className="reader-float-ai-body">
          <ReaderConversationBar
            sessions={sessions}
            activeId={activeConversationId}
            busy={sessionBusy}
            errorText={sessionError}
            onSwitch={switchSession}
            onNew={newSession}
            onDelete={removeSession}
            onRename={renameSession}
          />
          {branchNotice ? (
            <div className="aui-session-banner" role="status">
              {branchNotice}
            </div>
          ) : null}
          <div className="reader-float-ai-thread-wrap">
            {sessionBusy ? (
              <div
                className="aui-branch-pointer-shield"
                aria-hidden
                onPointerDown={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                }}
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                }}
              />
            ) : null}
            <AssistantRuntimeProvider runtime={runtime}>
              <ReaderAssistantThread
                jobId={jobId}
                citationsByMessageId={citationsByMessageId}
                progressByMessageId={progressByMessageId}
                contentByMessageId={contentByMessageId}
                streamingAssistantId={streamingAssistantId}
                isRunning={isRunning}
                onJumpCitation={safeJumpCitation}
                onBranchFromAnswer={handleBranch}
                branchBusy={sessionBusy}
                retrievalScope={retrievalScope}
                onRetrievalScopeChange={setRetrievalScope}
              />
            </AssistantRuntimeProvider>
          </div>
        </div>
      )}
    </ReaderFloatShell>
  );
}

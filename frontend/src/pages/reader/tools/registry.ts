// 阅读器工具定义（对齐 legacy 顶栏四件套 + 下载在 FAB 另区）

export type ReaderToolId = "notes" | "favorites" | "markdown" | "ai" | "entities";

export type ReaderToolDef = {
  id: ReaderToolId;
  label: string;
  /** 副文案（关 / 开） */
  subIdle: string;
  subOpen: string;
  /** 源文档只读时是否禁用 */
  needsJob: boolean;
};

/** 与 legacy ReaderTopbarActions.TOOL_BUTTONS 同一套能力 */
export const READER_TOOLS: readonly ReaderToolDef[] = Object.freeze([
  {
    id: "notes",
    label: "批注",
    subIdle: "选中文字后添加",
    subOpen: "关闭悬浮窗",
    needsJob: false,
  },
  {
    id: "favorites",
    label: "摘录",
    subIdle: "本书云端收藏",
    subOpen: "关闭悬浮窗",
    needsJob: false,
  },
  {
    id: "markdown",
    label: "Markdown",
    subIdle: "识别 / 译文文本",
    subOpen: "关闭悬浮窗",
    needsJob: true,
  },
  {
    id: "ai",
    label: "AI 问答",
    subIdle: "基于文档提问",
    subOpen: "关闭悬浮窗",
    needsJob: true,
  },
  {
    id: "entities",
    label: "概念",
    subIdle: "实体 · 证据 · 关系",
    subOpen: "关闭悬浮窗",
    needsJob: false,
  },
]);

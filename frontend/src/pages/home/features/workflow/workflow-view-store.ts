import { createStore, normalizeSourceLang, normalizeTargetLang } from "../../composition/external.js";
import type { Store } from "../../composition/external.js";

// workflow 域视图 store + React viewPort。
//
// mountWorkflowFeature(纯逻辑控制器,原样复用)的 viewPort 契约在这里落到
// store,由 WorkflowPanel/HeroUpload/PageRangeDialog 订阅渲染。
// applyMockUpload/applyWorkflowUpload/setSubmitControls/renderBudgetNote
// 逐条镜像 features/workflow/view.js 的语义(该文件属旧 DOM 视图,禁 import)。
//
// 开发者设置对话框(developer-settings-dialog)是 3b 杂项范围:
// setDeveloperDialog/readDeveloperDialog 先以 store 值往返(不接 DOM 表单),
// 3b React 化该对话框时替换这两个方法的实现即可,控制器无感。

/** 开发者设置里可选术语表选项（归一化后） */
export type WorkflowGlossaryOption = {
  glossaryId: string;
  name: string;
  entryCount: number | null;
};

/** API 列表原始项（setDeveloperGlossaryOptions 输入） */
export type WorkflowGlossarySource = {
  glossary_id?: string;
  name?: string;
  entry_count?: number | string | null;
  [key: string]: unknown;
};

export type WorkflowBudgetNote = {
  visible: boolean;
  tone: string;
  message: string;
  blocking: boolean;
  topUpUrl: string;
};

/** 开发者对话框持久字段（形状由 controller 写入，消费时按需读） */
export type WorkflowDeveloperDialog = {
  workflow?: string;
  renderSourceJobId?: string;
  model?: unknown;
  baseUrl?: unknown;
  glossaryId?: string;
  sourceLang?: string;
  targetLang?: string;
  workers?: unknown;
  batchSize?: unknown;
  classifyBatchSize?: unknown;
  compileWorkers?: unknown;
  timeoutSeconds?: unknown;
  [key: string]: unknown;
};

export type WorkflowViewState = {
  submitLabel: string;
  submitDisabled: boolean;
  submitBusy: boolean;
  pageRangeButtonVisible: boolean;
  skipOcr: boolean;
  budget: WorkflowBudgetNote;
  jobWarningVisible: boolean;
  glossaries: WorkflowGlossaryOption[];
  selectedGlossaryId: string;
  /** 任务级语言覆盖（专业翻译对话框内设置；null = 未覆盖，回退开发者默认） */
  taskSourceLang: string | null;
  taskTargetLang: string | null;
  developerDialog: WorkflowDeveloperDialog;
  developerFormState: Record<string, unknown>;
};

export type WorkflowViewActions = {
  patch(
    currentState: WorkflowViewState,
    payload?: Partial<WorkflowViewState>,
  ): WorkflowViewState;
};

export type WorkflowViewStore = Store<WorkflowViewState, WorkflowViewActions>;

/** workflow → upload 瓦片端口（upload-view-store.uploadTilePort） */
export type WorkflowUploadTilePort = {
  setUploadActionSlotVisible?: (visible?: boolean) => void;
  setUploadTileLocked?: (options?: { locked?: boolean; enabled?: boolean }) => void;
  setUploadTileText?: (options?: {
    label?: string;
    labelTitle?: string;
    help?: string;
    status?: string;
    statusVisible?: boolean | null;
    labelVisible?: boolean;
    helpVisible?: boolean;
  }) => void;
};

export function createWorkflowViewStore(): WorkflowViewStore {
  return createStore<WorkflowViewState, WorkflowViewActions>({
    name: "homeWorkflowView",
    initialState: {
      submitLabel: "直接翻译",
      submitDisabled: true,
      submitBusy: false,
      pageRangeButtonVisible: true,
      skipOcr: false,
      budget: {
        visible: false,
        tone: "",
        message: "",
        blocking: false,
        topUpUrl: "",
      },
      jobWarningVisible: false,
      glossaries: [],
      selectedGlossaryId: "",
      taskSourceLang: null,
      taskTargetLang: null,
      developerDialog: {},
      developerFormState: {},
    },
    actions: {
      patch(currentState, payload = {}) {
        return { ...currentState, ...payload };
      },
    },
  });
}

export function createWorkflowViewFeature({
  store = createWorkflowViewStore(),
  uploadTilePort,
}: {
  store?: WorkflowViewStore;
  uploadTilePort?: WorkflowUploadTilePort | null;
} = {}) {
  const patch = (payload: Partial<WorkflowViewState> = {}) => store.actions.patch(payload);

  function setSubmitBusy(busy = false) {
    patch({ submitBusy: Boolean(busy) });
  }

  function setSubmitDisabled(disabled = true) {
    patch({ submitDisabled: Boolean(disabled) });
  }

  function selectedGlossaryId() {
    return `${store.getSnapshot().selectedGlossaryId || ""}`.trim();
  }

  function setSelectedGlossaryId(value = "") {
    patch({ selectedGlossaryId: `${value || ""}`.trim() });
  }

  function setTaskSourceLang(value = "") {
    patch({ taskSourceLang: `${value || ""}`.trim() || null });
  }

  function setTaskTargetLang(value = "") {
    patch({ taskTargetLang: `${value || ""}`.trim() || null });
  }

  // 语言默认合并：任务级覆盖 → developerDialog 默认 → 注册表硬默认。
  // 与 readDeveloperDialog 的 glossaryId 合并模式一致（override 优先于默认）。
  function readSourceLang() {
    const task = `${store.getSnapshot().taskSourceLang || ""}`.trim();
    const saved = store.getSnapshot().developerDialog?.sourceLang;
    return normalizeSourceLang(task || saved);
  }

  function readTargetLang() {
    const task = `${store.getSnapshot().taskTargetLang || ""}`.trim();
    const saved = store.getSnapshot().developerDialog?.targetLang;
    return normalizeTargetLang(task || saved);
  }

  function setJobWarningVisible(visible: boolean) {
    patch({ jobWarningVisible: Boolean(visible) });
  }

  function setSkipOcr(value = false) {
    patch({ skipOcr: Boolean(value) });
  }

  function readSkipOcr() {
    return Boolean(store.getSnapshot().skipOcr);
  }

  // ---- features/workflow/view.js 镜像 ----

  function setSubmitControls({
    disabled,
    label,
    actionVisible,
    pageRangeVisible,
  }: {
    disabled?: boolean;
    label?: string;
    actionVisible?: boolean;
    pageRangeVisible?: boolean;
  } = {}) {
    patch({
      submitDisabled: Boolean(disabled),
      submitLabel: `${label ?? ""}` || store.getSnapshot().submitLabel,
      pageRangeButtonVisible: Boolean(pageRangeVisible),
    });
    uploadTilePort?.setUploadActionSlotVisible(actionVisible);
  }

  function renderBudgetNote(budget?: Partial<WorkflowBudgetNote> | null) {
    patch({
      budget: {
        visible: Boolean(budget?.visible),
        tone: `${budget?.tone || ""}`,
        message: `${budget?.message || ""}`,
        blocking: Boolean(budget?.blocking),
        topUpUrl: `${budget?.topUpUrl || ""}`,
      },
    });
  }

  function applyMockUpload({
    mockScenario,
    submitLabel,
    showPageRangeButton,
  }: {
    mockScenario?: string;
    submitLabel?: string;
    showPageRangeButton?: boolean;
  } = {}) {
    uploadTilePort?.setUploadTileLocked({ locked: true, enabled: false });
    uploadTilePort?.setUploadTileText({
      label: "Mock 模式",
      labelTitle: "",
      help: `当前为 mock 模式：${mockScenario || "running"}。不会上传文件，也不会请求真实后端。`,
      status: "Mock 模式已启用，可直接点击开始翻译。",
      statusVisible: true,
    });
    setSubmitControls({
      disabled: false,
      label: submitLabel,
      actionVisible: true,
      pageRangeVisible: showPageRangeButton,
    });
  }

  function applyWorkflowUpload({
    needsUpload,
    uploadReady,
    defaultFileLabel,
    headline,
    renderSourceJobId,
  }: {
    needsUpload?: boolean;
    uploadReady?: boolean;
    defaultFileLabel?: string;
    headline?: string;
    renderSourceJobId?: string;
  } = {}) {
    uploadTilePort?.setUploadTileLocked({ locked: !needsUpload, enabled: needsUpload });
    uploadTilePort?.setUploadTileText({
      label: !uploadReady ? (needsUpload ? defaultFileLabel : "复用已有任务产物") : "",
      labelTitle: "",
      help: headline,
      status: !needsUpload
        ? (renderSourceJobId
            ? `当前将复用任务: ${renderSourceJobId}`
            : "请先在开发者设置里填写 Render 源任务 ID。")
        : "",
      statusVisible: !needsUpload ? true : (!uploadReady ? false : null),
    });
  }

  function setDeveloperGlossaryOptions(
    glossaries: WorkflowGlossarySource[] = [],
    selectedId = "",
  ) {
    patch({
      glossaries: (Array.isArray(glossaries) ? glossaries : [])
        .map((glossary) => ({
          glossaryId: `${glossary?.glossary_id || ""}`.trim(),
          name: `${glossary?.name || glossary?.glossary_id || ""}`.trim(),
          entryCount: Number.isFinite(Number(glossary?.entry_count))
            ? Number(glossary.entry_count)
            : null,
        }))
        .filter((glossary) => glossary.glossaryId),
      selectedGlossaryId: `${selectedId || ""}`.trim(),
    });
  }

  // ---- 开发者设置对话框(3b 接管点) ----

  function setDeveloperDialog(config: WorkflowDeveloperDialog = {}) {
    patch({ developerDialog: { ...config } });
  }

  function readDeveloperDialog(defaults: Partial<WorkflowDeveloperDialog> = {}) {
    const saved = store.getSnapshot().developerDialog || {};
    return {
      workflow: saved.workflow,
      renderSourceJobId: `${saved.renderSourceJobId || ""}`.trim(),
      model: saved.model || defaults.model,
      baseUrl: saved.baseUrl || defaults.baseUrl,
      glossaryId: selectedGlossaryId() || `${saved.glossaryId || ""}`.trim(),
      sourceLang: readSourceLang(),
      targetLang: readTargetLang(),
      workers: saved.workers ?? defaults.workers,
      batchSize: saved.batchSize ?? defaults.batchSize,
      classifyBatchSize: saved.classifyBatchSize ?? defaults.classifyBatchSize,
      compileWorkers: saved.compileWorkers ?? defaults.compileWorkers,
      timeoutSeconds: saved.timeoutSeconds ?? defaults.timeoutSeconds,
    };
  }

  function readDeveloperWorkflow() {
    return store.getSnapshot().developerDialog?.workflow;
  }

  function setDeveloperWorkflowFormState(payload: Record<string, unknown> = {}) {
    patch({ developerFormState: { ...payload } });
  }

  const viewPort = {
    applyMockUpload,
    applyWorkflowUpload,
    closeDeveloperDialog: () => {},
    readDeveloperDialog,
    readDeveloperWorkflow,
    renderBudgetNote,
    setDeveloperDialog,
    setDeveloperGlossaryOptions,
    setDeveloperWorkflowFormState,
    setSubmitControls,
  };

  return {
    patch,
    readSkipOcr,
    readSourceLang,
    readTargetLang,
    selectedGlossaryId,
    setJobWarningVisible,
    setSelectedGlossaryId,
    setSkipOcr,
    setSubmitBusy,
    setSubmitDisabled,
    setTaskSourceLang,
    setTaskTargetLang,
    store,
    viewPort,
  };
}

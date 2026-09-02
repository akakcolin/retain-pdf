// StatusDetailDialog(蓝图 §1 主组件)——对照
// components/dialogs/status-detail-dialog-template.js 逐 id/class 镜像。
//
// Dialog 渲染层(阶段 C 第二批,shadcn 改造):从原生 <dialog>+showModal/close
// 换成 radix-ui 的 Dialog 原语(DialogPrimitive.Root/Portal/Overlay/Content),
// 不经 src/components/ui/dialog.jsx 默认皮肤(className 继续用现有的
// desktop-dialog/desktop-shell 这套 bespoke CSS,和 status-detail-dialog 专属
// 覆盖并存)。open 受控于 dialogStore(useStatusDetailOverview 的 open),
// onOpenChange 在 next===false 时统一调用 dialogStore.close()——这不是
// TranslationWorkflowDialog 那种两态语义(上传/状态),关闭就是关闭,不需要
// 分流。Escape、点击背板(DismissableLayer 的 outside-click 检测)、点击关闭
// 按钮(DialogPrimitive.Close,替代原来 <form method="dialog"> 的
// type="submit" 隐式提交关闭)三条路径都走这一个回调。
//
// 不 forceMount Content(同 CredentialsDialog 等 4 个阶段 C 第一批对话框的
// 决策,见 use-dialog-return-focus.js 头注释——forceMount 会让 Radix modal
// Content 内部的 hideOthers() 副作用在应用启动时就永久生效)。
//
// 双层 forceMount 交互(本文件的风险点):内层 4 个 tab 依旧各自
// TabsPrimitive.Content forceMount + 显式 hidden 覆盖(阶段 B 决策，语义见下方
// 面板函数注释)——外层 Dialog 不再 forceMount 意味着对话框整个关闭时 Content
// 连同内部 4 个 Tabs 一起卸载,tab 内部 useState(TranslationDebugTab 选中的
// item 等)会被清空。这在产品语义上是可接受的：forceMount+hidden 的常驻挂载
// 语义原本就只服务于"对话框打开期间切 tab 不丢状态"，从未承诺"对话框关闭
// 再重开也保留"，两者并不冲突（已用 fresh Playwright 实测：打开期间切 4 个
// tab、翻译调试选中态跨切换保留，见阶段 C 报告）。
//
// Tabs 实现(阶段 B,shadcn 改造):同 SettingsHubDialog/CredentialsDialog 的
// 选择——直接用 radix-ui 的 Tabs 原语(不经 src/components/ui/tabs.jsx 默认
// 皮肤,避免和 detail-tabs/detail-tab-panel 这套 bespoke CSS 冲突)。activeTab
// 由 useStatusDetailOverview 的 controller.activateDetailTab 驱动,Radix 走
// 受控模式。4 个面板全部转成 TabsPrimitive.Content(forceMount + 显式 hidden
// 覆盖),验证过 Radix 的 forceMount 只保证"强制渲染 children"、可见性仍由
// contentProps 里显式传入的 hidden 决定(晚于 Radix 内部计算的 hidden 展开,
// 会覆盖它)——StageHistoryList/EventsList/TranslationDebugTab 的内部 useState
// 因此继续不受 tab 切换影响,这是本文件迁移的最大风险点,已用组件测试 +
// fresh Playwright 验证(见 status-detail-dialog-component.test.mjs 与阶段 B/C
// 报告)。

import { Dialog as DialogPrimitive, Tabs as TabsPrimitive } from "radix-ui";
import { useDialogReturnFocus } from "../../../../shared/react/use-dialog-return-focus.js";
import { StageHistoryList } from "./StageHistoryList.jsx";
import { EventsList, eventsStatusText } from "./EventsList.jsx";
import { TranslationDebugTab } from "./TranslationDebugTab.jsx";
import { useStatusDetailOverview } from "./useStatusDetailOverview.js";
import { useRerunAction } from "./useRerunAction.js";
import { STATUS_DETAIL_DIALOG_IDS, STATUS_DETAIL_MARKDOWN_BUNDLE_ID } from "./status-detail-dom-ids.js";
import { useHomeServices } from "../../home-services-context.js";
import { useStoreSnapshot } from "../../../../shared/react/use-store.js";
import { useArtifactDownloadBusy } from "../../state/use-artifact-download-busy.js";
import { Button } from "../../../../components/Button.jsx";

const TABS = [
  { key: "overview", label: "概览" },
  { key: "failure", label: "失败" },
  { key: "events", label: "事件" },
  { key: "translation", label: "高级诊断", advanced: true },
];

function DetailItem({ id, label, value, optional = false }) {
  // optional 行照搬旧世界 view.js#toggleOptionalRuntimeRow 的语义:元素常驻
  // DOM,只在值为空/"-"时给容器加 hidden 类(不是整行卸载)——lastTransition/
  // terminalReason 两行是这个语义唯一的两个消费者。
  const text = `${value ?? "-"}`.trim();
  const rowHidden = optional && (!text || text === "-");
  return (
    <div className={`detail-item${rowHidden ? " hidden" : ""}`}><span className="label">{label}</span><span id={id} className="info-value">{value}</span></div>
  );
}

function OverviewMarkdownBundleLink() {
  // artifact-downloads 域(蓝图 §7)——下载状态源于 statusCardStore(与
  // ResultActions.jsx 同一份 renderJob 回调注入点的产物,status-detail 打开时
  // 展示的永远是同一个当前轮询 job,详见 composition.js「StatusDetailDialog
  // 域」装配块注释;overview 自身的 fetch 段(events/diagnostics/resumePlan)
  // 不含 markdownBundleUrl/Ready,不重复造一份派生逻辑)。点击行为走 document
  // 级委托点击(controller.js 已在 composition.js 挂载 bindEvents()),本组件
  // 不需要接 onClick,只订阅 busy store 驱动"下载中..."文案(方案二)。
  const services = useHomeServices();
  const cardSnapshot = useStoreSnapshot(services.statusCard.store);
  const busyState = useArtifactDownloadBusy(services.artifactDownloads.busyStore, STATUS_DETAIL_MARKDOWN_BUNDLE_ID);
  const ready = Boolean(cardSnapshot.snapshot?.markdownBundleReady);
  const url = cardSnapshot.snapshot?.markdownBundleUrl || "";
  const enabled = ready && Boolean(url) && !busyState.busy;
  const label = busyState.busy
    ? (busyState.label || "下载中...")
    : (cardSnapshot.snapshot?.markdownBundleTranslated ? "下载译文 Markdown ZIP" : "下载 Markdown ZIP");
  return (
    <a
      id={STATUS_DETAIL_MARKDOWN_BUNDLE_ID}
      className={`button-link secondary${enabled ? "" : " disabled"}`}
      href={ready && url ? url : "#"}
      target="_blank"
      rel="noopener noreferrer"
      aria-disabled={enabled ? "false" : "true"}
      data-url={ready && url ? url : ""}
    >
      {label}
    </a>
  );
}

function OverviewPanel({ overview, active }) {
  const ids = STATUS_DETAIL_DIALOG_IDS;
  const runtime = overview.runtime;
  return (
    <TabsPrimitive.Content
      value="overview"
      forceMount
      id={ids.panels.overview}
      className={`detail-tab-panel${active ? " is-active" : ""}`}
      data-panel="overview"
      hidden={!active}
    >
      <div className="detail-download-row">
        <OverviewMarkdownBundleLink />
      </div>
      <div className="detail-grid">
        <DetailItem id={ids.runtime.currentStage} label="当前阶段" value={runtime.currentStage} />
        <DetailItem id={ids.runtime.stageElapsed} label="当前阶段耗时" value={runtime.stageElapsed} />
        <DetailItem id={ids.runtime.totalElapsed} label="累计耗时" value={runtime.totalElapsed} />
        <DetailItem id={ids.runtime.retryCount} label="重试次数" value={runtime.retryCount} />
        <DetailItem id={ids.runtime.lastTransition} label="最近切换" value={runtime.lastTransition} optional />
        <DetailItem id={ids.runtime.terminalReason} label="终态原因" value={runtime.terminalReason} optional />
        <DetailItem id={ids.runtime.inputProtocol} label="输入协议" value={runtime.inputProtocol} />
        <DetailItem id={ids.runtime.stageSpecVersion} label="Stage Schema" value={runtime.stageSpecVersion} />
        <DetailItem id={ids.runtime.mathMode} label="公式模式" value={runtime.mathMode} />
      </div>
      <div className="status-panel detail-stage-panel">
        <div className="status-panel-head"><h3>过程时间线</h3></div>
        <StageHistoryList job={overview.job} finishedAtFallback={overview.finishedAtFallback} />
      </div>
    </TabsPrimitive.Content>
  );
}

function FailurePanel({ overview, rerunPending, controller, active }) {
  const ids = STATUS_DETAIL_DIALOG_IDS;
  const failure = overview.failure;
  const rerun = useRerunAction({ overview, rerunPending, controller });
  return (
    <TabsPrimitive.Content
      value="failure"
      forceMount
      id={ids.panels.failure}
      className={`detail-tab-panel${active ? " is-active" : ""}`}
      data-panel="failure"
      hidden={!active}
    >
      <div className="status-panel">
        <div className="status-panel-head">
          <h3>失败诊断</h3>
          <span className="status-panel-note">结构化失败摘要与排查建议</span>
        </div>
        <div className="failure-action-row">
          <button id={ids.failure.rerunButton} type="button" className="button-link secondary" disabled={rerun.disabled} onClick={rerun.run}>从断点恢复/重新运行</button>
          <span id={ids.failure.rerunStatus} className="status-panel-note">{rerun.status || "失败后如后端允许，可基于已有产物创建恢复任务。"}</span>
        </div>
        <div className="failure-hero-card">
          <span className="label">失败摘要</span>
          <span id={ids.failure.summary} className="info-value">{failure.summary}</span>
        </div>
        <div className="info-list detail-info-list">
          <div className="info-row"><span className="label">分类</span><span id={ids.failure.category} className="info-value">{failure.category}</span></div>
          <div className="info-row"><span className="label">阶段</span><span id={ids.failure.stage} className="info-value">{failure.stage}</span></div>
          <div className="info-row"><span className="label">根因</span><span id={ids.failure.rootCause} className="info-value">{failure.rootCause}</span></div>
          <div className="info-row"><span className="label">建议</span><span id={ids.failure.suggestion} className="info-value">{failure.suggestion}</span></div>
          <div className="info-row"><span className="label">最近日志</span><span id={ids.failure.lastLogLine} className="info-value">{failure.lastLogLine}</span></div>
          <div className="info-row"><span className="label">可重试</span><span id={ids.failure.retryable} className="info-value">{failure.retryable}</span></div>
        </div>
      </div>
    </TabsPrimitive.Content>
  );
}

function EventsPanel({ overview, active }) {
  const ids = STATUS_DETAIL_DIALOG_IDS;
  return (
    <TabsPrimitive.Content
      value="events"
      forceMount
      id={ids.panels.events}
      className={`detail-tab-panel${active ? " is-active" : ""}`}
      data-panel="events"
      hidden={!active}
    >
      <div className="status-panel">
        <div className="status-panel-head">
          <h3>事件流</h3>
          <span id={ids.events.status} className="status-panel-note">{eventsStatusText(overview.eventsPayload)}</span>
        </div>
        <p className="events-lead">按时间倒序展示最近事件，适合定位任务卡在哪个阶段以及最后一次失败前发生了什么。</p>
        <EventsList eventsPayload={overview.eventsPayload} />
      </div>
    </TabsPrimitive.Content>
  );
}

function TranslationPanel({ translation, controller, active }) {
  const ids = STATUS_DETAIL_DIALOG_IDS;
  return (
    <TabsPrimitive.Content
      value="translation"
      forceMount
      id={ids.panels.translation}
      className={`detail-tab-panel${active ? " is-active" : ""}`}
      data-panel="translation"
      hidden={!active}
    >
      <TranslationDebugTab translation={translation} controller={controller} />
    </TabsPrimitive.Content>
  );
}

export function StatusDetailDialog() {
  const { open, activeTab, overview, translation, rerunPending, controller, dialogStore } = useStatusDetailOverview();
  const ids = STATUS_DETAIL_DIALOG_IDS;
  const { onCloseAutoFocus } = useDialogReturnFocus(open);

  // Escape / 背板点击(DismissableLayer 的 outside-click 检测)/ 关闭按钮
  // (DialogPrimitive.Close)都经这一个回调回写 store——不是 TranslationWorkflowDialog
  // 那种两态语义,next===false 直接 close() 即可。
  function handleOpenChange(nextOpen) {
    if (!nextOpen) {
      dialogStore.close();
    }
  }

  const headline = overview.headline;

  return (
    <DialogPrimitive.Root open={open} onOpenChange={handleOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="status-detail-dialog-overlay" />
        <DialogPrimitive.Content
          id={ids.dialog}
          className="desktop-dialog status-detail-dialog"
          onCloseAutoFocus={onCloseAutoFocus}
        >
          <div className="desktop-shell">
            <div className="desktop-head">
              <div className="status-detail-headline">
                <span
                  id={ids.headline.icon}
                  className="status-detail-head-icon"
                  aria-hidden="true"
                  // eslint-disable-next-line react/no-danger
                  dangerouslySetInnerHTML={{ __html: headline.iconMarkup || "" }}
                />
                <div className="status-detail-head-copy">
                  <div className="status-detail-head-top">
                    <DialogPrimitive.Title asChild>
                      <h2>任务详情</h2>
                    </DialogPrimitive.Title>
                    <p className="status-detail-job-meta">Job ID <span id={ids.headline.jobId} className="status-detail-job-id mono">{headline.jobId}</span></p>
                  </div>
                  <p id={ids.headline.note} className="status-panel-note">{headline.note}</p>
                </div>
              </div>
              <DialogPrimitive.Close asChild>
                <Button size={undefined} id={ids.headline.closeButton} className="dialog-close-btn" aria-label="关闭">×</Button>
              </DialogPrimitive.Close>
            </div>
            <TabsPrimitive.Root
              className="contents"
              value={activeTab}
              onValueChange={(tab) => controller.activateDetailTab(tab)}
            >
              <div className="desktop-body status-detail-body">
                <TabsPrimitive.List className="detail-tabs" aria-label="任务详情">
                  {TABS.map((tab) => (
                    <TabsPrimitive.Trigger
                      key={tab.key}
                      value={tab.key}
                      id={ids.tabs[tab.key]}
                      className={`detail-tab${tab.advanced ? " detail-tab-advanced" : ""}${activeTab === tab.key ? " is-active" : ""}`}
                      data-tab={tab.key}
                    >
                      {tab.label}
                    </TabsPrimitive.Trigger>
                  ))}
                </TabsPrimitive.List>

                <div className="detail-tab-panels">
                  <OverviewPanel overview={overview} active={activeTab === "overview"} />
                  <FailurePanel overview={overview} rerunPending={rerunPending} controller={controller} active={activeTab === "failure"} />
                  <EventsPanel overview={overview} active={activeTab === "events"} />
                  <TranslationPanel translation={translation} controller={controller} active={activeTab === "translation"} />
                </div>
              </div>
            </TabsPrimitive.Root>
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

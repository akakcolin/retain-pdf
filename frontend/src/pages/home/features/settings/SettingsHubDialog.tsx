// SettingsHubDialog v2：左导航 + 右内容区（原"门厅弹窗"横向 pill 布局退役）。
//
// 布局：左侧竖排导航（图标+名称，Radix Tabs orientation=vertical，方向键可用），
// 右侧内容窗格（每区自带标题行 + 正文，独立滚动）。外观区升格为主题卡片网格
// 的主舞台；API/词表因真实表单仍是独立顶层对话框（CredentialsDialog/
// GlossariesDialog，各有 controller/store/测试契约），本面板作为"启动区"
// 保留入口按钮——后续如要内嵌，动的是那两个 feature，不是这里。
//
// 【测试契约，改版不许破】（credentials/glossaries/app-update component tests）：
// - #app-settings-dialog / #app-settings-close-btn
// - [data-settings-tab="api|glossary|appearance|update"] 可点击
// - [data-settings-panel=…] forceMount + hidden 属性切换（测试断言 .hidden）
// - #credentials-btn / #glossary-btn 打开对应子对话框
// - 外观面板 #theme-appearance-panel 与 #theme-option-<id>
//
// 开合状态跨子树走 settings-hub-dialog-store；tab 切换是子树内瞬态（useState）。
// 不 forceMount Dialog 的 Content/Overlay（Radix hideOthers 依赖真实
// mount/unmount，见 CredentialsDialog 头注释）。AppUpdateBanner 的挂载生命
// 周期说明见旧版头注释结论：后台自检由 composition 的纯逻辑控制器驱动，
// 与本组件是否挂载无关。

import { useEffect, useState } from "react";
import { Dialog as DialogPrimitive, Tabs as TabsPrimitive } from "radix-ui";
import { useHomeServices } from "../../home-services-context.js";
import { useDialogState } from "../../state/use-dialog-state.js";
import { useDialogReturnFocus } from "../../../../shared/react/use-dialog-return-focus.js";
import { APP_SETTINGS_DIALOG_IDS } from "../credentials/credentials-dom-ids.js";
import { AppUpdateBanner } from "../app-update/AppUpdateBanner.jsx";
import { CredentialsWorkbench } from "../credentials/CredentialsWorkbench.jsx";
import { ThemeAppearancePanel } from "./ThemeAppearancePanel.jsx";
import { LanguageDefaultsPanel } from "./LanguageDefaultsPanel.jsx";
import { Button as ButtonBase } from "../../../../components/Button.jsx";

// Button.size 在未注解源文件里被推断为必填;unstyled 路径运行时不用 size。
const Button = ButtonBase as any;

function IconKey(props) {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true" {...props}>
      <path d="M14.5 9.5a4 4 0 1 1-1.2 2.86L5 20.65 3.35 19 11.6 10.7A4 4 0 0 1 14.5 9.5Z" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M18 6.5h.01" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" />
    </svg>
  );
}
function IconBook(props) {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true" {...props}>
      <path d="M5.5 5.2A2.2 2.2 0 0 1 7.7 3H19v15.5H7.7a2.2 2.2 0 0 0-2.2 2.2V5.2Z" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round" />
      <path d="M5.5 5.2A2.2 2.2 0 0 0 3.3 3H3v15.5h.3a2.2 2.2 0 0 1 2.2 2.2" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round" />
    </svg>
  );
}
function IconPalette(props) {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true" {...props}>
      <path d="M12 3a9 9 0 1 0 9 9c0-.5-.04-1-.12-1.48a5 5 0 0 1-6.4-6.4A9 9 0 0 0 12 3Z" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round" />
      <circle cx="8.5" cy="10" r="1.1" fill="currentColor" />
      <circle cx="11.5" cy="7.2" r="1.1" fill="currentColor" />
      <circle cx="15.2" cy="9" r="1.1" fill="currentColor" />
    </svg>
  );
}
function IconUpdate(props) {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true" {...props}>
      <path d="M12 5v2.1M12 16.9V19M5 12h2.1M16.9 12H19M7.05 7.05l1.5 1.5M15.45 15.45l1.5 1.5M16.95 7.05l-1.5 1.5M8.55 15.45l-1.5 1.5" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
      <circle cx="12" cy="12" r="3.2" stroke="currentColor" strokeWidth="1.7" />
    </svg>
  );
}
function IconLanguage(props) {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true" {...props}>
      <circle cx="12" cy="12" r="8.5" stroke="currentColor" strokeWidth="1.7" />
      <path d="M3.5 12h17M12 3.5c2.2 2.3 3.4 5.1 3.4 8.5s-1.2 6.2-3.4 8.5c-2.2-2.3-3.4-5.1-3.4-8.5S9.8 5.8 12 3.5Z" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round" />
    </svg>
  );
}

const TABS = [
  { id: "api", label: "API 设置", Icon: IconKey },
  { id: "glossary", label: "词表", Icon: IconBook },
  { id: "appearance", label: "外观", Icon: IconPalette },
  { id: "language", label: "语言", Icon: IconLanguage },
  { id: "update", label: "更新", Icon: IconUpdate },
];

const PANE_HEADS = {
  api: { title: "API 设置", desc: "配置 OCR Token、DeepSeek Key、模型地址和任务选项，保存后立即生效。" },
  glossary: { title: "术语表", desc: "维护固定译法、保留词和专业术语偏好。" },
  appearance: { title: "外观", desc: "选择界面配色，立即生效并记住本机选择。" },
  language: { title: "语言", desc: "设置整本翻译默认的原文与译文语言，任务发起时生效。" },
  update: { title: "更新", desc: "查看当前版本，并从 GitHub Releases 重新检查更新。" },
};

function PaneHead({ tab }: { tab: keyof typeof PANE_HEADS }) {
  const head = PANE_HEADS[tab];
  return (
    <header className="app-settings-pane-head">
      <h3>{head.title}</h3>
      <p>{head.desc}</p>
    </header>
  );
}

export function SettingsHubDialog() {
  const services = useHomeServices();
  const { dialogStore } = services.settingsHub;
  const dialogState = useDialogState(dialogStore);
  const open = Boolean(dialogState.open);
  const { onCloseAutoFocus } = useDialogReturnFocus(open);
  const [activeTab, setActiveTab] = useState(dialogState.payload?.tab || "api");

  useEffect(() => {
    if (open) {
      setActiveTab(dialogState.payload?.tab || "api");
    }
  }, [open]);

  // API 区内嵌凭据工作台：进入 api tab 时从凭据状态回填表单（不开二层弹窗）。
  // forceMount 保证面板已挂载；rAF 再补一次，避免 ref 尚未挂上导致密码框空白、保存读到空串。
  useEffect(() => {
    if (!open || activeTab !== "api") {
      return;
    }
    const prepare = () => services.credentials?.feature?.prepareCredentialsPanels?.();
    prepare();
    const raf = requestAnimationFrame(prepare);
    return () => cancelAnimationFrame(raf);
  }, [open, activeTab, services]);

  function handleOpenChange(nextOpen) {
    if (!nextOpen) {
      dialogStore.close();
    }
  }

  function openGlossaries() {
    services.glossaries.dialogStore.open();
  }

  function panelClass(tab: string) {
    // 纯字面量拼接（含空格分隔），避开 v4 扫描器的 `x${y}` 模板坑
    return activeTab === tab ? "app-settings-panel is-current" : "app-settings-panel";
  }

  return (
    <DialogPrimitive.Root open={open} onOpenChange={handleOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="desktop-dialog-overlay" />
        <DialogPrimitive.Content
          id={APP_SETTINGS_DIALOG_IDS.dialog}
          className="desktop-dialog app-settings-dialog"
          onCloseAutoFocus={onCloseAutoFocus}
        >
          <div className="desktop-shell app-settings-shell">
            <TabsPrimitive.Root
              className="app-settings-layout"
              orientation="vertical"
              value={activeTab}
              onValueChange={setActiveTab}
            >
              <aside className="app-settings-rail">
                <DialogPrimitive.Title asChild>
                  <h2>设置</h2>
                </DialogPrimitive.Title>
                <TabsPrimitive.List className="app-settings-nav" aria-label="设置分类">
                  {TABS.map(({ id, label, Icon }) => (
                    <TabsPrimitive.Trigger
                      key={id}
                      value={id}
                      className={activeTab === id ? "is-active" : ""}
                      data-settings-tab={id}
                    >
                      <Icon />
                      {label}
                    </TabsPrimitive.Trigger>
                  ))}
                </TabsPrimitive.List>
              </aside>

              <div className="app-settings-pane">
                <DialogPrimitive.Close asChild>
                  <Button
                    id={APP_SETTINGS_DIALOG_IDS.closeButton}
                    className="dialog-close-btn app-settings-close"
                    aria-label="关闭"
                  >
                    ×
                  </Button>
                </DialogPrimitive.Close>

                <TabsPrimitive.Content
                  value="api"
                  forceMount
                  hidden={activeTab !== "api"}
                  className={panelClass("api")}
                  data-settings-panel="api"
                >
                  <PaneHead tab="api" />
                  {/* 凭据工作台直接内嵌（无二层弹窗）；与首次配置门共用
                      CredentialsWorkbench，状态同源。 */}
                  <CredentialsWorkbench />
                </TabsPrimitive.Content>

                <TabsPrimitive.Content
                  value="glossary"
                  forceMount
                  hidden={activeTab !== "glossary"}
                  className={panelClass("glossary")}
                  data-settings-panel="glossary"
                >
                  <PaneHead tab="glossary" />
                  <div className="app-settings-launcher">
                    <p>
                      词表决定翻译时的固定译法与保留词。可维护多张词表并
                      按需启用，翻译任务发起时生效。
                    </p>
                    <Button id={APP_SETTINGS_DIALOG_IDS.glossaryButton} className="app-settings-action" onClick={openGlossaries}>
                      打开词表
                    </Button>
                  </div>
                </TabsPrimitive.Content>

                <TabsPrimitive.Content
                  value="appearance"
                  forceMount
                  hidden={activeTab !== "appearance"}
                  className={panelClass("appearance")}
                  data-settings-panel="appearance"
                >
                  <PaneHead tab="appearance" />
                  <ThemeAppearancePanel />
                </TabsPrimitive.Content>

                <TabsPrimitive.Content
                  value="language"
                  forceMount
                  hidden={activeTab !== "language"}
                  className={panelClass("language")}
                  data-settings-panel="language"
                >
                  <PaneHead tab="language" />
                  <LanguageDefaultsPanel />
                </TabsPrimitive.Content>

                <TabsPrimitive.Content
                  value="update"
                  forceMount
                  hidden={activeTab !== "update"}
                  className={panelClass("update")}
                  data-settings-panel="update"
                >
                  <PaneHead tab="update" />
                  {/* AppUpdateBanner:按钮 + 详情 dialog 合并一体(蓝图 §5)。
                      挂载生命周期与后台自检解耦的结论见文件头注释。 */}
                  <AppUpdateBanner />
                </TabsPrimitive.Content>
              </div>
            </TabsPrimitive.Root>
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

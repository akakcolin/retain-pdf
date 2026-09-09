// CredentialsWorkbench：凭据表单主体（API/任务选项双 tab + 面板 + 保存行），
// 从 CredentialsDialog 抽出的双宿主组件：
//   1. SettingsHubDialog 的 API 区内嵌（常规入口，无二层弹窗）
//   2. CredentialsDialog（仅剩首次配置门 setupMode 一个场景）
// 两个宿主互斥挂载（设置是模态、门弹窗只从上传引导触发），BROWSER_IDS 的
// DOM id 不会同屏重复。状态/保存/校验全部走 useCredentialsController 的
// 单例 store——宿主只是壳。
//
// TaskOptionsPanel 常驻挂载（不随 tab 卸载）的约束沿用 CredentialsDialog
// 头注释结论：其字段 ref 在保存时被统一读取，卸载会复现"切到 API 面板点
// 保存，任务选项静默丢失"。

import { useState } from "react";
import { Tabs as TabsPrimitive } from "radix-ui";
import { CREDENTIAL_DOM_IDS } from "./credentials-dom-ids.js";
import { useCredentialsController } from "./useCredentialsController.js";
import { OcrProviderPanels } from "./OcrProviderPanels.jsx";
import { DeepSeekPanel } from "./DeepSeekPanel.jsx";
import { ChatModelPanel } from "./ChatModelPanel.jsx";
import { TaskOptionsPanel } from "./TaskOptionsPanel.jsx";
import { Button as ButtonBase } from "../../../../components/Button.jsx";

// Button.size 在未注解源文件里被推断为必填;unstyled 路径运行时不用 size。
const Button = ButtonBase as any;

const { browser: BROWSER_IDS } = CREDENTIAL_DOM_IDS;

const TABS = [
  { id: "api", label: "API 设置" },
  { id: "task", label: "任务选项" },
];

export function CredentialsWorkbench() {
  const { view, feature, handlers } = useCredentialsController();
  // 保存是异步写 localStorage + 桌面 snapshot/IPC,双击会并发跑两次
  const [saving, setSaving] = useState(false);

  const setupMode = Boolean(view.setupMode);
  const activeTab = view.activeTab || "api";
  const dialogStatus = view.dialogStatus || { message: "", tone: "" };
  const statusContent = `${dialogStatus.message || ""}`.trim();
  const statusClasses = [
    "upload-status",
    statusContent ? "" : "hidden",
    dialogStatus.tone === "valid" ? "is-valid" : "",
    dialogStatus.tone === "error" ? "is-error" : "",
  ].filter(Boolean).join(" ");

  return (
    <TabsPrimitive.Root
      className="contents"
      value={activeTab}
      onValueChange={(tab) => feature?.activateCredentialTab(tab)}
    >
      <div className="credential-workbench">
        <TabsPrimitive.List
          id={BROWSER_IDS.tabs}
          className={`developer-tabs credential-tabs${setupMode ? " hidden" : ""}`}
          aria-label="接口设置"
        >
          {TABS.map((tab) => (
            <TabsPrimitive.Trigger
              key={tab.id}
              value={tab.id}
              id={tab.id === "api" ? BROWSER_IDS.tabApi : BROWSER_IDS.tabTask}
              className={`developer-tab credential-tab${activeTab === tab.id ? " is-active" : ""}`}
              data-credential-tab={tab.id}
            >
              {tab.label}
            </TabsPrimitive.Trigger>
          ))}
        </TabsPrimitive.List>
        <div className="credential-panels">
          <TabsPrimitive.Content
            value="api"
            forceMount
            hidden={activeTab !== "api"}
            className={`credential-panel${activeTab === "api" ? " is-active" : ""}`}
            data-credential-panel="api"
          >
            <div className="credential-card-grid credential-card-grid-compact credential-api-grid">
              <section className="credential-card">
                <div className="credential-card-head">
                  <h3>OCR</h3>
                </div>
                <OcrProviderPanels />
              </section>
              <DeepSeekPanel />
              <ChatModelPanel />
            </div>
          </TabsPrimitive.Content>
          {/* 不套 TabsPrimitive.Content 的理由见 CredentialsDialog 原注释：
              TaskOptionsPanel 自带 role=tabpanel，再包一层语义重复 */}
          <TaskOptionsPanel hidden={activeTab !== "task"} />
        </div>
        <div className="actions credential-dialog-actions">
          <span id={BROWSER_IDS.status} className={statusClasses}>{statusContent}</span>
          <Button
            id={BROWSER_IDS.saveButton}
            className="app-button"
            disabled={saving}
            onClick={async () => {
              if (saving) return;
              setSaving(true);
              try {
                await handlers?.save?.();
              } finally {
                setSaving(false);
              }
            }}
          >
            {setupMode ? "保存并启动" : "保存"}
          </Button>
        </div>
      </div>
    </TabsPrimitive.Root>
  );
}

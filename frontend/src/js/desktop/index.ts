import { $ } from "../../shared/dom/query.js";
import {
  loadPersistedConfig,
  savePersistedDesktopConfig,
} from "../config/desktop-persistence.js";
import { savePersistedBrowserStoredConfig } from "../config/persisted-config.js";
import {
  applyDefaultCredentialInputs,
} from "../features/credentials/default-state-port.js";
import { createInitialState } from "../state/slices.js";
import {
  setDesktopConfigured,
  setDesktopMode,
  setDeveloperConfig,
} from "../state/actions.js";
import { getDeveloperConfig } from "../state/developer-state.js";
import { isDesktopConfigured } from "../state/desktop-state.js";
import {
  APP_DIALOG_IDS,
  APP_EVENTS,
} from "../contracts/app-contract.js";

// desktop 入口独占的 state 实例(原 js/state/store.ts 全局单例已下线,见架构评审 P2-6)。
// 导出仅供 desktop-first-run-smoke 脚本断言,其他模块不得 import。
export const desktopState = createInitialState();

export function showDesktopUi() {
  $("open-output-btn").classList.remove("hidden");
}

export function setDesktopBusy(message = "") {
  const targetIds = ["browser-credentials-status"];
  for (const id of targetIds) {
    const el = $(id);
    if (!el) {
      continue;
    }
    if (message) {
      el.textContent = message;
      el.classList.remove("hidden");
    } else {
      el.textContent = "";
      el.classList.add("hidden");
    }
  }
}

export function openSetupDialog() {
  document.dispatchEvent(new CustomEvent(APP_EVENTS.openBrowserCredentials, {
    detail: { setupMode: true },
  }));
}

export function closeSetupDialog() {
  const dialog = $(APP_DIALOG_IDS.browserCredentials) as any;
  if (dialog?.open && dialog.dataset.setupMode === "1") {
    dialog.close();
  }
}

export async function bootstrapDesktop(initialConfig = null) {
  setDesktopMode(desktopState, true);
  showDesktopUi();
  const payload = initialConfig || await loadPersistedConfig();
  setDeveloperConfig(desktopState, payload.developerConfig || {});
  applyDefaultCredentialInputs(payload.browserConfig || {});
  setDesktopConfigured(desktopState, payload.firstRunCompleted);
  if (!isDesktopConfigured(desktopState)) {
    openSetupDialog();
  } else {
    closeSetupDialog();
  }
}

export async function saveDesktopConfig(browserConfig: any = {}, afterSave) {
  const source = (typeof browserConfig === "object" && browserConfig !== null) ? browserConfig : {};
  const nextBrowserConfig = { ...(source.browserConfig || source) };
  const markConfigured = !!source.markConfigured;
  const callback = afterSave;
  let persisted = await savePersistedBrowserStoredConfig({
    ...nextBrowserConfig,
  });
  setDeveloperConfig(desktopState, persisted.developerConfig || getDeveloperConfig(desktopState));
  applyDefaultCredentialInputs(persisted.browserConfig || {});
  if (markConfigured && !persisted.firstRunCompleted) {
    persisted = await savePersistedDesktopConfig({ firstRunCompleted: true });
    setDeveloperConfig(desktopState, persisted.developerConfig || getDeveloperConfig(desktopState));
    applyDefaultCredentialInputs(persisted.browserConfig || {});
  }
  setDesktopConfigured(desktopState, persisted.firstRunCompleted);
  if (isDesktopConfigured(desktopState)) {
    closeSetupDialog();
    const errorBox = $("error-box") || $("error-box-inline");
    if (errorBox) {
      errorBox.textContent = "-";
      errorBox.classList?.add("hidden");
    }
  }
  if (callback) {
    try {
      await callback();
    } catch (error) {
      if (isDesktopConfigured(desktopState)) {
        const message = error?.message || String(error);
        throw new Error(`首次配置已保存，但当前无法连接本地后端。${message}`);
      }
      throw error;
    }
  }
  setDeveloperConfig(desktopState, persisted.developerConfig || getDeveloperConfig(desktopState));
  applyDefaultCredentialInputs(persisted.browserConfig || {});
  return persisted;
}

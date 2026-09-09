import { normalizeTranslationProvider } from "../../config/providers.js";
import {
  buildBrowserCredentialConfig,
  buildTaskOptionsFromDialogValues,
} from "./dialog-values.js";
import { createCredentialSetupModePort } from "./setup-mode-port.js";

export function persistBrowserCredentialsFromDialog({
  applyHiddenCredentialInputs,
  applyCredentialInputs = applyHiddenCredentialInputs,
  currentOcrProvider,
  defaultModelApiKey,
  defaultModelBaseUrl,
  readHiddenCredentialInputs: _readHiddenCredentialInputs,
  readCredentialInputs: _readCredentialInputs,
  saveTaskOptions,
  saveBrowserStoredConfig,
  values,
}: any) {
  // 直接持久化表单结果，避免再从 state/DOM 回读时拿到旧值
  const next = buildBrowserCredentialConfig({
    values,
    currentOcrProvider,
    defaultModelApiKey,
  });
  applyCredentialInputs(next);
  saveBrowserStoredConfig?.(next);
  saveTaskOptions?.(buildTaskOptionsFromDialogValues({
    values,
    defaultModelBaseUrl,
  }));
  return next;
}

export async function persistDesktopCredentialsFromDialog({
  currentOcrProvider,
  defaultModelApiKey,
  defaultModelBaseUrl,
  saveTaskOptions,
  saveDesktopConfig,
  checkApiConnectivity,
  values,
  setupModePort = createCredentialSetupModePort(),
}: any) {
  const provider = currentOcrProvider();
  const mineruToken = `${values.mineruToken || ""}`.trim();
  const paddleToken = values.paddleToken;
  // 与浏览器一致：只存用户在设置里填的 Key，不从 runtime 静默回填
  void defaultModelApiKey;
  const modelApiKey = `${values.modelApiKey || ""}`.trim();
  await saveDesktopConfig?.(
    {
      ocrProvider: provider,
      translationProvider: normalizeTranslationProvider(values.translationProvider),
      mineruToken,
      paddleToken,
      modelApiKey,
      chatModelApiKey: `${values.chatModelApiKey || ""}`.trim(),
      chatModelName: `${values.chatModelName || ""}`.trim(),
      chatModelBaseUrl: `${values.chatModelBaseUrl || ""}`.trim(),
      markConfigured: setupModePort.currentSetupMode(),
    },
    async () => {
      await checkApiConnectivity?.();
    },
  );
  saveTaskOptions?.(buildTaskOptionsFromDialogValues({
    values,
    defaultModelBaseUrl,
  }));
}

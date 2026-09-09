import { normalizeOcrProvider } from "../../config/providers.js";
import { createCredentialDialogElementsPort } from "./dialog-elements-port.js";

export function syncCredentialDialogFields({
  credentials,
  taskOptions = {},
  defaultModelBaseUrl,
  defaultModelApiKey,
  elementsPort = createCredentialDialogElementsPort(),
}: any) {
  const {
    mineruInput,
    paddleInput,
    apiKeyInput,
    modelBaseUrlInput,
    modelNameInput,
    chatModelApiKeyInput,
    chatModelBaseUrlInput,
    chatModelNameInput,
    mathModeSelect,
  } = elementsPort.elements();

  if (mineruInput) {
    mineruInput.value = credentials.mineruToken || "";
  }
  if (paddleInput) {
    paddleInput.value = credentials.paddleToken || "";
  }
  if (apiKeyInput) {
    // 只展示设置里已存的 Key，不从 runtime 回填（避免「设置空白却仍能问答」）
    void defaultModelApiKey;
    apiKeyInput.value = `${credentials.modelApiKey || ""}`.trim();
  }
  if (modelBaseUrlInput) {
    modelBaseUrlInput.value = taskOptions.baseUrl || defaultModelBaseUrl?.() || "";
  }
  if (modelNameInput) {
    modelNameInput.value = taskOptions.model || "";
  }
  // 对话模型三项都来自 credentials(不是 taskOptions):留空即回落到翻译模型
  if (chatModelApiKeyInput) {
    chatModelApiKeyInput.value = `${credentials.chatModelApiKey || ""}`.trim();
  }
  if (chatModelBaseUrlInput) {
    chatModelBaseUrlInput.value = `${credentials.chatModelBaseUrl || ""}`.trim();
  }
  if (chatModelNameInput) {
    chatModelNameInput.value = `${credentials.chatModelName || ""}`.trim();
  }
  if (mathModeSelect) {
    mathModeSelect.value = taskOptions.mathMode === "placeholder" ? "placeholder" : "direct_typst";
  }
  elementsPort.syncOcrProviderControls(normalizeOcrProvider(credentials.ocrProvider));
}

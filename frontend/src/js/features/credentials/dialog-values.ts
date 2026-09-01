import { normalizeOcrProvider, normalizeTranslationProvider } from "../../config/providers.js";
import { createCredentialDialogElementsPort } from "./dialog-elements-port.js";

/** Values read from the browser credential dialog inputs. */
export interface CredentialDialogValues {
  mineruToken: string;
  paddleToken: string;
  modelApiKey: string;
  modelBaseUrl: string;
  modelName: string;
  mathMode: string;
  translationProvider: string;
}

export interface CredentialDialogElementsLike {
  mineruInput?: { value?: string } | null;
  paddleInput?: { value?: string } | null;
  apiKeyInput?: { value?: string } | null;
  modelBaseUrlInput?: { value?: string } | null;
  modelNameInput?: { value?: string } | null;
  mathModeSelect?: { value?: string } | null;
  translationProviderSelect?: { value?: string } | null;
}

export interface ReadCredentialDialogValuesOptions {
  elementsPort?: {
    elements: () => CredentialDialogElementsLike;
  };
}

export interface BuildBrowserCredentialConfigOptions {
  values: Pick<CredentialDialogValues, "mineruToken" | "paddleToken" | "modelApiKey" | "translationProvider">;
  currentOcrProvider: () => string;
  defaultModelApiKey?: () => string;
}

export interface BuildTaskOptionsFromDialogValuesOptions {
  values: Pick<CredentialDialogValues, "modelName" | "modelBaseUrl" | "mathMode">;
  defaultModelBaseUrl?: () => string;
}

export function readCredentialDialogValues({
  elementsPort = createCredentialDialogElementsPort(),
}: ReadCredentialDialogValuesOptions = {}): CredentialDialogValues {
  const {
    mineruInput,
    paddleInput,
    apiKeyInput,
    modelBaseUrlInput,
    modelNameInput,
    mathModeSelect,
    translationProviderSelect,
  } = elementsPort.elements();
  return {
    mineruToken: mineruInput?.value?.trim() || "",
    paddleToken: paddleInput?.value?.trim() || "",
    modelApiKey: apiKeyInput?.value?.trim() || "",
    modelBaseUrl: modelBaseUrlInput?.value?.trim() || "",
    modelName: modelNameInput?.value?.trim() || "",
    mathMode: mathModeSelect?.value || "direct_typst",
    translationProvider: `${translationProviderSelect?.value || ""}`.trim(),
  };
}

export function buildBrowserCredentialConfig({
  values,
  currentOcrProvider,
  // defaultModelApiKey 保留参数兼容调用方，但不再静默写入设置（密钥只认对话框/用户输入）
  defaultModelApiKey: _defaultModelApiKey,
}: BuildBrowserCredentialConfigOptions) {
  return {
    ocrProvider: currentOcrProvider(),
    translationProvider: normalizeTranslationProvider(values.translationProvider),
    mineruToken: `${values.mineruToken || ""}`.trim(),
    paddleToken: values.paddleToken,
    modelApiKey: `${values.modelApiKey || ""}`.trim(),
  };
}

export function buildTaskOptionsFromDialogValues({
  values,
  defaultModelBaseUrl,
}: BuildTaskOptionsFromDialogValuesOptions) {
  return {
    model: values.modelName,
    baseUrl: values.modelBaseUrl || defaultModelBaseUrl?.() || "",
    mathMode: values.mathMode,
    translateTitles: true,
  };
}

export function ocrTokenFromDialogValues(
  values: Partial<Pick<CredentialDialogValues, "mineruToken" | "paddleToken">> = {},
  provider = "",
) {
  const normalized = normalizeOcrProvider(provider);
  if (normalized === "local") {
    return "";
  }
  return normalized === "paddle" ? values.paddleToken : values.mineruToken;
}

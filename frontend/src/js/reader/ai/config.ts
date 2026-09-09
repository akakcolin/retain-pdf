import {
  defaultModelBaseUrl,
  defaultModelName,
} from "../../config/runtime.js";
import {
  loadBrowserStoredConfig,
  loadDeveloperStoredConfig,
} from "../../config/persisted-config.js";
import { defaultCredentialsStatePort } from "../../features/credentials/default-state-port.js";

/** 取第一个 trim 后非空的字符串；空白 / 空串不算有效凭据。 */
function firstNonEmpty(...candidates: unknown[]): string {
  for (const candidate of candidates) {
    const value = `${candidate ?? ""}`.trim();
    if (value) {
      return value;
    }
  }
  return "";
}

/**
 * 读取「设置 → API 设置」里的模型 API Key。
 * 优先级：内存 credentials 状态 → 持久化配置（桌面 snapshot / localStorage）。
 * 不读 runtime-config 密钥。
 */
export function readSettingsModelApiKey(
  browserConfig = loadBrowserStoredConfig(),
): string {
  try {
    const live = defaultCredentialsStatePort.getCredentials?.()?.modelApiKey;
    const fromLive = `${live ?? ""}`.trim();
    if (fromLive) {
      return fromLive;
    }
  } catch {
    /* ignore */
  }
  return `${browserConfig?.modelApiKey ?? ""}`.trim();
}

export function resolveReaderAiConfig({
  browserConfig = loadBrowserStoredConfig(),
  developerConfig = loadDeveloperStoredConfig(),
} = {}) {
  // 模型 Key：仅用户设置；baseUrl / model 可回退 runtime 默认（非密钥）
  return {
    apiKey: readSettingsModelApiKey(browserConfig),
    baseUrl: firstNonEmpty(developerConfig?.baseUrl, defaultModelBaseUrl()),
    model: firstNonEmpty(developerConfig?.model, defaultModelName()),
    provider: firstNonEmpty(browserConfig?.translationProvider, "deepseek"),
  };
}

/**
 * 读取「设置 → API 设置 → AI 对话模型」的 Key。
 * 优先级同 readSettingsModelApiKey（内存状态 → 持久化配置）。
 */
export function readSettingsChatModelApiKey(
  browserConfig = loadBrowserStoredConfig(),
): string {
  try {
    const live = defaultCredentialsStatePort.getCredentials?.()?.chatModelApiKey;
    const fromLive = `${live ?? ""}`.trim();
    if (fromLive) {
      return fromLive;
    }
  } catch {
    /* ignore */
  }
  return `${browserConfig?.chatModelApiKey ?? ""}`.trim();
}

/**
 * AI 对话框（阅读器面板 / 首页问答）用的模型配置。
 * 对话模型三项留空时逐项回落到翻译模型，老用户零配置不回归；
 * 选中文字翻译不走这里（见 resolveReaderAiConfig）。
 */
export function resolveReaderChatConfig({
  browserConfig = loadBrowserStoredConfig(),
  developerConfig = loadDeveloperStoredConfig(),
} = {}) {
  const translation = resolveReaderAiConfig({ browserConfig, developerConfig });
  return {
    apiKey: firstNonEmpty(
      readSettingsChatModelApiKey(browserConfig),
      translation.apiKey,
    ),
    baseUrl: firstNonEmpty(browserConfig?.chatModelBaseUrl, translation.baseUrl),
    model: firstNonEmpty(browserConfig?.chatModelName, translation.model),
    provider: translation.provider,
  };
}

/** 是否已在设置中配置下游模型 API Key（翻译前置门禁）。 */
export function hasModelApiKey(): boolean {
  return Boolean(readSettingsModelApiKey());
}

/** 对话门禁：对话模型 Key 或翻译模型 Key 任一存在即可。 */
export function hasChatModelApiKey(): boolean {
  return Boolean(resolveReaderChatConfig().apiKey);
}

/** 凭据保存后派发，供 AI 输入门禁立刻刷新。 */
export const CREDENTIALS_CHANGED_EVENT = "retainpdf:credentials-changed";

export function notifyCredentialsChanged(): void {
  try {
    document.dispatchEvent(new CustomEvent(CREDENTIALS_CHANGED_EVENT));
  } catch {
    /* ignore non-DOM env */
  }
}

export const MISSING_MODEL_API_KEY_MESSAGE =
  "缺少模型 API Key：请到设置 → API 设置填写 DeepSeek 等模型 Key（不是后端 X-API-Key）。";

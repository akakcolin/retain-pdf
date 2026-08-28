export const DEFAULT_OCR_PROVIDER = "mineru";

export const OCR_PROVIDER_DEFINITIONS = [
  {
    id: "mineru",
    label: "MinerU",
    description: "在线 OCR。",
    tokenField: "mineru_token",
    runtimeConfigKey: "mineruToken",
    tokenLabel: "MinerU Access Token",
    tokenPlaceholder: "MinerU Access Token",
    validationButtonLabel: "检测 MinerU",
    validationIdleMessage: "未检测",
    validationMissingMessage: "请先填写 MinerU Access Token。",
    validationUnavailableMessage: "",
    docsUrl: "https://mineru.net/",
    docsLabel: "获取 Token",
    supportsValidation: true,
  },
  {
    id: "paddle",
    label: "PaddleOCR",
    description: "在线 OCR。",
    tokenField: "paddle_token",
    runtimeConfigKey: "paddleToken",
    tokenLabel: "Paddle Access Token",
    tokenPlaceholder: "Paddle Access Token",
    validationButtonLabel: "检测 Paddle",
    validationIdleMessage: "未检测",
    validationMissingMessage: "请先填写 Paddle Access Token。",
    validationUnavailableMessage: "",
    docsUrl: "https://aistudio.baidu.com/account/accessToken",
    docsLabel: "获取 Token",
    supportsValidation: true,
  },
];

export const DEFAULT_TRANSLATION_PROVIDER = "deepseek";

export const TRANSLATION_PROVIDER_DEFINITIONS = [
  {
    id: "deepseek",
    label: "DeepSeek",
    keyLabel: "DeepSeek Key",
    keyPlaceholder: "DeepSeek API Key",
    description: "翻译模型。",
    docsUrl: "https://platform.deepseek.com/api_keys",
    docsLabel: "获取 Key",
    validationButtonLabel: "检测 DeepSeek",
    validationIdleMessage: "未检测",
    validationMissingMessage: "请先填写 DeepSeek Key。",
    validationSuccessMessage: "DeepSeek 接口连接成功。",
    validationNetworkMessage: "DeepSeek 接口检测失败，请检查网络或浏览器跨域限制。",
    validationUnauthorizedMessage: "DeepSeek Key 无效或已过期。",
    balanceSupported: true,
  },
  {
    id: "openai-compatible",
    label: "OpenAI 兼容（自定义）",
    keyLabel: "API Key",
    keyPlaceholder: "API Key",
    description: "自定义 OpenAI 兼容端点。",
    docsUrl: "",
    docsLabel: "",
    validationButtonLabel: "检测连接",
    validationIdleMessage: "未检测",
    validationMissingMessage: "请先填写 API Key。",
    validationSuccessMessage: "OpenAI 兼容接口连接成功。",
    validationNetworkMessage: "OpenAI 兼容接口检测失败，请检查网络或浏览器跨域限制。",
    validationUnauthorizedMessage: "API Key 无效或已过期。",
    balanceSupported: false,
  },
];

export const TRANSLATION_PROVIDER_DEFINITION = TRANSLATION_PROVIDER_DEFINITIONS[0];

export function normalizeOcrProvider(value) {
  const provider = `${value || ""}`.trim().toLowerCase();
  return OCR_PROVIDER_DEFINITIONS.some((item) => item.id === provider) ? provider : DEFAULT_OCR_PROVIDER;
}

export function getOcrProviderDefinition(provider) {
  return OCR_PROVIDER_DEFINITIONS.find((item) => item.id === normalizeOcrProvider(provider)) || OCR_PROVIDER_DEFINITIONS[0];
}

export function normalizeTranslationProvider(value) {
  const provider = `${value || ""}`.trim().toLowerCase();
  return TRANSLATION_PROVIDER_DEFINITIONS.some((item) => item.id === provider) ? provider : DEFAULT_TRANSLATION_PROVIDER;
}

export function getTranslationProviderDefinition(provider) {
  return TRANSLATION_PROVIDER_DEFINITIONS.find((item) => item.id === normalizeTranslationProvider(provider)) || TRANSLATION_PROVIDER_DEFINITIONS[0];
}

// 源/目标语言注册表：整本翻译语言的唯一事实源。
// code 为跨层统一词表（前端 / Rust / Python 同一套）。displayName 是进入
// LLM 提示词的 target_language_name。Python 侧镜像见
// backend/scripts/services/translation/core/languages.py，改动需两处同步。

export type ScriptFamily = "zh" | "cjk" | "latin";

export interface LanguageEntry {
  code: string;
  /** 下拉展示名 */
  label: string;
  /** target_language_name：进入提示词的人类语言名 */
  displayName: string;
  /** 脚本族：zh/cjk/latin 决定校验与渲染启发式口径 */
  scriptFamily: ScriptFamily;
  /** 译文下载文件名前缀 */
  filePrefix: string;
}

export const AUTO_SOURCE_CODE = "auto";

export const DEFAULT_SOURCE_LANG = AUTO_SOURCE_CODE;
export const DEFAULT_TARGET_LANG = "zh-CN";
export const DEFAULT_TARGET_LANGUAGE_NAME = "简体中文";

export const LANGUAGE_ENTRIES: LanguageEntry[] = [
  { code: "zh-CN", label: "简体中文", displayName: "简体中文", scriptFamily: "zh", filePrefix: "zh" },
  { code: "zh-TW", label: "繁體中文", displayName: "繁體中文", scriptFamily: "zh", filePrefix: "zh-Hant" },
  { code: "en", label: "English", displayName: "English", scriptFamily: "latin", filePrefix: "en" },
  { code: "ja", label: "日本語", displayName: "日本語", scriptFamily: "cjk", filePrefix: "ja" },
  { code: "ko", label: "한국어", displayName: "한국어", scriptFamily: "cjk", filePrefix: "ko" },
  { code: "fr", label: "Français", displayName: "Français", scriptFamily: "latin", filePrefix: "fr" },
  { code: "de", label: "Deutsch", displayName: "Deutsch", scriptFamily: "latin", filePrefix: "de" },
  { code: "es", label: "Español", displayName: "Español", scriptFamily: "latin", filePrefix: "es" },
  { code: "ru", label: "Русский", displayName: "Русский", scriptFamily: "latin", filePrefix: "ru" },
];

const BY_CODE: Map<string, LanguageEntry> = new Map(
  LANGUAGE_ENTRIES.map((entry) => [entry.code, entry]),
);

export function lookupLanguage(code?: string | null): LanguageEntry | undefined {
  if (!code) {
    return undefined;
  }
  return BY_CODE.get(String(code).trim());
}

/** 源语言选项：先「自动检测」，再逐语种。 */
export function sourceOptions(): { code: string; label: string }[] {
  return [
    { code: AUTO_SOURCE_CODE, label: "自动检测" },
    ...LANGUAGE_ENTRIES.map((entry) => ({ code: entry.code, label: entry.label })),
  ];
}

/** 目标语言选项：不含自动。 */
export function targetOptions(): { code: string; label: string }[] {
  return LANGUAGE_ENTRIES.map((entry) => ({ code: entry.code, label: entry.label }));
}

export function normalizeSourceLang(value?: string | null): string {
  if (!value) {
    return DEFAULT_SOURCE_LANG;
  }
  const trimmed = String(value).trim();
  if (trimmed === AUTO_SOURCE_CODE || BY_CODE.has(trimmed)) {
    return trimmed;
  }
  return DEFAULT_SOURCE_LANG;
}

export function normalizeTargetLang(value?: string | null): string {
  if (!value) {
    return DEFAULT_TARGET_LANG;
  }
  const trimmed = String(value).trim();
  return BY_CODE.has(trimmed) ? trimmed : DEFAULT_TARGET_LANG;
}

/** 源语言的人类名（auto/未知 → ""，调用方自行兜底）。 */
export function sourceLanguageName(sourceLang?: string | null): string {
  return lookupLanguage(sourceLang)?.displayName ?? "";
}

/** 目标语言进入提示词的展示名（缺省简体中文）。 */
export function targetLanguageName(targetLang?: string | null): string {
  return lookupLanguage(normalizeTargetLang(targetLang))?.displayName ?? DEFAULT_TARGET_LANGUAGE_NAME;
}

/** 目标脚本族：决定英文残留校验与排版启发式口径。 */
export function targetScriptFamily(targetLang?: string | null): ScriptFamily {
  return lookupLanguage(normalizeTargetLang(targetLang))?.scriptFamily ?? "zh";
}

/** 译文下载文件名前缀（缺省 zh，保持旧命名）。 */
export function targetFilePrefix(targetLang?: string | null): string {
  return lookupLanguage(normalizeTargetLang(targetLang))?.filePrefix ?? "zh";
}

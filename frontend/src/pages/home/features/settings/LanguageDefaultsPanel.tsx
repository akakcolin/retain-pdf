// 设置 · 语言：整本翻译的全局默认源/目标语言（开发者级默认）。
// 真值：developer config（sourceLang/targetLang，与 workflow developer
// dialog 同一持久层）。「专业翻译」对话框内可再做任务级覆盖。

import { useState } from "react";
import { useHomeServices } from "../../home-services-context.js";
import {
  loadDeveloperStoredConfig,
  normalizeSourceLang,
  normalizeTargetLang,
  sourceOptions,
  targetOptions,
} from "../../composition/external.js";

export function LanguageDefaultsPanel() {
  const services = useHomeServices();
  const saved = loadDeveloperStoredConfig();
  const [sourceLang, setSourceLang] = useState(() => normalizeSourceLang(saved?.sourceLang));
  const [targetLang, setTargetLang] = useState(() => normalizeTargetLang(saved?.targetLang));

  function changeSource(code: string) {
    const next = normalizeSourceLang(code);
    setSourceLang(next);
    services.settingsHub.saveLanguageDefaults?.({ sourceLang: next, targetLang });
  }

  function changeTarget(code: string) {
    const next = normalizeTargetLang(code);
    setTargetLang(next);
    services.settingsHub.saveLanguageDefaults?.({ sourceLang, targetLang: next });
  }

  return (
    <div className="language-defaults" id="language-defaults-panel">
      <label className="professional-glossary-field">
        <span>原文语言</span>
        <select
          id="language-default-source-lang"
          value={sourceLang}
          onChange={(event) => changeSource(event.target.value)}
        >
          {sourceOptions().map((option) => (
            <option key={option.code} value={option.code}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
      <label className="professional-glossary-field">
        <span>译文语言</span>
        <select
          id="language-default-target-lang"
          value={targetLang}
          onChange={(event) => changeTarget(event.target.value)}
        >
          {targetOptions().map((option) => (
            <option key={option.code} value={option.code}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}

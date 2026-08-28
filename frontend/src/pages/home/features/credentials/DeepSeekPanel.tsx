// 翻译模型(翻译 Provider)卡片——顶部下拉选择 DeepSeek / OpenAI 兼容(自定义);
// 自定义端点时显示 base_url/model 可见输入(复用 browser-model-base-url/-name
// 槽位与 elementsRef.modelBaseUrlInput/modelNameInput,保存走 task-options
// 持久化),隐藏 DeepSeek 余额/充值;DeepSeek 保持原行为。

import { useEffect } from "react";
import { CREDENTIAL_DOM_IDS } from "./credentials-dom-ids.js";
import { useCredentialsController } from "./useCredentialsController.js";
import { TRANSLATION_PROVIDER_DEFINITIONS } from "../../composition/external.js";

const { browser: BROWSER_IDS } = CREDENTIAL_DOM_IDS;

function validationIcon(tone = "", content = "") {
  if (!content) {
    return "";
  }
  if (tone === "valid") {
    return "✓";
  }
  if (tone === "error") {
    return "!";
  }
  return "…";
}

function translationDefinition(providerId = "") {
  return TRANSLATION_PROVIDER_DEFINITIONS.find((item) => item.id === providerId)
    || TRANSLATION_PROVIDER_DEFINITIONS[0];
}

export function DeepSeekPanel() {
  const { credentials, view, handlers, elementsRef } = useCredentialsController();
  const translationProvider = credentials.translationProvider || TRANSLATION_PROVIDER_DEFINITIONS[0].id;
  const definition = translationDefinition(translationProvider);
  const isDeepSeek = translationProvider === "deepseek";
  const validation = view.deepSeek || { message: "", tone: "" };
  const content = `${validation.message || ""}`.trim();
  const badgeClasses = [
    "token-inline-status",
    content ? "" : "hidden",
    validation.tone === "valid" ? "is-valid" : "",
    validation.tone === "error" ? "is-error" : "",
    content && !validation.tone ? "is-pending" : "",
  ].filter(Boolean).join(" ");

  // provider 切换后,自定义端点的 base_url/model 可见输入在本组件 re-render
  // 时才挂载;等挂载完成再从存储态回填(syncCredentialFields → browser.js 的
  // syncBrowserDialogFromCredentialState)。
  useEffect(() => {
    handlers?.syncCredentialFields?.();
  }, [translationProvider]);

  return (
    <section className="credential-card">
      <div className="credential-card-head">
        <h3>翻译模型</h3>
      </div>
      <label>
        <span className="developer-label">
          <span>翻译 Provider</span>
        </span>
        <select
          id={BROWSER_IDS.translationProviderSelect}
          aria-label="翻译 Provider"
          value={translationProvider}
          ref={(node) => { elementsRef.translationProviderSelect = node || null; }}
          onChange={(event) => handlers?.changeTranslationProvider?.(event)}
        >
          {TRANSLATION_PROVIDER_DEFINITIONS.map((provider) => (
            <option key={provider.id} value={provider.id}>{provider.label}</option>
          ))}
        </select>
      </label>
      {!isDeepSeek ? (
        <>
          <label>
            <span className="developer-label">
              <span>Base URL</span>
            </span>
            <input
              id={BROWSER_IDS.modelBaseUrl}
              name="model_base_url"
              type="text"
              autoComplete="off"
              placeholder="https://api.example.com/v1"
              defaultValue=""
              ref={(node) => { elementsRef.modelBaseUrlInput = node || null; }}
              onInput={() => handlers?.resetDeepSeekValidation?.()}
            />
          </label>
          <label>
            <span className="developer-label">
              <span>模型名</span>
            </span>
            <input
              id={BROWSER_IDS.modelName}
              name="model_name"
              type="text"
              autoComplete="off"
              placeholder="gpt-4o-mini"
              defaultValue=""
              ref={(node) => { elementsRef.modelNameInput = node || null; }}
              onInput={() => handlers?.resetDeepSeekValidation?.()}
            />
          </label>
        </>
      ) : null}
      <label>
        <span className="credential-input-row">
          <span className="credential-secret-field">
            <input
              id={BROWSER_IDS.apiKey}
              type="password"
              autoComplete="off"
              placeholder={definition.keyPlaceholder}
              defaultValue=""
              ref={(node) => { elementsRef.apiKeyInput = node || null; }}
              onInput={() => handlers?.resetDeepSeekValidation?.()}
            />
          </span>
          {definition.docsUrl ? (
            <a className="credential-card-link" href={definition.docsUrl} target="_blank" rel="noopener noreferrer">
              {definition.docsLabel}
            </a>
          ) : null}
        </span>
      </label>
      <div className="credential-card-actions">
        <button
          id={BROWSER_IDS.deepSeekValidateButton}
          type="button"
          className="app-button secondary"
          onClick={() => handlers?.validateDeepSeek?.()}
        >
          {definition.validationButtonLabel}
        </button>
        <span id={BROWSER_IDS.deepSeekValidation} className={badgeClasses} title={content || definition.validationIdleMessage}>
          {validationIcon(validation.tone, content)}
        </span>
        {isDeepSeek ? (
          <a
            id={BROWSER_IDS.deepSeekTopUpLink}
            className={`credential-top-up-link${view.deepSeekTopUpVisible ? "" : " hidden"}`}
            href="https://platform.deepseek.com/top_up"
            target="_blank"
            rel="noopener noreferrer"
          >
            充值
          </a>
        ) : null}
      </div>
    </section>
  );
}

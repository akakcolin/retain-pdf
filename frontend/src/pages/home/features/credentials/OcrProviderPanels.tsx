// OCR provider 卡片(对照旧 components/dialogs/browser-credentials-dialog.js
// 的 ocrProviderPanels 拼接 + features/credentials/validation-view.js 的
// 校验徽标语义,死文件不 import,这里用 JSX 结构化重写)。
//
// OCR_PROVIDER_DEFINITIONS 注册 mineru/paddle 两个 provider(config/providers.js);
// 顶部下拉切换激活 provider(changeProvider → patchCredentials),面板按配置数组
// 渲染,不硬编码 provider id——未来加回 provider 只需扩数组。
// token 输入是非受控 ref(见 credentials-view-store.js elementsRef),
// dialog-values.js/dialog-sync.js(kept)直接读写 .value。

import { useEffect } from "react";
import {
  CREDENTIAL_DOM_IDS,
  credentialTokenInputId,
  credentialValidateButtonId,
  credentialValidationId,
} from "./credentials-dom-ids.js";
import { useCredentialsController } from "./useCredentialsController.js";
import { OCR_PROVIDER_DEFINITIONS } from "../../composition/external.js";

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

function resetHandlerFor(handlers) {
  return handlers?.resetPaddleValidation;
}

export function OcrProviderPanels() {
  const { credentials, view, handlers, tokenInputRef } = useCredentialsController();
  const activeProvider = OCR_PROVIDER_DEFINITIONS.some((item) => item.id === credentials.ocrProvider)
    ? credentials.ocrProvider
    : OCR_PROVIDER_DEFINITIONS[0].id;

  // provider 切换后，新激活面板的 token input 是已挂载的非受控节点
  // (defaultValue="")；从 store 回填已保存 token(镜像 DeepSeekPanel 做法)。
  useEffect(() => {
    handlers?.syncCredentialFields?.();
  }, [activeProvider]);

  return (
    <div className="credential-provider-panels">
      <label>
        <span className="developer-label">
          <span>OCR Provider</span>
        </span>
        <select
          id={CREDENTIAL_DOM_IDS.browser.ocrProviderSelect}
          aria-label="OCR Provider"
          value={activeProvider}
          onChange={(event) => handlers?.changeProvider?.(event)}
        >
          {OCR_PROVIDER_DEFINITIONS.map((provider) => (
            <option key={provider.id} value={provider.id}>{provider.label}</option>
          ))}
        </select>
      </label>
      {OCR_PROVIDER_DEFINITIONS.map((provider) => {
        const active = provider.id === activeProvider;
        const validation = view.validations[provider.id] || { message: "", tone: "" };
        const content = `${validation.message || ""}`.trim();
        const badgeClasses = [
          "token-inline-status",
          content ? "" : "hidden",
          validation.tone === "valid" ? "is-valid" : "",
          validation.tone === "error" ? "is-error" : "",
          content && !validation.tone ? "is-pending" : "",
        ].filter(Boolean).join(" ");

        return (
          <section
            key={provider.id}
            className={`credential-panel credential-provider-panel${active ? " is-active" : ""}`}
            data-ocr-provider-panel={provider.id}
            role="tabpanel"
            hidden={!active}
          >
            <label>
              <span className="credential-input-row">
                <span className="credential-secret-field">
                  <input
                    id={credentialTokenInputId(provider.id)}
                    type="password"
                    autoComplete="off"
                    placeholder={provider.tokenPlaceholder}
                    defaultValue=""
                    ref={tokenInputRef(provider.id)}
                    onInput={() => resetHandlerFor(handlers)?.()}
                  />
                </span>
                <a className="credential-card-link" href={provider.docsUrl} target="_blank" rel="noopener noreferrer">
                  {provider.docsLabel}
                </a>
              </span>
            </label>
            <div className="credential-card-actions">
              {provider.supportsValidation ? (
                <button
                  id={credentialValidateButtonId(provider.id)}
                  type="button"
                  className="app-button secondary"
                  onClick={() => handlers?.validateOcr?.()}
                >
                  {provider.validationButtonLabel}
                </button>
              ) : null}
              <span id={credentialValidationId(provider.id)} className={badgeClasses} title={content || provider.validationIdleMessage}>
                {validationIcon(validation.tone, content)}
              </span>
            </div>
          </section>
        );
      })}
    </div>
  );
}

// AI 对话模型卡片：阅读器 AI 面板 / 首页问答用的模型，与翻译模型分开。
// 三项都留空时回落到翻译模型的 Key/BaseURL/模型名（老用户零配置不回归）。
// 非受控 ref 槽位与 DeepSeekPanel 同一套契约，读写走 dialog-values/dialog-sync。

import { CREDENTIAL_DOM_IDS } from "./credentials-dom-ids.js";
import { useCredentialsController } from "./useCredentialsController.js";

const { browser: BROWSER_IDS } = CREDENTIAL_DOM_IDS;

export function ChatModelPanel() {
  const { elementsRef } = useCredentialsController();

  return (
    <section className="credential-card">
      <div className="credential-card-head">
        <h3>AI 对话模型</h3>
      </div>
      <label>
        <span className="credential-input-row">
          <span className="credential-secret-field">
            <input
              id={BROWSER_IDS.chatModelApiKey}
              type="password"
              autoComplete="off"
              placeholder="留空则沿用翻译模型的 API Key"
              defaultValue=""
              ref={(node) => { elementsRef.chatModelApiKeyInput = node || null; }}
            />
          </span>
        </span>
      </label>
      <label>
        <span className="developer-label">
          <span>Base URL</span>
        </span>
        <input
          id={BROWSER_IDS.chatModelBaseUrl}
          name="chat_model_base_url"
          type="text"
          autoComplete="off"
          placeholder="留空则沿用翻译模型端点"
          defaultValue=""
          ref={(node) => { elementsRef.chatModelBaseUrlInput = node || null; }}
        />
      </label>
      <label>
        <span className="developer-label">
          <span>模型名</span>
        </span>
        <input
          id={BROWSER_IDS.chatModelName}
          name="chat_model_name"
          type="text"
          autoComplete="off"
          placeholder="留空则沿用翻译模型名"
          defaultValue=""
          ref={(node) => { elementsRef.chatModelNameInput = node || null; }}
        />
      </label>
    </section>
  );
}

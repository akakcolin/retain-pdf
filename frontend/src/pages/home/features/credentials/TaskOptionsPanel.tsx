// 任务选项 tab(对照旧 components/dialogs/browser-credentials-dialog.js 的
// "task" 面板——公式模式下拉;模型地址/模型名不在旧模板里渲染但
// dialog-values.js/dialog-sync.js(kept)仍会读写对应字段,这里补上对应的
// 非受控 ref 容器,保持字段契约完整,不改变可见布局)。

import { useCredentialsController } from "./useCredentialsController.js";
import { CREDENTIAL_DOM_IDS } from "./credentials-dom-ids.js";

const { browser: BROWSER_IDS } = CREDENTIAL_DOM_IDS;

export function TaskOptionsPanel({ hidden = false } = {}) {
  const { credentials, elementsRef } = useCredentialsController();
  // 自定义 OpenAI 兼容端点时,base_url/model 可见输入改由 DeepSeekPanel 渲染
  // (复用同名 id + elementsRef 槽位);这里隐藏副本,避免重复 DOM id。
  const hideModelFields = credentials.translationProvider === "openai-compatible";

  return (
    <section
      className={`credential-card credential-panel${hidden ? "" : " is-active"}`}
      data-credential-panel="task"
      role="tabpanel"
      hidden={hidden}
    >
      <div className="credential-card-grid credential-card-grid-compact">
        <section className="credential-card">
          <div className="credential-card-head">
            <h3>任务选项</h3>
          </div>
          <label>
            <span className="developer-label">
              <span>公式模式</span>
            </span>
            <select
              id={BROWSER_IDS.mathMode}
              aria-label="公式模式"
              defaultValue="direct_typst"
              ref={(node) => { elementsRef.mathModeSelect = node || null; }}
            >
              <option value="placeholder">占位保护</option>
              <option value="direct_typst">直出公式</option>
            </select>
          </label>
          {/* 模型地址/模型名不在旧模板可见布局里,但 dialog-values.js/
              dialog-sync.js 仍读写这两个字段——保留隐藏字段契约,不新增可见 UI。
              自定义翻译端点时改由 DeepSeekPanel 渲染可见输入,这里跳过。 */}
          {!hideModelFields ? (
            <>
              <input
                id={BROWSER_IDS.modelBaseUrl}
                name="model_base_url"
                type="hidden"
                defaultValue=""
                ref={(node) => { elementsRef.modelBaseUrlInput = node || null; }}
              />
              <input
                id={BROWSER_IDS.modelName}
                name="model_name"
                type="hidden"
                defaultValue=""
                ref={(node) => { elementsRef.modelNameInput = node || null; }}
              />
            </>
          ) : null}
        </section>
      </div>
    </section>
  );
}

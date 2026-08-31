// 上传瓦片家族(对照 partials/main-content.html 的 .upload-tile-hero 区块逐 id 镜像)。
//
// 视图状态全部来自 upload/workflow 两个视图 store(纯逻辑控制器写入);
// 交互镜像旧 bindMainShellEvents + bindUploadTilePicker:
// - 瓦片空白处点击 → 触发文件选择(按钮/链接/输入除外)
// - #file click → prepareFilePicker(清空 value,保证重选同名文件也触发 change)
// - #file change → uploadFeature.handleFileSelected()
// - 页码区间 input → uploadFeature.constrainPageRanges({source})
// - #credential-gate-action → openBrowserCredentials（非 setupMode → 设置 → API）

import { useCallback } from "react";
import { useStoreSnapshot } from "../../../../shared/react/use-store.js";
import { useHomeServices } from "../../home-services-context.js";
import { APP_EVENTS } from "../../composition/external.js";

function CredentialGate({ visible }) {
  function handleGateAction(event) {
    event.preventDefault();
    document.dispatchEvent(new CustomEvent(APP_EVENTS.openBrowserCredentials));
  }

  return (
    <div id="credential-gate" className={`credential-gate${visible ? "" : " hidden"}`}>
      <div className="credential-gate-panel" aria-live="polite">
        <span className="credential-gate-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none">
            <path d="M8 11V8a4 4 0 1 1 8 0v3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
            <rect x="5" y="11" width="14" height="10" rx="3" stroke="currentColor" strokeWidth="1.5" />
            <circle cx="12" cy="16" r="1.2" fill="currentColor" />
          </svg>
        </span>
        <strong id="credential-gate-title">请先完成 API 设置</strong>
        <em id="credential-gate-help">在设置 → API 设置中填写 OCR Token 和 DeepSeek Key 后即可上传 PDF。</em>
        <button id="credential-gate-action" type="button" className="credential-gate-action" onClick={handleGateAction}>
          打开设置
        </button>
      </div>
    </div>
  );
}

function InlinePageRange({ upload, onConstrain, onPatch }) {
  const maxAttr = upload.pageRangeMax > 0 ? { max: `${upload.pageRangeMax}` } : {};

  function handleInput(source) {
    return (event) => {
      onPatch(source === "start"
        ? { pageRangeStart: event.target.value }
        : { pageRangeEnd: event.target.value });
      onConstrain(source);
    };
  }

  return (
    <div
      id="inline-page-range"
      className={`inline-page-range${upload.inlinePageRangeVisible ? "" : " hidden"}`}
      aria-label="翻译页码范围"
    >
      <label>
        <span>起始页</span>
        <input
          id="page-range-start"
          type="number"
          min="1"
          step="1"
          inputMode="numeric"
          autoComplete="off"
          placeholder="1"
          {...maxAttr}
          value={upload.pageRangeStart}
          onInput={handleInput("start")}
        />
      </label>
      <label>
        <span>结束页</span>
        <input
          id="page-range-end"
          type="number"
          min="1"
          step="1"
          inputMode="numeric"
          autoComplete="off"
          placeholder="总页数"
          {...maxAttr}
          value={upload.pageRangeEnd}
          onInput={handleInput("end")}
        />
      </label>
    </div>
  );
}

function TranslationBudgetNote({ budget }) {
  const classes = [
    "translation-budget-note",
    budget.visible ? "" : "hidden",
    budget.tone === "error" ? "is-error" : "",
    budget.tone === "valid" ? "is-valid" : "",
  ].filter(Boolean).join(" ");
  return (
    <div id="translation-budget-note" className={classes} aria-live="polite">
      {budget.visible ? budget.message : null}
      {budget.visible && budget.blocking ? (
        <>
          {" · "}
          <a href={budget.topUpUrl} target="_blank" rel="noopener noreferrer">去充值</a>
        </>
      ) : null}
    </div>
  );
}

export function HeroUpload() {
  const services = useHomeServices();
  const upload = useStoreSnapshot(services.stores.uploadView);
  const workflow = useStoreSnapshot(services.stores.workflowView);

  const fileInputRef = useCallback((node) => {
    services.uploadDomRefs.fileInput = node;
  }, [services]);

  // 镜像 bindUploadTilePicker:空白处点击代理到文件选择
  function handleTileClick(event) {
    const target = event.target;
    if (!(target instanceof HTMLElement)) {
      return;
    }
    if (target.closest("button") || target.closest("a") || target.closest("input")) {
      return;
    }
    const fileInput = services.uploadDomRefs.fileInput;
    if (!fileInput || fileInput.disabled) {
      return;
    }
    fileInput.click();
  }

  const tileClasses = [
    "upload-tile",
    "upload-tile-hero",
    upload.tileLocked ? "is-locked" : "",
    upload.ready ? "is-ready" : "",
    upload.uploading ? "is-uploading" : "",
  ].filter(Boolean).join(" ");

  return (
    <>
      <div className={tileClasses} onClick={handleTileClick}>
        <input
          id="file"
          name="file"
          type="file"
          accept="application/pdf,.pdf"
          ref={fileInputRef}
          disabled={!upload.tileEnabled}
          onClick={() => services.uploadDomRefs.fileInput && (services.uploadDomRefs.fileInput.value = "")}
          onChange={() => void services.features.uploadFeature?.handleFileSelected()}
        />
        <span
          id="upload-fill"
          className="upload-fill"
          aria-hidden="true"
          style={{ width: `${upload.progressPercent}%` }}
        ></span>
        <CredentialGate visible={upload.credentialGateVisible} />
        <span id="upload-glyph" className={`upload-glyph${upload.tileEnabled ? "" : " hidden"}`} aria-hidden="true">
          <span className="upload-glyph-h"></span>
          <span className="upload-glyph-v"></span>
        </span>
        <strong
          id="file-label"
          className={upload.labelVisible ? "" : "hidden"}
          title={upload.labelTitle}
        >
          {upload.label}
        </strong>
        <em id="upload-help" className={upload.helpVisible ? "" : "hidden"}>{upload.help}</em>
        <div className={`upload-meta upload-meta-inline${upload.tileEnabled ? "" : " hidden"}`}>
          <span>单个 PDF</span>
          <span>最大 50MB</span>
          <span>最多 999 页</span>
        </div>
        <div id="upload-status" className={`upload-status${upload.statusVisible ? "" : " hidden"}`}>
          {upload.status}
        </div>
        <div
          id="upload-progress-panel"
          className={`upload-progress-panel${upload.progressVisible ? "" : " hidden"}`}
          aria-live="polite"
        >
          <span id="upload-progress-text">{upload.progressText}</span>
        </div>
        <InlinePageRange
          upload={upload}
          onPatch={services.uploadViewActions.patch}
          onConstrain={(source) => services.features.uploadFeature?.constrainPageRanges({ source })}
        />
        <TranslationBudgetNote budget={workflow.budget} />
      </div>

      {/* 上传完成后给出明确二选一：直接翻译（提交 job）/ 仅收藏（关对话框入库） */}
      <div
        id="upload-ready-hint"
        className={`upload-ready-hint${upload.ready ? "" : " hidden"}`}
        aria-live="polite"
      >
        文件已就绪：可<strong>直接翻译</strong>、<strong>仅 OCR</strong>识别入库，或<strong>仅收藏</strong>到书架稍后再翻。
      </div>

      <div id="upload-action-slot" className={`upload-action-slot${upload.actionSlotVisible ? "" : " hidden"}`}>
        <div className="upload-action-group">
          <button
            id="page-range-btn"
            type="button"
            className={`page-range-mini secondary${workflow.pageRangeButtonVisible ? "" : " hidden"}`}
            aria-label="专业翻译设置"
            title="页码范围等专业选项"
            onClick={() => services.features.uploadFeature?.openPageRangeDialog()}
          >
            选项
          </button>
          <button
            id="store-only-btn"
            type="button"
            className={`secondary${upload.ready ? "" : " hidden"}`}
            disabled={!upload.ready || workflow.submitBusy}
            title="只加入书架，不开始翻译"
            onClick={() => services.library.actions.storeOnly?.()}
          >
            仅收藏
          </button>
          <button
            id="ocr-only-btn"
            type="button"
            className={`secondary${upload.ready ? "" : " hidden"}`}
            disabled={!upload.ready || workflow.submitBusy}
            title="只做 OCR 识别并入库，不翻译"
            onClick={() => {
              services.features.workflowFeature?.setWorkflowMode?.("ocr");
              (document.getElementById("job-form") as HTMLFormElement | null)?.requestSubmit?.();
            }}
          >
            仅 OCR
          </button>
          <button
            id="submit-btn"
            type="submit"
            disabled={workflow.submitDisabled || workflow.submitBusy}
            {...(workflow.submitBusy ? { "data-busy": "1" } : {})}
            title="上传完成后立即发起翻译任务"
          >
            {workflow.submitBusy ? "提交中…" : (workflow.submitLabel || "直接翻译")}
          </button>
        </div>
      </div>
    </>
  );
}

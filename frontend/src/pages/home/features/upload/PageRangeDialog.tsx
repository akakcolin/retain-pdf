// 专业翻译对话框(React 版 <page-range-dialog>,对照 components/dialogs/page-range-dialog.js)。
//
// Dialog 渲染层(阶段 C 收官批,shadcn 改造):从原生 <dialog>+showModal/close
// 换成 radix-ui 的 Dialog 原语(DialogPrimitive.Root/Portal/Overlay/Content),
// 继续用现有 desktop-dialog/desktop-shell 视觉体系,不套默认皮肤。开合状态
// 仍然是 uploadView store 的 pageRangeDialogOpen 字段(铁律:不改 store,只换
// 渲染层),onOpenChange(false) 统一走 uploadViewActions.patch 回写。
//
// 顺手修的真实 bug(蓝图早就记录、commit d238471 也提过的已知遗留):背板点击
// 原来触发的是 uploadFeature.applyPageRanges()(相当于"确认应用"),Escape
// 走的却是另一条只清 pageRangeDialogOpen 标志、不应用的路径——同一个对话框
// 两种关闭方式语义不一致,且和其余 8 个对话框"背板/Esc/关闭按钮都是纯关闭"
// 的约定不符。这里统一成纯关闭语义:三种关闭方式(背板点击走 Radix 的
// onPointerDownOutside/outside-click 检测、Esc、关闭按钮 DialogPrimitive.Close)
// 都只 patch pageRangeDialogOpen:false,不触发任何应用副作用。
//
// 确认过不会导致功能缺失:读 upload/controller.js#applyPageRanges 可知它的
// 全部实现就是 viewPort.closePageRangeDialog()——这个对话框早已没有独立的
// "确认后才提交"的字段(页码区间在上传区域的输入框里直接读写,术语表选择也是
// <select onChange> 直接写 store,两者都是实时生效,不经这个对话框把关)。也
// 就是说 apply 和"纯关闭"在当前实现下本来就是同一件事,统一语义不丢失任何
// 用户可达的操作路径:对话框内"完成"按钮(#page-range-apply-btn)依然在,
// 效果和背板点击/Esc 完全一致。
//
// 术语表下拉由 workflow store 的 glossaries/selectedGlossaryId 驱动
// (镜像 workflow/view.js setDeveloperGlossaryOptions 的选项语义,含
// 「已删除或不可用」兜底项)。
//
// 触发按钮(HeroUpload.jsx 的 #page-range-btn)和本对话框跨子树,Radix 默认
// 的 triggerRef 焦点归还失效,复用 use-dialog-return-focus.js(同其余 8 个
// 对话框的先例)。

import { Dialog as DialogPrimitive } from "radix-ui";
import { useStoreSnapshot } from "../../../../shared/react/use-store.js";
import { useHomeServices } from "../../home-services-context.js";
import { useDialogReturnFocus } from "../../../../shared/react/use-dialog-return-focus.js";

export function PageRangeDialog() {
  const services = useHomeServices();
  const upload = useStoreSnapshot(services.stores.uploadView);
  const workflow = useStoreSnapshot(services.stores.workflowView);

  const open = Boolean(upload.pageRangeDialogOpen);
  const { onCloseAutoFocus } = useDialogReturnFocus(open);

  // Esc / 背板点击 / 关闭按钮都经这一个回调回写 store,纯关闭,不触发应用副作用。
  function handleOpenChange(nextOpen) {
    if (!nextOpen) {
      services.uploadViewActions.patch({ pageRangeDialogOpen: false });
    }
  }

  const selectedId = `${workflow.selectedGlossaryId || ""}`.trim();
  const hasSelected = !selectedId
    || workflow.glossaries.some((glossary) => glossary.glossaryId === selectedId);

  return (
    <page-range-dialog data-hydrated="1">
      <DialogPrimitive.Root open={open} onOpenChange={handleOpenChange}>
        <DialogPrimitive.Portal>
          <DialogPrimitive.Overlay className="desktop-dialog-overlay" />
          <DialogPrimitive.Content
            id="page-range-dialog"
            className="desktop-dialog page-range-dialog professional-translate-dialog"
            onCloseAutoFocus={onCloseAutoFocus}
          >
            <div className="desktop-shell">
              <div className="desktop-head">
                <DialogPrimitive.Title asChild>
                  <h2 id="page-range-title">专业翻译</h2>
                </DialogPrimitive.Title>
                <DialogPrimitive.Close asChild>
                  <button id="page-range-close-btn" type="button" className="dialog-close-btn" aria-label="关闭">×</button>
                </DialogPrimitive.Close>
              </div>
              <div className="desktop-body">
                <p id="page-range-limit-text" className="muted">选择本次翻译使用的术语表。页码范围可直接在上传区域填写。</p>
                <label className="professional-glossary-field">
                  <span>术语表</span>
                  <select
                    id="job-glossary-id"
                    value={selectedId}
                    onChange={(event) => services.workflowViewActions.setSelectedGlossaryId(event.target.value)}
                  >
                    <option value="">不使用术语表</option>
                    {workflow.glossaries.map((glossary) => (
                      <option key={glossary.glossaryId} value={glossary.glossaryId}>
                        {glossary.name}
                        {Number.isFinite(glossary.entryCount) ? ` (${glossary.entryCount})` : ""}
                      </option>
                    ))}
                    {!hasSelected ? (
                      <option value={selectedId}>{`已删除或不可用: ${selectedId}`}</option>
                    ) : null}
                  </select>
                </label>
                <label className="professional-skip-ocr-field">
                  <input
                    id="skip-ocr-toggle"
                    type="checkbox"
                    checked={Boolean(workflow.skipOcr)}
                    onChange={(event) => services.workflowViewActions.setSkipOcr(event.target.checked)}
                  />
                  <span>跳过 OCR（PDF 自带文本层时直接提取）</span>
                </label>
                <div className="actions">
                  <button
                    id="page-range-clear-btn"
                    type="button"
                    className="app-button secondary"
                    onClick={() => services.features.uploadFeature?.clearPageRanges()}
                  >
                    不使用
                  </button>
                  <button
                    id="page-range-apply-btn"
                    type="button"
                    className="app-button"
                    onClick={() => services.features.uploadFeature?.applyPageRanges()}
                  >
                    完成
                  </button>
                </div>
              </div>
            </div>
          </DialogPrimitive.Content>
        </DialogPrimitive.Portal>
      </DialogPrimitive.Root>
    </page-range-dialog>
  );
}

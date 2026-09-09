// 新建/管理合集对话框(shadcn 改造后新增的第 10 个对话框,和其余 9 个同一套
// 路:DialogPrimitive.Root/Portal/Overlay/Content + desktop-dialog/
// desktop-shell + useDialogReturnFocus)。
//
// 交互借鉴参考项目 PDF_MD_lib 的 FolderManageModal(名称输入 + 从书库勾选),
// 简化成单栏勾选(不做手动排序——本次不做拖拽/排序,见调研计划「不做的事」)。
//
// dialogStore.payload = 正在编辑的 CollectionRecord,或 null(新建模式)。
// open() 由 CategoriesView.jsx 调用。这个对话框和 CategoriesView 是 HomeApp.jsx
// 下的兄弟节点(不是父子),保存/删除成功后没法直接 prop 回调回去——靠
// services.collections.reloadSignal(一个只有 version 字段的极简 store)桥接,
// 这里 bump 一次,CategoriesView 订阅到变化就重新拉取列表。

import { useEffect, useState } from "react";
import { Dialog as DialogPrimitive } from "radix-ui";
import { Button as ButtonBase } from "../../../../components/Button.jsx";

// Button.size 在未注解源文件里被推断为必填;unstyled 路径运行时不用 size。
const Button = ButtonBase as any;
import { useHomeServices } from "../../home-services-context.js";
import { useDialogState } from "../../state/use-dialog-state.js";
import { useDialogReturnFocus } from "../../../../shared/react/use-dialog-return-focus.js";

export function CollectionManageDialog() {
  const services = useHomeServices();
  const { controller, dialogStore, reloadSignal } = services.collections;
  const dialogState = useDialogState(dialogStore);
  const open = Boolean(dialogState.open);
  const editing = dialogState.payload;
  const isCreate = !editing;
  const { onCloseAutoFocus } = useDialogReturnFocus(open);

  const [name, setName] = useState("");
  const [allDocuments, setAllDocuments] = useState([]);
  const [selectedIds, setSelectedIds] = useState([]);
  const [originalIds, setOriginalIds] = useState([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!open) {
      // 关闭即清 busy:删除成功分支直接 close() 没有复位 saving,而对话框常驻
      // 挂载(HomeApp 顶层),下次打开会一直显示"保存中…"且保存/删除全禁用。
      setConfirmingDelete(false);
      setSaving(false);
      setLoading(false);
      return undefined;
    }
    let cancelled = false;
    setError("");
    setName(editing?.name || "");
    // 勾选态先清空再等成员列表回来:否则复用上一份 allDocuments 的旧列表会
    // 带着上次会话的勾选闪一下(新建模式尤其明显——memberIds 恒为空)。
    setSelectedIds([]);
    setOriginalIds([]);
    // 已有书目数据时 soft 重拉（切编辑目标），不整表 loading 闪空
    setLoading((prev) => (allDocuments.length === 0 ? true : prev));
    const documentsPromise = controller.listAllDocuments();
    const memberIdsPromise = editing
      ? controller.listCollectionDocumentIds(editing.collection_id)
      : Promise.resolve([]);
    Promise.all([documentsPromise, memberIdsPromise])
      .then(([documents, memberIds]) => {
        if (cancelled) {
          return;
        }
        setAllDocuments(documents);
        setSelectedIds(memberIds);
        setOriginalIds(memberIds);
      })
      .catch((err) => {
        if (cancelled) {
          return;
        }
        setError(err?.message || "加载书目失败，请稍后重试。");
      })
      .finally(() => {
        if (cancelled) {
          return;
        }
        setLoading(false);
      });
    // 关闭后快速为另一个合集重新打开(比如先编辑"化学"再编辑"机器学习"),
    // 两次 fetch 谁先 resolve 不确定——没有这个守卫的话,后关闭的那次请求
    // 如果晚到,会把已经在显示"机器学习"的表单覆盖回"化学"的书目数据。
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, editing?.collection_id]);

  function handleOpenChange(nextOpen) {
    if (!nextOpen) {
      dialogStore.close();
    }
  }

  function toggleDocument(documentId) {
    setSelectedIds((prev) => (prev.includes(documentId)
      ? prev.filter((id) => id !== documentId)
      : [...prev, documentId]));
  }

  async function handleSave() {
    const trimmed = name.trim();
    if (!trimmed) {
      setError("请输入合集名称。");
      return;
    }
    setSaving(true);
    setError("");
    try {
      let collectionId = editing?.collection_id || "";
      if (isCreate) {
        const created = await controller.createCollection({ name: trimmed }) as {
          collection_id?: string;
        };
        collectionId = created.collection_id || "";
      } else if (trimmed !== editing.name) {
        await controller.patchCollection(collectionId, { name: trimmed });
      }
      const toAdd = selectedIds.filter((id) => !originalIds.includes(id));
      const toRemove = originalIds.filter((id) => !selectedIds.includes(id));
      if (toAdd.length) {
        await controller.addDocuments(collectionId, toAdd);
      }
      for (const documentId of toRemove) {
        await controller.removeDocument(collectionId, documentId);
      }
      reloadSignal.actions.bump();
      dialogStore.close();
    } catch (err) {
      setError(err?.message || (isCreate ? "新建合集失败，请稍后重试。" : "保存失败，请稍后重试。"));
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete() {
    if (!confirmingDelete) {
      setConfirmingDelete(true);
      return;
    }
    setSaving(true);
    setError("");
    try {
      await controller.deleteCollection(editing.collection_id);
      reloadSignal.actions.bump();
      dialogStore.close();
    } catch (err) {
      setError(err?.message || "删除合集失败，请稍后重试。");
      setSaving(false);
    }
  }

  return (
    <DialogPrimitive.Root open={open} onOpenChange={handleOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="desktop-dialog-overlay" />
        <DialogPrimitive.Content
          id="collection-manage-dialog"
          className="desktop-dialog collection-manage-dialog"
          onCloseAutoFocus={onCloseAutoFocus}
        >
          <div className="desktop-shell">
            <div className="desktop-head">
              <DialogPrimitive.Title asChild>
                <h2>{isCreate ? "新建合集" : "管理合集"}</h2>
              </DialogPrimitive.Title>
              <DialogPrimitive.Close asChild>
                <button id="collection-manage-close-btn" type="button" className="dialog-close-btn" aria-label="关闭">×</button>
              </DialogPrimitive.Close>
            </div>
            <div className="desktop-body collection-manage-body">
              <label className="collection-name-field">
                <span>名称</span>
                <input
                  id="collection-name-input"
                  type="text"
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  placeholder="例如：化学"
                  autoFocus
                />
              </label>
              <div className="collection-doc-picker">
                <p className="muted">从书库勾选加入这个合集的书</p>
                {loading ? (
                  <div className="collection-doc-list-empty">正在加载书目…</div>
                ) : allDocuments.length === 0 ? (
                  <div className="collection-doc-list-empty">书库还没有书</div>
                ) : (
                  <ul className="collection-doc-list">
                    {allDocuments.map((doc) => (
                      <li key={doc.document_id}>
                        <label className="collection-doc-item">
                          <input
                            type="checkbox"
                            checked={selectedIds.includes(doc.document_id)}
                            onChange={() => toggleDocument(doc.document_id)}
                          />
                          <span className="collection-doc-title" title={doc.title}>{doc.title || doc.source_filename}</span>
                        </label>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
              {error ? <p className="collection-manage-error">{error}</p> : null}
            </div>
            <div className="collection-manage-actions">
              {!isCreate ? (
                <Button
                  id="collection-delete-btn"
                  className={`app-button secondary danger${confirmingDelete ? " is-confirming" : ""}`}
                  disabled={saving}
                  onClick={handleDelete}
                >
                  {confirmingDelete ? "确认删除？" : "删除合集"}
                </Button>
              ) : <span />}
              <Button
                id="collection-save-btn"
                className="app-button"
                disabled={saving || loading}
                onClick={handleSave}
              >
                {saving ? "保存中…" : "保存"}
              </Button>
            </div>
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

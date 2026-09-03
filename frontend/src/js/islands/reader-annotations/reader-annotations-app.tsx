import { useCallback, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  ANNOTATION_KIND_META,
  annotationAnchor,
  buildAnnotationsMarkdown,
  groupAnnotationsByPage,
} from "../../reader/annotations/view-model.js";

function AnnotationItem({ annotation, onJump, onDelete, onSaveNote }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const meta = ANNOTATION_KIND_META[annotation.kind] || ANNOTATION_KIND_META.sentence;

  const startEdit = () => {
    setDraft(annotation.note || "");
    setEditing(true);
  };

  const commit = async () => {
    setEditing(false);
    await onSaveNote(annotation, draft);
  };

  return (
    <div className="reader-annotations-item">
      <div className="reader-annotations-item-top">
        <span className={`reader-annotations-kind is-${annotation.kind}`}>{meta.label}</span>
        <div className="reader-annotations-actions">
          <button
            type="button"
            className="reader-annotations-locate"
            onClick={() => onJump(annotationAnchor(annotation))}
          >
            定位
          </button>
          <button
            type="button"
            className="reader-annotations-remove"
            onClick={() => onDelete(annotation)}
          >
            删除
          </button>
        </div>
      </div>
      <p className="reader-annotations-quote">{annotation.quoteText}</p>
      {annotation.translatedQuoteText
        ? <p className="reader-annotations-translated">{annotation.translatedQuoteText}</p>
        : null}
      {editing
        ? (
          <div className="reader-annotations-note-editor">
            <textarea
              className="reader-annotations-note-input"
              value={draft}
              placeholder="写点想法…"
              onChange={(event) => setDraft(event.target.value)}
            />
            <div className="reader-annotations-note-editor-actions">
              <button type="button" className="reader-annotations-note-save" onClick={commit}>保存</button>
              <button type="button" className="reader-annotations-note-cancel" onClick={() => setEditing(false)}>取消</button>
            </div>
          </div>
        )
        : annotation.note
          ? (
            <p className="reader-annotations-note" title="点击编辑笔记" onClick={startEdit}>
              {annotation.note}
            </p>
          )
          : (
            <button type="button" className="reader-annotations-note-add" onClick={startEdit}>
              添加笔记
            </button>
          )}
    </div>
  );
}

// 具名导出:reader 页(src/pages/reader)已打包,直接复用组件源码渲染进
// 批注抽屉(Phase 2b);mountReaderAnnotationsApp 保留给组件级测试当挂载入口。
// 「已复制」按钮反馈的展示时长（评审 P2-7，工程经验值）。
const EXPORT_COPIED_FEEDBACK_MS = 2000;

export function ReaderAnnotationsPanel({ ports }) {
  const [open, setOpen] = useState(false);
  const [annotations, setAnnotations] = useState([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const loadSeqRef = useRef(0);
  const copyTimerRef = useRef<any>(0);

  const load = useCallback(async () => {
    const seq = loadSeqRef.current + 1;
    loadSeqRef.current = seq;
    setLoading(true);
    setError("");
    try {
      const list = await ports.loadAnnotations();
      if (loadSeqRef.current !== seq) {
        return;
      }
      setAnnotations(Array.isArray(list) ? list : []);
    } catch (loadError) {
      if (loadSeqRef.current === seq) {
        setError(loadError?.message || "加载批注失败");
      }
    } finally {
      if (loadSeqRef.current === seq) {
        setLoading(false);
      }
    }
  }, [ports]);

  useEffect(() => ports.subscribeOpen((visible) => setOpen(Boolean(visible))), [ports]);

  // 首次可见时加载,之后每次重新变为可见时刷新
  useEffect(() => {
    if (open) {
      load();
    }
  }, [open, load]);

  useEffect(() => () => clearTimeout(copyTimerRef.current), []);

  const handleExport = useCallback(async () => {
    const markdown = buildAnnotationsMarkdown({
      title: ports.documentTitle(),
      annotations,
    });
    const ok = await ports.exportMarkdown(markdown);
    if (ok) {
      setCopied(true);
      clearTimeout(copyTimerRef.current);
      copyTimerRef.current = setTimeout(() => setCopied(false), EXPORT_COPIED_FEEDBACK_MS);
    }
  }, [ports, annotations]);

  const handleDelete = useCallback(async (annotation) => {
    // 乐观移除,失败恢复
    setAnnotations((current) => current.filter((item) => item.favoriteId !== annotation.favoriteId));
    let ok = false;
    try {
      ok = await ports.deleteAnnotation(annotation.favoriteId);
    } catch (_err) {
      ok = false;
    }
    if (!ok) {
      setAnnotations((current) => (
        current.some((item) => item.favoriteId === annotation.favoriteId)
          ? current
          : [...current, annotation]
      ));
    }
  }, [ports]);

  const handleSaveNote = useCallback(async (annotation, note) => {
    const previousNote = annotation.note;
    // 乐观更新,失败回滚
    setAnnotations((current) => current.map((item) => (
      item.favoriteId === annotation.favoriteId ? { ...item, note } : item
    )));
    let updated = null;
    try {
      updated = await ports.saveNote(annotation, note);
    } catch (_err) {
      updated = null;
    }
    setAnnotations((current) => current.map((item) => (
      item.favoriteId === annotation.favoriteId
        ? (updated || { ...item, note: previousNote })
        : item
    )));
  }, [ports]);

  if (!open) {
    return null;
  }

  const groups = groupAnnotationsByPage(annotations);

  return (
    <div className="reader-annotations-panel" role="region" aria-label="批注列表">
      <div className="reader-annotations-head">
        <span className="reader-annotations-count">{annotations.length} 条批注</span>
        <button
          type="button"
          className="reader-annotations-export"
          onClick={handleExport}
          disabled={copied}
        >
          {copied ? "已复制" : "导出 Markdown"}
        </button>
      </div>
      {loading
        ? <p className="reader-annotations-loading">加载批注中…</p>
        : error
          ? (
            <div className="reader-annotations-error">
              <p>{error}</p>
              <button type="button" className="reader-annotations-retry" onClick={load}>重试</button>
            </div>
          )
          : groups.length === 0
            ? <p className="reader-annotations-empty">暂无批注,框选原文即可创建</p>
            : groups.map((group) => (
              <section key={group.pageIdx} className="reader-annotations-group">
                <h4 className="reader-annotations-group-title">第 {group.pageIdx + 1} 页</h4>
                <div className="reader-annotations-group-items">
                  {group.items.map((annotation) => (
                    <AnnotationItem
                      key={annotation.favoriteId}
                      annotation={annotation}
                      onJump={ports.jumpToAnchor}
                      onDelete={handleDelete}
                      onSaveNote={handleSaveNote}
                    />
                  ))}
                </div>
              </section>
            ))}
    </div>
  );
}

export function mountReaderAnnotationsApp(host, ports) {
  const root = createRoot(host);
  root.render(<ReaderAnnotationsPanel ports={ports} />);
  return {
    unmount: () => root.unmount(),
  };
}

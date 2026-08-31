// 详情「翻译」Tab：页码范围 + 发起翻译 + 静默 attachJobProgress。
// 进度只在 bd-job-status-inner，不打开工作流弹窗。

import { useEffect, useState } from "react";

/**
 * @param {object} options
 * @param {boolean} options.open
 * @param {string} options.documentId
 * @param {number} options.pageCount
 * @param {object} options.actions library.actions（含 attachJobProgress / translateDocument）
 * @param {(key: string, fn: Function, fail: string) => Promise<void>} options.withBusy
 * @param {(msg: string) => void} options.setError
 * @param {() => void} [options.onTranslateStarted] 成功提交后切到翻译 Tab 等
 */
export function useBookDetailTranslate({
  open,
  documentId,
  pageCount,
  actions,
  withBusy,
  setError,
  onTranslateStarted,
}: any) {
  const [rangeOn, setRangeOn] = useState(false);
  const [startPage, setStartPage] = useState("1");
  const [endPage, setEndPage] = useState("");

  useEffect(() => {
    if (!open) {
      setRangeOn(false);
      setStartPage("1");
      setEndPage("");
    }
  }, [open, documentId]);

  useEffect(() => {
    if (pageCount && !endPage) {
      setEndPage(`${pageCount}`);
    }
  }, [pageCount, endPage]);

  async function handleTranslate() {
    const payload: any = {};
    if (rangeOn) {
      const s = Number(startPage);
      const e = Number(endPage);
      if (
        !Number.isInteger(s)
        || !Number.isInteger(e)
        || s < 1
        || e < s
        || (pageCount && e > pageCount)
      ) {
        setError(`页码范围不合法（1–${pageCount || "总页数"}）`);
        return;
      }
      payload.ocr = { page_ranges: `${s}-${e}` };
      // OCR 已按 page_ranges 裁剪 PDF(后端重建 PDF,页号从 1 重编号),translation 的
      // start/end 是相对裁剪后 pages 列表的 0-based 位置;这里翻译整段选中页(0..e-s),
      // 不能透传 1-based 原始页码(单页 1-1 会因 start>stop 直接失败)。
      payload.translation = { start_page: 0, end_page: e - s };
    }
    // 先切到翻译 Tab，保证 bd-job-status-inner 在视口内再接进度
    onTranslateStarted?.();
    await withBusy(
      "translate",
      async () => {
        // promoteDocumentToJob：改详情 payload + silent attachJobProgress
        // 不 openTranslationWorkflow
        await actions.translateDocument(documentId, payload);
        onTranslateStarted?.();
      },
      "发起翻译失败",
    );
  }

  return {
    rangeOn,
    startPage,
    endPage,
    setRangeOn,
    setStartPage,
    setEndPage,
    handleTranslate,
  };
}

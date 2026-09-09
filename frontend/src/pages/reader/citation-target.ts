// 引用点击的跳转目标：同文档就地跳页，跨文档整页跳到目标文献的对应页。
import {
  buildFrontendPageUrl,
  resolveCitationPageIdx,
  type AiCitationLike,
} from "./external.js";

export type CitationContext = {
  /** 当前阅读器任务 job_id（job 模式必有；阅读器 AI 面板只在 job 模式启用） */
  currentJobId?: string;
  /** 当前文档 document_id（document 模式；job 模式下为空） */
  currentDocumentId?: string;
};

export type CitationTarget =
  | { kind: "page"; page1: number }
  | { kind: "document"; url: string }
  | null;

/**
 * 跨文档判定：引用带 job_id，且与当前 job 不同（job 模式，阅读器常态）；
 * 当前无 job 时回落到 document_id 比较（document 模式）。
 * 判不出「当前」时一律回落页码，避免跳到错文档。
 */
export function resolveCitationTarget(
  citation: AiCitationLike | null | undefined,
  context: CitationContext = {},
): CitationTarget {
  if (!citation || typeof citation !== "object") {
    return null;
  }
  const citationJobId = `${citation.job_id || ""}`.trim();
  const citationDoc = `${citation.document_id || ""}`.trim();
  const currentJobId = `${context.currentJobId || ""}`.trim();
  const currentDoc = `${context.currentDocumentId || ""}`.trim();
  const pageIdx = resolveCitationPageIdx(citation);
  const crossDocument = Boolean(citationJobId)
    && (currentJobId
      ? citationJobId !== currentJobId
      : Boolean(currentDoc) && Boolean(citationDoc) && citationDoc !== currentDoc);
  if (crossDocument) {
    return {
      kind: "document",
      url: buildFrontendPageUrl("./reader.html", {
        job_id: citationJobId,
        page_idx: pageIdx === null ? "" : pageIdx,
        block_id: `${citation.block_id || ""}`.trim(),
      }),
    };
  }
  if (pageIdx === null) {
    return null;
  }
  return { kind: "page", page1: pageIdx + 1 };
}

// 概念图谱悬浮窗：本文档实体 → 证据（可跳页）/ 关系（可继续游走）。
// 建链零 LLM 成本；AI 抽取按一次模型调用计，先确认再发。

import { useCallback, useEffect, useRef, useState } from "react";
import { Network } from "lucide-react";
import {
  API_PREFIX,
  fetchDocumentByJobId,
  hasChatModelApiKey,
  MISSING_MODEL_API_KEY_MESSAGE,
  resolveReaderChatConfig,
} from "../../external.js";
import {
  extractDocumentGraph,
  linkDocumentGraph,
  listDocumentEntities,
  listEntityMentions,
  listEntityRelations,
  type EntityMention,
  type EntitySummary,
  type RelatedEntity,
} from "../../entities/api.js";
import {
  directionArrow,
  entityTypeLabel,
  mentionPageLabel,
  relationTypeLabel,
} from "../../entities/labels.js";
import { ReaderFloatShell } from "./ReaderFloatShell.js";

export type ReaderEntitiesPanelProps = {
  open: boolean;
  jobId: string;
  documentId: string;
  onClose: () => void;
  /** 1-based page jump */
  onJumpPage: (page: number) => void;
};

function errText(err: unknown, fallback: string) {
  return err instanceof Error && err.message ? err.message : fallback;
}

export function ReaderEntitiesPanel({
  open,
  jobId,
  documentId,
  onClose,
  onJumpPage,
}: ReaderEntitiesPanelProps) {
  const [docId, setDocId] = useState("");
  const [entities, setEntities] = useState<EntitySummary[]>([]);
  const [selected, setSelected] = useState<EntitySummary | null>(null);
  const [mentions, setMentions] = useState<EntityMention[]>([]);
  const [relations, setRelations] = useState<RelatedEntity[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState("");
  const listReq = useRef(0);
  const detailReq = useRef(0);

  const loadList = useCallback(async (id: string) => {
    const token = ++listReq.current;
    setLoading(true);
    setError("");
    try {
      const list = await listDocumentEntities(id);
      if (listReq.current === token) setEntities(list);
    } catch (err) {
      if (listReq.current === token) {
        setEntities([]);
        setError(errText(err, "读取概念图谱失败"));
      }
    } finally {
      if (listReq.current === token) setLoading(false);
    }
  }, []);

  // 打开时解析文档（任务会话只有 job_id）并拉实体概览
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setSelected(null);
    setMentions([]);
    setRelations([]);
    setNotice("");
    setError("");
    void (async () => {
      let id = `${documentId || ""}`.trim();
      let resolveFailed = false;
      if (!id && jobId) {
        try {
          const document = await fetchDocumentByJobId(API_PREFIX, jobId);
          id = `${document?.document_id || ""}`.trim();
        } catch {
          resolveFailed = true;
        }
      }
      if (cancelled) return;
      setDocId(id);
      if (!id) {
        setEntities([]);
        setError(resolveFailed ? "解析文档失败，请稍后重试。" : "当前没有可关联的文档");
        return;
      }
      await loadList(id);
    })();
    return () => {
      cancelled = true;
      listReq.current += 1;
    };
  }, [open, jobId, documentId, loadList]);

  const openEntity = useCallback(
    async (entity: EntitySummary) => {
      const token = ++detailReq.current;
      setSelected(entity);
      setMentions([]);
      setRelations([]);
      setLoading(true);
      setError("");
      try {
        const [nextMentions, nextRelations] = await Promise.all([
          listEntityMentions(entity.entity_id, { documentId: docId }),
          listEntityRelations(entity.entity_id),
        ]);
        if (detailReq.current !== token) return;
        setMentions(nextMentions);
        setRelations(nextRelations);
      } catch (err) {
        if (detailReq.current === token) {
          setError(errText(err, "读取实体详情失败"));
        }
      } finally {
        if (detailReq.current === token) setLoading(false);
      }
    },
    [docId],
  );

  const backToList = useCallback(() => {
    detailReq.current += 1;
    setSelected(null);
    setMentions([]);
    setRelations([]);
    setError("");
  }, []);

  const runLink = useCallback(async () => {
    if (!docId || busy) return;
    setBusy("link");
    setError("");
    setNotice("");
    try {
      const result = await linkDocumentGraph(docId);
      setNotice(`已扫描：${result.entities} 个实体 · ${result.mentions} 条证据`);
      backToList();
      await loadList(docId);
    } catch (err) {
      setError(errText(err, "扫描术语表失败"));
    } finally {
      setBusy("");
    }
  }, [docId, busy, backToList, loadList]);

  const runExtract = useCallback(async () => {
    if (!docId || busy) return;
    if (!hasChatModelApiKey()) {
      setError(MISSING_MODEL_API_KEY_MESSAGE);
      return;
    }
    if (!window.confirm("AI 抽取会调用一次模型（消耗 token），并重建本文档的抽取实体与关系。继续？")) {
      return;
    }
    setBusy("extract");
    setError("");
    setNotice("");
    try {
      const config = resolveReaderChatConfig();
      const result = await extractDocumentGraph(docId, {
        apiKey: config.apiKey,
        baseUrl: config.baseUrl,
        model: config.model,
      });
      setNotice(
        `抽取完成：${result.entities} 个实体 · ${result.mentions} 条证据 · ${result.relations} 条关系`,
      );
      backToList();
      await loadList(docId);
    } catch (err) {
      setError(errText(err, "AI 抽取失败"));
    } finally {
      setBusy("");
    }
  }, [docId, busy, backToList, loadList]);

  const toolbar = (
    <>
      <span className="reader-notes-count">
        {loading && !selected ? "加载中…" : `${entities.length} 个实体`}
      </span>
      <button
        type="button"
        className="reader-notes-export"
        disabled={!docId || Boolean(busy)}
        title="术语表种子 + 全文扫描，不调用模型"
        onClick={() => void runLink()}
      >
        {busy === "link" ? "扫描中…" : "扫描术语表"}
      </button>
      <button
        type="button"
        className="reader-notes-export"
        disabled={!docId || Boolean(busy)}
        title="调用一次模型抽取实体与关系"
        onClick={() => void runExtract()}
      >
        {busy === "extract" ? "抽取中…" : "AI 抽取"}
      </button>
    </>
  );

  return (
    <ReaderFloatShell
      id="reader-entities-panel"
      open={open}
      title="概念"
      subtitle="本文档实体 · 证据与关系"
      titleIcon={<Network size={14} strokeWidth={2.25} aria-hidden />}
      storageKey="retainpdf.reader.entities-float.pos.v1"
      ariaLabel="概念图谱"
      width={380}
      onClose={onClose}
      toolbar={toolbar}
    >
      {notice ? <p className="reader-entities-notice">{notice}</p> : null}
      {error ? (
        <p className="reader-notes-empty" role="alert">{error}</p>
      ) : null}

      {selected ? (
        <div className="reader-entities-detail">
          <div className="reader-entities-detail-head">
            <button type="button" className="reader-entities-back" onClick={backToList}>
              ← 返回
            </button>
            <span className="reader-entities-type">
              {entityTypeLabel(selected.entity_type)}
            </span>
          </div>
          <h4 className="reader-entities-name">{selected.name}</h4>
          {selected.aliases.length > 0 ? (
            <p className="reader-entities-aliases">
              别名：{selected.aliases.join(" / ")}
            </p>
          ) : null}

          <section className="reader-entities-section">
            <h5>
              证据
              <span className="reader-entities-section-count">{mentions.length}</span>
            </h5>
            {loading ? (
              <p className="reader-notes-empty">正在加载…</p>
            ) : mentions.length === 0 ? (
              <p className="reader-notes-empty">本文档暂无该实体的证据。</p>
            ) : (
              mentions.map((mention) => (
                <article
                  key={`${mention.document_id}:${mention.page_idx}:${mention.block_id}`}
                  className="reader-notes-item"
                >
                  <div className="reader-notes-item-top">
                    <button
                      type="button"
                      className="reader-notes-link"
                      onClick={() => onJumpPage(Math.max(1, (mention.page_idx || 0) + 1))}
                    >
                      {mentionPageLabel(mention.page_idx)}
                    </button>
                  </div>
                  <p className="reader-notes-quote">{mention.snippet}</p>
                </article>
              ))
            )}
          </section>

          <section className="reader-entities-section">
            <h5>
              关系
              <span className="reader-entities-section-count">{relations.length}</span>
            </h5>
            {loading ? (
              <p className="reader-notes-empty">正在加载…</p>
            ) : relations.length === 0 ? (
              <p className="reader-notes-empty">暂无关系。AI 抽取可产出实体间关系。</p>
            ) : (
              relations.map((relation) => (
                <article
                  key={`${relation.entity_id}:${relation.relation_type}:${relation.direction}`}
                  className="reader-entities-relation"
                >
                  <div className="reader-entities-relation-top">
                    <button
                      type="button"
                      className="reader-entities-relation-name"
                      onClick={() => void openEntity(relation)}
                    >
                      {relation.name}
                    </button>
                    <span className="reader-entities-relation-kind">
                      {relationTypeLabel(relation.relation_type)} {directionArrow(relation.direction)}
                    </span>
                  </div>
                  {relation.explanation ? (
                    <p className="reader-entities-relation-why">{relation.explanation}</p>
                  ) : null}
                </article>
              ))
            )}
          </section>
        </div>
      ) : loading && entities.length === 0 ? (
        <p className="reader-notes-empty">正在加载概念…</p>
      ) : entities.length === 0 && !error ? (
        <p className="reader-notes-empty">
          本文档还没有实体。点「扫描术语表」按术语表与正文建链（不调用模型），或用「AI 抽取」让模型抽实体与关系。
        </p>
      ) : (
        entities.map((entity) => (
          <button
            key={entity.entity_id}
            type="button"
            className="reader-entities-row"
            onClick={() => void openEntity(entity)}
          >
            <span className="reader-entities-row-name">{entity.name}</span>
            <span className="reader-entities-type">{entityTypeLabel(entity.entity_type)}</span>
            <span className="reader-entities-row-count">{entity.mention_count}</span>
          </button>
        ))
      )}
    </ReaderFloatShell>
  );
}

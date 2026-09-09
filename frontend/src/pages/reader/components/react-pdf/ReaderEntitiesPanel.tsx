// 概念图谱悬浮窗：本文档实体 → 证据（可跳页）/ 关系（可继续游走）。
// 建链零 LLM 成本；AI 抽取按一次模型调用计，先确认再发。

import { useCallback, useEffect, useRef, useState } from "react";
import { Network } from "lucide-react";
import {
  API_PREFIX,
  fetchDocumentByJobId,
  hasChatModelApiKey,
  injectCitationMarkers,
  MISSING_MODEL_API_KEY_MESSAGE,
  renderCitationFooter,
  renderFinalAnswerHtml,
  resolveReaderChatConfig,
  type AiCitationLike,
} from "../../external.js";
import {
  extractDocumentGraph,
  generateEntityPage,
  getEntityPage,
  linkDocumentGraph,
  listDocumentEntities,
  listEntityBacklinks,
  listEntityFavorites,
  listEntityMentions,
  listEntityRelations,
  listPendingEntityPages,
  revertEntityPage,
  saveEntityPage,
  type EntityBacklink,
  type EntityFavorite,
  type EntityMention,
  type EntityPage,
  type EntityPageLink,
  type EntitySummary,
  type RelatedEntity,
} from "../../entities/api.js";
import {
  directionArrow,
  entityTypeLabel,
  mentionPageLabel,
  relationTypeLabel,
} from "../../entities/labels.js";
import { injectWikiLinks } from "../../entities/wikilink.js";
import { ReaderFloatShell } from "./ReaderFloatShell.js";

/** 切实体只需这几个字段（列表行 / 关系 / wikilink 都能赋值）。 */
type EntityRef = Pick<EntitySummary, "entity_id" | "name" | "entity_type" | "aliases">;

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
  const [selected, setSelected] = useState<EntityRef | null>(null);
  const [mentions, setMentions] = useState<EntityMention[]>([]);
  const [relations, setRelations] = useState<RelatedEntity[]>([]);
  const [backlinks, setBacklinks] = useState<EntityBacklink[]>([]);
  const [favorites, setFavorites] = useState<EntityFavorite[]>([]);
  const [page, setPage] = useState<EntityPage | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState("");
  const [pageBusy, setPageBusy] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [batch, setBatch] = useState<{ done: number; total: number } | null>(null);
  const listReq = useRef(0);
  const detailReq = useRef(0);
  // token 失效（切文档/卸载）+ 用户停止，两个信号分开：前者不写状态，后者要报「已停止」。
  const batchReq = useRef(0);
  const batchStop = useRef(false);
  const pageBodyRef = useRef<HTMLDivElement | null>(null);

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
    setBacklinks([]);
    setFavorites([]);
    setPage(null);
    setEditing(false);
    setDraft("");
    setBatch(null);
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
      batchReq.current += 1;
    };
  }, [open, jobId, documentId, loadList]);

  const openEntity = useCallback(
    async (entity: EntityRef) => {
      const token = ++detailReq.current;
      setSelected(entity);
      setMentions([]);
      setRelations([]);
      setBacklinks([]);
      setFavorites([]);
      setPage(null);
      setEditing(false);
      setDraft("");
      setLoading(true);
      setError("");
      try {
        const [nextMentions, nextRelations, nextBacklinks, nextFavorites, nextPage] =
          await Promise.all([
            listEntityMentions(entity.entity_id, { documentId: docId }),
            listEntityRelations(entity.entity_id),
            listEntityBacklinks(entity.entity_id),
            listEntityFavorites(entity.entity_id),
            getEntityPage(entity.entity_id),
          ]);
        if (detailReq.current !== token) return;
        setMentions(nextMentions);
        setRelations(nextRelations);
        setBacklinks(nextBacklinks);
        setFavorites(nextFavorites);
        setPage(nextPage);
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
    setBacklinks([]);
    setFavorites([]);
    setPage(null);
    setEditing(false);
    setDraft("");
    setError("");
  }, []);

  const jumpToCitation = useCallback(
    (citation: AiCitationLike) => {
      const citationDoc = `${citation.document_id || ""}`.trim();
      if (citationDoc && docId && citationDoc !== docId) {
        setNotice(`引用来自《${citation.document_title || "其他文档"}》，不在当前文档中`);
        return;
      }
      const idx = Number(citation.page_idx);
      onJumpPage(Number.isFinite(idx) && idx >= 0 ? idx + 1 : 1);
    },
    [docId, onJumpPage],
  );

  const openWikiLink = useCallback(
    (link: EntityPageLink) => {
      void openEntity({
        entity_id: link.entity_id,
        name: link.name,
        entity_type: link.entity_type,
        aliases: link.aliases,
      });
    },
    [openEntity],
  );

  // 概念页正文渲染成安全 HTML 后注入容器，再把 [n] 换成可跳页按钮、[[X]] 换成实体链接。
  useEffect(() => {
    const host = pageBodyRef.current;
    if (!host || !page?.has_page) return;
    let cancelled = false;
    void (async () => {
      const html = await renderFinalAnswerHtml(page.body_md);
      if (cancelled || !pageBodyRef.current) return;
      pageBodyRef.current.innerHTML = html;
      const citationByRef = new Map<string, AiCitationLike>();
      for (const citation of page.citations) {
        citationByRef.set(`${citation.ref}`, citation);
      }
      injectCitationMarkers(pageBodyRef.current, citationByRef, jumpToCitation);
      renderCitationFooter(pageBodyRef.current, page.citations, {
        onJump: jumpToCitation,
        answerText: page.body_md,
      });
      const linkBySurface = new Map<string, EntityPageLink>();
      for (const link of page.links || []) {
        linkBySurface.set(link.surface, link);
      }
      injectWikiLinks(pageBodyRef.current, linkBySurface, openWikiLink);
    })();
    return () => {
      cancelled = true;
    };
  }, [page, jumpToCitation, openWikiLink]);

  // 非编辑态时把草稿同步成生效正文。只依赖正文文本与编辑态：
  // 依赖 page 对象会在保存同内容后重渲染时覆盖用户输入。
  useEffect(() => {
    if (!editing) setDraft(page?.body_md ?? "");
  }, [page?.body_md, editing]);

  const runGeneratePage = useCallback(async () => {
    if (!selected || pageBusy) return;
    if (!hasChatModelApiKey()) {
      setError(MISSING_MODEL_API_KEY_MESSAGE);
      return;
    }
    const overwriteManual = Boolean(page?.edited);
    const message = overwriteManual
      ? "重新生成会覆盖你的修订（模型原文与修订都会被新内容替换，消耗 token）。继续？"
      : "生成概念页会调用一次模型（消耗 token），并覆盖该实体的综述。继续？";
    if (!window.confirm(message)) {
      return;
    }
    setPageBusy(true);
    setError("");
    setNotice("");
    try {
      const config = resolveReaderChatConfig();
      const next = await generateEntityPage(
        selected.entity_id,
        {
          apiKey: config.apiKey,
          baseUrl: config.baseUrl,
          model: config.model,
        },
        { overwriteManual },
      );
      setPage(next);
      setEditing(false);
      setNotice("概念页已更新");
    } catch (err) {
      setError(errText(err, "生成概念页失败"));
    } finally {
      setPageBusy(false);
    }
  }, [selected, pageBusy, page?.edited]);

  const runSavePage = useCallback(async () => {
    if (!selected || saving) return;
    if (!draft.trim()) {
      setError("正文不能为空");
      return;
    }
    setSaving(true);
    setError("");
    setNotice("");
    try {
      const next = await saveEntityPage(selected.entity_id, draft);
      setPage(next);
      setEditing(false);
      setNotice("概念页修订已保存");
    } catch (err) {
      // 失败保持编辑态，别丢用户的长文。
      setError(errText(err, "保存概念页失败"));
    } finally {
      setSaving(false);
    }
  }, [selected, saving, draft]);

  const runRevertPage = useCallback(async () => {
    if (!selected || saving) return;
    if (!window.confirm("撤销你的修订，回到模型生成的版本？")) {
      return;
    }
    setSaving(true);
    setError("");
    setNotice("");
    try {
      const next = await revertEntityPage(selected.entity_id);
      setPage(next);
      setEditing(false);
      setNotice("已回到模型原文");
    } catch (err) {
      setError(errText(err, "撤销修订失败"));
    } finally {
      setSaving(false);
    }
  }, [selected, saving]);

  // 批量维护：顺序给本文档缺页/陈旧的实体生成概念页，可中止、单个失败不中断。
  const runBatchPages = useCallback(async () => {
    if (batch) {
      batchStop.current = true;
      return;
    }
    if (!docId || busy || pageBusy) return;
    if (!hasChatModelApiKey()) {
      setError(MISSING_MODEL_API_KEY_MESSAGE);
      return;
    }
    setError("");
    setNotice("");
    let pending;
    try {
      pending = await listPendingEntityPages(docId);
    } catch (err) {
      setError(errText(err, "读取待维护概念页失败"));
      return;
    }
    if (pending.length === 0) {
      setNotice("本文档实体都已有最新概念页。");
      return;
    }
    if (
      !window.confirm(
        `将为 ${pending.length} 个实体生成/刷新概念页，每个消耗一次模型调用。继续？`,
      )
    ) {
      return;
    }
    const config = resolveReaderChatConfig();
    const credentials = {
      apiKey: config.apiKey,
      baseUrl: config.baseUrl,
      model: config.model,
    };
    batchStop.current = false;
    const token = ++batchReq.current;
    let ok = 0;
    let failed = 0;
    let stopped = false;
    setBatch({ done: 0, total: pending.length });
    for (const [index, item] of pending.entries()) {
      if (batchReq.current !== token) return;
      if (batchStop.current) {
        stopped = true;
        break;
      }
      setBatch({ done: index, total: pending.length });
      try {
        await generateEntityPage(item.entity_id, credentials);
        ok += 1;
      } catch {
        failed += 1;
      }
    }
    if (batchReq.current !== token) return;
    setBatch(null);
    setNotice(
      `${stopped ? "已停止：" : ""}更新 ${ok} 个概念页${failed > 0 ? `，${failed} 个失败` : ""}`,
    );
    if (selected) void openEntity(selected);
  }, [batch, docId, busy, pageBusy, selected, openEntity]);

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
        {batch
          ? `生成中 ${batch.done}/${batch.total}`
          : loading && !selected
            ? "加载中…"
            : `${entities.length} 个实体`}
      </span>
      <button
        type="button"
        className="reader-notes-export"
        disabled={!docId || Boolean(busy) || Boolean(batch)}
        title="术语表种子 + 全文扫描，不调用模型"
        onClick={() => void runLink()}
      >
        {busy === "link" ? "扫描中…" : "扫描术语表"}
      </button>
      <button
        type="button"
        className="reader-notes-export"
        disabled={!docId || Boolean(busy) || Boolean(batch)}
        title="调用一次模型抽取实体与关系"
        onClick={() => void runExtract()}
      >
        {busy === "extract" ? "抽取中…" : "AI 抽取"}
      </button>
      <button
        type="button"
        className="reader-notes-export"
        disabled={!docId || Boolean(busy) || pageBusy}
        title="为本文档缺页或已陈旧的实体逐个生成概念页"
        onClick={() => void runBatchPages()}
      >
        {batch ? "停止" : "批量概念页"}
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
              概念页
              {page?.has_page ? (
                <span className="reader-entities-section-count">{page.citations.length}</span>
              ) : null}
            </h5>
            {!page ? (
              <p className="reader-notes-empty">正在加载…</p>
            ) : !page.has_page ? (
              <>
                <p className="reader-notes-empty">
                  还没有概念页。生成后会把跨文档证据合成为一份带引用的综述。
                </p>
                <button
                  type="button"
                  className="reader-notes-export"
                  disabled={pageBusy || Boolean(batch)}
                  onClick={() => void runGeneratePage()}
                >
                  {pageBusy ? "生成中…" : "生成概念页"}
                </button>
              </>
            ) : (
              <>
                <div className="reader-entities-page-meta">
                  {page.stale ? (
                    <span className="reader-entities-stale">证据已更新</span>
                  ) : null}
                  {page.edited ? (
                    <span className="reader-entities-stale">你的修订</span>
                  ) : null}
                  {!editing ? (
                    <button
                      type="button"
                      className="reader-notes-export"
                      disabled={saving || Boolean(batch)}
                      onClick={() => {
                        setDraft(page.body_md);
                        setEditing(true);
                        setError("");
                        setNotice("");
                      }}
                    >
                      编辑
                    </button>
                  ) : null}
                  {page.edited && !editing ? (
                    <button
                      type="button"
                      className="reader-notes-export"
                      disabled={saving || Boolean(batch)}
                      onClick={() => void runRevertPage()}
                    >
                      撤销修订
                    </button>
                  ) : null}
                  <button
                    type="button"
                    className="reader-notes-export"
                    disabled={pageBusy || saving || Boolean(batch)}
                    onClick={() => void runGeneratePage()}
                  >
                    {pageBusy ? "生成中…" : page.stale ? "重新生成" : "刷新"}
                  </button>
                </div>
                <div
                  className="reader-entities-page-body"
                  ref={pageBodyRef}
                  style={{ display: editing ? "none" : undefined }}
                />
                {editing ? (
                  <div className="reader-notes-editor">
                    <textarea
                      className="reader-notes-textarea"
                      rows={10}
                      maxLength={200000}
                      value={draft}
                      onChange={(event) => setDraft(event.target.value)}
                    />
                    <div className="reader-notes-editor-actions">
                      <button
                        type="button"
                        className="reader-notes-primary"
                        disabled={saving}
                        onClick={() => void runSavePage()}
                      >
                        {saving ? "保存中…" : "保存"}
                      </button>
                      <button
                        type="button"
                        className="reader-notes-link"
                        disabled={saving}
                        onClick={() => {
                          setEditing(false);
                          setDraft(page.body_md);
                        }}
                      >
                        取消
                      </button>
                    </div>
                  </div>
                ) : null}
              </>
            )}
          </section>

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
              我的标注
              <span className="reader-entities-section-count">{favorites.length}</span>
            </h5>
            {loading ? (
              <p className="reader-notes-empty">正在加载…</p>
            ) : favorites.length === 0 ? (
              <p className="reader-notes-empty">还没有关于它的标注。</p>
            ) : (
              favorites.map((favorite) => (
                <article key={favorite.favorite_id} className="reader-notes-item">
                  <div className="reader-notes-item-top">
                    <button
                      type="button"
                      className="reader-notes-link"
                      onClick={() => jumpToCitation(favorite)}
                    >
                      {favorite.document_title || "未命名文献"} · {mentionPageLabel(favorite.page_idx)}
                    </button>
                  </div>
                  <p className="reader-notes-quote">{favorite.quote_text}</p>
                  {favorite.translated_quote_text ? (
                    <p className="reader-notes-note">译文：{favorite.translated_quote_text}</p>
                  ) : null}
                  {favorite.note ? (
                    <p className="reader-notes-note">备注：{favorite.note}</p>
                  ) : null}
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

          <section className="reader-entities-section">
            <h5>
              被提及
              <span className="reader-entities-section-count">{backlinks.length}</span>
            </h5>
            {loading ? (
              <p className="reader-notes-empty">正在加载…</p>
            ) : backlinks.length === 0 ? (
              <p className="reader-notes-empty">还没有概念页提到它。</p>
            ) : (
              backlinks.map((backlink) => (
                <article key={backlink.entity_id} className="reader-entities-relation">
                  <div className="reader-entities-relation-top">
                    <button
                      type="button"
                      className="reader-entities-relation-name"
                      onClick={() =>
                        void openEntity({
                          entity_id: backlink.entity_id,
                          name: backlink.name,
                          entity_type: backlink.entity_type,
                          aliases: [],
                        })
                      }
                    >
                      {backlink.name}
                    </button>
                    <span className="reader-entities-type">
                      {entityTypeLabel(backlink.entity_type)}
                    </span>
                  </div>
                  {backlink.snippet ? (
                    <p className="reader-entities-relation-why">{backlink.snippet}</p>
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

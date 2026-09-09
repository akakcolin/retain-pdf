//! 概念页:把某实体散落在多篇文档里的证据合成为一份带引用的 Markdown 综述。
//! 一次 LLM 补全;正文 [n] 由 Rust 校验并重编号,模型不产出 block_id(会幻觉)。

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use crate::db::graph::normalize_entity_name;
use crate::db::Db;
use crate::error::AppError;
use crate::models::api::{
    EntityBacklink, EntityPageCitation, EntityPageEvidence, EntityPageLink, EntityPageRecord,
    EntityPageView, EntityRecord, PendingEntityPage, RelatedEntity,
};
use crate::models::domain::now_iso;
use crate::services::ai::llm::Chat;

use super::extract::strip_code_fence;
use super::GraphDeps;

const PAGE_MAX_EVIDENCE: u32 = 40;
const PAGE_MAX_RELATIONS: u32 = 30;

/// 待维护清单最多扫这么多候选实体(每实体 1~2 次查询)。
const PENDING_SCAN_CAP: u32 = 500;

const PAGE_SYSTEM_PROMPT: &str = "\
你是文献库的综述编辑。根据用户给出的证据片段,为指定实体写一份中文 Markdown 综述页。

要求:
- 只使用给出的证据,不得引入证据之外的知识或推测。
- 建议结构:一句话定义、核心要点、与其他概念的关系、证据不足之处。
- 每条事实性论断句末用 [n] 标注证据编号,n 取证据列表里的编号。
- 提到「已知关系」或证据里出现的其他实体时写成 [[实体名]];不确定的实体不要加链接。
- 证据不足以支撑的方面明确写「现有证据不足」,不要编造。
- 不要输出 entity_id、block_id、页码等内部标识。
- 直接输出 Markdown 正文,不要包裹代码块。";

static CITE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(\d+)\]").expect("cite regex"));

static WIKILINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\[\]\n]{1,80})\]\]").expect("wikilink regex"));

/// 正文里所有 [n] 一律删除。给模型看的版本用它,避免与工具结果的全局引用编号冲突。
pub(crate) fn strip_citation_markers(text: &str) -> String {
    CITE_RE.replace_all(text, "").into_owned()
}

/// 去掉 [[ ]] 只留实体名(给模型看的版本,避免它照抄括号)。
pub(crate) fn strip_wikilinks(text: &str) -> String {
    WIKILINK_RE.replace_all(text, "$1").into_owned()
}

/// 扫描正文里的 [[实体名]],按出现顺序去重并解析到实体;解析不到的直接跳过
/// (前端对未命中一律还原成纯文本,不报错)。
fn resolve_page_links(db: &Db, body: &str) -> Vec<EntityPageLink> {
    let mut links: Vec<EntityPageLink> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for capture in WIKILINK_RE.captures_iter(body) {
        let surface = capture[1].trim().to_string();
        if surface.is_empty() || seen.iter().any(|item| item == &surface) {
            continue;
        }
        seen.push(surface.clone());
        let Ok(Some(entity)) = db.resolve_entity(&normalize_entity_name(&surface)) else {
            continue;
        };
        links.push(EntityPageLink {
            surface,
            entity_id: entity.entity_id,
            name: entity.name,
            entity_type: entity.entity_type,
            aliases: entity.aliases,
        });
    }
    links
}

/// 反链:哪些已生成的概念页正文里 [[...]] 解析到本实体。读取时现算,不建表。
pub fn list_entity_backlinks(
    db: &Db,
    entity_id: &str,
    limit: u32,
) -> Result<Vec<EntityBacklink>, AppError> {
    let entity = db
        .get_entity(entity_id)
        .map_err(|_| entity_not_found(entity_id))?;
    let mut targets = vec![normalize_entity_name(&entity.name)];
    for alias in &entity.aliases {
        let norm = normalize_entity_name(alias);
        if !norm.is_empty() && !targets.iter().any(|item| item == &norm) {
            targets.push(norm);
        }
    }
    let mut backlinks = Vec::new();
    for (source_id, body) in db.list_entity_page_bodies()? {
        if source_id == entity.entity_id {
            continue;
        }
        let Some(snippet) = backlink_snippet(&body, &targets) else {
            continue;
        };
        let Ok(source) = db.get_entity(&source_id) else {
            continue;
        };
        backlinks.push(EntityBacklink {
            entity_id: source.entity_id,
            name: source.name,
            entity_type: source.entity_type,
            snippet,
        });
    }
    backlinks.sort_by(|left, right| left.name.cmp(&right.name));
    backlinks.truncate(limit as usize);
    Ok(backlinks)
}

/// 正文里第一处指向目标实体的 [[...]] 附近的上下文(前后各约 30 字符)。
fn backlink_snippet(body: &str, targets: &[String]) -> Option<String> {
    for capture in WIKILINK_RE.captures_iter(body) {
        let surface = normalize_entity_name(capture[1].trim());
        if !targets.iter().any(|target| target == &surface) {
            continue;
        }
        let Some(whole) = capture.get(0) else {
            continue;
        };
        let start = body[..whole.start()]
            .char_indices()
            .rev()
            .nth(30)
            .map(|(index, _)| index)
            .unwrap_or(0);
        let end = body[whole.end()..]
            .char_indices()
            .nth(30)
            .map(|(index, _)| whole.end() + index)
            .unwrap_or(body.len());
        let mut snippet = body[start..end].replace('\n', " ");
        snippet = snippet.trim().to_string();
        if start > 0 {
            snippet.insert(0, '…');
        }
        if end < body.len() {
            snippet.push('…');
        }
        return Some(snippet);
    }
    None
}

/// 待维护的概念页:该文档里缺页或页已陈旧的实体,陈旧的排前。
/// 候选来自该文档的 block_entities,所以只在关系里出现、本文档无提及的实体看不见。
/// ponytail: 每实体 1~2 次查询的线性扫描,候选集几千前够用;要快再一条 SQL 现算签名。
pub fn list_pending_entity_pages(
    db: &Db,
    document_id: &str,
    limit: u32,
) -> Result<Vec<PendingEntityPage>, AppError> {
    let mut items = Vec::new();
    for summary in db.entities_for_document(document_id, PENDING_SCAN_CAP)? {
        let page = db.get_entity_page(&summary.entity_id)?;
        let has_page = page.is_some();
        // 只对有页的实体算签名:缺页必然要生成,不必多查两次。
        let stale = match &page {
            Some(page) => page.evidence_sig != db.entity_page_evidence_sig(&summary.entity_id)?,
            None => false,
        };
        if has_page && !stale {
            continue;
        }
        items.push(PendingEntityPage {
            entity_id: summary.entity_id,
            name: summary.name,
            entity_type: summary.entity_type,
            has_page,
            stale,
        });
    }
    // 稳定排序:陈旧的排前,组内保持 entities_for_document 的提及数降序。
    items.sort_by_key(|item| !item.stale);
    items.truncate(limit as usize);
    Ok(items)
}

/// 生成/刷新概念页。模型调用成功后才覆盖旧页(失败保留上一次结果)。
pub async fn generate_entity_page<C: Chat>(
    deps: &GraphDeps<'_>,
    client: &C,
    entity_id: &str,
) -> Result<EntityPageView, AppError> {
    let entity = deps
        .db
        .get_entity(entity_id)
        .map_err(|_| entity_not_found(entity_id))?;
    let evidence = deps.db.list_entity_page_evidence(entity_id, PAGE_MAX_EVIDENCE)?;
    let relations = deps.db.related_entities(entity_id, None, PAGE_MAX_RELATIONS)?;
    if evidence.is_empty() && relations.is_empty() {
        return Err(AppError::bad_request(
            "该实体还没有证据,请先对相关文档做图谱抽取",
        ));
    }
    let messages = vec![
        json!({"role": "system", "content": PAGE_SYSTEM_PROMPT}),
        json!({
            "role": "user",
            "content": build_page_prompt(&entity, &evidence, &relations),
        }),
    ];
    // 空 tools = 纯回答,不触发 function calling。
    let message = client.chat(messages, Vec::new()).await?;
    let (body_md, citations) = parse_page_body(&message.content, &evidence);
    if body_md.is_empty() {
        return Err(AppError::bad_gateway("模型没有产出综述正文"));
    }
    let evidence_sig = deps.db.entity_page_evidence_sig(entity_id)?;
    let links = resolve_page_links(deps.db, &body_md);
    let record = EntityPageRecord {
        entity_id: entity.entity_id.clone(),
        body_md,
        citations,
        evidence_sig: evidence_sig.clone(),
        generated_at: now_iso(),
    };
    deps.db.upsert_entity_page(&record)?;
    Ok(page_view(&entity, Some(record), &evidence_sig, links))
}

/// 读概念页。无页时 has_page=false(实体存在,不是 404)。
pub fn get_entity_page(deps: &GraphDeps<'_>, entity_id: &str) -> Result<EntityPageView, AppError> {
    let entity = deps
        .db
        .get_entity(entity_id)
        .map_err(|_| entity_not_found(entity_id))?;
    let page = deps.db.get_entity_page(entity_id)?;
    let current_sig = deps.db.entity_page_evidence_sig(entity_id)?;
    let links = page
        .as_ref()
        .map(|page| resolve_page_links(deps.db, &page.body_md))
        .unwrap_or_default();
    Ok(page_view(&entity, page, &current_sig, links))
}

fn page_view(
    entity: &EntityRecord,
    page: Option<EntityPageRecord>,
    current_sig: &str,
    links: Vec<EntityPageLink>,
) -> EntityPageView {
    let (has_page, stale, generated_at, body_md, citations) = match page {
        Some(page) => (
            true,
            page.evidence_sig != current_sig,
            page.generated_at,
            page.body_md,
            page.citations,
        ),
        None => (false, false, String::new(), String::new(), Vec::new()),
    };
    EntityPageView {
        entity_id: entity.entity_id.clone(),
        name: entity.name.clone(),
        entity_type: entity.entity_type.clone(),
        has_page,
        stale,
        generated_at,
        body_md,
        citations,
        links,
    }
}

fn entity_not_found(entity_id: &str) -> AppError {
    AppError::not_found(format!("entity not found: {entity_id}"))
}

fn build_page_prompt(
    entity: &EntityRecord,
    evidence: &[EntityPageEvidence],
    relations: &[RelatedEntity],
) -> String {
    let mut out = String::new();
    out.push_str(&format!("实体:{}\n类型:{}\n", entity.name, entity.entity_type));
    if !entity.aliases.is_empty() {
        out.push_str(&format!("别名:{}\n", entity.aliases.join(" / ")));
    }
    if !entity.description.trim().is_empty() {
        out.push_str(&format!("已有描述:{}\n", entity.description.trim()));
    }
    if !relations.is_empty() {
        out.push_str("\n已知关系:\n");
        for relation in relations {
            let arrow = if relation.direction == "out" { "→" } else { "←" };
            let why = relation.explanation.trim();
            let suffix = if why.is_empty() {
                String::new()
            } else {
                format!(":{why}")
            };
            out.push_str(&format!(
                "- {} {} {} ({}){suffix}\n",
                entity.name, arrow, relation.name, relation.relation_type
            ));
        }
    }
    out.push_str(&format!("\n证据片段(共 {} 条):\n", evidence.len()));
    for (index, item) in evidence.iter().enumerate() {
        let title = match item.document_title.trim() {
            "" => "未命名文献",
            title => title,
        };
        out.push_str(&format!(
            "[{}] 《{}》第 {} 页:{}\n",
            index + 1,
            title,
            item.page_idx + 1,
            item.snippet.trim()
        ));
    }
    out
}

/// 校验并重编号 [n]:越界引用直接删掉,按首次出现顺序压成 1..M,
/// citations 与正文编号一一对应。
fn parse_page_body(
    raw: &str,
    evidence: &[EntityPageEvidence],
) -> (String, Vec<EntityPageCitation>) {
    let body = strip_code_fence(raw.trim());
    let mut out = String::new();
    let mut citations: Vec<EntityPageCitation> = Vec::new();
    let mut slot_of: BTreeMap<usize, usize> = BTreeMap::new();
    let mut cursor = 0usize;
    for capture in CITE_RE.captures_iter(body) {
        let Some(whole) = capture.get(0) else {
            continue;
        };
        let number: usize = capture[1].parse().unwrap_or(0);
        out.push_str(&body[cursor..whole.start()]);
        cursor = whole.end();
        if number == 0 || number > evidence.len() {
            continue;
        }
        let slot = match slot_of.get(&number) {
            Some(slot) => *slot,
            None => {
                let slot = citations.len();
                slot_of.insert(number, slot);
                let item = &evidence[number - 1];
                citations.push(EntityPageCitation {
                    ref_num: (slot + 1) as i64,
                    document_id: item.document_id.clone(),
                    document_title: item.document_title.clone(),
                    job_id: item.job_id.clone(),
                    page_idx: item.page_idx,
                    block_id: item.block_id.clone(),
                    snippet: item.snippet.clone(),
                });
                slot
            }
        };
        out.push_str(&format!("[{}]", slot + 1));
    }
    out.push_str(&body[cursor..]);
    (out.trim().to_string(), citations)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use futures_util::future::BoxFuture;
    use serde_json::Value;

    use crate::db::Db;
    use crate::models::api::{BlockEntityLink, NewEntity};
    use crate::models::{now_iso, UploadRecord};
    use crate::services::ai::llm::{AssistantMessage, Chat};

    use super::*;

    struct ScriptedChat {
        reply: String,
        seen: Mutex<Vec<Value>>,
    }

    impl Chat for ScriptedChat {
        fn chat(
            &self,
            messages: Vec<Value>,
            tools: Vec<Value>,
        ) -> BoxFuture<'static, Result<AssistantMessage, AppError>> {
            self.seen.lock().expect("lock").push(json!({
                "messages": messages,
                "tools": tools,
            }));
            let reply = self.reply.clone();
            Box::pin(async move {
                Ok(AssistantMessage {
                    content: reply,
                    tool_calls: Vec::new(),
                })
            })
        }
    }

    struct Fixture {
        root: PathBuf,
        db: Db,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn fixture(name: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "graph-page-{name}-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        let data_root = root.join("data");
        fs::create_dir_all(&data_root).expect("data root");
        fs::create_dir_all(root.join("db")).expect("db dir");
        let db = Db::new(root.join("db").join("jobs.db"), data_root);
        db.init().expect("init");
        Fixture { root, db }
    }

    fn seed(db: &Db) -> String {
        db.upsert_document_from_upload(&UploadRecord {
            upload_id: "up-1".to_string(),
            filename: "GNN 综述.pdf".to_string(),
            stored_path: "uploads/x/paper.pdf".to_string(),
            bytes: 10,
            page_count: 2,
            uploaded_at: now_iso(),
            developer_mode: false,
            content_hash: "doc-1".to_string(),
        })
        .expect("insert document");
        let entity = db
            .upsert_entity(&NewEntity {
                name: "GNN".to_string(),
                entity_type: "method".to_string(),
                aliases: vec!["图神经网络".to_string()],
                description: "图神经网络".to_string(),
            })
            .expect("entity");
        for (index, block) in ["p001-b0000", "p001-b0001"].iter().enumerate() {
            db.link_block_entity(&BlockEntityLink {
                document_id: "doc-1".to_string(),
                entity_id: entity.entity_id.clone(),
                page_idx: 0,
                block_id: block.to_string(),
                job_id: "job-1".to_string(),
                surface_form: "GNN".to_string(),
                snippet: format!("GNN 片段 {index}"),
                confidence: 1.0,
                source: "extraction".to_string(),
            })
            .expect("link");
        }
        entity.entity_id
    }

    /// 给指定实体写一张概念页(反链测试用)。
    fn add_page_of(db: &Db, entity_id: &str, body: &str) {
        db.upsert_entity_page(&EntityPageRecord {
            entity_id: entity_id.to_string(),
            body_md: body.to_string(),
            citations: Vec::new(),
            evidence_sig: String::new(),
            generated_at: now_iso(),
        })
        .expect("page");
    }

    /// 建一个新实体并写页,返回 entity_id(反链测试用)。
    fn add_page(db: &Db, name: &str, body: &str) -> String {
        let entity = db
            .upsert_entity(&NewEntity {
                name: name.to_string(),
                entity_type: "concept".to_string(),
                aliases: Vec::new(),
                description: String::new(),
            })
            .expect("entity");
        add_page_of(db, &entity.entity_id, body);
        entity.entity_id
    }

    #[tokio::test]
    async fn page_renumbers_and_drops_out_of_range_citations() {
        let fixture = fixture("renumber");
        let entity_id = seed(&fixture.db);
        let client = ScriptedChat {
            // [9] 越界应被丢弃;[2] 先出现,故压成 [1],[1] 变成 [2]
            reply: "```markdown\nGNN 是一种方法 [9],覆盖 QM9 [2],定义见 [1]。\n```".to_string(),
            seen: Mutex::new(Vec::new()),
        };
        let deps = GraphDeps {
            db: &fixture.db,
            data_root: &fixture.root.join("data"),
        };
        let view = generate_entity_page(&deps, &client, &entity_id)
            .await
            .expect("generate");
        assert!(view.has_page);
        assert!(!view.stale);
        assert!(!view.body_md.contains("```"));
        assert!(!view.body_md.contains("[9]"));
        assert!(view.body_md.contains("[1]"));
        assert!(view.body_md.contains("[2]"));
        assert_eq!(view.citations.len(), 2);
        // 先出现的是证据 2,再是证据 1
        assert_eq!(view.citations[0].ref_num, 1);
        assert_eq!(view.citations[0].block_id, "p001-b0001");
        assert_eq!(view.citations[0].document_title, "GNN 综述");
        assert_eq!(view.citations[1].ref_num, 2);
        assert_eq!(view.citations[1].block_id, "p001-b0000");
        // 重复引用同一证据不重复计入
        let client = ScriptedChat {
            reply: "GNN [1],还是 GNN [1]。".to_string(),
            seen: Mutex::new(Vec::new()),
        };
        let view = generate_entity_page(&deps, &client, &entity_id)
            .await
            .expect("regenerate");
        assert_eq!(view.citations.len(), 1);
    }

    #[tokio::test]
    async fn page_stale_flips_when_evidence_added() {
        let fixture = fixture("stale");
        let entity_id = seed(&fixture.db);
        let client = ScriptedChat {
            reply: "GNN 简介 [1]。".to_string(),
            seen: Mutex::new(Vec::new()),
        };
        let deps = GraphDeps {
            db: &fixture.db,
            data_root: &fixture.root.join("data"),
        };
        generate_entity_page(&deps, &client, &entity_id)
            .await
            .expect("generate");
        assert!(!get_entity_page(&deps, &entity_id).expect("read").stale);

        fixture
            .db
            .link_block_entity(&BlockEntityLink {
                document_id: "doc-1".to_string(),
                entity_id: entity_id.clone(),
                page_idx: 1,
                block_id: "p002-b0000".to_string(),
                job_id: "job-1".to_string(),
                surface_form: "GNN".to_string(),
                snippet: "新增证据".to_string(),
                confidence: 1.0,
                source: "extraction".to_string(),
            })
            .expect("link");
        assert!(get_entity_page(&deps, &entity_id).expect("read2").stale);
    }

    #[tokio::test]
    async fn page_generation_requires_evidence_and_keeps_previous_on_failure() {
        let fixture = fixture("empty");
        let entity = fixture
            .db
            .upsert_entity(&NewEntity {
                name: "无证据实体".to_string(),
                entity_type: "concept".to_string(),
                aliases: Vec::new(),
                description: String::new(),
            })
            .expect("entity");
        let client = ScriptedChat {
            reply: "不该被调用".to_string(),
            seen: Mutex::new(Vec::new()),
        };
        let deps = GraphDeps {
            db: &fixture.db,
            data_root: &fixture.root.join("data"),
        };
        let error = generate_entity_page(&deps, &client, &entity.entity_id)
            .await
            .expect_err("must fail without evidence");
        assert!(error.to_string().contains("还没有证据"));
        assert!(client.seen.lock().expect("lock").is_empty());

        // 已有页时,模型失败不覆盖旧页
        let entity_id = seed(&fixture.db);
        let ok = ScriptedChat {
            reply: "GNN 简介 [1]。".to_string(),
            seen: Mutex::new(Vec::new()),
        };
        generate_entity_page(&deps, &ok, &entity_id).await.expect("first");
        let broken = ScriptedChat {
            reply: String::new(),
            seen: Mutex::new(Vec::new()),
        };
        assert!(generate_entity_page(&deps, &broken, &entity_id).await.is_err());
        let view = get_entity_page(&deps, &entity_id).expect("read");
        assert!(view.has_page);
        assert_eq!(view.body_md, "GNN 简介 [1]。");
    }

    #[tokio::test]
    async fn unknown_entity_is_not_found() {
        let fixture = fixture("missing");
        let client = ScriptedChat {
            reply: "x".to_string(),
            seen: Mutex::new(Vec::new()),
        };
        let deps = GraphDeps {
            db: &fixture.db,
            data_root: &fixture.root.join("data"),
        };
        assert!(generate_entity_page(&deps, &client, "ent-nope").await.is_err());
        assert!(get_entity_page(&deps, "ent-nope").is_err());
    }

    #[tokio::test]
    async fn page_links_resolve_aliases_and_skip_unknown() {
        let fixture = fixture("links");
        let entity_id = seed(&fixture.db);
        let client = ScriptedChat {
            reply: "GNN 属于 [[图神经网络]] 家族,与 [[不存在]] 无关,也叫 [[GNN]]、[[GNN]] [1]。"
                .to_string(),
            seen: Mutex::new(Vec::new()),
        };
        let deps = GraphDeps {
            db: &fixture.db,
            data_root: &fixture.root.join("data"),
        };
        let view = generate_entity_page(&deps, &client, &entity_id)
            .await
            .expect("generate");
        // 别名与规范名都解析到同一实体;未知名跳过;重复 surface 只出一条
        assert_eq!(view.links.len(), 2);
        assert_eq!(view.links[0].surface, "图神经网络");
        assert_eq!(view.links[0].entity_id, entity_id);
        assert_eq!(view.links[0].name, "GNN");
        assert_eq!(view.links[1].surface, "GNN");
        // 正文保留 [[ ]] 供前端渲染
        assert!(view.body_md.contains("[[图神经网络]]"));
        // 读取时也现算
        let read = get_entity_page(&deps, &entity_id).expect("read");
        assert_eq!(read.links.len(), 2);
    }

    #[test]
    fn backlinks_match_name_and_alias_and_skip_unknown_and_self() {
        let fixture = fixture("backlinks");
        let target = fixture
            .db
            .upsert_entity(&NewEntity {
                name: "GNN".to_string(),
                entity_type: "method".to_string(),
                aliases: vec!["图神经网络".to_string()],
                description: String::new(),
            })
            .expect("target");
        add_page(&fixture.db, "QM9", "评测 [[GNN]] 与 [[不存在]]。");
        add_page(&fixture.db, "综述", "介绍 [[图神经网络]] 的进展。");
        // 自链:目标实体自己的页提到自己,不算反链
        add_page_of(&fixture.db, &target.entity_id, "GNN 也叫 [[GNN]]。");

        let items = list_entity_backlinks(&fixture.db, &target.entity_id, 10).expect("backlinks");
        let names: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(names, vec!["QM9", "综述"]);
        assert!(items[0].snippet.contains("GNN"));
        assert!(items[1].snippet.contains("图神经网络"));
        assert!(!items.iter().any(|item| item.name == "不存在"));
    }

    #[test]
    fn backlinks_empty_without_pages_and_error_for_unknown_entity() {
        let fixture = fixture("backlinks-empty");
        let target = fixture
            .db
            .upsert_entity(&NewEntity {
                name: "无人提及".to_string(),
                entity_type: "concept".to_string(),
                aliases: Vec::new(),
                description: String::new(),
            })
            .expect("target");
        assert!(list_entity_backlinks(&fixture.db, &target.entity_id, 10)
            .expect("ok")
            .is_empty());
        assert!(list_entity_backlinks(&fixture.db, "ent-nope", 10).is_err());
    }

    #[test]
    fn backlink_snippet_keeps_utf8_boundaries() {
        let fixture = fixture("backlinks-utf8");
        let target = fixture
            .db
            .upsert_entity(&NewEntity {
                name: "注意力".to_string(),
                entity_type: "concept".to_string(),
                aliases: Vec::new(),
                description: String::new(),
            })
            .expect("target");
        let prefix = "很长很长".repeat(20);
        let suffix = "结尾结尾".repeat(20);
        add_page(
            &fixture.db,
            "长文",
            &format!("{prefix}[[注意力]]{suffix}"),
        );
        let items = list_entity_backlinks(&fixture.db, &target.entity_id, 10).expect("backlinks");
        assert_eq!(items.len(), 1);
        let snippet = &items[0].snippet;
        assert!(snippet.contains("[[注意力]]"));
        assert!(snippet.starts_with('…') && snippet.ends_with('…'));
        // 前后各 31 字符 + 链接 6 字符 + 两端省略号(nth(30) 取的是第 31 个)
        assert_eq!(snippet.chars().count(), 70);
    }

    /// 给实体在 doc-1 挂一条证据(待维护清单的候选来源)。
    fn link_doc(db: &Db, entity_id: &str, block: &str) {
        db.link_block_entity(&BlockEntityLink {
            document_id: "doc-1".to_string(),
            entity_id: entity_id.to_string(),
            page_idx: 0,
            block_id: block.to_string(),
            job_id: "job-1".to_string(),
            surface_form: "x".to_string(),
            snippet: "x 片段".to_string(),
            confidence: 1.0,
            source: "extraction".to_string(),
        })
        .expect("link");
    }

    fn new_entity(db: &Db, name: &str) -> String {
        db.upsert_entity(&NewEntity {
            name: name.to_string(),
            entity_type: "concept".to_string(),
            aliases: Vec::new(),
            description: String::new(),
        })
        .expect("entity")
        .entity_id
    }

    #[test]
    fn pending_pages_include_missing_and_stale_exclude_fresh() {
        let fixture = fixture("pending");
        let missing_id = seed(&fixture.db);

        // 有页但签名对不上 = 陈旧
        let stale_id = new_entity(&fixture.db, "陈旧实体");
        link_doc(&fixture.db, &stale_id, "p001-b0000");
        add_page_of(&fixture.db, &stale_id, "旧正文");

        // 有页且签名一致 = 新鲜,不该进清单
        let fresh_id = new_entity(&fixture.db, "新鲜实体");
        link_doc(&fixture.db, &fresh_id, "p001-b0001");
        let fresh_sig = fixture
            .db
            .entity_page_evidence_sig(&fresh_id)
            .expect("sig");
        fixture
            .db
            .upsert_entity_page(&EntityPageRecord {
                entity_id: fresh_id.clone(),
                body_md: "新正文".to_string(),
                citations: Vec::new(),
                evidence_sig: fresh_sig,
                generated_at: now_iso(),
            })
            .expect("fresh page");

        let items = list_pending_entity_pages(&fixture.db, "doc-1", 10).expect("pending");
        let ids: Vec<&str> = items.iter().map(|item| item.entity_id.as_str()).collect();
        assert!(ids.contains(&stale_id.as_str()), "ids: {ids:?}");
        assert!(ids.contains(&missing_id.as_str()), "ids: {ids:?}");
        assert!(!ids.contains(&fresh_id.as_str()), "ids: {ids:?}");
        // 陈旧的排最前,且带 has_page=true
        assert!(items[0].stale);
        assert_eq!(items[0].entity_id, stale_id);
        assert!(items[0].has_page);
        // 缺页实体 has_page=false / stale=false
        let missing = items
            .iter()
            .find(|item| item.entity_id == missing_id)
            .expect("missing present");
        assert!(!missing.has_page && !missing.stale);
    }

    #[test]
    fn pending_pages_stale_first_and_limit_truncates() {
        let fixture = fixture("pending-order");
        // 缺页实体有 2 条提及,自然序在陈旧实体(1 条)之前
        let _missing_id = seed(&fixture.db);
        let stale_id = new_entity(&fixture.db, "陈旧实体");
        link_doc(&fixture.db, &stale_id, "p001-b0000");
        add_page_of(&fixture.db, &stale_id, "旧正文");

        let items = list_pending_entity_pages(&fixture.db, "doc-1", 1).expect("limit");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].entity_id, stale_id);
        assert!(items[0].stale);

        // 未知文档 = 空清单(404 由视图层负责)
        assert!(list_pending_entity_pages(&fixture.db, "doc-nope", 10)
            .expect("unknown doc")
            .is_empty());
    }

    #[test]
    fn strip_citation_markers_removes_all_markers() {
        assert_eq!(strip_citation_markers("A [1] B [12]"), "A  B ");
    }

    #[test]
    fn strip_wikilinks_keeps_names_and_citations() {
        assert_eq!(
            strip_wikilinks("见 [[GNN]] 与 [[图神经网络]] [1]"),
            "见 GNN 与 图神经网络 [1]"
        );
    }
}

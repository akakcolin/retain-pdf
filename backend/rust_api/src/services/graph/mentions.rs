//! 实体 → block 证据挂载:用实体名 + 别名在文档块里做字面扫描。
//! 抽取模型不产出 block_id(会幻觉),归属由这里机械完成。

use std::collections::HashSet;

use crate::db::graph::normalize_entity_name;
use crate::error::AppError;
use crate::models::api::{BlockEntityLink, EntityRecord};
use crate::services::ai::blocks::{load_job_blocks, Block};
use crate::services::ai::tools::safe_job_root;

use super::text_match::needle_hit;
use super::GraphDeps;

const SNIPPET_MAX_CHARS: usize = 200;
/// 单个 needle 归一化后的长度下限(尤其 CJK/短 ASCII)误报率过高,跳过。
const MIN_NEEDLE_CHARS: usize = 2;
/// 一次扫描最多遍历的实体数;超大库靠 1b 抽取而不是全表扫描。
const MAX_ENTITIES_SCANNED: u32 = 5000;

/// 匹配用的实体名写法:原始写法用于 surface_form,归一化写法用于匹配。
struct Needle {
    raw: String,
    norm: String,
}

/// 扫描某文档的所有块,用全库实体挂证据(glossary 链路径)。
pub fn link_document_mentions(
    deps: &GraphDeps<'_>,
    document_id: &str,
    source: &str,
) -> Result<usize, AppError> {
    let entities = deps.db.list_entities(MAX_ENTITIES_SCANNED)?;
    link_entities_mentions(deps, document_id, &entities, source)
}

/// 扫描某文档的块,只挂给定实体(抽取路径:刚抽出的实体不必回查全库)。
/// 返回本次挂载的 (entity, block) 命中数(重复挂载被唯一键忽略)。
pub fn link_entities_mentions(
    deps: &GraphDeps<'_>,
    document_id: &str,
    entities: &[EntityRecord],
    source: &str,
) -> Result<usize, AppError> {
    let Some((job_id, hits)) = scan_document_mentions(deps, document_id, entities)? else {
        return Ok(0);
    };
    for hit in &hits {
        deps.db.link_block_entity(&BlockEntityLink {
            document_id: document_id.to_string(),
            entity_id: hit.entity_id.clone(),
            page_idx: hit.page_idx,
            block_id: hit.block_id.clone(),
            job_id: job_id.clone(),
            surface_form: hit.surface_form.clone(),
            snippet: hit.snippet.clone(),
            confidence: 1.0,
            source: source.to_string(),
        })?;
    }
    Ok(hits.len())
}

/// 一次扫描命中的 (entity, block) 证据(尚未落库)。
pub struct ScannedMention {
    pub entity_id: String,
    pub page_idx: i64,
    pub block_id: String,
    pub surface_form: String,
    pub snippet: String,
}

/// 只扫描不落库。None = 无活动任务/块。
///
/// ponytail: O(entities × blocks) 嵌套扫描。实体上万或文档上千时改倒排索引
/// (按 trigram 建 entities_fts,或先给块建 token 集合)。
pub fn scan_document_mentions(
    deps: &GraphDeps<'_>,
    document_id: &str,
    entities: &[EntityRecord],
) -> Result<Option<(String, Vec<ScannedMention>)>, AppError> {
    let Some((job_id, blocks)) = load_document_blocks(deps, document_id)? else {
        return Ok(None);
    };
    let mut hits = Vec::new();
    for entity in entities {
        let needles = build_needles(entity);
        if needles.is_empty() {
            continue;
        }
        for block in &blocks {
            let Some((haystack, raw)) =
                match_block(&block.source_text, &block.translated_text, &needles)
            else {
                continue;
            };
            hits.push(ScannedMention {
                entity_id: entity.entity_id.clone(),
                page_idx: block.page_idx,
                block_id: block.block_id.clone(),
                surface_form: raw.to_string(),
                snippet: clip(haystack, SNIPPET_MAX_CHARS),
            });
        }
    }
    Ok(Some((job_id, hits)))
}

/// relink 结果计数。
pub struct RelinkOutcome {
    /// relink 后本文档仍命中的实体数
    pub entities: usize,
    /// 本次新增的证据条数
    pub mentions: usize,
    /// 本次移除的证据条数
    pub removed: usize,
}

/// 用当前匹配器重扫本文档,差量修正证据(零 LLM)。
///
/// 不碰 entity_relations / graph_extracted_at / 实体行。新行 source 按实体定:
/// 该实体在本文档已有 extraction 行 → extraction,否则 glossary——否则
/// `clear_document_extraction` 清不掉、重抽时 ON CONFLICT 也改不了标签。
pub fn relink_document_mentions(
    deps: &GraphDeps<'_>,
    document_id: &str,
) -> Result<RelinkOutcome, AppError> {
    let entities = deps.db.list_entities(MAX_ENTITIES_SCANNED)?;
    relink_entities_mentions(deps, document_id, &entities)
}

/// 差量修正的实体集版本:只有 `entities` 里的实体参与删除判定。
/// 未扫到的实体(超出 MAX_ENTITIES_SCANNED)保留原证据——"没命中"未知,不是否。
pub fn relink_entities_mentions(
    deps: &GraphDeps<'_>,
    document_id: &str,
    entities: &[EntityRecord],
) -> Result<RelinkOutcome, AppError> {
    let Some((job_id, hits)) = scan_document_mentions(deps, document_id, entities)? else {
        return Ok(RelinkOutcome {
            entities: 0,
            mentions: 0,
            removed: 0,
        });
    };
    let existing_rows = deps.db.list_document_block_entities(document_id)?;
    let extraction_entities: HashSet<String> = existing_rows
        .iter()
        .filter(|row| row.source == "extraction")
        .map(|row| row.entity_id.clone())
        .collect();
    let existing: HashSet<(String, String)> = existing_rows
        .into_iter()
        .map(|row| (row.entity_id, row.block_id))
        .collect();

    let mut inserts = Vec::new();
    let mut desired: HashSet<(String, String)> = HashSet::with_capacity(hits.len());
    for hit in hits {
        let pair = (hit.entity_id.clone(), hit.block_id.clone());
        desired.insert(pair.clone());
        if existing.contains(&pair) {
            continue;
        }
        let source = if extraction_entities.contains(&hit.entity_id) {
            "extraction"
        } else {
            "glossary"
        };
        inserts.push(BlockEntityLink {
            document_id: document_id.to_string(),
            entity_id: hit.entity_id,
            page_idx: hit.page_idx,
            block_id: hit.block_id,
            job_id: job_id.clone(),
            surface_form: hit.surface_form,
            snippet: hit.snippet,
            confidence: 1.0,
            source: source.to_string(),
        });
    }
    // 只删扫过的实体的行:扫描有 5000 上限,没扫到的实体"没命中"是未知而非否,
    // 删了就是静默丢证据。
    let scanned: HashSet<&str> = entities.iter().map(|e| e.entity_id.as_str()).collect();
    let deletes: Vec<(String, String)> = existing
        .difference(&desired)
        .filter(|(entity_id, _)| scanned.contains(entity_id.as_str()))
        .cloned()
        .collect();
    let (inserted, removed) = (inserts.len(), deletes.len());

    deps.db
        .apply_document_relink(document_id, &inserts, &deletes)?;

    Ok(RelinkOutcome {
        entities: deps.db.entities_for_document(document_id, u32::MAX)?.len(),
        mentions: inserted,
        removed,
    })
}

/// 取文档当前任务的块;无活动任务 / job_id 非法时返回 None(不报错)。
fn load_document_blocks(
    deps: &GraphDeps<'_>,
    document_id: &str,
) -> Result<Option<(String, Vec<Block>)>, AppError> {
    let document = deps.db.get_document(document_id)?;
    let Some(job_id) = document.active_job_id.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some(job_root) = safe_job_root(deps.data_root, &job_id) else {
        return Ok(None);
    };
    Ok(Some((job_id, load_job_blocks(&job_root)?)))
}

/// 实体名 + 别名 → 归一化去重的 needle,按归一化名排序(ASCII < CJK)。
fn build_needles(entity: &EntityRecord) -> Vec<Needle> {
    let mut needles: Vec<Needle> = std::iter::once(&entity.name)
        .chain(entity.aliases.iter())
        .filter_map(|raw| {
            let norm = normalize_entity_name(raw);
            (!norm.is_empty()).then(|| Needle {
                raw: raw.clone(),
                norm,
            })
        })
        .collect();
    needles.sort_by(|a, b| a.norm.cmp(&b.norm).then_with(|| a.raw.cmp(&b.raw)));
    needles.dedup_by(|a, b| a.norm == b.norm);
    needles.retain(|needle| needle.norm.chars().count() >= MIN_NEEDLE_CHARS);
    needles
}

/// 先原文后译文,返回 (命中所在文本, 命中的原始写法)。边界/大小写折叠交给
/// `needle_hit`;命中文本保持原样供 snippet 用。
fn match_block<'a>(
    source_text: &'a str,
    translated_text: &'a str,
    needles: &'a [Needle],
) -> Option<(&'a str, &'a str)> {
    for text in [source_text, translated_text] {
        if text.is_empty() {
            continue;
        }
        for needle in needles {
            if needle_hit(text, &needle.norm) {
                return Some((text, needle.raw.as_str()));
            }
        }
    }
    None
}

fn clip(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut truncated: String = normalized.chars().take(max_chars.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::db::Db;
    use crate::models::api::{NewEntity, NewEntityRelation};
    use crate::models::{now_iso, UploadRecord};

    use super::*;

    struct Fixture {
        root: PathBuf,
        data_root: PathBuf,
        db: Db,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn blocks_json(blocks: &[(&str, &str)]) -> serde_json::Value {
        serde_json::json!({
            "pages": [{"page_index": 0, "blocks": blocks
                .iter()
                .map(|(id, text)| serde_json::json!({"block_id": id, "text": text}))
                .collect::<Vec<_>>()}]
        })
    }

    impl Fixture {
        fn new(name: &str, blocks: &[(&str, &str)]) -> Self {
            let root = std::env::temp_dir().join(format!(
                "graph-mentions-{name}-{}-{}",
                std::process::id(),
                fastrand::u64(..)
            ));
            let data_root: PathBuf = root.join("data");
            fs::create_dir_all(&data_root).expect("data root");
            fs::create_dir_all(root.join("db")).expect("db dir");
            let db = Db::new(root.join("db").join("jobs.db"), data_root.clone());
            db.init().expect("init");
            db.upsert_document_from_upload(&UploadRecord {
                upload_id: "up-1".to_string(),
                filename: "paper.pdf".to_string(),
                stored_path: "uploads/x/paper.pdf".to_string(),
                bytes: 10,
                page_count: 1,
                uploaded_at: now_iso(),
                developer_mode: false,
                content_hash: "doc-1".to_string(),
            })
            .expect("insert document");
            db.set_document_active_job("doc-1", "job-1", None)
                .expect("active job");
            let fixture = Fixture {
                root,
                data_root,
                db,
            };
            fixture.write_blocks(blocks);
            fixture
        }

        fn write_blocks(&self, blocks: &[(&str, &str)]) {
            let normalized = self.data_root.join("jobs/job-1/ocr/normalized");
            fs::create_dir_all(&normalized).expect("mkdir normalized");
            fs::write(
                normalized.join("document.v1.json"),
                blocks_json(blocks).to_string(),
            )
            .expect("write document");
        }

        fn deps(&self) -> GraphDeps<'_> {
            GraphDeps {
                db: &self.db,
                data_root: &self.data_root,
            }
        }

        fn entity(&self, name: &str, aliases: &[&str]) -> String {
            self.db
                .upsert_entity(&NewEntity {
                    name: name.to_string(),
                    entity_type: "term".to_string(),
                    aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
                    description: String::new(),
                })
                .expect("entity")
                .entity_id
        }

        fn all_entities(&self) -> Vec<EntityRecord> {
            self.db.list_entities(100).expect("entities")
        }

        fn scan(&self) -> Vec<ScannedMention> {
            scan_document_mentions(&self.deps(), "doc-1", &self.all_entities())
                .expect("scan")
                .expect("active job")
                .1
        }
    }

    fn link(document_id: &str, entity_id: &str, block_id: &str, source: &str) -> BlockEntityLink {
        BlockEntityLink {
            document_id: document_id.to_string(),
            entity_id: entity_id.to_string(),
            page_idx: 0,
            block_id: block_id.to_string(),
            job_id: "job-1".to_string(),
            surface_form: "预置".to_string(),
            snippet: "预置".to_string(),
            confidence: 1.0,
            source: source.to_string(),
        }
    }

    /// 建一个带 job 产物的文档:块文本含实体别名,扫描后应挂出 1 条证据。
    #[test]
    fn link_document_mentions_scans_blocks_into_evidence() {
        let fixture = Fixture::new(
            "scan",
            &[
                ("p001-b0000", "halogen lithium exchange reaction"),
                ("p001-b0001", "unrelated text"),
            ],
        );
        let entity = fixture.entity("卤素锂交换", &["halogen"]);
        let deps = fixture.deps();
        assert_eq!(
            link_document_mentions(&deps, "doc-1", "glossary").expect("link"),
            1
        );
        let mentions = fixture
            .db
            .list_entity_mentions(&entity, None, 10)
            .expect("mentions");
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].block_id, "p001-b0000");
        assert!(mentions[0].snippet.contains("halogen"));
        // 幂等:重复扫描被唯一键忽略,不产生重复证据
        assert_eq!(
            link_document_mentions(&deps, "doc-1", "glossary").expect("relink"),
            1
        );
        assert_eq!(
            fixture
                .db
                .list_entity_mentions(&entity, None, 10)
                .expect("after relink")
                .len(),
            1
        );
    }

    #[test]
    fn scan_folds_case_but_keeps_raw_surface_and_snippet() {
        let fixture = Fixture::new("case", &[("p001-b0000", "using gnn models")]);
        let entity = fixture.entity("GNN", &[]);
        let hits = fixture.scan();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entity_id, entity);
        // surface_form 是实体原始写法;snippet 保留正文原大小写
        assert_eq!(hits[0].surface_form, "GNN");
        assert!(hits[0].snippet.contains("gnn"));
    }

    #[test]
    fn scan_rejects_ascii_word_interior_and_one_char_norm() {
        let fixture = Fixture::new("boundary", &[("p001-b0000", "retain the knowledge")]);
        fixture.entity("AI", &[]);
        fixture.entity(" A ", &[]);
        assert!(fixture.scan().is_empty());
    }

    #[test]
    fn relink_removes_stale_and_adds_new_mentions() {
        let fixture = Fixture::new("relink-diff", &[("p001-b0000", "gnn models")]);
        let entity = fixture.entity("GNN", &[]);
        // 预置一条误挂(块里没有 gnn)
        fixture
            .db
            .link_block_entity(&link("doc-1", &entity, "p001-b9999", "glossary"))
            .expect("stale");

        let outcome = relink_document_mentions(&fixture.deps(), "doc-1").expect("relink");
        assert_eq!((outcome.mentions, outcome.removed), (1, 1));
        assert_eq!(outcome.entities, 1);
        let rows = fixture
            .db
            .list_document_block_entities("doc-1")
            .expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].block_id, "p001-b0000");
        assert_eq!(rows[0].source, "glossary");
    }

    #[test]
    fn relink_keeps_evidence_of_unscanned_entities() {
        let fixture = Fixture::new("relink-cap", &[("p001-b0000", "gnn models")]);
        let gnn = fixture.entity("GNN", &[]);
        let outside = fixture.entity("ZZZ", &[]);
        // 预置一条不在本次扫描集里的证据(模拟实体落在 MAX_ENTITIES_SCANNED 之外):
        // 未扫到 = "没命中"未知,不能当误挂删掉。
        fixture
            .db
            .link_block_entity(&link("doc-1", &outside, "p001-b0000", "glossary"))
            .expect("seed");
        let scanned: Vec<EntityRecord> = fixture
            .all_entities()
            .into_iter()
            .filter(|entity| entity.entity_id == gnn)
            .collect();

        let outcome =
            relink_entities_mentions(&fixture.deps(), "doc-1", &scanned).expect("relink");

        assert_eq!((outcome.mentions, outcome.removed), (1, 0));
        let rows = fixture
            .db
            .list_document_block_entities("doc-1")
            .expect("rows");
        assert!(rows.iter().any(|row| row.entity_id == outside));
    }

    #[test]
    fn relink_assigns_source_per_entity() {
        let fixture = Fixture::new(
            "relink-source",
            &[("p001-b0000", "gnn and bert models"), ("p001-b0001", "gnn again")],
        );
        let gnn = fixture.entity("GNN", &[]);
        let bert = fixture.entity("BERT", &[]);
        // GNN 已是抽取产物 → 它的新行也该是 extraction
        fixture
            .db
            .link_block_entity(&link("doc-1", &gnn, "p001-b0000", "extraction"))
            .expect("extraction seed");

        let outcome = relink_document_mentions(&fixture.deps(), "doc-1").expect("relink");
        assert_eq!((outcome.mentions, outcome.removed), (2, 0));
        let rows = fixture
            .db
            .list_document_block_entities("doc-1")
            .expect("rows");
        let source_of = |entity_id: &str, block_id: &str| {
            rows.iter()
                .find(|row| row.entity_id == entity_id && row.block_id == block_id)
                .map(|row| row.source.clone())
                .expect("row")
        };
        assert_eq!(source_of(&gnn, "p001-b0001"), "extraction");
        assert_eq!(source_of(&bert, "p001-b0000"), "glossary");
    }

    #[test]
    fn relink_leaves_relations_untouched() {
        let fixture = Fixture::new("relink-relations", &[("p001-b0000", "gnn models")]);
        let gnn = fixture.entity("GNN", &[]);
        let other = fixture.entity("BERT", &[]);
        fixture
            .db
            .add_entity_relation(&NewEntityRelation {
                from_entity_id: gnn.clone(),
                to_entity_id: other.clone(),
                relation_type: "related".to_string(),
                confidence: 0.9,
                explanation: String::new(),
                source_document_id: "doc-1".to_string(),
                source_block_id: "p001-b0000".to_string(),
            })
            .expect("relation");

        relink_document_mentions(&fixture.deps(), "doc-1").expect("relink");

        let related = fixture
            .db
            .related_entities(&gnn, None, 10)
            .expect("related");
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].entity_id, other);
    }

    #[test]
    fn relink_noop_preserves_evidence_sig_but_real_change_flips_it() {
        let fixture = Fixture::new("relink-sig", &[("p001-b0000", "gnn models")]);
        let entity = fixture.entity("GNN", &[]);
        let deps = fixture.deps();
        assert_eq!(
            link_document_mentions(&deps, "doc-1", "glossary").expect("link"),
            1
        );
        let sig = fixture
            .db
            .entity_page_evidence_sig(&entity)
            .expect("sig");

        // no-op relink 不重插存活行 → rowid 不变 → 概念页不 stale
        let outcome = relink_document_mentions(&deps, "doc-1").expect("noop");
        assert_eq!((outcome.mentions, outcome.removed), (0, 0));
        assert_eq!(
            fixture
                .db
                .entity_page_evidence_sig(&entity)
                .expect("sig again"),
            sig
        );

        // 真变化(新增命中块)→ sig 翻
        fixture.write_blocks(&[("p001-b0000", "gnn models"), ("p001-b0002", "gnn again")]);
        let outcome = relink_document_mentions(&deps, "doc-1").expect("relink");
        assert_eq!((outcome.mentions, outcome.removed), (1, 0));
        assert_ne!(
            fixture
                .db
                .entity_page_evidence_sig(&entity)
                .expect("sig after"),
            sig
        );
    }

    #[test]
    fn match_block_prefers_source_then_translation() {
        let needle = |raw: &str| Needle {
            raw: raw.to_string(),
            norm: normalize_entity_name(raw),
        };
        let needles = vec![needle("卤素"), needle("halogen")];
        let hit = match_block("halogen lithium exchange", "卤素锂交换", &needles).expect("hit");
        assert_eq!(hit.1, "halogen");
        let hit = match_block("", "卤素锂交换", &needles).expect("hit");
        assert_eq!(hit.1, "卤素");
        assert!(match_block("no terms here", "", &needles).is_none());
    }

    #[test]
    fn clip_collapses_and_truncates() {
        assert_eq!(clip("a   b\nc", 10), "a b c");
        let long = "x".repeat(300);
        let clipped = clip(&long, 10);
        assert_eq!(clipped.chars().count(), 10);
        assert!(clipped.ends_with('…'));
    }
}

//! 实体 → block 证据挂载:用实体名 + 别名在文档块里做字面扫描。
//! 抽取模型不产出 block_id(会幻觉),归属由这里机械完成。

use crate::error::AppError;
use crate::models::api::{BlockEntityLink, EntityRecord};
use crate::services::ai::blocks::{load_job_blocks, Block};
use crate::services::ai::tools::safe_job_root;

use super::GraphDeps;

const SNIPPET_MAX_CHARS: usize = 200;
/// 单字符 needle(尤其 CJK)误报率过高,跳过。
const MIN_NEEDLE_CHARS: usize = 2;
/// 一次扫描最多遍历的实体数;超大库靠 1b 抽取而不是全表扫描。
const MAX_ENTITIES_SCANNED: u32 = 5000;

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
///
/// ponytail: O(entities × blocks) 嵌套扫描。实体上万或文档上千时改倒排索引
/// (按 trigram 建 entities_fts,或先给块建 token 集合)。
pub fn link_entities_mentions(
    deps: &GraphDeps<'_>,
    document_id: &str,
    entities: &[EntityRecord],
    source: &str,
) -> Result<usize, AppError> {
    let Some((job_id, blocks)) = load_document_blocks(deps, document_id)? else {
        return Ok(0);
    };
    let mut linked = 0usize;
    for entity in entities {
        let mut needles: Vec<String> = vec![entity.name.clone()];
        needles.extend(entity.aliases.iter().cloned());
        needles.retain(|value| value.chars().count() >= MIN_NEEDLE_CHARS);
        needles.sort();
        needles.dedup();
        if needles.is_empty() {
            continue;
        }
        for block in &blocks {
            let Some((haystack, surface)) = match_block(&block.source_text, &block.translated_text, &needles)
            else {
                continue;
            };
            deps.db.link_block_entity(&BlockEntityLink {
                document_id: document_id.to_string(),
                entity_id: entity.entity_id.clone(),
                page_idx: block.page_idx,
                block_id: block.block_id.clone(),
                job_id: job_id.clone(),
                surface_form: surface.to_string(),
                snippet: clip(haystack, SNIPPET_MAX_CHARS),
                confidence: 1.0,
                source: source.to_string(),
            })?;
            linked += 1;
        }
    }
    Ok(linked)
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

/// 先原文后译文,返回 (命中所在文本, 命中的 needle)。
fn match_block<'a>(
    source_text: &'a str,
    translated_text: &'a str,
    needles: &'a [String],
) -> Option<(&'a str, &'a str)> {
    for text in [source_text, translated_text] {
        if text.is_empty() {
            continue;
        }
        for needle in needles {
            if text.contains(needle.as_str()) {
                return Some((text, needle.as_str()));
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
    use crate::models::api::NewEntity;
    use crate::models::{now_iso, UploadRecord};

    use super::*;

    /// 建一个带 job 产物的文档:块文本含实体别名,扫描后应挂出 1 条证据。
    #[test]
    fn link_document_mentions_scans_blocks_into_evidence() {
        let root = std::env::temp_dir().join(format!(
            "graph-mentions-{}-{}",
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

        let normalized = data_root.join("jobs/job-1/ocr/normalized");
        fs::create_dir_all(&normalized).expect("mkdir normalized");
        fs::write(
            normalized.join("document.v1.json"),
            serde_json::json!({
                "pages": [{"page_index": 0, "blocks": [
                    {"block_id": "p001-b0000", "text": "halogen lithium exchange reaction"},
                    {"block_id": "p001-b0001", "text": "unrelated text"},
                ]}]
            })
            .to_string(),
        )
        .expect("write document");

        let entity = db
            .upsert_entity(&NewEntity {
                name: "卤素锂交换".to_string(),
                entity_type: "term".to_string(),
                aliases: vec!["halogen".to_string()],
                description: String::new(),
            })
            .expect("entity");
        let deps = GraphDeps {
            db: &db,
            data_root: &data_root,
        };
        assert_eq!(
            link_document_mentions(&deps, "doc-1", "glossary").expect("link"),
            1
        );
        let mentions = db
            .list_entity_mentions(&entity.entity_id, None, 10)
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
            db.list_entity_mentions(&entity.entity_id, None, 10)
                .expect("after relink")
                .len(),
            1
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn match_block_prefers_source_then_translation() {
        let needles = vec!["卤素".to_string(), "halogen".to_string()];
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

//! 工具注册表(移植自 retainpdf_ai/tools.py)。
//! 检索类工具直接落库 / 读任务目录,不再经 HTTP 回环到 Rust API。

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::db::Db;

use super::blocks::{read_page_blocks, Block};

/// job_id 白名单:字母数字开头 + [-._] 组成,禁止路径分隔符/..
/// 关键安全边界——job_id 来自模型工具参数(上下文含文档内容 = 提示注入面),
/// 直接拼进 data_root/jobs/<job_id> 前必须过这道闸,否则可目录穿越。
pub(crate) fn safe_job_root(data_root: &Path, job_id: &str) -> Option<PathBuf> {
    if job_id.is_empty() || job_id.contains("..") || job_id.len() > 128 {
        return None;
    }
    let bytes = job_id.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        return None;
    }
    if bytes
        .iter()
        .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-')))
    {
        return None;
    }
    Some(data_root.join("jobs").join(job_id))
}

fn percent_encode_segment(segment: &str) -> String {
    let mut out = String::new();
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// 列出该页 OCR Markdown 图片,返回可鉴权拉取的 API 相对路径。
/// 磁盘: jobs/<job>/md/images/page-<1-based>/...
/// API:  /api/v1/jobs/<job>/markdown/images/<rel-without-images-prefix>
fn list_markdown_image_urls(
    job_root: &Path,
    job_id: &str,
    page_idx: i64,
    limit: usize,
) -> Vec<String> {
    let page_dir = job_root
        .join("md/images")
        .join(format!("page-{}", page_idx + 1));
    if !page_dir.is_dir() {
        return Vec::new();
    }
    let mut urls = Vec::new();
    let images_root = job_root.join("md/images");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_image_files(&page_dir, &mut files);
    files.sort();
    for path in files {
        let Ok(rel) = path.strip_prefix(&images_root) else {
            continue;
        };
        let encoded = rel
            .components()
            .map(|component| percent_encode_segment(&component.as_os_str().to_string_lossy()))
            .collect::<Vec<_>>()
            .join("/");
        urls.push(format!("/api/v1/jobs/{job_id}/markdown/images/{encoded}"));
        if urls.len() >= limit {
            break;
        }
    }
    urls
}

fn collect_image_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_image_files(&path, out);
        } else if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            if matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp"
            ) {
                out.push(path);
            }
        }
    }
}

pub struct AiTools<'a> {
    db: &'a Db,
    data_root: &'a Path,
}

impl<'a> AiTools<'a> {
    pub fn new(db: &'a Db, data_root: &'a Path) -> Self {
        Self { db, data_root }
    }

    /// OpenAI function-calling 工具定义。scoped_document_id 非空时移除
    /// list_documents(整本问答不暴露"浏览图书馆")。
    pub fn specs(&self, scoped_document_id: &str) -> Vec<Value> {
        let mut specs: Vec<Value> = vec![
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_documents",
                    "description": "列出图书馆中的文档(标题、标签、阅读状态)。回答涉及'哪篇文档/我的库里'时先用它确认范围。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "tag": {"type": "string", "description": "按标签过滤,可选"},
                            "reading_status": {"type": "string", "enum": ["unread", "reading", "done"], "description": "按阅读状态过滤,可选"},
                            "limit": {"type": "integer", "minimum": 1, "maximum": 200}
                        }
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "search_fulltext",
                    "description": "全文检索(中英文均可),返回带 (document_id, job_id, page_idx, block_id) 锚点的命中片段;命中页若有 OCR 图会附 image_urls(可嵌入回答的 Markdown 图片路径)。这是找证据的主要工具,可多次换关键词调用。若会话已限定文档,请务必传 document_id,只在该文档内检索。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string", "description": "检索关键词或短语"},
                            "document_id": {"type": "string", "description": "限定单文档;整本问答时必传当前 document_id"},
                            "limit": {"type": "integer", "minimum": 1, "maximum": 30}
                        },
                        "required": ["query"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "read_blocks",
                    "description": "读取某文档某页的原文与译文块,并附带该页 Markdown 图片 image_urls。用于查看检索命中处的完整上下文(传 around_block_id 以命中块为中心取窗口);回答图表相关问题时用 image_urls 嵌入 Markdown 图片。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "document_id": {"type": "string"},
                            "page_idx": {"type": "integer", "minimum": 0},
                            "job_id": {"type": "string", "description": "优先读该任务产物;缺省用文档 active_job_id"},
                            "around_block_id": {"type": "string", "description": "以此块为中心取上下文,可选"},
                            "max_blocks": {"type": "integer", "minimum": 1, "maximum": 30}
                        },
                        "required": ["document_id", "page_idx"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "search_favorites",
                    "description": "检索用户收藏过的句子/数据(可按关键词与文档过滤)。问题涉及'我收藏的/我标记过的'内容时使用。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "keyword": {"type": "string", "description": "在引文与备注里做关键词过滤,可选"},
                            "document_id": {"type": "string", "description": "限定某文档,可选"}
                        }
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "search_entities",
                    "description": "检索知识图谱中的实体(概念/方法/材料/数据集/人物/机构/指标/公式/术语),返回 entity_id、类型、别名与提及次数。用户问'我库里关于 X''X 是什么'时先用它定位实体,再用 find_mentions 取证据。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string", "description": "实体名或关键词,中英文均可"},
                            "entity_type": {"type": "string", "enum": ["concept", "method", "material", "dataset", "person", "org", "metric", "formula", "term"], "description": "按类型过滤,可选"},
                            "limit": {"type": "integer", "minimum": 1, "maximum": 50}
                        },
                        "required": ["query"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "find_mentions",
                    "description": "查某个实体在文档中的出现位置,返回带页码与引用编号的片段。用于回答'X 在哪些文献/哪一页提到'。需要先由 search_entities 拿到 entity_id。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "entity_id": {"type": "string", "description": "search_entities 返回的 entity_id"},
                            "document_id": {"type": "string", "description": "限定某文档,可选"},
                            "limit": {"type": "integer", "minimum": 1, "maximum": 30}
                        },
                        "required": ["entity_id"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "related_entities",
                    "description": "查某个实体在图谱中的关联实体(有向关系:uses/improves_on/contradicts/part_of/related_to/defines/evaluates/produces)。用于回答'X 与什么相关''哪些方法用了 X'。需要先由 search_entities 拿到 entity_id。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "entity_id": {"type": "string", "description": "search_entities 返回的 entity_id"},
                            "relation_type": {
                                "type": "string",
                                "enum": crate::services::graph::extract::RELATION_TYPES,
                                "description": "按关系类型过滤,可选"
                            },
                            "limit": {"type": "integer", "minimum": 1, "maximum": 50}
                        },
                        "required": ["entity_id"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "get_entity_page",
                    "description": "读某实体已生成的概念页——跨文档综述,带引用证据。回答'X 是什么''总结一下 X'时优先用它。需要先由 search_entities 拿到 entity_id。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "entity_id": {"type": "string", "description": "search_entities 返回的 entity_id"}
                        },
                        "required": ["entity_id"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "find_entity_favorites",
                    "description": "查用户标注(收藏的引文/译文/备注)里提到某实体的那些。回答'我在 X 上标过什么''我对 X 的记录'时使用。需要先由 search_entities 拿到 entity_id。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "entity_id": {"type": "string", "description": "search_entities 返回的 entity_id"},
                            "limit": {"type": "integer", "minimum": 1, "maximum": 50}
                        },
                        "required": ["entity_id"]
                    }
                }
            }),
        ];
        if !scoped_document_id.trim().is_empty() {
            specs
                .retain(|spec| spec["function"]["name"].as_str().unwrap_or("") != "list_documents");
        }
        specs
    }

    /// 工具失败作为结果反馈给模型,不中断循环。
    pub fn invoke(&self, name: &str, arguments: &Map<String, Value>) -> Value {
        match name {
            "search_fulltext" => self.search_fulltext(arguments),
            "list_documents" => self.list_documents(arguments),
            "read_blocks" => self.read_blocks(arguments),
            "search_favorites" => self.search_favorites(arguments),
            "search_entities" => self.search_entities(arguments),
            "find_mentions" => self.find_mentions(arguments),
            "related_entities" => self.related_entities(arguments),
            "get_entity_page" => self.get_entity_page(arguments),
            "find_entity_favorites" => self.find_entity_favorites(arguments),
            other => serde_json::json!({"error": format!("unknown tool: {other}")}),
        }
    }

    fn search_fulltext(&self, arguments: &Map<String, Value>) -> Value {
        let query = string_arg(arguments, "query").trim().to_string();
        if query.is_empty() {
            return serde_json::json!({"error": "query must not be empty"});
        }
        let limit = int_arg(arguments, "limit").unwrap_or(10).clamp(1, 30) as u32;
        let document_id = string_arg(arguments, "document_id").trim().to_string();
        let hits = match self.db.search_blocks(
            &query,
            limit,
            if document_id.is_empty() {
                None
            } else {
                Some(&document_id)
            },
        ) {
            Ok(hits) => hits,
            Err(err) => return serde_json::json!({"error": format!("search failed: {err}")}),
        };
        let mut enriched_hits: Vec<Value> = Vec::new();
        for hit in hits {
            let mut item = serde_json::json!({
                "document_id": hit.document_id,
                "job_id": hit.job_id,
                "page_idx": hit.page_idx,
                "block_id": hit.block_id,
                "source_snippet": hit.source_snippet,
                "translated_snippet": hit.translated_snippet,
            });
            let hit_job_id = hit.job_id;
            let hit_page = hit.page_idx;
            if let Some(job_root) = safe_job_root(self.data_root, &hit_job_id) {
                let images = list_markdown_image_urls(&job_root, &hit_job_id, hit_page, 4);
                if !images.is_empty() {
                    item["image_urls"] =
                        Value::Array(images.into_iter().map(Value::String).collect());
                }
            }
            enriched_hits.push(item);
        }
        let mut payload = serde_json::json!({"hits": enriched_hits});
        if !document_id.is_empty() {
            payload["document_id"] = Value::String(document_id.clone());
            if enriched_hits.is_empty() {
                payload["hint"] = Value::String(
                    "该文档全文索引无命中：可能尚未建立 blocks_fts，或关键词不在原文/译文中。可换关键词，或说明暂无证据。".to_string(),
                );
            }
        }
        payload
    }

    fn list_documents(&self, arguments: &Map<String, Value>) -> Value {
        let scoped_id = string_arg(arguments, "document_id").trim().to_string();
        if !scoped_id.is_empty() {
            return match self.db.get_document(&scoped_id) {
                Ok(document) => serde_json::json!({
                    "documents": [project_document(&document)]
                }),
                Err(err) => serde_json::json!({
                    "error": format!("document not found: {err}"),
                    "documents": []
                }),
            };
        }
        let tag = string_arg(arguments, "tag").trim().to_string();
        let reading_status = string_arg(arguments, "reading_status").trim().to_string();
        let limit = int_arg(arguments, "limit").unwrap_or(50).clamp(1, 200) as u32;
        let documents = match self.db.list_documents(
            limit,
            0,
            if reading_status.is_empty() {
                None
            } else {
                Some(&reading_status)
            },
            if tag.is_empty() { None } else { Some(&tag) },
            None,
        ) {
            Ok(documents) => documents,
            Err(err) => return serde_json::json!({"error": format!("list failed: {err}")}),
        };
        serde_json::json!({
            "documents": documents.iter().map(project_document).collect::<Vec<_>>()
        })
    }

    fn read_blocks(&self, arguments: &Map<String, Value>) -> Value {
        let document_id = string_arg(arguments, "document_id").trim().to_string();
        let Some(page_idx) = int_arg(arguments, "page_idx") else {
            return serde_json::json!({"error": "document_id and page_idx are required"});
        };
        if document_id.is_empty() {
            return serde_json::json!({"error": "document_id and page_idx are required"});
        }
        let mut job_id = string_arg(arguments, "job_id").trim().to_string();
        if job_id.is_empty() {
            job_id = match self.db.get_document(&document_id) {
                Ok(document) => document.active_job_id.unwrap_or_default(),
                Err(err) => {
                    return serde_json::json!({"error": format!("document not found: {err}")})
                }
            };
        }
        if job_id.is_empty() {
            return serde_json::json!({"error": format!("document {document_id} has no active job")});
        }
        let Some(job_root) = safe_job_root(self.data_root, &job_id) else {
            return serde_json::json!({"error": format!("invalid job_id: {job_id:?}")});
        };
        let around_block_id = string_arg(arguments, "around_block_id").trim().to_string();
        let max_blocks = int_arg(arguments, "max_blocks").unwrap_or(12).clamp(1, 30) as usize;
        let blocks = read_page_blocks(&job_root, page_idx, &around_block_id, max_blocks);
        let image_urls = list_markdown_image_urls(&job_root, &job_id, page_idx, 8);
        serde_json::json!({
            "document_id": document_id,
            "job_id": job_id,
            "page_idx": page_idx,
            "blocks": blocks.iter().map(project_block).collect::<Vec<_>>(),
            "image_urls": image_urls,
        })
    }

    fn search_favorites(&self, arguments: &Map<String, Value>) -> Value {
        let keyword = string_arg(arguments, "keyword").trim().to_ascii_lowercase();
        let document_id = string_arg(arguments, "document_id").trim().to_string();
        let favorites = match self.db.list_favorites(if document_id.is_empty() {
            None
        } else {
            Some(&document_id)
        }) {
            Ok(favorites) => favorites,
            Err(err) => {
                return serde_json::json!({"error": format!("list favorites failed: {err}")})
            }
        };
        let mut out: Vec<Value> = Vec::new();
        for favorite in favorites {
            if !keyword.is_empty()
                && !favorite.quote_text.to_ascii_lowercase().contains(&keyword)
                && !favorite
                    .translated_quote_text
                    .to_ascii_lowercase()
                    .contains(&keyword)
                && !favorite.note.to_ascii_lowercase().contains(&keyword)
            {
                continue;
            }
            out.push(serde_json::json!({
                "favorite_id": favorite.favorite_id,
                "document_id": favorite.document_id,
                "job_id": favorite.job_id,
                "page_idx": favorite.page_idx,
                "block_id": favorite.block_id,
                "kind": favorite.kind,
                "quote_text": favorite.quote_text,
                "translated_quote_text": favorite.translated_quote_text,
                "note": favorite.note,
            }));
            if out.len() >= 30 {
                break;
            }
        }
        serde_json::json!({"favorites": out})
    }

    fn search_entities(&self, arguments: &Map<String, Value>) -> Value {
        let query = string_arg(arguments, "query").trim().to_string();
        if query.is_empty() {
            return serde_json::json!({"error": "query must not be empty"});
        }
        let entity_type = string_arg(arguments, "entity_type").trim().to_string();
        let limit = int_arg(arguments, "limit").unwrap_or(20).clamp(1, 50) as u32;
        match self.db.search_entities(
            &query,
            if entity_type.is_empty() {
                None
            } else {
                Some(&entity_type)
            },
            limit,
        ) {
            Ok(entities) => serde_json::json!({
                "entities": entities.iter().map(project_entity).collect::<Vec<_>>()
            }),
            Err(err) => serde_json::json!({"error": format!("search entities failed: {err}")}),
        }
    }

    /// 返回 blocks 键:复用既有 citation 机制(hits/favorites/blocks 都过 assign_refs)。
    fn find_mentions(&self, arguments: &Map<String, Value>) -> Value {
        let entity_id = string_arg(arguments, "entity_id").trim().to_string();
        if entity_id.is_empty() {
            return serde_json::json!({"error": "entity_id must not be empty"});
        }
        let document_id = string_arg(arguments, "document_id").trim().to_string();
        let limit = int_arg(arguments, "limit").unwrap_or(20).clamp(1, 30) as u32;
        match self.db.list_entity_mentions(
            &entity_id,
            if document_id.is_empty() {
                None
            } else {
                Some(&document_id)
            },
            limit,
        ) {
            Ok(mentions) => serde_json::json!({
                "blocks": mentions.iter().map(project_mention).collect::<Vec<_>>()
            }),
            Err(err) => serde_json::json!({"error": format!("find mentions failed: {err}")}),
        }
    }

    /// 也返回 entities 键(与 search_entities 同形,多带关系字段)。
    fn related_entities(&self, arguments: &Map<String, Value>) -> Value {
        let entity_id = string_arg(arguments, "entity_id").trim().to_string();
        if entity_id.is_empty() {
            return serde_json::json!({"error": "entity_id must not be empty"});
        }
        let relation_type = string_arg(arguments, "relation_type").trim().to_string();
        let limit = int_arg(arguments, "limit").unwrap_or(20).clamp(1, 50) as u32;
        match self.db.related_entities(
            &entity_id,
            if relation_type.is_empty() {
                None
            } else {
                Some(&relation_type)
            },
            limit,
        ) {
            Ok(items) => serde_json::json!({
                "entities": items.iter().map(project_related).collect::<Vec<_>>()
            }),
            Err(err) => serde_json::json!({"error": format!("related entities failed: {err}")}),
        }
    }

    /// 返回 entity_page(综述正文,已剥掉 [n])+ blocks(引用证据,走既有编号机制)。
    fn get_entity_page(&self, arguments: &Map<String, Value>) -> Value {
        let entity_id = string_arg(arguments, "entity_id").trim().to_string();
        if entity_id.is_empty() {
            return serde_json::json!({"error": "entity_id must not be empty"});
        }
        let entity = match self.db.get_entity(&entity_id) {
            Ok(entity) => entity,
            Err(_) => return serde_json::json!({"error": "entity not found"}),
        };
        let page = match self.db.get_entity_page(&entity_id) {
            Ok(page) => page,
            Err(err) => {
                return serde_json::json!({"error": format!("get entity page failed: {err}")})
            }
        };
        let Some(page) = page else {
            return serde_json::json!({
                "error": "该实体还没有概念页",
                "hint": "用 find_mentions 取证据直接回答,或让用户在概念面板生成后再问",
            });
        };
        let stale = self
            .db
            .entity_page_evidence_sig(&entity_id)
            .map(|sig| sig != page.evidence_sig)
            .unwrap_or(false);
        serde_json::json!({
            "entity_page": {
                "entity_id": page.entity_id,
                "name": entity.name,
                "entity_type": entity.entity_type,
                "stale": stale,
                "edited": page.edited(),
                "body_md": crate::services::graph::page::strip_wikilinks(
                    &crate::services::graph::page::strip_citation_markers(page.effective_body()),
                ),
            },
            "blocks": page.citations.iter().map(project_page_citation).collect::<Vec<_>>(),
        })
    }

    /// 返回 favorites 键:走既有 citation 编号机制(public_anchor 会带上 note)。
    fn find_entity_favorites(&self, arguments: &Map<String, Value>) -> Value {
        let entity_id = string_arg(arguments, "entity_id").trim().to_string();
        if entity_id.is_empty() {
            return serde_json::json!({"error": "entity_id must not be empty"});
        }
        let limit = int_arg(arguments, "limit").unwrap_or(20).clamp(1, 50) as u32;
        match crate::services::graph::favorites::list_entity_favorites(self.db, &entity_id, limit) {
            Ok(items) => serde_json::json!({
                "favorites": items.iter().map(project_entity_favorite).collect::<Vec<_>>()
            }),
            Err(err) => {
                serde_json::json!({"error": format!("find entity favorites failed: {err}")})
            }
        }
    }
}

fn string_arg(arguments: &Map<String, Value>, key: &str) -> String {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn int_arg(arguments: &Map<String, Value>, key: &str) -> Option<i64> {
    arguments.get(key).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
    })
}

fn project_document(document: &crate::models::api::DocumentRecord) -> Value {
    serde_json::json!({
        "document_id": document.document_id,
        "title": document.title,
        "page_count": document.page_count,
        "tags": document.tags,
        "reading_status": document.reading_status,
    })
}

fn project_entity(entity: &crate::models::api::EntitySummary) -> Value {
    serde_json::json!({
        "entity_id": entity.entity_id,
        "name": entity.name,
        "entity_type": entity.entity_type,
        "aliases": entity.aliases,
        "mention_count": entity.mention_count,
        "document_count": entity.document_count,
    })
}

fn project_related(item: &crate::models::api::RelatedEntity) -> Value {
    serde_json::json!({
        "entity_id": item.entity_id,
        "name": item.name,
        "entity_type": item.entity_type,
        "aliases": item.aliases,
        "mention_count": item.mention_count,
        "document_count": item.document_count,
        "relation_type": item.relation_type,
        "direction": item.direction,
        "explanation": item.explanation,
    })
}

fn project_page_citation(citation: &crate::models::api::EntityPageCitation) -> Value {
    serde_json::json!({
        "document_id": citation.document_id,
        "job_id": citation.job_id,
        "page_idx": citation.page_idx,
        "block_id": citation.block_id,
        "snippet": citation.snippet,
    })
}

fn project_mention(mention: &crate::models::api::EntityMention) -> Value {
    serde_json::json!({
        "document_id": mention.document_id,
        "job_id": mention.job_id,
        "page_idx": mention.page_idx,
        "block_id": mention.block_id,
        "snippet": mention.snippet,
    })
}

fn project_entity_favorite(favorite: &crate::models::api::EntityFavorite) -> Value {
    serde_json::json!({
        "favorite_id": favorite.favorite_id,
        "document_id": favorite.document_id,
        "job_id": favorite.job_id,
        "page_idx": favorite.page_idx,
        "block_id": favorite.block_id,
        "quote_text": favorite.quote_text,
        "translated_quote_text": favorite.translated_quote_text,
        "note": favorite.note,
    })
}

fn project_block(block: &Block) -> Value {
    let source: String = block.source_text.chars().take(600).collect();
    let translated: String = block.translated_text.chars().take(600).collect();
    serde_json::json!({
        "block_id": block.block_id,
        "source_text": source,
        "translated_text": translated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_job_root_rejects_path_traversal() {
        let data_root = Path::new("/data");
        assert!(safe_job_root(data_root, "job-123").is_some());
        assert!(safe_job_root(data_root, "job/../../etc").is_none());
        assert!(safe_job_root(data_root, "..").is_none());
        assert!(safe_job_root(data_root, "").is_none());
        assert!(safe_job_root(data_root, "bad job").is_none());
        assert!(safe_job_root(data_root, "-leading-dash").is_none());
        assert!(safe_job_root(data_root, "ok_dash.job-1").is_some());
    }

    #[test]
    fn percent_encode_segment_encodes_reserved() {
        assert_eq!(percent_encode_segment("page-1"), "page-1");
        assert_eq!(percent_encode_segment("a b.png"), "a%20b.png");
        assert_eq!(percent_encode_segment("图1"), "%E5%9B%BE1");
    }

    #[test]
    fn find_entity_favorites_returns_matching_annotations() {
        use crate::models::api::{FavoriteRecord, NewEntity};
        use crate::models::{now_iso, UploadRecord};

        let root = std::env::temp_dir().join(format!(
            "ai-tools-favorites-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        let data_root = root.join("data");
        std::fs::create_dir_all(&data_root).expect("data root");
        std::fs::create_dir_all(root.join("db")).expect("db dir");
        let db = Db::new(root.join("db").join("jobs.db"), data_root);
        db.init().expect("init");
        db.upsert_document_from_upload(&UploadRecord {
            upload_id: "up-1".to_string(),
            filename: "化学.pdf".to_string(),
            stored_path: "uploads/x/chem.pdf".to_string(),
            bytes: 10,
            page_count: 1,
            uploaded_at: now_iso(),
            developer_mode: false,
            content_hash: "doc-1".to_string(),
        })
        .expect("document");
        let entity = db
            .upsert_entity(&NewEntity {
                name: "GNN".to_string(),
                entity_type: "method".to_string(),
                aliases: Vec::new(),
                description: String::new(),
            })
            .expect("entity");
        db.save_favorite(&FavoriteRecord {
            favorite_id: "fav-1".to_string(),
            document_id: "doc-1".to_string(),
            job_id: "job-1".to_string(),
            page_idx: 2,
            block_id: "p003-b0000".to_string(),
            char_start: None,
            char_end: None,
            kind: "sentence".to_string(),
            quote_text: "GNN 片段".to_string(),
            translated_quote_text: String::new(),
            note: "我的备注".to_string(),
            asset_id: String::new(),
            rect_json: String::new(),
            created_at: now_iso(),
            updated_at: now_iso(),
        })
        .expect("favorite");

        let tools = AiTools::new(&db, Path::new("/data"));
        let mut arguments = Map::new();
        arguments.insert(
            "entity_id".to_string(),
            Value::String(entity.entity_id.clone()),
        );
        let result = tools.invoke("find_entity_favorites", &arguments);
        assert_eq!(result["favorites"].as_array().map(Vec::len), Some(1));
        assert_eq!(result["favorites"][0]["note"], serde_json::json!("我的备注"));
        assert_eq!(
            result["favorites"][0]["quote_text"],
            serde_json::json!("GNN 片段")
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

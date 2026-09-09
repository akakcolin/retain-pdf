//! 概念图谱持久化:实体消歧、block 证据挂载、实体检索。
//! 全部走 Db facade,路由/服务层不直接写 SQL。

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::models::domain::{build_job_id, now_iso};
use crate::models::api::{
    BlockEntityLink, EntityMention, EntityPageEvidence, EntityPageRecord, EntityRecord,
    EntitySummary, NewEntity, NewEntityRelation, RelatedEntity,
};

use super::Db;

/// 归一化实体名:折叠空白 + 小写。中英文都适用(中文无大小写)。
pub fn normalize_entity_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn row_to_entity(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntityRecord> {
    let aliases_json: String = row.get(4)?;
    Ok(EntityRecord {
        entity_id: row.get(0)?,
        name: row.get(1)?,
        name_norm: row.get(2)?,
        entity_type: row.get(3)?,
        aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
        description: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn row_to_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntitySummary> {
    let aliases_json: String = row.get(3)?;
    Ok(EntitySummary {
        entity_id: row.get(0)?,
        name: row.get(1)?,
        entity_type: row.get(2)?,
        aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
        mention_count: row.get(4)?,
        document_count: row.get(5)?,
    })
}

fn row_to_related(row: &rusqlite::Row<'_>) -> rusqlite::Result<RelatedEntity> {
    let aliases_json: String = row.get(3)?;
    Ok(RelatedEntity {
        entity_id: row.get(0)?,
        name: row.get(1)?,
        entity_type: row.get(2)?,
        aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
        mention_count: row.get(4)?,
        document_count: row.get(5)?,
        relation_type: row.get(6)?,
        direction: row.get(7)?,
        confidence: row.get(8)?,
        explanation: row.get(9)?,
        source_document_id: row.get(10)?,
    })
}

const ENTITY_COLUMNS: &str =
    "entity_id, name, name_norm, entity_type, aliases_json, description, created_at, updated_at";

/// 实体邻居查询:?1 实体 id,?2 关系类型('' = 不过滤),?3 limit。
const RELATED_ENTITIES_SQL: &str = r#"
    SELECT e.entity_id, e.name, e.entity_type, e.aliases_json,
        (SELECT COUNT(*) FROM block_entities b WHERE b.entity_id = e.entity_id),
        (SELECT COUNT(DISTINCT b.document_id) FROM block_entities b
         WHERE b.entity_id = e.entity_id),
        r.relation_type,
        CASE WHEN r.from_entity_id = ?1 THEN 'out' ELSE 'in' END,
        r.confidence, r.explanation, r.source_document_id
    FROM entity_relations r
    JOIN entities e ON e.entity_id =
        CASE WHEN r.from_entity_id = ?1 THEN r.to_entity_id ELSE r.from_entity_id END
    WHERE (r.from_entity_id = ?1 OR r.to_entity_id = ?1)
      AND (?2 = '' OR r.relation_type = ?2)
    ORDER BY r.confidence DESC, e.name ASC
    LIMIT ?3
    "#;

const ENTITY_PAGE_COLUMNS: &str =
    "entity_id, body_md, citations_json, evidence_sig, generated_at";

fn row_to_entity_page(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntityPageRecord> {
    let citations_json: String = row.get(2)?;
    Ok(EntityPageRecord {
        entity_id: row.get(0)?,
        body_md: row.get(1)?,
        citations: serde_json::from_str(&citations_json).unwrap_or_default(),
        evidence_sig: row.get(3)?,
        generated_at: row.get(4)?,
    })
}

/// 实体摘要的计数子查询(提及数 / 覆盖文档数)。
const SUMMARY_COLUMNS: &str = "e.entity_id, e.name, e.entity_type, e.aliases_json,
    (SELECT COUNT(*) FROM block_entities b WHERE b.entity_id = e.entity_id),
    (SELECT COUNT(DISTINCT b.document_id) FROM block_entities b WHERE b.entity_id = e.entity_id)";

fn find_entity_exact_conn(
    conn: &Connection,
    name_norm: &str,
    entity_type: &str,
) -> Result<Option<EntityRecord>> {
    let sql = format!(
        "SELECT {ENTITY_COLUMNS} FROM entities WHERE name_norm = ?1 AND entity_type = ?2"
    );
    let record = conn
        .query_row(&sql, params![name_norm, entity_type], row_to_entity)
        .optional()?;
    Ok(record)
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// 按别名精确解析实体:aliases_json 用首个词做 LIKE 预筛,再在 Rust 侧按归一化名
/// 精确比对。预筛只取首词是因为词内无空白,折叠空白不会改变它——直接 LIKE 整个
/// 归一化名会漏掉 `halogen  lithium`(双空格)这类写法。
fn find_entity_by_alias_conn(conn: &Connection, name_norm: &str) -> Result<Option<EntityRecord>> {
    let needle = name_norm.split(' ').next().unwrap_or(name_norm);
    let like = format!("%{}%", escape_like(needle));
    let sql = format!("SELECT {ENTITY_COLUMNS} FROM entities WHERE aliases_json LIKE ?1 ESCAPE '\\'");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![like])?;
    while let Some(row) = rows.next()? {
        let record = row_to_entity(row)?;
        if record
            .aliases
            .iter()
            .any(|alias| normalize_entity_name(alias) == name_norm)
        {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

/// 清洗别名:trim、丢弃空串与归一化后等于规范名本身的项、按归一化去重。
fn clean_aliases(name_norm: &str, aliases: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for alias in aliases {
        let alias = alias.trim();
        if alias.is_empty() || normalize_entity_name(alias) == name_norm {
            continue;
        }
        if out
            .iter()
            .any(|current| normalize_entity_name(current) == normalize_entity_name(alias))
        {
            continue;
        }
        out.push(alias.to_string());
    }
    out
}

/// 已存在实体时并入新别名/描述:非空描述不被覆盖。返回合并后的记录。
fn merge_entity_conn(
    conn: &Connection,
    existing: EntityRecord,
    new: &NewEntity,
) -> Result<EntityRecord> {
    let mut combined: Vec<String> = existing.aliases.clone();
    combined.extend(new.aliases.iter().cloned());
    let aliases = clean_aliases(&existing.name_norm, &combined);
    let description = if existing.description.trim().is_empty() {
        new.description.trim().to_string()
    } else {
        existing.description.clone()
    };
    if aliases == existing.aliases && description == existing.description {
        return Ok(existing);
    }
    conn.execute(
        "UPDATE entities SET aliases_json = ?1, description = ?2, updated_at = ?3 \
         WHERE entity_id = ?4",
        params![
            serde_json::to_string(&aliases)?,
            description,
            now_iso(),
            existing.entity_id
        ],
    )?;
    find_entity_exact_conn(conn, &existing.name_norm, &existing.entity_type)?
        .context("entity reload after merge failed")
}

impl Db {
    /// 建实体;同 (name_norm, entity_type) 已存在则并入别名后复用(不覆盖非空描述)。
    /// 归一化名为空的输入直接拒绝,避免污染唯一索引。
    ///
    /// 查-改-写在 `BEGIN IMMEDIATE` 里完成:别名是读-并-写,若走自动提交,
    /// 并发写会互相覆盖导致别名丢失(1b 并行抽取会踩到)。
    pub fn upsert_entity(&self, new: &NewEntity) -> Result<EntityRecord> {
        let name_norm = normalize_entity_name(&new.name);
        anyhow::ensure!(!name_norm.is_empty(), "entity name must not be empty");
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = if let Some(existing) =
            find_entity_exact_conn(&tx, &name_norm, &new.entity_type)?
        {
            merge_entity_conn(&tx, existing, new)?
        } else {
            let entity_id = format!("ent-{}", build_job_id());
            let now = now_iso();
            tx.execute(
                r#"
                INSERT INTO entities (
                    entity_id, name, name_norm, entity_type, aliases_json, description, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                ON CONFLICT(name_norm, entity_type) DO NOTHING
                "#,
                params![
                    entity_id,
                    new.name.trim(),
                    name_norm,
                    new.entity_type,
                    serde_json::to_string(&clean_aliases(&name_norm, &new.aliases))?,
                    new.description.trim(),
                    now,
                ],
            )?;
            find_entity_exact_conn(&tx, &name_norm, &new.entity_type)?
                .with_context(|| format!("entity upsert failed: {}", new.name))?
        };
        tx.commit()?;
        Ok(record)
    }

    pub fn find_entity_exact(
        &self,
        name_norm: &str,
        entity_type: &str,
    ) -> Result<Option<EntityRecord>> {
        let conn = self.connect()?;
        find_entity_exact_conn(&conn, name_norm, entity_type)
    }

    /// 按归一化名/别名解析实体(不限类型):先规范名精确,再别名精确。
    /// 抽取消歧(新写法并进已有实体)与关系端点解析都用它。
    pub fn resolve_entity(&self, name_norm: &str) -> Result<Option<EntityRecord>> {
        if name_norm.is_empty() {
            return Ok(None);
        }
        let conn = self.connect()?;
        let sql = format!("SELECT {ENTITY_COLUMNS} FROM entities WHERE name_norm = ?1 LIMIT 1");
        let by_name = conn
            .query_row(&sql, params![name_norm], row_to_entity)
            .optional()?;
        if by_name.is_some() {
            return Ok(by_name);
        }
        find_entity_by_alias_conn(&conn, name_norm)
    }

    pub fn get_entity(&self, entity_id: &str) -> Result<EntityRecord> {
        let conn = self.connect()?;
        let sql = format!("SELECT {ENTITY_COLUMNS} FROM entities WHERE entity_id = ?1");
        let record = conn
            .query_row(&sql, params![entity_id], row_to_entity)
            .with_context(|| format!("entity not found: {entity_id}"))?;
        Ok(record)
    }

    /// 词面检索:规范名精确 > 规范名/别名包含。按提及数排序。
    pub fn search_entities(
        &self,
        query: &str,
        entity_type: Option<&str>,
        limit: u32,
    ) -> Result<Vec<EntitySummary>> {
        let query = query.trim();
        let name_norm = normalize_entity_name(query);
        let like = format!("%{}%", query.replace(['%', '_'], ""));
        let type_filter = entity_type.map(str::trim).filter(|value| !value.is_empty());
        let conn = self.connect()?;

        let (sql, binds_type) = if type_filter.is_some() {
            (
                format!(
                    r#"
                    SELECT {SUMMARY_COLUMNS}
                    FROM entities e
                    WHERE (e.name_norm = ?1 OR e.name LIKE ?2 OR e.aliases_json LIKE ?2)
                      AND e.entity_type = ?4
                    ORDER BY (e.name_norm = ?1) DESC, 5 DESC, e.name ASC
                    LIMIT ?3
                    "#
                ),
                true,
            )
        } else {
            (
                format!(
                    r#"
                    SELECT {SUMMARY_COLUMNS}
                    FROM entities e
                    WHERE e.name_norm = ?1 OR e.name LIKE ?2 OR e.aliases_json LIKE ?2
                    ORDER BY (e.name_norm = ?1) DESC, 5 DESC, e.name ASC
                    LIMIT ?3
                    "#
                ),
                false,
            )
        };

        let mut stmt = conn.prepare(&sql)?;
        let mut rows = if binds_type {
            stmt.query(params![
                name_norm,
                like,
                limit as i64,
                type_filter.unwrap_or("")
            ])?
        } else {
            stmt.query(params![name_norm, like, limit as i64])?
        };
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(row_to_summary(row)?);
        }
        Ok(items)
    }

    /// 列出实体(供提及扫描遍历)。上限保护避免超大库拖垮扫描。
    pub fn list_entities(&self, limit: u32) -> Result<Vec<EntityRecord>> {
        let conn = self.connect()?;
        let sql = format!(
            "SELECT {ENTITY_COLUMNS} FROM entities ORDER BY updated_at DESC LIMIT ?1"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit as i64], row_to_entity)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    /// 挂一条实体→block 证据。同 (doc,page,block,entity) 重复挂载直接忽略。
    pub fn link_block_entity(&self, link: &BlockEntityLink) -> Result<()> {
        let conn = self.connect()?;
        conn.execute(
            r#"
            INSERT INTO block_entities (
                document_id, entity_id, page_idx, block_id, job_id,
                surface_form, snippet, confidence, source, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(document_id, page_idx, block_id, entity_id) DO NOTHING
            "#,
            params![
                link.document_id,
                link.entity_id,
                link.page_idx,
                link.block_id,
                link.job_id,
                link.surface_form,
                link.snippet,
                link.confidence,
                link.source,
                now_iso(),
            ],
        )?;
        Ok(())
    }

    /// 写一条有向关系。同 (from, to, type) 已存在则忽略——多次抽取/多文档
    /// 断言同一关系只留第一条(关系是启发式产物,不追多来源)。
    /// 返回是否真的写入。
    ///
    /// 查-写在 `BEGIN IMMEDIATE` 里完成,并发抽取不会插出重复三元组。
    ///
    /// ponytail: 只留首条来源,清掉首条所在文档的抽取产物会连带丢掉其他文档
    /// 也断言过的同一关系;要保多来源时改成按 (doc, triple) 存 + 读取去重。
    pub fn add_entity_relation(&self, relation: &NewEntityRelation) -> Result<bool> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            r#"
            INSERT INTO entity_relations (
                relation_id, from_entity_id, to_entity_id, relation_type, confidence,
                explanation, source_document_id, source_block_id, created_at
            )
            SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9
            WHERE NOT EXISTS (
                SELECT 1 FROM entity_relations
                WHERE from_entity_id = ?2 AND to_entity_id = ?3 AND relation_type = ?4
            )
            "#,
            params![
                format!("rel-{}", build_job_id()),
                relation.from_entity_id,
                relation.to_entity_id,
                relation.relation_type,
                relation.confidence,
                relation.explanation,
                relation.source_document_id,
                relation.source_block_id,
                now_iso(),
            ],
        )?;
        tx.commit()?;
        Ok(changed > 0)
    }

    /// 某实体的邻居:出边 + 入边,按 confidence 降序。
    pub fn related_entities(
        &self,
        entity_id: &str,
        relation_type: Option<&str>,
        limit: u32,
    ) -> Result<Vec<RelatedEntity>> {
        let relation_type = relation_type.map(str::trim).filter(|value| !value.is_empty());
        let conn = self.connect()?;
        let mut stmt = conn.prepare(RELATED_ENTITIES_SQL)?;
        let mut rows = if let Some(relation_type) = relation_type {
            stmt.query(params![entity_id, relation_type, limit as i64])?
        } else {
            stmt.query(params![entity_id, "", limit as i64])?
        };
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(row_to_related(row)?);
        }
        Ok(items)
    }

    /// 清空某文档的术语表证据(重建 glossary 链前调用)。抽取证据与关系不动。
    pub fn clear_document_graph(&self, document_id: &str) -> Result<()> {
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM block_entities WHERE document_id = ?1 AND source = 'glossary'",
            params![document_id],
        )?;
        Ok(())
    }

    /// 清空某文档的抽取产物(重新抽取前调用):抽取来源的证据 + 该文档发起的关系,
    /// 并重置抽取状态。
    pub fn clear_document_extraction(&self, document_id: &str) -> Result<()> {
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM block_entities WHERE document_id = ?1 AND source = 'extraction'",
            params![document_id],
        )?;
        conn.execute(
            "DELETE FROM entity_relations WHERE source_document_id = ?1",
            params![document_id],
        )?;
        conn.execute(
            "UPDATE documents SET graph_extracted_at = NULL WHERE document_id = ?1",
            params![document_id],
        )?;
        Ok(())
    }

    pub fn mark_document_graph_extracted(&self, document_id: &str) -> Result<()> {
        let conn = self.connect()?;
        conn.execute(
            "UPDATE documents SET graph_extracted_at = ?2 WHERE document_id = ?1",
            params![document_id, now_iso()],
        )?;
        Ok(())
    }

    pub fn list_entity_mentions(
        &self,
        entity_id: &str,
        document_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<EntityMention>> {
        let doc_filter = document_id.map(str::trim).filter(|value| !value.is_empty());
        let conn = self.connect()?;
        let (sql, binds_doc) = if doc_filter.is_some() {
            (
                "SELECT document_id, job_id, page_idx, block_id, snippet FROM block_entities
                 WHERE entity_id = ?1 AND document_id = ?2
                 ORDER BY document_id, page_idx, block_id LIMIT ?3",
                true,
            )
        } else {
            (
                "SELECT document_id, job_id, page_idx, block_id, snippet FROM block_entities
                 WHERE entity_id = ?1
                 ORDER BY document_id, page_idx, block_id LIMIT ?2",
                false,
            )
        };
        let mut stmt = conn.prepare(sql)?;
        let mut rows = if binds_doc {
            stmt.query(params![entity_id, doc_filter.unwrap_or(""), limit as i64])?
        } else {
            stmt.query(params![entity_id, limit as i64])?
        };
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(EntityMention {
                document_id: row.get(0)?,
                job_id: row.get(1)?,
                page_idx: row.get(2)?,
                block_id: row.get(3)?,
                snippet: row.get(4)?,
            });
        }
        Ok(items)
    }

    /// 某文档命中的实体概览。mention_count 是本文档内的提及数,
    /// document_count 仍是全库覆盖文档数。
    pub fn entities_for_document(
        &self,
        document_id: &str,
        limit: u32,
    ) -> Result<Vec<EntitySummary>> {
        let conn = self.connect()?;
        let sql = r#"
            SELECT e.entity_id, e.name, e.entity_type, e.aliases_json,
                (SELECT COUNT(*) FROM block_entities b
                 WHERE b.entity_id = e.entity_id AND b.document_id = ?1),
                (SELECT COUNT(DISTINCT b.document_id) FROM block_entities b
                 WHERE b.entity_id = e.entity_id)
            FROM entities e
            WHERE EXISTS (
                SELECT 1 FROM block_entities b
                WHERE b.entity_id = e.entity_id AND b.document_id = ?1
            )
            ORDER BY (
                SELECT COUNT(*) FROM block_entities b
                WHERE b.entity_id = e.entity_id AND b.document_id = ?1
            ) DESC, e.name ASC
            LIMIT ?2
            "#;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![document_id, limit as i64], row_to_summary)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    /// 概念页的证据片段:block_entities 挂文档标题,按文档/页码/块排序。
    pub fn list_entity_page_evidence(
        &self,
        entity_id: &str,
        limit: u32,
    ) -> Result<Vec<EntityPageEvidence>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT b.document_id, COALESCE(d.title, ''), b.job_id, b.page_idx, b.block_id, b.snippet
             FROM block_entities b
             LEFT JOIN documents d ON d.document_id = b.document_id
             WHERE b.entity_id = ?1
             ORDER BY b.document_id, b.page_idx, b.block_id
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![entity_id, limit as i64], |row| {
            Ok(EntityPageEvidence {
                document_id: row.get(0)?,
                document_title: row.get(1)?,
                job_id: row.get(2)?,
                page_idx: row.get(3)?,
                block_id: row.get(4)?,
                snippet: row.get(5)?,
            })
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    /// 证据签名:提及与关系的 (count, max(rowid))。新增/删除证据或关系都会改变它,
    /// 用来判断概念页是否 stale —— 不用在抽取路径上写标记。
    pub fn entity_page_evidence_sig(&self, entity_id: &str) -> Result<String> {
        let conn = self.connect()?;
        let (mentions, mention_row): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(MAX(rowid), 0) FROM block_entities WHERE entity_id = ?1",
            params![entity_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let (relations, relation_row): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(MAX(rowid), 0) FROM entity_relations
             WHERE from_entity_id = ?1 OR to_entity_id = ?1",
            params![entity_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(format!("m{mentions}:{mention_row}:r{relations}:{relation_row}"))
    }

    pub fn get_entity_page(&self, entity_id: &str) -> Result<Option<EntityPageRecord>> {
        let conn = self.connect()?;
        let sql =
            format!("SELECT {ENTITY_PAGE_COLUMNS} FROM entity_pages WHERE entity_id = ?1");
        let page = conn
            .query_row(&sql, params![entity_id], row_to_entity_page)
            .optional()?;
        Ok(page)
    }

    /// 整页覆盖写(生成/刷新都是全量替换)。
    pub fn upsert_entity_page(&self, page: &EntityPageRecord) -> Result<()> {
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO entity_pages (entity_id, body_md, citations_json, evidence_sig, generated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(entity_id) DO UPDATE SET
                 body_md = excluded.body_md,
                 citations_json = excluded.citations_json,
                 evidence_sig = excluded.evidence_sig,
                 generated_at = excluded.generated_at",
            params![
                page.entity_id,
                page.body_md,
                serde_json::to_string(&page.citations).unwrap_or_else(|_| "[]".to_string()),
                page.evidence_sig,
                page.generated_at,
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    struct TestDbFs {
        root: PathBuf,
        data_root: PathBuf,
        db_path: PathBuf,
    }

    impl TestDbFs {
        fn new(test_name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rust-api-db-graph-{test_name}-{}",
                fastrand::u64(..)
            ));
            let data_root = root.join("data");
            let db_path = root.join("db").join("jobs.db");
            fs::create_dir_all(&data_root).expect("create data root");
            fs::create_dir_all(db_path.parent().expect("db parent")).expect("create db dir");
            Self {
                root,
                data_root,
                db_path,
            }
        }

        fn db(&self) -> Db {
            Db::new(self.db_path.clone(), self.data_root.clone())
        }
    }

    impl Drop for TestDbFs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn seed_document(db: &Db, document_id: &str) {
        let conn = db.connect().expect("connect");
        conn.execute(
            "INSERT INTO documents (document_id, title, source_filename, added_at, updated_at)
             VALUES (?1, 'paper', 'paper.pdf', '2026-09-09T00:00:00Z', '2026-09-09T00:00:00Z')",
            params![document_id],
        )
        .expect("insert document");
    }

    fn new_entity(name: &str, entity_type: &str, aliases: &[&str]) -> NewEntity {
        NewEntity {
            name: name.to_string(),
            entity_type: entity_type.to_string(),
            aliases: aliases.iter().map(|value| value.to_string()).collect(),
            description: String::new(),
        }
    }

    #[test]
    fn normalize_collapses_space_and_case() {
        assert_eq!(normalize_entity_name("  Halogen   Lithium  "), "halogen lithium");
        assert_eq!(normalize_entity_name("卤素锂交换"), "卤素锂交换");
    }

    #[test]
    fn upsert_is_idempotent_on_norm_and_type() {
        let fs = TestDbFs::new("upsert");
        let db = fs.db();
        let first = db
            .upsert_entity(&new_entity("Halogen Lithium", "method", &["卤素锂交换"]))
            .expect("first");
        let second = db
            .upsert_entity(&new_entity("halogen  lithium", "method", &["halogen-lithium"]))
            .expect("second");
        assert_eq!(first.entity_id, second.entity_id);
        // 同规范名不同类型 = 两个实体
        let other = db
            .upsert_entity(&new_entity("Halogen Lithium", "concept", &[]))
            .expect("other type");
        assert_ne!(first.entity_id, other.entity_id);
    }

    #[test]
    fn upsert_merges_aliases_into_existing() {
        let fs = TestDbFs::new("merge-aliases");
        let db = fs.db();
        let first = db
            .upsert_entity(&new_entity("卤素锂交换", "term", &["HLE"]))
            .expect("first");
        let second = db
            .upsert_entity(&new_entity(
                "卤素锂交换",
                "term",
                &["halogen lithium exchange", "HLE", "卤素锂交换"],
            ))
            .expect("second");
        assert_eq!(first.entity_id, second.entity_id);
        let mut aliases = second.aliases.clone();
        aliases.sort();
        // 新别名并入、重复别名与规范名本身都被去掉
        assert_eq!(
            aliases,
            vec!["HLE".to_string(), "halogen lithium exchange".to_string()]
        );
        // 非空描述不被覆盖
        db.upsert_entity(&NewEntity {
            name: "X".to_string(),
            entity_type: "concept".to_string(),
            aliases: Vec::new(),
            description: "first description".to_string(),
        })
        .expect("x first");
        let kept = db
            .upsert_entity(&NewEntity {
                name: "X".to_string(),
                entity_type: "concept".to_string(),
                aliases: vec!["alias-x".to_string()],
                description: "second description".to_string(),
            })
            .expect("x second");
        assert_eq!(kept.description, "first description");
        assert_eq!(kept.aliases, vec!["alias-x".to_string()]);
    }

    /// 并发 upsert 同一实体:BEGIN IMMEDIATE 串行化读-改-写,别名不许丢。
    #[test]
    fn concurrent_upserts_union_aliases() {
        let fs = TestDbFs::new("concurrent-aliases");
        let db = fs.db();
        db.upsert_entity(&new_entity("E", "concept", &["seed"]))
            .expect("seed");
        std::thread::scope(|scope| {
            for index in 0..12 {
                let db = db.clone();
                scope.spawn(move || {
                    let alias = format!("alias-{index:02}");
                    db.upsert_entity(&new_entity("E", "concept", &[alias.as_str()]))
                        .expect("concurrent upsert");
                });
            }
        });
        let record = db
            .find_entity_exact("e", "concept")
            .expect("find")
            .expect("entity exists");
        let mut aliases = record.aliases.clone();
        aliases.sort();
        assert_eq!(aliases.len(), 13, "aliases: {aliases:?}");
        for expected in std::iter::once("seed".to_string())
            .chain((0..12).map(|index| format!("alias-{index:02}")))
        {
            assert!(aliases.contains(&expected), "missing {expected}: {aliases:?}");
        }
    }

    #[test]
    fn search_prefers_exact_then_mentions() {
        let fs = TestDbFs::new("search");
        let db = fs.db();
        seed_document(&db, "doc-1");
        let exact = db
            .upsert_entity(&new_entity("芳基锂", "concept", &["aryllithium"]))
            .expect("exact");
        let fuzzy = db
            .upsert_entity(&new_entity("芳基锂交换", "method", &[]))
            .expect("fuzzy");
        db.link_block_entity(&BlockEntityLink {
            document_id: "doc-1".to_string(),
            entity_id: fuzzy.entity_id.clone(),
            page_idx: 0,
            block_id: "p001-b0000".to_string(),
            job_id: "job-1".to_string(),
            surface_form: "芳基锂交换".to_string(),
            snippet: "芳基锂交换…".to_string(),
            confidence: 1.0,
            source: "glossary".to_string(),
        })
        .expect("link");

        let by_name = db.search_entities("芳基锂", None, 10).expect("search");
        assert_eq!(by_name[0].entity_id, exact.entity_id);
        let by_alias = db.search_entities("aryllithium", None, 10).expect("search alias");
        assert_eq!(by_alias[0].entity_id, exact.entity_id);
        assert_eq!(by_name.iter().find(|item| item.entity_id == fuzzy.entity_id).unwrap().mention_count, 1);
    }

    #[test]
    fn resolve_entity_matches_name_and_alias() {
        let fs = TestDbFs::new("resolve");
        let db = fs.db();
        let entity = db
            .upsert_entity(&new_entity(
                "卤素锂交换",
                "term",
                &["HLE", "halogen  lithium exchange"],
            ))
            .expect("entity");
        assert_eq!(
            db.resolve_entity("卤素锂交换")
                .expect("by name")
                .expect("hit")
                .entity_id,
            entity.entity_id
        );
        assert_eq!(
            db.resolve_entity("hle").expect("by alias").expect("hit").entity_id,
            entity.entity_id
        );
        // 别名里的多空格按归一化名命中
        assert_eq!(
            db.resolve_entity("halogen lithium exchange")
                .expect("norm alias")
                .expect("hit")
                .entity_id,
            entity.entity_id
        );
        assert!(db.resolve_entity("nope").expect("miss").is_none());
    }

    #[test]
    fn relations_dedupe_and_read_both_directions() {
        let fs = TestDbFs::new("relations");
        let db = fs.db();
        seed_document(&db, "doc-1");
        let method = db.upsert_entity(&new_entity("方法A", "method", &[])).expect("a");
        let material = db.upsert_entity(&new_entity("材料B", "material", &[])).expect("b");
        let metric = db.upsert_entity(&new_entity("指标C", "metric", &[])).expect("c");
        let rel = |from: &str, to: &str, kind: &str| NewEntityRelation {
            from_entity_id: from.to_string(),
            to_entity_id: to.to_string(),
            relation_type: kind.to_string(),
            confidence: 0.9,
            explanation: "依据".to_string(),
            source_document_id: "doc-1".to_string(),
            source_block_id: String::new(),
        };
        assert!(db
            .add_entity_relation(&rel(&method.entity_id, &material.entity_id, "uses"))
            .expect("insert"));
        // 同 (from,to,type) 重复写入被忽略
        assert!(!db
            .add_entity_relation(&rel(&method.entity_id, &material.entity_id, "uses"))
            .expect("dup"));
        assert!(db
            .add_entity_relation(&rel(&metric.entity_id, &method.entity_id, "evaluates"))
            .expect("insert2"));

        let related = db.related_entities(&method.entity_id, None, 10).expect("related");
        assert_eq!(related.len(), 2);
        let out = related
            .iter()
            .find(|item| item.entity_id == material.entity_id)
            .expect("out edge");
        assert_eq!(out.direction, "out");
        assert_eq!(out.relation_type, "uses");
        assert_eq!(out.source_document_id, "doc-1");
        let incoming = related
            .iter()
            .find(|item| item.entity_id == metric.entity_id)
            .expect("in edge");
        assert_eq!(incoming.direction, "in");

        let filtered = db
            .related_entities(&method.entity_id, Some("uses"), 10)
            .expect("filtered");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].entity_id, material.entity_id);

        // 两种来源的证据分别可清:抽取清空不动 glossary 证据
        for (source, block) in [("glossary", "p001-b0000"), ("extraction", "p001-b0001")] {
            db.link_block_entity(&BlockEntityLink {
                document_id: "doc-1".to_string(),
                entity_id: method.entity_id.clone(),
                page_idx: 0,
                block_id: block.to_string(),
                job_id: "job-1".to_string(),
                surface_form: "方法A".to_string(),
                snippet: "方法A…".to_string(),
                confidence: 1.0,
                source: source.to_string(),
            })
            .expect("link");
        }
        db.clear_document_extraction("doc-1").expect("clear extraction");
        assert!(db
            .related_entities(&method.entity_id, None, 10)
            .expect("after")
            .is_empty());
        assert_eq!(
            db.list_entity_mentions(&method.entity_id, None, 10)
                .expect("mentions")
                .len(),
            1
        );
        db.clear_document_graph("doc-1").expect("clear glossary");
        assert!(db
            .list_entity_mentions(&method.entity_id, None, 10)
            .expect("after glossary clear")
            .is_empty());

        // 删文档:证据走 FK 级联,关系无 document FK 需显式清,否则留下死出处
        db.add_entity_relation(&rel(&method.entity_id, &material.entity_id, "uses"))
            .expect("reinsert");
        db.link_block_entity(&BlockEntityLink {
            document_id: "doc-1".to_string(),
            entity_id: method.entity_id.clone(),
            page_idx: 0,
            block_id: "p001-b0002".to_string(),
            job_id: "job-1".to_string(),
            surface_form: "方法A".to_string(),
            snippet: "方法A…".to_string(),
            confidence: 1.0,
            source: "glossary".to_string(),
        })
        .expect("link");
        assert!(db.delete_document("doc-1").expect("delete doc"));
        assert!(db
            .related_entities(&method.entity_id, None, 10)
            .expect("after delete")
            .is_empty());
        assert!(db
            .list_entity_mentions(&method.entity_id, None, 10)
            .expect("after delete mentions")
            .is_empty());
    }

    #[test]
    fn mentions_are_ordered_and_document_scoped() {
        let fs = TestDbFs::new("mentions");
        let db = fs.db();
        seed_document(&db, "doc-1");
        seed_document(&db, "doc-2");
        let entity = db.upsert_entity(&new_entity("卤素", "term", &[])).expect("entity");
        for (doc, page) in [("doc-1", 1_i64), ("doc-1", 0_i64), ("doc-2", 0_i64)] {
            db.link_block_entity(&BlockEntityLink {
                document_id: doc.to_string(),
                entity_id: entity.entity_id.clone(),
                page_idx: page,
                block_id: format!("p{page:03}-b0000"),
                job_id: format!("job-{doc}"),
                surface_form: "卤素".to_string(),
                snippet: "卤素…".to_string(),
                confidence: 1.0,
                source: "glossary".to_string(),
            })
            .expect("link");
        }
        let all = db.list_entity_mentions(&entity.entity_id, None, 10).expect("all");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].document_id, "doc-1");
        assert_eq!(all[0].page_idx, 0);
        let scoped = db
            .list_entity_mentions(&entity.entity_id, Some("doc-2"), 10)
            .expect("scoped");
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].document_id, "doc-2");

        let per_doc = db.entities_for_document("doc-1", 10).expect("per doc");
        assert_eq!(per_doc.len(), 1);
        assert_eq!(per_doc[0].mention_count, 2);
        assert_eq!(per_doc[0].document_count, 2);

        db.clear_document_graph("doc-1").expect("clear");
        assert_eq!(db.list_entity_mentions(&entity.entity_id, None, 10).expect("after").len(), 1);
    }

    #[test]
    fn entity_page_roundtrip_and_stale_signature() {
        let fs = TestDbFs::new("entity-page");
        let db = fs.db();
        seed_document(&db, "doc-1");
        let entity = db.upsert_entity(&new_entity("方法A", "method", &[])).expect("entity");
        assert!(db.get_entity_page(&entity.entity_id).expect("no page").is_none());

        // 无证据时签名为全零
        let empty_sig = db.entity_page_evidence_sig(&entity.entity_id).expect("sig");
        assert_eq!(empty_sig, "m0:0:r0:0");

        let link = |block: &str| BlockEntityLink {
            document_id: "doc-1".to_string(),
            entity_id: entity.entity_id.clone(),
            page_idx: 0,
            block_id: block.to_string(),
            job_id: "job-1".to_string(),
            surface_form: "方法A".to_string(),
            snippet: "方法A 的片段".to_string(),
            confidence: 1.0,
            source: "glossary".to_string(),
        };
        db.link_block_entity(&link("p001-b0000")).expect("link");
        let after_mention = db.entity_page_evidence_sig(&entity.entity_id).expect("sig");
        assert_ne!(empty_sig, after_mention);

        let page = EntityPageRecord {
            entity_id: entity.entity_id.clone(),
            body_md: "方法A 是一种方法 [1]。".to_string(),
            citations: vec![crate::models::api::EntityPageCitation {
                ref_num: 1,
                document_id: "doc-1".to_string(),
                document_title: "paper".to_string(),
                job_id: "job-1".to_string(),
                page_idx: 0,
                block_id: "p001-b0000".to_string(),
                snippet: "方法A 的片段".to_string(),
            }],
            evidence_sig: after_mention.clone(),
            generated_at: "2026-09-09T00:00:00Z".to_string(),
        };
        db.upsert_entity_page(&page).expect("upsert");
        let loaded = db
            .get_entity_page(&entity.entity_id)
            .expect("load")
            .expect("some");
        assert_eq!(loaded.body_md, page.body_md);
        assert_eq!(loaded.citations.len(), 1);
        assert_eq!(loaded.citations[0].ref_num, 1);
        assert_eq!(loaded.citations[0].document_title, "paper");
        // 签名一致 = 不算 stale
        assert_eq!(loaded.evidence_sig, db.entity_page_evidence_sig(&entity.entity_id).expect("sig"));

        // 新增证据 → 签名变化(调用方据此判定 stale)
        db.link_block_entity(&link("p001-b0001")).expect("link2");
        assert_ne!(
            loaded.evidence_sig,
            db.entity_page_evidence_sig(&entity.entity_id).expect("sig2")
        );

        // 覆盖写整页
        let mut updated = page.clone();
        updated.body_md = "重写 [1]。".to_string();
        db.upsert_entity_page(&updated).expect("re-upsert");
        assert_eq!(
            db.get_entity_page(&entity.entity_id).expect("load2").expect("some").body_md,
            "重写 [1]。"
        );

        // 证据列表带文档标题
        let evidence = db.list_entity_page_evidence(&entity.entity_id, 10).expect("evidence");
        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[0].document_title, "paper");
        assert_eq!(evidence[0].document_id, "doc-1");

        // 删实体 → 页随 FK 级联删除
        let conn = db.connect().expect("connect");
        conn.execute("DELETE FROM entities WHERE entity_id = ?1", params![entity.entity_id])
            .expect("delete entity");
        assert!(db.get_entity_page(&entity.entity_id).expect("after delete").is_none());
    }
}

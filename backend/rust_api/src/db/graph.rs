//! 概念图谱持久化:实体消歧、block 证据挂载、实体检索。
//! 全部走 Db facade,路由/服务层不直接写 SQL。

use anyhow::{Context, Result};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, TransactionBehavior};

use crate::models::domain::{build_job_id, now_iso};
use crate::models::api::{
    BlockEntityLink, DocumentBlockEntity, EntityMention, EntityPageEvidence, EntityPageRecord,
    EntityRecord, EntitySummary, NewEntity, NewEntityRelation, RelatedEntity,
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

/// 一次合并的源实体上限。数字占位符 + params_from_iter 拼 IN 列表,SQLite 上限
/// 32766,50 远低于它,无需临时表。
const MAX_MERGE_SOURCES: usize = 50;

/// 实体改名结果。校验在事务内完成,视图层只做映射,不存在 TOCTOU 窗口。
// 变体大小不均是刻意的:成功的记录原地携带,失败变体保持轻量。
#[allow(clippy::large_enum_variant)]
pub enum RenameOutcome {
    Renamed(EntityRecord),
    NotFound,
    EmptyName,
    /// 归一化后撞上另一个同类型实体:让用户改用合并,而不是静默合并。
    Conflict { existing_name: String },
}

/// 实体合并结果。
#[allow(clippy::large_enum_variant)]
pub enum MergeOutcome {
    Merged(MergeSummary),
    TargetNotFound,
    SourceNotFound(String),
    EmptySources,
    SourceIsTarget,
    TooManySources { max: usize },
}

pub struct MergeSummary {
    pub target: EntityRecord,
    pub merged: Vec<String>,
    pub mentions: i64,
    pub relations: i64,
    /// 目标原本无页、采纳了某个源页。
    pub page_adopted: bool,
}

/// 按 entity_id 载入实体(接受 `&Connection`,也接受事务——`&Transaction` 解引用)。
/// 所有事务内读取都必须走它,不能用 `self.get_entity`(会另开连接抢写锁 → 死锁)。
fn load_entity_conn(conn: &Connection, entity_id: &str) -> Result<Option<EntityRecord>> {
    let sql = format!("SELECT {ENTITY_COLUMNS} FROM entities WHERE entity_id = ?1");
    Ok(conn
        .query_row(&sql, params![entity_id], row_to_entity)
        .optional()?)
}

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
    "entity_id, body_md, citations_json, evidence_sig, generated_at, edited_body_md, edited_at";

fn row_to_entity_page(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntityPageRecord> {
    let citations_json: String = row.get(2)?;
    Ok(EntityPageRecord {
        entity_id: row.get(0)?,
        body_md: row.get(1)?,
        citations: serde_json::from_str(&citations_json).unwrap_or_default(),
        evidence_sig: row.get(3)?,
        generated_at: row.get(4)?,
        edited_body_md: row.get(5)?,
        edited_at: row.get(6)?,
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

    /// 问题文本里「出现」的实体:实体名/别名是问题整句的子串。
    /// 整句走 `search_entities` 的 `name LIKE %整句%` 永远不中,必须反过来用 instr。
    /// 单字符名/空别名排除——`instr(x,'')` 恒为 1,英文短名也会在词内命中(ai 命中 retain)。
    ///
    /// ponytail: 全表扫描无索引;实体上万再建 FTS 或倒排表。
    pub fn entities_mentioned_in(&self, text: &str, limit: u32) -> Result<Vec<EntitySummary>> {
        let haystack = normalize_entity_name(text);
        if haystack.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            r#"
            SELECT {SUMMARY_COLUMNS}
            FROM entities e
            WHERE length(e.name_norm) >= 2 AND (
                instr(?1, e.name_norm) > 0
                OR (json_valid(e.aliases_json) AND EXISTS (
                    SELECT 1 FROM json_each(e.aliases_json) j
                    WHERE length(j.value) >= 2 AND instr(?1, lower(j.value)) > 0
                ))
            )
            ORDER BY length(e.name_norm) DESC, 5 DESC, e.name ASC
            LIMIT ?2
            "#
        );
        let conn = self.connect()?;
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![haystack, limit as i64])?;
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

    /// 某文档的全部 block 证据行(relink 差量对比用)。
    pub fn list_document_block_entities(&self, document_id: &str) -> Result<Vec<DocumentBlockEntity>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT entity_id, block_id, source FROM block_entities
             WHERE document_id = ?1 ORDER BY entity_id, block_id",
        )?;
        let rows = stmt.query_map(params![document_id], |row| {
            Ok(DocumentBlockEntity {
                entity_id: row.get(0)?,
                block_id: row.get(1)?,
                source: row.get(2)?,
            })
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    /// relink 差量落库:删一批 (entity, block)、插一批新证据,同一事务内完成。
    ///
    /// 刻意不做「全删全插」:block_entities 的 rowid 参与 `entity_page_evidence_sig`,
    /// 重插会换 rowid → 全库概念页集体 stale。差量让未变行的 rowid 保持原样。
    pub fn apply_document_relink(
        &self,
        document_id: &str,
        inserts: &[BlockEntityLink],
        deletes: &[(String, String)],
    ) -> Result<()> {
        if inserts.is_empty() && deletes.is_empty() {
            return Ok(());
        }
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut delete_stmt = tx.prepare(
                "DELETE FROM block_entities
                 WHERE document_id = ?1 AND entity_id = ?2 AND block_id = ?3",
            )?;
            for (entity_id, block_id) in deletes {
                delete_stmt.execute(params![document_id, entity_id, block_id])?;
            }
        }
        {
            let mut insert_stmt = tx.prepare(
                r#"
                INSERT INTO block_entities (
                    document_id, entity_id, page_idx, block_id, job_id,
                    surface_form, snippet, confidence, source, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(document_id, page_idx, block_id, entity_id) DO NOTHING
                "#,
            )?;
            for link in inserts {
                insert_stmt.execute(params![
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
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 改名:旧原始名并入别名(读时的 `[[名]]`/反链/收藏解析靠别名继续命中),
    /// `entity_type` 与 `entity_id` 不动。归一化后撞上另一个同类型实体 → Conflict。
    ///
    /// 查-改-写在 `BEGIN IMMEDIATE` 里完成,校验与写入之间无 TOCTOU 窗口。
    pub fn rename_entity(&self, entity_id: &str, new_name: &str) -> Result<RenameOutcome> {
        let trimmed = new_name.trim();
        let new_norm = normalize_entity_name(trimmed);
        if new_norm.is_empty() {
            return Ok(RenameOutcome::EmptyName);
        }
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(target) = load_entity_conn(&tx, entity_id)? else {
            return Ok(RenameOutcome::NotFound);
        };
        if new_norm != target.name_norm {
            if let Some(existing) = find_entity_exact_conn(&tx, &new_norm, &target.entity_type)? {
                if existing.entity_id != target.entity_id {
                    return Ok(RenameOutcome::Conflict {
                        existing_name: existing.name,
                    });
                }
            }
        }
        let mut combined: Vec<String> = target.aliases.clone();
        combined.push(target.name.clone());
        let aliases = clean_aliases(&new_norm, &combined);
        tx.execute(
            "UPDATE entities SET name = ?1, name_norm = ?2, aliases_json = ?3, updated_at = ?4 \
             WHERE entity_id = ?5",
            params![
                trimmed,
                new_norm,
                serde_json::to_string(&aliases)?,
                now_iso(),
                target.entity_id
            ],
        )?;
        let updated =
            load_entity_conn(&tx, &target.entity_id)?.context("entity reload after rename failed")?;
        tx.commit()?;
        Ok(RenameOutcome::Renamed(updated))
    }

    /// 合并:`target_id` 是幸存者,源实体被删除。证据/关系/概念页全迁到目标,
    /// 源的 name + aliases 并入目标别名。全在一个 `BEGIN IMMEDIATE` 事务内完成。
    ///
    /// 不变量:
    /// - block_entities 用 `INSERT OR IGNORE ... SELECT` 再删源行:entity_id 在主键里,
    ///   `UPDATE` 会被 SQLite 实现成 delete+insert(换 rowid),而 rowid 参与
    ///   `entity_page_evidence_sig`。目标自己的行 rowid 原样保留。
    /// - entity_relations 无唯一键,去重按 `rowid`(relation_id 是 `rel-{ts}-{rand}`,
    ///   字典序 MIN 不等于最旧)。自环删除必须写 `AND from_entity_id = ?target`,
    ///   否则会误删全库无关自环。
    /// - entity_pages 主键是 entity_id,只能活一个:目标无页时采纳最佳源页
    ///   (人工修订优先,其次最新),其余源页随实体级联删除。
    /// - 不动 `documents.graph_extracted_at`,不改目标的 name/name_norm/entity_type。
    pub fn merge_entities(&self, target_id: &str, source_ids: &[String]) -> Result<MergeOutcome> {
        let mut sources: Vec<String> = Vec::new();
        for id in source_ids {
            let id = id.trim();
            if !id.is_empty() && !sources.iter().any(|seen| seen == id) {
                sources.push(id.to_string());
            }
        }
        if sources.is_empty() {
            return Ok(MergeOutcome::EmptySources);
        }
        if sources.iter().any(|source| source == target_id) {
            return Ok(MergeOutcome::SourceIsTarget);
        }
        if sources.len() > MAX_MERGE_SOURCES {
            return Ok(MergeOutcome::TooManySources {
                max: MAX_MERGE_SOURCES,
            });
        }

        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(target) = load_entity_conn(&tx, target_id)? else {
            return Ok(MergeOutcome::TargetNotFound);
        };
        let mut records = Vec::with_capacity(sources.len());
        for source_id in &sources {
            let Some(record) = load_entity_conn(&tx, source_id)? else {
                return Ok(MergeOutcome::SourceNotFound(source_id.clone()));
            };
            records.push(record);
        }

        // 源占位符 ?1..?N,目标恒为 ?N+1(每条语句独立编号)。
        let source_ph = (1..=sources.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let target_ph = format!("?{}", sources.len() + 1);
        // 源在前、目标在后的参数序列(供「IN 源 + 目标」的语句复用)。
        let mut source_then_target: Vec<&str> = sources.iter().map(String::as_str).collect();
        source_then_target.push(target.entity_id.as_str());

        // 迁移 block 证据,再删源行。目标已有的 (doc,page,block) 行连 rowid 保留。
        tx.execute(
            &format!(
                "INSERT OR IGNORE INTO block_entities
                   (document_id, entity_id, page_idx, block_id, job_id,
                    surface_form, snippet, confidence, source, created_at)
                 SELECT document_id, {target_ph}, page_idx, block_id, job_id,
                        surface_form, snippet, confidence, source, created_at
                 FROM block_entities WHERE entity_id IN ({source_ph})"
            ),
            params_from_iter(source_then_target.iter()),
        )?;
        tx.execute(
            &format!("DELETE FROM block_entities WHERE entity_id IN ({source_ph})"),
            params_from_iter(sources.iter()),
        )?;

        // 关系端点改指目标,再清自环、按 (from,to,type) 保留最小 rowid。
        tx.execute(
            &format!(
                "UPDATE entity_relations SET from_entity_id = {target_ph}
                 WHERE from_entity_id IN ({source_ph})"
            ),
            params_from_iter(source_then_target.iter()),
        )?;
        tx.execute(
            &format!(
                "UPDATE entity_relations SET to_entity_id = {target_ph}
                 WHERE to_entity_id IN ({source_ph})"
            ),
            params_from_iter(source_then_target.iter()),
        )?;
        tx.execute(
            "DELETE FROM entity_relations WHERE from_entity_id = ?1 AND to_entity_id = ?1",
            params![target.entity_id],
        )?;
        tx.execute(
            "DELETE FROM entity_relations
              WHERE (from_entity_id = ?1 OR to_entity_id = ?1)
                AND rowid NOT IN (
                  SELECT MIN(rowid) FROM entity_relations
                   WHERE from_entity_id = ?1 OR to_entity_id = ?1
                   GROUP BY from_entity_id, to_entity_id, relation_type
                )",
            params![target.entity_id],
        )?;

        // 目标无页时采纳最佳源页。
        let has_page = tx
            .query_row(
                "SELECT 1 FROM entity_pages WHERE entity_id = ?1",
                params![target.entity_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        let mut page_adopted = false;
        if !has_page {
            let best: Option<String> = tx
                .query_row(
                    &format!(
                        "SELECT entity_id FROM entity_pages WHERE entity_id IN ({source_ph})
                         ORDER BY (edited_body_md <> '') DESC, generated_at DESC LIMIT 1"
                    ),
                    params_from_iter(sources.iter()),
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(best_id) = best {
                tx.execute(
                    "UPDATE entity_pages SET entity_id = ?1 WHERE entity_id = ?2",
                    params![target.entity_id, best_id],
                )?;
                page_adopted = true;
            }
        }

        // 别名并集(每源 name + aliases),描述回退到首个非空源,再删源实体(级联清残留)。
        let mut combined: Vec<String> = target.aliases.clone();
        for record in &records {
            combined.push(record.name.clone());
            combined.extend(record.aliases.iter().cloned());
        }
        let aliases = clean_aliases(&target.name_norm, &combined);
        let description = if target.description.trim().is_empty() {
            records
                .iter()
                .map(|record| record.description.clone())
                .find(|value| !value.trim().is_empty())
                .unwrap_or_default()
        } else {
            target.description.clone()
        };
        tx.execute(
            "UPDATE entities SET aliases_json = ?1, description = ?2, updated_at = ?3 \
             WHERE entity_id = ?4",
            params![
                serde_json::to_string(&aliases)?,
                description,
                now_iso(),
                target.entity_id
            ],
        )?;
        tx.execute(
            &format!("DELETE FROM entities WHERE entity_id IN ({source_ph})"),
            params_from_iter(sources.iter()),
        )?;

        let mentions: i64 = tx.query_row(
            "SELECT COUNT(*) FROM block_entities WHERE entity_id = ?1",
            params![target.entity_id],
            |row| row.get(0),
        )?;
        let relations: i64 = tx.query_row(
            "SELECT COUNT(*) FROM entity_relations
             WHERE from_entity_id = ?1 OR to_entity_id = ?1",
            params![target.entity_id],
            |row| row.get(0),
        )?;
        let updated = load_entity_conn(&tx, &target.entity_id)?
            .context("entity reload after merge failed")?;
        tx.commit()?;
        Ok(MergeOutcome::Merged(MergeSummary {
            target: updated,
            merged: sources,
            mentions,
            relations,
            page_adopted,
        }))
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

    /// 单个实体的摘要(带提及数/覆盖文档数)。不存在返回 None。
    pub fn entity_summary(&self, entity_id: &str) -> Result<Option<EntitySummary>> {
        let conn = self.connect()?;
        let sql = format!("SELECT {SUMMARY_COLUMNS} FROM entities e WHERE e.entity_id = ?1");
        let summary = conn
            .query_row(&sql, params![entity_id], row_to_summary)
            .optional()?;
        Ok(summary)
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

    /// 所有已生成概念页的 (entity_id, 生效正文)。反链读取时现算,不建 page_links 表。
    /// 先滤掉没有 wikilink 的页:用 instr 而非 LIKE,'[[' 在 SQLite 的 LIKE 里是字符类语法。
    /// ponytail: 全表扫 + Rust 侧匹配,概念页上千张前够用;要快再建索引表。
    pub fn list_entity_page_bodies(&self) -> Result<Vec<(String, String)>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT entity_id, CASE WHEN edited_body_md <> '' THEN edited_body_md ELSE body_md END
             FROM entity_pages
             WHERE instr(body_md, '[[') > 0 OR instr(edited_body_md, '[[') > 0",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
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
    ///
    /// `allow_overwrite=false` 时,若库里已有用户修订(edited_body_md 非空)则**不写**
    /// 并返回 false —— 这条 WHERE 是原子守卫,关掉「检查后、写前」被插队的 TOCTOU 窗口。
    pub fn upsert_entity_page(&self, page: &EntityPageRecord, allow_overwrite: bool) -> Result<bool> {
        let conn = self.connect()?;
        let changed = conn.execute(
            "INSERT INTO entity_pages
                (entity_id, body_md, citations_json, evidence_sig, generated_at, edited_body_md, edited_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(entity_id) DO UPDATE SET
                 body_md = excluded.body_md,
                 citations_json = excluded.citations_json,
                 evidence_sig = excluded.evidence_sig,
                 generated_at = excluded.generated_at,
                 edited_body_md = excluded.edited_body_md,
                 edited_at = excluded.edited_at
             WHERE ?8 = 1 OR entity_pages.edited_body_md = ''",
            params![
                page.entity_id,
                page.body_md,
                serde_json::to_string(&page.citations).unwrap_or_else(|_| "[]".to_string()),
                page.evidence_sig,
                page.generated_at,
                page.edited_body_md,
                page.edited_at,
                allow_overwrite,
            ],
        )?;
        Ok(changed > 0)
    }

    /// 保存用户修订(只改修订两列,模型原文 body_md 不动)。返回是否命中该页。
    pub fn set_entity_page_edit(
        &self,
        entity_id: &str,
        edited_body_md: &str,
        edited_at: &str,
    ) -> Result<bool> {
        let conn = self.connect()?;
        let changed = conn.execute(
            "UPDATE entity_pages SET edited_body_md = ?1, edited_at = ?2 WHERE entity_id = ?3",
            params![edited_body_md, edited_at, entity_id],
        )?;
        Ok(changed > 0)
    }

    /// 撤销修订:清空修订两列,模型原文即刻生效。返回是否命中该页。
    pub fn clear_entity_page_edit(&self, entity_id: &str) -> Result<bool> {
        let conn = self.connect()?;
        let changed = conn.execute(
            "UPDATE entity_pages SET edited_body_md = '', edited_at = '' WHERE entity_id = ?1",
            params![entity_id],
        )?;
        Ok(changed > 0)
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
    fn entities_mentioned_in_matches_name_and_alias() {
        let fs = TestDbFs::new("mentioned-in");
        let db = fs.db();
        seed_document(&db, "doc-1");
        let by_name = db
            .upsert_entity(&new_entity("芳基锂", "concept", &[]))
            .expect("by name");
        let by_alias = db
            .upsert_entity(&new_entity("Halogen Lithium Exchange", "method", &["HLE"]))
            .expect("by alias");

        let hits = db
            .entities_mentioned_in("芳基锂的 HLE 反应怎么走", 10)
            .expect("search");
        let ids: Vec<&str> = hits.iter().map(|item| item.entity_id.as_str()).collect();
        assert!(ids.contains(&by_name.entity_id.as_str()), "canonical name matches");
        assert!(ids.contains(&by_alias.entity_id.as_str()), "alias matches");
    }

    #[test]
    fn entities_mentioned_in_ignores_short_names_and_misses() {
        let fs = TestDbFs::new("mentioned-in-short");
        let db = fs.db();
        let single = db
            .upsert_entity(&new_entity("X", "concept", &[]))
            .expect("single char");
        assert!(db
            .entities_mentioned_in("X 与 Y 的对比", 10)
            .expect("search")
            .iter()
            .all(|item| item.entity_id != single.entity_id));
        assert!(db
            .entities_mentioned_in("完全不相关的一句话", 10)
            .expect("search")
            .is_empty());
    }

    #[test]
    fn entities_mentioned_in_orders_by_mention_count_at_equal_length() {
        let fs = TestDbFs::new("mentioned-in-order");
        let db = fs.db();
        seed_document(&db, "doc-1");
        // 名字排序(乙 < 甲)与提及数排序相反,断言只能由 mention_count DESC 满足
        let low = db
            .upsert_entity(&new_entity("乙组", "concept", &[]))
            .expect("low");
        let high = db
            .upsert_entity(&new_entity("甲组", "concept", &[]))
            .expect("high");
        let links = [
            (0, low.entity_id.clone()),
            (1, high.entity_id.clone()),
            (2, high.entity_id.clone()),
        ];
        for (index, entity_id) in links {
            db.link_block_entity(&BlockEntityLink {
                document_id: "doc-1".to_string(),
                entity_id,
                page_idx: 0,
                block_id: format!("p001-b{index:04}"),
                job_id: "job-1".to_string(),
                surface_form: "组".to_string(),
                snippet: "组".to_string(),
                confidence: 1.0,
                source: "glossary".to_string(),
            })
            .expect("link");
        }
        let hits = db.entities_mentioned_in("甲组和乙组", 10).expect("search");
        assert_eq!(hits[0].entity_id, high.entity_id);
        assert_eq!(hits[1].entity_id, low.entity_id);
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
    fn entity_summary_counts_mentions_and_documents() {
        let fs = TestDbFs::new("entity-summary");
        let db = fs.db();
        seed_document(&db, "doc-1");
        seed_document(&db, "doc-2");
        let entity = db
            .upsert_entity(&new_entity("卤素", "term", &["halogen"]))
            .expect("entity");
        for (doc, block) in [
            ("doc-1", "p001-b0000"),
            ("doc-1", "p001-b0001"),
            ("doc-2", "p001-b0000"),
        ] {
            db.link_block_entity(&BlockEntityLink {
                document_id: doc.to_string(),
                entity_id: entity.entity_id.clone(),
                page_idx: 0,
                block_id: block.to_string(),
                job_id: format!("job-{doc}"),
                surface_form: "卤素".to_string(),
                snippet: "卤素…".to_string(),
                confidence: 1.0,
                source: "glossary".to_string(),
            })
            .expect("link");
        }

        let summary = db
            .entity_summary(&entity.entity_id)
            .expect("summary")
            .expect("some");
        assert_eq!(summary.entity_id, entity.entity_id);
        assert_eq!(summary.name, "卤素");
        assert_eq!(summary.aliases, vec!["halogen".to_string()]);
        assert_eq!(summary.mention_count, 3);
        assert_eq!(summary.document_count, 2);
        assert!(db.entity_summary("ent-nope").expect("miss").is_none());
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
            edited_body_md: String::new(),
            edited_at: String::new(),
        };
        assert!(db.upsert_entity_page(&page, true).expect("upsert"));
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
        db.upsert_entity_page(&updated, true).expect("re-upsert");
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

    /// 造一条概念页记录(edited 为空串 = 无修订)。
    fn page_record(entity_id: &str, body: &str, edited: &str) -> EntityPageRecord {
        EntityPageRecord {
            entity_id: entity_id.to_string(),
            body_md: body.to_string(),
            citations: Vec::new(),
            evidence_sig: "sig".to_string(),
            generated_at: now_iso(),
            edited_body_md: edited.to_string(),
            edited_at: if edited.is_empty() {
                String::new()
            } else {
                "2026-09-09T00:00:00Z".to_string()
            },
        }
    }

    #[test]
    fn guarded_upsert_preserves_manual_edit_unless_overwrite() {
        let fs = TestDbFs::new("entity-page-guard");
        let db = fs.db();
        seed_document(&db, "doc-1");
        let entity = db.upsert_entity(&new_entity("方法A", "method", &[])).expect("entity");
        assert!(db.upsert_entity_page(&page_record(&entity.entity_id, "模型一", ""), false)
            .expect("insert"));
        assert!(db.set_entity_page_edit(&entity.entity_id, "人工修订", "t1").expect("edit"));

        // 守卫:有修订 + 不允许覆盖 → 不写,正文不变
        assert!(!db
            .upsert_entity_page(&page_record(&entity.entity_id, "模型二", ""), false)
            .expect("blocked"));
        let loaded = db.get_entity_page(&entity.entity_id).expect("load").expect("some");
        assert_eq!(loaded.body_md, "模型一");
        assert_eq!(loaded.effective_body(), "人工修订");
        assert!(loaded.edited());

        // 允许覆盖 → 写入模型正文并清修订
        assert!(db
            .upsert_entity_page(&page_record(&entity.entity_id, "模型二", ""), true)
            .expect("overwrite"));
        let loaded = db.get_entity_page(&entity.entity_id).expect("load2").expect("some");
        assert_eq!(loaded.body_md, "模型二");
        assert_eq!(loaded.effective_body(), "模型二");
        assert!(!loaded.edited());
        assert_eq!(loaded.edited_at, "");
    }

    #[test]
    fn set_and_clear_entity_page_edit_roundtrip() {
        let fs = TestDbFs::new("entity-page-edit");
        let db = fs.db();
        seed_document(&db, "doc-1");
        let entity = db.upsert_entity(&new_entity("方法A", "method", &[])).expect("entity");
        // 无页时两列都改不到 → false
        assert!(!db.set_entity_page_edit(&entity.entity_id, "x", "t").expect("no page"));
        assert!(!db.clear_entity_page_edit(&entity.entity_id).expect("no page clear"));

        db.upsert_entity_page(&page_record(&entity.entity_id, "模型原文", ""), true)
            .expect("page");
        assert!(db
            .set_entity_page_edit(&entity.entity_id, "修订正文", "2026-09-09T01:00:00Z")
            .expect("edit"));
        let loaded = db.get_entity_page(&entity.entity_id).expect("load").expect("some");
        assert_eq!(loaded.body_md, "模型原文", "修订不动模型原文");
        assert_eq!(loaded.effective_body(), "修订正文");
        assert_eq!(loaded.edited_at, "2026-09-09T01:00:00Z");

        assert!(db.clear_entity_page_edit(&entity.entity_id).expect("clear"));
        let loaded = db.get_entity_page(&entity.entity_id).expect("load2").expect("some");
        assert_eq!(loaded.effective_body(), "模型原文");
        assert!(!loaded.edited());
        assert_eq!(loaded.edited_at, "");
    }

    #[test]
    fn list_entity_page_bodies_uses_effective_body() {
        let fs = TestDbFs::new("entity-page-bodies");
        let db = fs.db();
        seed_document(&db, "doc-1");
        let entity = db.upsert_entity(&new_entity("方法A", "method", &[])).expect("entity");
        // 模型原文没有 wikilink
        db.upsert_entity_page(&page_record(&entity.entity_id, "无链接正文", ""), true)
            .expect("page");
        assert!(db.list_entity_page_bodies().expect("empty").is_empty());

        // 修订里写了 [[X]] → 生效正文参与反链
        db.set_entity_page_edit(&entity.entity_id, "修订提到 [[目标]]", "t")
            .expect("edit");
        let bodies = db.list_entity_page_bodies().expect("bodies");
        assert_eq!(
            bodies,
            vec![(entity.entity_id.clone(), "修订提到 [[目标]]".to_string())]
        );

        // 撤销 → 回到无链接的模型原文 → 不再出现
        db.clear_entity_page_edit(&entity.entity_id).expect("clear");
        assert!(db.list_entity_page_bodies().expect("empty2").is_empty());
    }

    fn read_extracted_at(db: &Db, document_id: &str) -> Option<String> {
        db.connect()
            .expect("connect")
            .query_row(
                "SELECT graph_extracted_at FROM documents WHERE document_id = ?1",
                params![document_id],
                |row| row.get(0),
            )
            .expect("stamp")
    }

    fn link(document_id: &str, entity_id: &str, block_id: &str) -> BlockEntityLink {
        BlockEntityLink {
            document_id: document_id.to_string(),
            entity_id: entity_id.to_string(),
            page_idx: 0,
            block_id: block_id.to_string(),
            job_id: "job-1".to_string(),
            surface_form: "GNN".to_string(),
            snippet: "gnn".to_string(),
            confidence: 1.0,
            source: "glossary".to_string(),
        }
    }

    #[test]
    fn apply_document_relink_diffs_and_leaves_relations_and_stamp_untouched() {
        let fs = TestDbFs::new("relink");
        let db = fs.db();
        seed_document(&db, "doc-1");
        let entity = db
            .upsert_entity(&new_entity("GNN", "term", &[]))
            .expect("entity");
        let other = db
            .upsert_entity(&new_entity("BERT", "term", &[]))
            .expect("other");
        db.link_block_entity(&link("doc-1", &entity.entity_id, "p001-b0000"))
            .expect("seed link");
        db.add_entity_relation(&NewEntityRelation {
            from_entity_id: entity.entity_id.clone(),
            to_entity_id: other.entity_id.clone(),
            relation_type: "related".to_string(),
            confidence: 0.9,
            explanation: String::new(),
            source_document_id: "doc-1".to_string(),
            source_block_id: "p001-b0000".to_string(),
        })
        .expect("relation");
        db.mark_document_graph_extracted("doc-1").expect("stamp");
        let stamp = read_extracted_at(&db, "doc-1");
        assert!(stamp.is_some());

        db.apply_document_relink(
            "doc-1",
            &[link("doc-1", &other.entity_id, "p001-b0001")],
            &[(entity.entity_id.clone(), "p001-b0000".to_string())],
        )
        .expect("relink");

        let rows = db.list_document_block_entities("doc-1").expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].entity_id, other.entity_id);
        assert_eq!(rows[0].block_id, "p001-b0001");
        // 关系与抽取状态不动
        assert_eq!(
            db.related_entities(&entity.entity_id, None, 10)
                .expect("related")
                .len(),
            1
        );
        assert_eq!(read_extracted_at(&db, "doc-1"), stamp);
    }

    // ---- Phase 11: 实体改名 / 合并 ----

    fn relate(from: &str, to: &str, kind: &str) -> NewEntityRelation {
        NewEntityRelation {
            from_entity_id: from.to_string(),
            to_entity_id: to.to_string(),
            relation_type: kind.to_string(),
            confidence: 0.9,
            explanation: String::new(),
            source_document_id: "doc-1".to_string(),
            source_block_id: "p001-b0000".to_string(),
        }
    }

    /// 绕过 add_entity_relation 的三元组守卫,直接插行(rowid 去重测试要造重复)。
    fn raw_relation(db: &Db, id: &str, from: &str, to: &str, kind: &str, explanation: &str) {
        db.connect()
            .expect("connect")
            .execute(
                "INSERT INTO entity_relations (relation_id, from_entity_id, to_entity_id,
                    relation_type, confidence, explanation, source_document_id, source_block_id,
                    created_at)
                 VALUES (?1, ?2, ?3, ?4, 0.5, ?5, 'doc-1', 'p001-b0000', '2026-09-09T00:00:00Z')",
                params![id, from, to, kind, explanation],
            )
            .expect("raw relation");
    }

    fn relation_count(db: &Db, from: &str, to: &str, kind: &str) -> i64 {
        db.connect()
            .expect("connect")
            .query_row(
                "SELECT COUNT(*) FROM entity_relations
                 WHERE from_entity_id = ?1 AND to_entity_id = ?2 AND relation_type = ?3",
                params![from, to, kind],
                |row| row.get(0),
            )
            .expect("count")
    }

    fn mention_count(db: &Db, entity_id: &str) -> i64 {
        db.connect()
            .expect("connect")
            .query_row(
                "SELECT COUNT(*) FROM block_entities WHERE entity_id = ?1",
                params![entity_id],
                |row| row.get(0),
            )
            .expect("count")
    }

    fn block_rowid(db: &Db, document_id: &str, block_id: &str, entity_id: &str) -> Option<i64> {
        db.connect()
            .expect("connect")
            .query_row(
                "SELECT rowid FROM block_entities
                 WHERE document_id = ?1 AND block_id = ?2 AND entity_id = ?3",
                params![document_id, block_id, entity_id],
                |row| row.get(0),
            )
            .ok()
    }

    #[test]
    fn rename_preserves_identity_and_adds_old_name_to_aliases() {
        let fs = TestDbFs::new("rename");
        let db = fs.db();
        let entity = db
            .upsert_entity(&new_entity("GNN", "term", &["GNN模型"]))
            .expect("entity");

        let renamed = match db
            .rename_entity(&entity.entity_id, "Graph Neural Network")
            .expect("rename")
        {
            RenameOutcome::Renamed(record) => record,
            _ => panic!("expected Renamed"),
        };
        assert_eq!(renamed.entity_id, entity.entity_id);
        assert_eq!(renamed.entity_type, "term");
        assert_eq!(renamed.name, "Graph Neural Network");
        assert_eq!(renamed.name_norm, "graph neural network");
        let mut aliases = renamed.aliases.clone();
        aliases.sort();
        assert_eq!(aliases, vec!["GNN".to_string(), "GNN模型".to_string()]);
    }

    #[test]
    fn rename_case_variant_keeps_id_and_dedupes_aliases() {
        let fs = TestDbFs::new("rename-case");
        let db = fs.db();
        let entity = db
            .upsert_entity(&new_entity("GNN", "term", &["图神经网络"]))
            .expect("entity");

        // 大小写变体:归一化名不变 → 无冲突,旧名归一化后等于新名 → 不进别名
        let renamed = match db.rename_entity(&entity.entity_id, "gnn").expect("rename") {
            RenameOutcome::Renamed(record) => record,
            _ => panic!("expected Renamed"),
        };
        assert_eq!(renamed.entity_id, entity.entity_id);
        assert_eq!(renamed.name, "gnn");
        assert_eq!(renamed.aliases, vec!["图神经网络".to_string()]);

        // 改回原名:不产生重名冲突
        match db.rename_entity(&entity.entity_id, "GNN").expect("rename back") {
            RenameOutcome::Renamed(record) => assert_eq!(record.name, "GNN"),
            _ => panic!("expected Renamed"),
        }
    }

    #[test]
    fn rename_to_existing_alias_moves_it_out_of_aliases() {
        let fs = TestDbFs::new("rename-into-alias");
        let db = fs.db();
        let entity = db
            .upsert_entity(&new_entity("GNN", "term", &["图神经网络"]))
            .expect("entity");
        let renamed = match db
            .rename_entity(&entity.entity_id, "图神经网络")
            .expect("rename")
        {
            RenameOutcome::Renamed(record) => record,
            _ => panic!("expected Renamed"),
        };
        assert_eq!(renamed.name_norm, "图神经网络");
        assert_eq!(renamed.aliases, vec!["GNN".to_string()]);
    }

    #[test]
    fn rename_reports_empty_missing_and_conflict() {
        let fs = TestDbFs::new("rename-errors");
        let db = fs.db();
        let gnn = db.upsert_entity(&new_entity("GNN", "method", &[])).expect("gnn");
        db.upsert_entity(&new_entity("Transformer", "method", &[]))
            .expect("transformer");

        match db.rename_entity(&gnn.entity_id, "   ").expect("empty") {
            RenameOutcome::EmptyName => {}
            _ => panic!("expected EmptyName"),
        }
        match db.rename_entity("ent-missing", "x").expect("missing") {
            RenameOutcome::NotFound => {}
            _ => panic!("expected NotFound"),
        }
        match db
            .rename_entity(&gnn.entity_id, "  transformer ")
            .expect("conflict")
        {
            RenameOutcome::Conflict { existing_name } => assert_eq!(existing_name, "Transformer"),
            _ => panic!("expected Conflict"),
        }

        // 同归一化名、不同类型不算冲突
        let concept = db
            .upsert_entity(&new_entity("Transformer", "concept", &[]))
            .expect("concept");
        match db.rename_entity(&concept.entity_id, "gnn").expect("cross-type") {
            RenameOutcome::Renamed(record) => {
                assert_eq!(record.name, "gnn");
                assert_eq!(record.entity_type, "concept");
            }
            _ => panic!("expected Renamed"),
        }
    }

    #[test]
    fn merge_migrates_evidence_and_keeps_unrelated_signature() {
        let fs = TestDbFs::new("merge-evidence");
        let db = fs.db();
        seed_document(&db, "doc-1");
        let target = db.upsert_entity(&new_entity("GNN", "method", &[])).expect("target");
        let source = db.upsert_entity(&new_entity("图神经网络", "method", &[])).expect("source");
        let other = db.upsert_entity(&new_entity("BERT", "method", &[])).expect("other");

        // 共享 (doc, page, block) + 各自独有 + 无关实体
        db.link_block_entity(&link("doc-1", &target.entity_id, "p001-b0000"))
            .expect("t shared");
        db.link_block_entity(&link("doc-1", &target.entity_id, "p001-b0001"))
            .expect("t own");
        db.link_block_entity(&link("doc-1", &source.entity_id, "p001-b0000"))
            .expect("s shared");
        db.link_block_entity(&link("doc-1", &source.entity_id, "p001-b0002"))
            .expect("s own");
        db.link_block_entity(&link("doc-1", &other.entity_id, "p001-b0003"))
            .expect("other");
        let other_sig = db.entity_page_evidence_sig(&other.entity_id).expect("other sig");
        let target_shared_rowid =
            block_rowid(&db, "doc-1", "p001-b0000", &target.entity_id).expect("rowid");

        match db
            .merge_entities(&target.entity_id, std::slice::from_ref(&source.entity_id))
            .expect("merge")
        {
            MergeOutcome::Merged(summary) => assert_eq!(summary.mentions, 3),
            _ => panic!("expected Merged"),
        }

        assert_eq!(mention_count(&db, &target.entity_id), 3);
        assert_eq!(mention_count(&db, &source.entity_id), 0);
        // 目标自己的共享行 rowid 原样保留(INSERT OR IGNORE 不换 rowid)
        assert_eq!(
            block_rowid(&db, "doc-1", "p001-b0000", &target.entity_id),
            Some(target_shared_rowid)
        );
        // 无关实体证据签名前后不变
        assert_eq!(
            db.entity_page_evidence_sig(&other.entity_id).expect("other sig2"),
            other_sig
        );
    }

    #[test]
    fn merge_rewrites_relations_dedupes_by_rowid_and_spares_unrelated() {
        let fs = TestDbFs::new("merge-relations");
        let db = fs.db();
        seed_document(&db, "doc-1");
        let target = db.upsert_entity(&new_entity("GNN", "method", &[])).expect("target");
        let source = db.upsert_entity(&new_entity("图神经网络", "method", &[])).expect("source");
        let x = db.upsert_entity(&new_entity("X", "method", &[])).expect("x");
        let y = db.upsert_entity(&new_entity("Y", "method", &[])).expect("y");

        // T→S 迁移后会成为 T→T 自环,应被删
        db.add_entity_relation(&relate(&target.entity_id, &source.entity_id, "mentions"))
            .expect("t->s");
        // S→X 迁移成 T→X
        db.add_entity_relation(&relate(&source.entity_id, &x.entity_id, "uses"))
            .expect("s->x");
        // 目标自己的重复三元组,保留最早 rowid
        raw_relation(&db, "rel-dup-1", &target.entity_id, &y.entity_id, "uses", "first");
        raw_relation(&db, "rel-dup-2", &target.entity_id, &y.entity_id, "uses", "second");
        // 无关自环与无关重复,都不能被误伤
        raw_relation(&db, "rel-self", &x.entity_id, &x.entity_id, "loop", "self");
        raw_relation(&db, "rel-xa", &x.entity_id, &y.entity_id, "pair", "a");
        raw_relation(&db, "rel-xb", &x.entity_id, &y.entity_id, "pair", "b");

        db.merge_entities(&target.entity_id, std::slice::from_ref(&source.entity_id))
            .expect("merge");

        // T→S 自环消失
        assert_eq!(relation_count(&db, &target.entity_id, &target.entity_id, "mentions"), 0);
        // S→X 已改指目标
        assert_eq!(relation_count(&db, &target.entity_id, &x.entity_id, "uses"), 1);
        assert_eq!(relation_count(&db, &source.entity_id, &x.entity_id, "uses"), 0);
        // T→Y 去重保留最早 rowid(explanation "first")
        assert_eq!(relation_count(&db, &target.entity_id, &y.entity_id, "uses"), 1);
        let kept: String = db
            .connect()
            .expect("connect")
            .query_row(
                "SELECT explanation FROM entity_relations
                 WHERE from_entity_id = ?1 AND to_entity_id = ?2 AND relation_type = 'uses'",
                params![target.entity_id, y.entity_id],
                |row| row.get(0),
            )
            .expect("kept");
        assert_eq!(kept, "first");
        // 无关自环与无关重复原样
        assert_eq!(relation_count(&db, &x.entity_id, &x.entity_id, "loop"), 1);
        assert_eq!(relation_count(&db, &x.entity_id, &y.entity_id, "pair"), 2);
    }

    #[test]
    fn merge_adopts_edited_source_page_when_target_has_none() {
        let fs = TestDbFs::new("merge-page-adopt");
        let db = fs.db();
        let target = db.upsert_entity(&new_entity("GNN", "method", &[])).expect("target");
        let older = db.upsert_entity(&new_entity("甲", "method", &[])).expect("older");
        let edited = db.upsert_entity(&new_entity("乙", "method", &[])).expect("edited");
        db.upsert_entity_page(&page_record(&older.entity_id, "模型一", ""), true)
            .expect("older page");
        db.upsert_entity_page(&page_record(&edited.entity_id, "模型二", "人工修订"), true)
            .expect("edited page");

        match db
            .merge_entities(&target.entity_id, &[older.entity_id.clone(), edited.entity_id.clone()])
            .expect("merge")
        {
            MergeOutcome::Merged(summary) => {
                assert!(summary.page_adopted);
                assert_eq!(summary.target.entity_id, target.entity_id);
            }
            _ => panic!("expected Merged"),
        }

        // 只有一个页存活下来,采纳的是带人工修订的那个
        let page = db
            .get_entity_page(&target.entity_id)
            .expect("page")
            .expect("some");
        assert_eq!(page.effective_body(), "人工修订");
        assert_eq!(page.body_md, "模型二");
        assert!(db.get_entity_page(&older.entity_id).expect("older gone").is_none());
        assert!(db.get_entity_page(&edited.entity_id).expect("edited gone").is_none());
    }

    #[test]
    fn merge_keeps_target_page_and_drops_source_pages() {
        let fs = TestDbFs::new("merge-page-keep");
        let db = fs.db();
        let target = db.upsert_entity(&new_entity("GNN", "method", &[])).expect("target");
        let source = db.upsert_entity(&new_entity("图神经网络", "method", &[])).expect("source");
        db.upsert_entity_page(&page_record(&target.entity_id, "目标页", ""), true)
            .expect("target page");
        db.upsert_entity_page(&page_record(&source.entity_id, "源页", ""), true)
            .expect("source page");

        match db
            .merge_entities(&target.entity_id, std::slice::from_ref(&source.entity_id))
            .expect("merge")
        {
            MergeOutcome::Merged(summary) => assert!(!summary.page_adopted),
            _ => panic!("expected Merged"),
        }
        let page = db
            .get_entity_page(&target.entity_id)
            .expect("page")
            .expect("some");
        assert_eq!(page.body_md, "目标页");
        assert!(db.get_entity_page(&source.entity_id).expect("gone").is_none());
    }

    #[test]
    fn merge_unions_aliases_descriptions_and_stales_target_page() {
        let fs = TestDbFs::new("merge-aliases");
        let db = fs.db();
        seed_document(&db, "doc-1");
        let target = db
            .upsert_entity(&new_entity("GNN", "method", &["图神经网络"]))
            .expect("target");
        let mut s1 = new_entity("Graph Neural Network", "method", &["GNN-model"]);
        s1.description = "first non-empty".to_string();
        let s1 = db.upsert_entity(&s1).expect("s1");
        let s2 = db
            .upsert_entity(&new_entity("消息传递网络", "method", &["MPNN"]))
            .expect("s2");
        // 源带证据:合并后目标签名必变
        db.link_block_entity(&link("doc-1", &s1.entity_id, "p001-b0000"))
            .expect("s1 link");
        db.add_entity_relation(&relate(&s1.entity_id, &s2.entity_id, "related"))
            .expect("s1->s2");

        // 目标页:证据签名 = 合并前
        let sig_before = db.entity_page_evidence_sig(&target.entity_id).expect("sig");
        let mut page = page_record(&target.entity_id, "目标页", "");
        page.evidence_sig = sig_before.clone();
        db.upsert_entity_page(&page, true).expect("page");

        match db
            .merge_entities(&target.entity_id, &[s1.entity_id.clone(), s2.entity_id.clone()])
            .expect("merge")
        {
            MergeOutcome::Merged(summary) => {
                assert_eq!(summary.merged.len(), 2);
                let mut aliases = summary.target.aliases.clone();
                aliases.sort();
                assert_eq!(
                    aliases,
                    vec![
                        "GNN-model".to_string(),
                        "Graph Neural Network".to_string(),
                        "MPNN".to_string(),
                        "图神经网络".to_string(),
                        "消息传递网络".to_string(),
                    ]
                );
                assert_eq!(summary.target.description, "first non-empty");
                // 目标自身标识不动
                assert_eq!(summary.target.name, "GNN");
                assert_eq!(summary.target.name_norm, "gnn");
                assert_eq!(summary.target.entity_type, "method");
            }
            _ => panic!("expected Merged"),
        }

        assert!(db.get_entity(&s1.entity_id).is_err());
        assert!(db.get_entity(&s2.entity_id).is_err());
        // 合并后证据签名变化 → 目标概念页 stale
        assert_ne!(
            db.entity_page_evidence_sig(&target.entity_id).expect("sig2"),
            sig_before
        );
    }

    #[test]
    fn merge_dedupes_sources_and_reports_validation_outcomes() {
        let fs = TestDbFs::new("merge-validation");
        let db = fs.db();
        let target = db.upsert_entity(&new_entity("GNN", "method", &[])).expect("target");
        let source = db.upsert_entity(&new_entity("图神经网络", "method", &[])).expect("source");

        match db.merge_entities(&target.entity_id, &[]).expect("empty") {
            MergeOutcome::EmptySources => {}
            _ => panic!("expected EmptySources"),
        }
        match db
            .merge_entities(&target.entity_id, &[String::new(), "   ".to_string()])
            .expect("blank")
        {
            MergeOutcome::EmptySources => {}
            _ => panic!("expected EmptySources for blanks"),
        }
        match db
            .merge_entities(&target.entity_id, std::slice::from_ref(&target.entity_id))
            .expect("self")
        {
            MergeOutcome::SourceIsTarget => {}
            _ => panic!("expected SourceIsTarget"),
        }
        match db
            .merge_entities("ent-missing", std::slice::from_ref(&source.entity_id))
            .expect("target")
        {
            MergeOutcome::TargetNotFound => {}
            _ => panic!("expected TargetNotFound"),
        }
        match db
            .merge_entities(&target.entity_id, &["ent-missing".to_string()])
            .expect("source")
        {
            MergeOutcome::SourceNotFound(id) => assert_eq!(id, "ent-missing"),
            _ => panic!("expected SourceNotFound"),
        }
        let many: Vec<String> = (0..MAX_MERGE_SOURCES + 1)
            .map(|index| format!("ent-{index}"))
            .collect();
        match db.merge_entities(&target.entity_id, &many).expect("too many") {
            MergeOutcome::TooManySources { max } => assert_eq!(max, MAX_MERGE_SOURCES),
            _ => panic!("expected TooManySources"),
        }
        // 重复源按序去重 → 单个源,合并成功
        match db
            .merge_entities(&target.entity_id, &[source.entity_id.clone(), source.entity_id.clone()])
            .expect("dup")
        {
            MergeOutcome::Merged(summary) => assert_eq!(summary.merged, vec![source.entity_id]),
            _ => panic!("expected Merged"),
        }
    }
}

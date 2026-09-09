use anyhow::Result;
use rusqlite::Connection;

/// 图书馆数据层的编号迁移阶梯(PRAGMA user_version)。
///
/// 现有 ensure_schema 的幂等 DDL 与 ensure_*_column 增量加列继续负责
/// 任务系统的表;平台新表(documents/favorites/...)从这里走版本化
/// 迁移,后续破坏性变更只能追加新版本,不允许改历史条目。
const VERSIONED_MIGRATIONS: &[&str] = &[
    // v1: 图书馆地基 —— 文档一等公民 + 锚点收藏 + 合集/标签 + FTS5
    r#"
    CREATE TABLE IF NOT EXISTS documents (
        document_id     TEXT PRIMARY KEY,
        title           TEXT NOT NULL DEFAULT '',
        authors_json    TEXT NOT NULL DEFAULT '[]',
        year            INTEGER,
        doi             TEXT NOT NULL DEFAULT '',
        source_filename TEXT NOT NULL,
        page_count      INTEGER NOT NULL DEFAULT 0,
        bytes           INTEGER NOT NULL DEFAULT 0,
        active_job_id   TEXT,
        reading_status  TEXT NOT NULL DEFAULT 'unread',
        added_at        TEXT NOT NULL,
        last_opened_at  TEXT,
        updated_at      TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_documents_added_at ON documents(added_at DESC);
    CREATE TABLE IF NOT EXISTS favorites (
        favorite_id     TEXT PRIMARY KEY,
        document_id     TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
        job_id          TEXT NOT NULL,
        page_idx        INTEGER NOT NULL,
        block_id        TEXT NOT NULL,
        char_start      INTEGER,
        char_end        INTEGER,
        kind            TEXT NOT NULL DEFAULT 'sentence',
        quote_text      TEXT NOT NULL,
        translated_quote_text TEXT NOT NULL DEFAULT '',
        note            TEXT NOT NULL DEFAULT '',
        created_at      TEXT NOT NULL,
        updated_at      TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_favorites_document ON favorites(document_id, page_idx);
    CREATE TABLE IF NOT EXISTS collections (
        collection_id   TEXT PRIMARY KEY,
        name            TEXT NOT NULL,
        parent_id       TEXT REFERENCES collections(collection_id) ON DELETE SET NULL,
        sort_order      INTEGER NOT NULL DEFAULT 0,
        created_at      TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS collection_documents (
        collection_id   TEXT NOT NULL REFERENCES collections(collection_id) ON DELETE CASCADE,
        document_id     TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
        added_at        TEXT NOT NULL,
        PRIMARY KEY(collection_id, document_id)
    );
    CREATE TABLE IF NOT EXISTS document_tags (
        document_id     TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
        tag             TEXT NOT NULL,
        PRIMARY KEY(document_id, tag)
    );
    CREATE INDEX IF NOT EXISTS idx_document_tags_tag ON document_tags(tag);
    CREATE VIRTUAL TABLE IF NOT EXISTS blocks_fts USING fts5(
        document_id UNINDEXED, job_id UNINDEXED, page_idx UNINDEXED, block_id UNINDEXED,
        source_text, translated_text,
        tokenize='trigram'
    );
    "#,
    // v2: 资产存储(内容寻址,收藏图片附件)+ AI 问答会话/消息。
    // 设计原则:用户策展(收藏)是硬锚点,机器生成(问答引用)是软锚点
    // ——引用只存 citations_json 快照,不做 job 删除保护。
    r#"
    CREATE TABLE IF NOT EXISTS assets (
        asset_id    TEXT PRIMARY KEY,          -- sha256(文件字节)
        mime        TEXT NOT NULL,
        bytes       INTEGER NOT NULL,
        width       INTEGER,
        height      INTEGER,
        created_at  TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS ai_conversations (
        conversation_id TEXT PRIMARY KEY,
        title           TEXT NOT NULL DEFAULT '',
        document_id     TEXT REFERENCES documents(document_id) ON DELETE SET NULL,
        created_at      TEXT NOT NULL,
        updated_at      TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_ai_conversations_updated ON ai_conversations(updated_at DESC);
    CREATE TABLE IF NOT EXISTS ai_messages (
        message_id      TEXT PRIMARY KEY,
        conversation_id TEXT NOT NULL REFERENCES ai_conversations(conversation_id) ON DELETE CASCADE,
        seq             INTEGER NOT NULL,
        role            TEXT NOT NULL,
        content         TEXT NOT NULL,
        citations_json  TEXT NOT NULL DEFAULT '[]',
        tool_trace_json TEXT NOT NULL DEFAULT '[]',
        model           TEXT NOT NULL DEFAULT '',
        created_at      TEXT NOT NULL,
        UNIQUE(conversation_id, seq)
    );
    ALTER TABLE favorites ADD COLUMN asset_id  TEXT NOT NULL DEFAULT '';
    ALTER TABLE favorites ADD COLUMN rect_json TEXT NOT NULL DEFAULT '';
    "#,
    // v3: AI 消息树分支 —— parent_id 形成兄弟分支; head_id 记录当前可见叶。
    // 与 ChatGPT / assistant-ui 一致:同 parent 的多条 message 即 alternate。
    // 兼容:旧行 parent_id 为空,按 seq 串成线性链;load 时无 head 或 max(seq)。
    r#"
    ALTER TABLE ai_conversations ADD COLUMN head_id TEXT NOT NULL DEFAULT '';
    ALTER TABLE ai_messages ADD COLUMN parent_id TEXT NOT NULL DEFAULT '';
    CREATE INDEX IF NOT EXISTS idx_ai_messages_parent
        ON ai_messages(conversation_id, parent_id);
    "#,
    // v4: 概念图谱实体层。证据单位是 block(自带几何 + 原文/译文),比整篇文档更细。
    // entities = 可复用实体;block_entities = 实体→block 证据挂载(多对多);
    // entity_relations = 有向类型边(1b 才写入)。graph_extracted_at 记录抽取状态。
    r#"
    CREATE TABLE IF NOT EXISTS entities (
        entity_id    TEXT PRIMARY KEY,
        name         TEXT NOT NULL,
        name_norm    TEXT NOT NULL,
        entity_type  TEXT NOT NULL DEFAULT 'concept',
        aliases_json TEXT NOT NULL DEFAULT '[]',
        description  TEXT NOT NULL DEFAULT '',
        created_at   TEXT NOT NULL,
        updated_at   TEXT NOT NULL
    );
    CREATE UNIQUE INDEX IF NOT EXISTS idx_entities_norm_type
        ON entities(name_norm, entity_type);
    CREATE TABLE IF NOT EXISTS block_entities (
        document_id  TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
        entity_id    TEXT NOT NULL REFERENCES entities(entity_id) ON DELETE CASCADE,
        page_idx     INTEGER NOT NULL,
        block_id     TEXT NOT NULL,
        job_id       TEXT NOT NULL DEFAULT '',
        surface_form TEXT NOT NULL DEFAULT '',
        snippet      TEXT NOT NULL DEFAULT '',
        confidence   REAL NOT NULL DEFAULT 1.0,
        source       TEXT NOT NULL DEFAULT 'extraction',
        created_at   TEXT NOT NULL,
        PRIMARY KEY (document_id, page_idx, block_id, entity_id)
    );
    CREATE INDEX IF NOT EXISTS idx_block_entities_entity ON block_entities(entity_id);
    CREATE INDEX IF NOT EXISTS idx_block_entities_document ON block_entities(document_id, page_idx);
    CREATE TABLE IF NOT EXISTS entity_relations (
        relation_id        TEXT PRIMARY KEY,
        from_entity_id     TEXT NOT NULL REFERENCES entities(entity_id) ON DELETE CASCADE,
        to_entity_id       TEXT NOT NULL REFERENCES entities(entity_id) ON DELETE CASCADE,
        relation_type      TEXT NOT NULL DEFAULT 'related_to',
        confidence         REAL NOT NULL DEFAULT 1.0,
        explanation        TEXT NOT NULL DEFAULT '',
        source_document_id TEXT NOT NULL DEFAULT '',
        source_block_id    TEXT NOT NULL DEFAULT '',
        created_at         TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_entity_relations_from ON entity_relations(from_entity_id);
    CREATE INDEX IF NOT EXISTS idx_entity_relations_to   ON entity_relations(to_entity_id);
    ALTER TABLE documents ADD COLUMN graph_extracted_at TEXT;
    "#,
    // v5: 概念页 —— 每个实体一份维护式综述(带引用的 Markdown)。
    // 证据不另建表:block_entities 本身就是 page_evidence。evidence_sig 记录
    // 生成时的证据签名,读取时现算对比得出 stale,不侵入抽取路径、不落标记列。
    r#"
    CREATE TABLE IF NOT EXISTS entity_pages (
        entity_id      TEXT PRIMARY KEY REFERENCES entities(entity_id) ON DELETE CASCADE,
        body_md        TEXT NOT NULL DEFAULT '',
        citations_json TEXT NOT NULL DEFAULT '[]',
        evidence_sig   TEXT NOT NULL DEFAULT '',
        generated_at   TEXT NOT NULL
    );
    "#,
];

pub(super) fn run_versioned_migrations(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (index, migration) in VERSIONED_MIGRATIONS.iter().enumerate() {
        let version = (index + 1) as i64;
        if version <= current {
            continue;
        }
        conn.execute_batch(&format!(
            "BEGIN;\n{migration}\nPRAGMA user_version = {version};\nCOMMIT;"
        ))?;
    }
    Ok(())
}

pub(super) fn ensure_uploads_column(
    conn: &Connection,
    column: &str,
    column_def: &str,
) -> Result<()> {
    ensure_table_column(conn, "uploads", column, column_def)
}

pub(super) fn ensure_jobs_column(conn: &Connection, column: &str, column_def: &str) -> Result<()> {
    ensure_table_column(conn, "jobs", column, column_def)
}

pub(super) fn ensure_events_column(
    conn: &Connection,
    column: &str,
    column_def: &str,
) -> Result<()> {
    ensure_table_column(conn, "events", column, column_def)
}

pub(super) fn ensure_glossaries_column(
    conn: &Connection,
    column: &str,
    column_def: &str,
) -> Result<()> {
    ensure_table_column(conn, "glossaries", column, column_def)
}

fn ensure_table_column(
    conn: &Connection,
    table: &str,
    column: &str,
    column_def: &str,
) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut has_column = false;
    for row in rows {
        if row? == column {
            has_column = true;
            break;
        }
    }
    if !has_column {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {column_def}"),
            [],
        )?;
    }
    Ok(())
}

pub(super) fn ensure_no_legacy_artifacts_json(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(jobs)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut has_legacy_column = false;
    for row in rows {
        if row? == "artifacts_json" {
            has_legacy_column = true;
            break;
        }
    }
    if !has_legacy_column {
        return Ok(());
    }
    let legacy_count: i64 = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM jobs
        WHERE artifacts_json IS NOT NULL AND TRIM(artifacts_json) <> ''
        "#,
        [],
        |row| row.get(0),
    )?;
    if legacy_count > 0 {
        anyhow::bail!(
            "legacy jobs.artifacts_json storage is no longer supported; found {legacy_count} legacy rows, clear the DB or rerun those jobs"
        );
    }
    Ok(())
}

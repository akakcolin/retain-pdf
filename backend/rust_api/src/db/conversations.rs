use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};

use crate::models::api::{ConversationRecord, MessageRecord};
use crate::models::domain::now_iso;

use super::Db;

const CONVERSATION_COLUMNS: &str =
    "c.conversation_id, c.title, c.document_id, c.created_at, c.updated_at,
     (SELECT COUNT(*) FROM ai_messages m WHERE m.conversation_id = c.conversation_id),
     COALESCE(c.head_id, '')";

const MESSAGE_COLUMNS: &str = "message_id, conversation_id, seq, role, content,
                   citations_json, tool_trace_json, model, created_at,
                   COALESCE(parent_id, '')";

impl Db {
    pub fn create_conversation(
        &self,
        conversation_id: &str,
        title: &str,
        document_id: Option<&str>,
    ) -> Result<ConversationRecord> {
        let conn = self.connect()?;
        let now = now_iso();
        conn.execute(
            r#"
            INSERT INTO ai_conversations (conversation_id, title, document_id, created_at, updated_at, head_id)
            VALUES (?1, ?2, ?3, ?4, ?4, '')
            "#,
            params![conversation_id, title, document_id, now],
        )?;
        self.get_conversation(conversation_id)?
            .context("conversation vanished after insert")
    }

    pub fn get_conversation(&self, conversation_id: &str) -> Result<Option<ConversationRecord>> {
        let conn = self.connect()?;
        let record = conn
            .query_row(
                &format!(
                    "SELECT {CONVERSATION_COLUMNS} FROM ai_conversations c WHERE c.conversation_id = ?1"
                ),
                params![conversation_id],
                row_to_conversation,
            )
            .optional()?;
        Ok(record)
    }

    pub fn list_conversations(&self, limit: u32, offset: u32) -> Result<Vec<ConversationRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {CONVERSATION_COLUMNS} FROM ai_conversations c ORDER BY c.updated_at DESC LIMIT ?1 OFFSET ?2"
        ))?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], row_to_conversation)?;
        let mut conversations = Vec::new();
        for row in rows {
            conversations.push(row?);
        }
        Ok(conversations)
    }

    pub fn list_conversations_for_document(
        &self,
        document_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ConversationRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {CONVERSATION_COLUMNS} FROM ai_conversations c
             WHERE c.document_id = ?1
             ORDER BY c.updated_at DESC LIMIT ?2 OFFSET ?3"
        ))?;
        let rows = stmt.query_map(
            params![document_id, limit as i64, offset as i64],
            row_to_conversation,
        )?;
        let mut conversations = Vec::new();
        for row in rows {
            conversations.push(row?);
        }
        Ok(conversations)
    }

    pub fn count_conversations(&self, document_id: Option<&str>) -> Result<u64> {
        let conn = self.connect()?;
        let count = match document_id {
            Some(document_id) => conn.query_row(
                "SELECT COUNT(*) FROM ai_conversations WHERE document_id = ?1",
                params![document_id],
                |row| row.get::<_, i64>(0),
            )?,
            None => conn.query_row("SELECT COUNT(*) FROM ai_conversations", [], |row| {
                row.get::<_, i64>(0)
            })?,
        };
        Ok(count.max(0) as u64)
    }

    pub fn delete_conversation(&self, conversation_id: &str) -> Result<bool> {
        let conn = self.connect()?;
        let changed = conn.execute(
            "DELETE FROM ai_conversations WHERE conversation_id = ?1",
            params![conversation_id],
        )?;
        Ok(changed > 0)
    }

    pub fn set_conversation_head(
        &self,
        conversation_id: &str,
        head_id: &str,
    ) -> Result<Option<ConversationRecord>> {
        let conn = self.connect()?;
        let now = now_iso();
        let changed = conn.execute(
            "UPDATE ai_conversations SET head_id = ?1, updated_at = ?2 WHERE conversation_id = ?3",
            params![head_id, now, conversation_id],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        self.get_conversation(conversation_id)
    }

    pub fn patch_conversation_title(
        &self,
        conversation_id: &str,
        title: &str,
    ) -> Result<Option<ConversationRecord>> {
        let conn = self.connect()?;
        let now = now_iso();
        let changed = conn.execute(
            "UPDATE ai_conversations SET title = ?1, updated_at = ?2 WHERE conversation_id = ?3",
            params![title, now, conversation_id],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        self.get_conversation(conversation_id)
    }

    /// 返回会话内全部消息(按 seq 升序),供前端重建分支树。
    pub fn list_messages(&self, conversation_id: &str, limit: u32) -> Result<Vec<MessageRecord>> {
        let conn = self.connect()?;
        // 分支树需要全量(或大窗口);仍按 seq 正序,便于 fromBranchableArray 父先于子。
        let mut stmt = conn.prepare(&format!(
            r#"
            SELECT {MESSAGE_COLUMNS}
            FROM ai_messages
            WHERE conversation_id = ?1
            ORDER BY seq ASC
            LIMIT ?2
            "#
        ))?;
        let rows = stmt.query_map(params![conversation_id, limit as i64], row_to_message)?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }
        Ok(messages)
    }

    pub fn get_message(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<Option<MessageRecord>> {
        let conn = self.connect()?;
        let record = conn
            .query_row(
                &format!(
                    "SELECT {MESSAGE_COLUMNS} FROM ai_messages
                     WHERE conversation_id = ?1 AND message_id = ?2"
                ),
                params![conversation_id, message_id],
                row_to_message,
            )
            .optional()?;
        Ok(record)
    }

    /// 追加消息:seq 自增、刷新会话时间与 head;会话标题为空时取首条 user 消息前缀。
    #[allow(clippy::too_many_arguments)] // 参数即消息字段集合，收拢结构体收益低
    pub fn append_message(
        &self,
        conversation_id: &str,
        message_id: &str,
        role: &str,
        content: &str,
        citations_json: &str,
        tool_trace_json: &str,
        model: &str,
        parent_id: &str,
        set_head: bool,
    ) -> Result<MessageRecord> {
        let mut conn = self.connect()?;
        let now = now_iso();
        let tx = conn.transaction()?;
        let next_seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM ai_messages WHERE conversation_id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )?;
        let parent = parent_id.trim();
        tx.execute(
            r#"
            INSERT INTO ai_messages (
                message_id, conversation_id, seq, role, content,
                citations_json, tool_trace_json, model, created_at, parent_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                message_id,
                conversation_id,
                next_seq,
                role,
                content,
                citations_json,
                tool_trace_json,
                model,
                now,
                parent,
            ],
        )?;
        if set_head {
            tx.execute(
                "UPDATE ai_conversations SET updated_at = ?1, head_id = ?2 WHERE conversation_id = ?3",
                params![now, message_id, conversation_id],
            )?;
        } else {
            tx.execute(
                "UPDATE ai_conversations SET updated_at = ?1 WHERE conversation_id = ?2",
                params![now, conversation_id],
            )?;
        }
        if role == "user" {
            let title: String = content.chars().take(40).collect();
            tx.execute(
                "UPDATE ai_conversations SET title = ?1 WHERE conversation_id = ?2 AND title = ''",
                params![title, conversation_id],
            )?;
        }
        tx.commit()?;
        Ok(MessageRecord {
            message_id: message_id.to_string(),
            conversation_id: conversation_id.to_string(),
            seq: next_seq,
            role: role.to_string(),
            content: content.to_string(),
            citations_json: citations_json.to_string(),
            tool_trace_json: tool_trace_json.to_string(),
            model: model.to_string(),
            created_at: now,
            parent_id: parent.to_string(),
        })
    }
}

fn row_to_conversation(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationRecord> {
    Ok(ConversationRecord {
        conversation_id: row.get(0)?,
        title: row.get(1)?,
        document_id: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        message_count: row.get(5)?,
        head_id: row.get(6).unwrap_or_default(),
    })
}

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        message_id: row.get(0)?,
        conversation_id: row.get(1)?,
        seq: row.get(2)?,
        role: row.get(3)?,
        content: row.get(4)?,
        citations_json: row.get(5)?,
        tool_trace_json: row.get(6)?,
        model: row.get(7)?,
        created_at: row.get(8)?,
        parent_id: row.get(9).unwrap_or_default(),
    })
}

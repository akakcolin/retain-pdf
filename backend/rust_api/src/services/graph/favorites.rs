//! 标注反查:哪些收藏(引文/译文/备注)提到了本实体。
//! 读取时现算,不写 block_entities(source=manual 仍留空),也不喂进概念页合成正文
//! ——避免每改一条标注就 stale、以及重复烧 token 重生成。

use std::collections::HashMap;

use crate::db::graph::normalize_entity_name;
use crate::db::Db;
use crate::error::AppError;
use crate::models::api::EntityFavorite;

/// 单个 needle 归一化后的长度下限。与 mentions.rs 的 MIN_NEEDLE_CHARS 一致:
/// 太短的字面(尤其 ASCII)在任意文本里误命中率高。
const MIN_NEEDLE_CHARS: usize = 2;

/// 反查该实体的标注(created_at DESC,与 list_favorites 顺序一致)。
/// ponytail: 全表扫 favorites + Rust 侧匹配,收藏量小够用;要快再建 favorite_entities 表。
pub fn list_entity_favorites(
    db: &Db,
    entity_id: &str,
    limit: u32,
) -> Result<Vec<EntityFavorite>, AppError> {
    let entity = db
        .get_entity(entity_id)
        .map_err(|_| AppError::not_found(format!("entity not found: {entity_id}")))?;
    let mut needles = vec![normalize_entity_name(&entity.name)];
    for alias in &entity.aliases {
        let norm = normalize_entity_name(alias);
        if !norm.is_empty() && !needles.iter().any(|item| item == &norm) {
            needles.push(norm);
        }
    }
    needles.retain(|needle| needle.chars().count() >= MIN_NEEDLE_CHARS);
    if needles.is_empty() {
        return Ok(Vec::new());
    }

    let mut titles: HashMap<String, String> = HashMap::new();
    let mut items: Vec<EntityFavorite> = Vec::new();
    for favorite in db.list_favorites(None)? {
        let hit = [
            &favorite.quote_text,
            &favorite.translated_quote_text,
            &favorite.note,
        ]
        .iter()
        .any(|text| needles.iter().any(|needle| needle_hit(text, needle)));
        if !hit {
            continue;
        }
        let document_title = titles
            .entry(favorite.document_id.clone())
            .or_insert_with(|| {
                db.get_document(&favorite.document_id)
                    .map(|document| document.title)
                    .unwrap_or_default()
            })
            .clone();
        items.push(EntityFavorite {
            favorite_id: favorite.favorite_id,
            document_id: favorite.document_id,
            document_title,
            job_id: favorite.job_id,
            page_idx: favorite.page_idx,
            block_id: favorite.block_id,
            quote_text: favorite.quote_text,
            translated_quote_text: favorite.translated_quote_text,
            note: favorite.note,
        });
        if items.len() >= limit as usize {
            break;
        }
    }
    Ok(items)
}

/// 归一化后做边界感知匹配。纯 ASCII needle 要求命中处两侧不是 ASCII 字母数字
/// (否则 "ai" 会命中 "said"/"email");CJK 邻居不是 ASCII 字母数字,所以
/// "GNN模型"/"用AI中台" 正常命中。含 CJK 的 needle 直接 contains(中文无词边界)。
fn needle_hit(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let hay = normalize_entity_name(hay);
    if hay.is_empty() {
        return false;
    }
    if !needle.is_ascii() {
        return hay.contains(needle);
    }
    hay.match_indices(needle).any(|(start, _)| {
        let end = start + needle.len();
        let before_ok = hay[..start]
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric());
        // 右侧:CJK/空格/标点都算边界;英文复数 -s(GNNs/CNNs)也算,
        // 但 "aids" 这类词内命中仍被拦下。
        let rest = &hay[end..];
        let after_ok = match rest.chars().next() {
            None => true,
            Some('s') => rest[1..]
                .chars()
                .next()
                .is_none_or(|ch| !ch.is_ascii_alphanumeric()),
            Some(ch) => !ch.is_ascii_alphanumeric(),
        };
        before_ok && after_ok
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::models::api::{FavoriteRecord, NewEntity};
    use crate::models::{now_iso, UploadRecord};

    use super::*;

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
            "graph-favorites-{name}-{}-{}",
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

    fn seed_document(db: &Db) {
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
    }

    fn favorite(id: &str, quote: &str, note: &str) -> FavoriteRecord {
        FavoriteRecord {
            favorite_id: id.to_string(),
            document_id: "doc-1".to_string(),
            job_id: "job-1".to_string(),
            page_idx: 2,
            block_id: format!("p003-b{id}"),
            char_start: None,
            char_end: None,
            kind: "sentence".to_string(),
            quote_text: quote.to_string(),
            translated_quote_text: String::new(),
            note: note.to_string(),
            asset_id: String::new(),
            rect_json: String::new(),
            created_at: now_iso(),
            updated_at: now_iso(),
        }
    }

    fn entity(db: &Db, name: &str, aliases: &[&str]) -> String {
        db.upsert_entity(&NewEntity {
            name: name.to_string(),
            entity_type: "term".to_string(),
            aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
            description: String::new(),
        })
        .expect("entity")
        .entity_id
    }

    #[test]
    fn matches_quote_translation_note_and_alias() {
        let fixture = fixture("match");
        seed_document(&fixture.db);
        let entity_id = entity(&fixture.db, "GNN", &["图神经网络"]);
        fixture
            .db
            .save_favorite(&favorite("fav-1", "GNN 的表示学习", ""))
            .expect("fav-1");
        fixture
            .db
            .save_favorite(&favorite("fav-2", "无关句子", "和 图神经网络 有关"))
            .expect("fav-2");
        let mut translated = favorite("fav-3", "plain quote", "");
        translated.translated_quote_text = "关于 GNN 的译文".to_string();
        fixture.db.save_favorite(&translated).expect("fav-3");
        fixture
            .db
            .save_favorite(&favorite("fav-4", "完全无关", "也没关系"))
            .expect("fav-4");
        // 中文正文里实体名紧贴汉字(最常见形态)
        fixture
            .db
            .save_favorite(&favorite("fav-5", "GNN模型很有效", ""))
            .expect("fav-5");

        let items = list_entity_favorites(&fixture.db, &entity_id, 10).expect("favorites");
        let ids: Vec<&str> = items.iter().map(|item| item.favorite_id.as_str()).collect();
        assert_eq!(ids.len(), 4);
        assert!(ids.contains(&"fav-1"));
        assert!(ids.contains(&"fav-2"));
        assert!(ids.contains(&"fav-3"));
        assert!(ids.contains(&"fav-5"));
        // 文档标题解析进结果
        assert_eq!(items[0].document_title, "化学");
        assert_eq!(items[0].page_idx, 2);
    }

    #[test]
    fn ascii_needle_respects_word_boundaries() {
        assert!(needle_hit("AI 很强", "ai"));
        assert!(needle_hit("使用 ai 模型", "ai"));
        // CJK 紧邻 ASCII 实体名(中文正文里的常见形态)必须命中
        assert!(needle_hit("GNN模型很有效", "gnn"));
        assert!(needle_hit("用AI中台", "ai"));
        assert!(needle_hit("GPT技术", "gpt"));
        // 英文复数
        assert!(needle_hit("GNNs are great", "gnn"));
        assert!(!needle_hit("gnnsx", "gnn"));
        // 词内命中仍拦下
        assert!(!needle_hit("the said email", "ai"));
        assert!(!needle_hit("domain", "ai"));
        assert!(!needle_hit("aids the process", "ai"));
        // CJK 无词边界,直接包含
        assert!(needle_hit("关于注意力机制", "注意力"));
        assert!(!needle_hit("无关文本", "注意力"));
    }

    #[test]
    fn empty_and_unknown_and_limit() {
        let fixture = fixture("edges");
        seed_document(&fixture.db);
        let entity_id = entity(&fixture.db, "GNN", &[]);
        assert!(list_entity_favorites(&fixture.db, &entity_id, 10)
            .expect("empty")
            .is_empty());
        assert!(list_entity_favorites(&fixture.db, "ent-nope", 10).is_err());

        for index in 0..3 {
            fixture
                .db
                .save_favorite(&favorite(&format!("fav-{index}"), "GNN 相关", ""))
                .expect("favorite");
        }
        assert_eq!(
            list_entity_favorites(&fixture.db, &entity_id, 2)
                .expect("limited")
                .len(),
            2
        );
    }

    #[test]
    fn short_needle_is_ignored() {
        let fixture = fixture("short");
        seed_document(&fixture.db);
        let entity_id = entity(&fixture.db, "X", &[]);
        fixture
            .db
            .save_favorite(&favorite("fav-1", "x 出现在这里", ""))
            .expect("favorite");
        assert!(list_entity_favorites(&fixture.db, &entity_id, 10)
            .expect("short")
            .is_empty());
    }
}

//! 术语表 → 实体种子。零 LLM 成本:术语表本身就是跨文档、带类型的人工词表。

use std::collections::HashSet;

use crate::db::graph::normalize_entity_name;
use crate::error::AppError;
use crate::models::api::NewEntity;

use super::GraphDeps;

#[derive(Debug, Default)]
pub struct SeedOutcome {
    /// 扫描的术语条目数
    pub entries: usize,
    /// 落到实体表的条目数(同名词条去重后)
    pub entities: usize,
}

/// 把每张术语表的每条 `{source, target}` 灌成一个 `term` 实体。
/// name 取译文(target)优先,别名带上原文;同名词条(跨表也算)由
/// `upsert_entity` 并入别名,所以同一个实体能攒下多个写法。重复调用幂等。
pub fn seed_entities_from_glossaries(deps: &GraphDeps<'_>) -> Result<SeedOutcome, AppError> {
    let glossaries = deps.db.list_glossaries()?;
    let mut outcome = SeedOutcome::default();
    let mut distinct: HashSet<String> = HashSet::new();
    for glossary in glossaries {
        for entry in glossary.entries {
            let source = entry.source.trim();
            let target = entry.target.trim();
            if source.is_empty() && target.is_empty() {
                continue;
            }
            let name = if target.is_empty() { source } else { target };
            let name_norm = normalize_entity_name(name);
            if name_norm.is_empty() {
                continue;
            }
            outcome.entries += 1;
            let aliases: Vec<String> = [source, target]
                .iter()
                .filter(|value| !value.is_empty() && normalize_entity_name(value) != name_norm)
                .map(|value| value.to_string())
                .collect();
            deps.db.upsert_entity(&NewEntity {
                name: name.to_string(),
                entity_type: "term".to_string(),
                aliases,
                description: String::new(),
            })?;
            if distinct.insert(name_norm) {
                outcome.entities += 1;
            }
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::db::Db;
    use crate::models::{now_iso, GlossaryEntryInput, GlossaryRecord};

    use super::*;

    fn entry(source: &str, target: &str) -> GlossaryEntryInput {
        GlossaryEntryInput {
            source: source.to_string(),
            target: target.to_string(),
            note: String::new(),
            level: String::new(),
            match_mode: String::new(),
            context: String::new(),
        }
    }

    /// 两个词条指向同一 target:实体合并成一个,两个 source 都成为别名
    /// (否则第二个写法的提及永远扫不到)。
    #[test]
    fn seed_merges_aliases_of_entries_sharing_a_target() {
        let root = std::env::temp_dir().join(format!(
            "graph-seed-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        let data_root: PathBuf = root.join("data");
        fs::create_dir_all(&data_root).expect("data root");
        fs::create_dir_all(root.join("db")).expect("db dir");
        let db = Db::new(root.join("db").join("jobs.db"), data_root.clone());
        db.init().expect("init");
        db.save_glossary(&GlossaryRecord {
            glossary_id: "g1".to_string(),
            name: "terms".to_string(),
            description: String::new(),
            source_lang: "en".to_string(),
            target_lang: "zh".to_string(),
            enabled: true,
            entries: vec![
                entry("HLE", "卤素锂交换"),
                entry("halogen lithium exchange", "卤素锂交换"),
            ],
            created_at: now_iso(),
            updated_at: now_iso(),
        })
        .expect("save glossary");

        let deps = GraphDeps {
            db: &db,
            data_root: &data_root,
        };
        let outcome = seed_entities_from_glossaries(&deps).expect("seed");
        assert_eq!(outcome.entries, 2);
        assert_eq!(outcome.entities, 1);
        let found = db.search_entities("卤素锂交换", None, 10).expect("search");
        assert_eq!(found.len(), 1);
        let mut aliases = found[0].aliases.clone();
        aliases.sort();
        assert_eq!(
            aliases,
            vec![
                "HLE".to_string(),
                "halogen lithium exchange".to_string()
            ]
        );
        fs::remove_dir_all(&root).ok();
    }
}

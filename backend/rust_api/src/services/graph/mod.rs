//! 概念图谱服务:1a 只做 glossary 种子 + 文档块提及扫描(零 LLM 成本)。
//! 1b 的 LLM 抽取与 entity_relations 在此基础上追加。

pub mod mentions;
pub mod seed;

use std::path::Path;

use crate::db::Db;

/// 图谱服务依赖:db + 任务产物根(读块)。
pub struct GraphDeps<'a> {
    pub db: &'a Db,
    pub data_root: &'a Path,
}

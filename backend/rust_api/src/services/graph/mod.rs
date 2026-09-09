//! 概念图谱服务:glossary 种子 + 文档块提及扫描(零 LLM 成本),
//! 文档级 LLM 抽取(实体 + 关系),以及实体概念页(跨文档综述)。

pub mod extract;
pub mod favorites;
pub mod mentions;
pub mod page;
pub mod seed;

use std::path::Path;

use crate::db::Db;

/// 图谱服务依赖:db + 任务产物根(读块)。
pub struct GraphDeps<'a> {
    pub db: &'a Db,
    pub data_root: &'a Path,
}

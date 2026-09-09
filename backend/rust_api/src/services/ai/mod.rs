//! AI 问答(agentic 检索)原生实现,移植自 backend/ai_service/retainpdf_ai。
//! 取代第三进程 FastAPI 反代:数据面直读 db / 任务目录,不再 HTTP 回环。

mod agent;
pub mod ask;
pub(crate) mod blocks;
mod llm;
mod memory;
pub(crate) mod tools;

pub use ask::{run_ask, AskPayload, AskRequest};
pub use llm::LlmClient;

use std::path::Path;

use crate::config::AiRuntimeConfig;
use crate::db::Db;

/// AI 问答依赖:db + 任务产物根 + 运行时配置。
pub struct AiDeps<'a> {
    pub db: &'a Db,
    pub data_root: &'a Path,
    pub config: &'a AiRuntimeConfig,
}

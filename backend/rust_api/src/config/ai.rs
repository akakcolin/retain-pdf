use super::env_vars::{env_string, env_u64, env_usize};

/// retainpdf-ai 并入 Rust 后的运行时配置。
///
/// 直接读 `RETAIN_AI_*` 环境变量(兼容旧 third-process 配置面),无第三个进程。
/// LLM 凭据为空时由前端经 `/api/v1/ai/ask` 请求体逐请求覆盖
/// `llm_base_url` / `llm_model` / `llm_api_key`。
#[derive(Clone, Debug)]
pub struct AiRuntimeConfig {
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_api_key: String,
    pub llm_timeout_s: u64,
    pub max_tool_rounds: usize,
    pub memory_window_turns: usize,
    pub memory_compress_after_turns: usize,
    pub memory_max_chars: usize,
}

impl AiRuntimeConfig {
    pub fn from_env() -> Self {
        Self {
            llm_base_url: env_string("RETAIN_AI_LLM_BASE_URL", "https://api.deepseek.com/v1")
                .trim_end_matches('/')
                .to_string(),
            llm_model: env_string("RETAIN_AI_LLM_MODEL", "deepseek-v4-flash"),
            llm_api_key: env_string("RETAIN_AI_LLM_API_KEY", ""),
            llm_timeout_s: env_u64("RETAIN_AI_LLM_TIMEOUT_S", 60),
            max_tool_rounds: env_usize("RETAIN_AI_MAX_TOOL_ROUNDS", 6),
            memory_window_turns: env_usize("RETAIN_AI_MEMORY_WINDOW_TURNS", 6),
            memory_compress_after_turns: env_usize("RETAIN_AI_MEMORY_COMPRESS_AFTER_TURNS", 12),
            memory_max_chars: env_usize("RETAIN_AI_MEMORY_MAX_CHARS", 24000),
        }
    }
}

impl Default for AiRuntimeConfig {
    fn default() -> Self {
        Self {
            llm_base_url: "https://api.deepseek.com/v1".to_string(),
            llm_model: "deepseek-v4-flash".to_string(),
            llm_api_key: String::new(),
            llm_timeout_s: 60,
            max_tool_rounds: 6,
            memory_window_turns: 6,
            memory_compress_after_turns: 12,
            memory_max_chars: 24000,
        }
    }
}

//! 跨模块共享的默认值字面量(Rust 侧单一事实源)。
//!
//! 与 Python 侧同名字面量的一致性由门禁脚本
//! `backend/scripts/devtools/check_default_parity.py` 保证;改动此处必须同步
//! Python 定义点,否则门禁报错。

/// DeepSeek 云端默认 base_url。Python 侧定义点:
/// `services/translation/llm/providers/deepseek/transport.py::DEFAULT_BASE_URL`
pub(crate) const DEEPSEEK_DEFAULT_BASE_URL: &str = "https://api.deepseek.com/v1";

/// DeepSeek 余额查询默认地址(与 base_url 同主机,仅 Rust 侧使用)。
pub(crate) const DEEPSEEK_DEFAULT_BALANCE_URL: &str = "https://api.deepseek.com/user/balance";

/// 本地 OpenAI 兼容端点(Ollama)默认模型。Python 侧定义点:
/// `services/translation/llm/shared/provider_registry.py`
pub(crate) const LOCAL_LLM_DEFAULT_MODEL: &str = "qwen2.5:7b";

/// 本地 OpenAI 兼容端点(Ollama)默认 base_url。Python 侧定义点同上。
pub(crate) const LOCAL_LLM_DEFAULT_BASE_URL: &str = "http://localhost:11434/v1";

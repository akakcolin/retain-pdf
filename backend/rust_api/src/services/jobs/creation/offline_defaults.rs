use crate::config::env_vars::{local_llm_default_base_url, local_llm_default_model};
use crate::models::request::CreateJobInput;

/// RETAIN_OFFLINE 软优先的翻译默认值:离线模式且调用方未显式传 model/base_url 时,
/// 回填本地 LLM 端点(model/base_url 空会被后端校验拦截,这里补默认防 400)。
/// 显式传值永不覆盖;非离线模式原样返回。
pub(crate) fn apply_offline_defaults(input: &mut CreateJobInput, offline_mode: bool) {
    if !offline_mode {
        return;
    }
    if input.translation.model.trim().is_empty() {
        input.translation.model = local_llm_default_model();
    }
    if input.translation.base_url.trim().is_empty() {
        input.translation.base_url = local_llm_default_base_url();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_with_empty_translation() -> CreateJobInput {
        let mut input = CreateJobInput::default();
        input.translation.model = String::new();
        input.translation.base_url = String::new();
        input
    }

    #[test]
    fn fills_empty_translation_defaults_when_offline() {
        let mut input = input_with_empty_translation();
        apply_offline_defaults(&mut input, true);
        // 与 env helper 自洽:同一进程读同一 env,无 env 时两端都回落默认值。
        assert_eq!(input.translation.model, local_llm_default_model());
        assert_eq!(input.translation.base_url, local_llm_default_base_url());
    }

    #[test]
    fn keeps_explicit_values_when_offline() {
        let mut input = input_with_empty_translation();
        input.translation.model = "custom-model".to_string();
        input.translation.base_url = "https://example.com/v1".to_string();
        apply_offline_defaults(&mut input, true);
        assert_eq!(input.translation.model, "custom-model");
        assert_eq!(input.translation.base_url, "https://example.com/v1");
    }

    #[test]
    fn is_noop_when_not_offline() {
        let mut input = input_with_empty_translation();
        apply_offline_defaults(&mut input, false);
        assert_eq!(input.translation.model, "");
        assert_eq!(input.translation.base_url, "");
    }
}

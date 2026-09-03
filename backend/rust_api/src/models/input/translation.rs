use serde::{Deserialize, Serialize};

use crate::models::defaults::*;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GlossaryEntryInput {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub match_mode: String,
    #[serde(default)]
    pub context: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct TranslationInput {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_math_mode")]
    pub math_mode: String,
    #[serde(default)]
    pub skip_title_translation: bool,
    #[serde(default = "default_classify_batch_size")]
    pub classify_batch_size: i64,
    #[serde(default = "default_rule_profile_name")]
    pub rule_profile_name: String,
    #[serde(default)]
    pub custom_rules_text: String,
    #[serde(default)]
    pub glossary_id: String,
    #[serde(default)]
    pub glossary_name: String,
    #[serde(default)]
    pub glossary_resource_entry_count: i64,
    #[serde(default)]
    pub glossary_inline_entry_count: i64,
    #[serde(default)]
    pub glossary_overridden_entry_count: i64,
    #[serde(default)]
    pub glossary_entries: Vec<GlossaryEntryInput>,
    #[serde(default = "default_translation_context_mode")]
    pub context_mode: String,
    #[serde(default = "default_translation_glossary_mode")]
    pub glossary_mode: String,
    #[serde(default = "default_translation_memory_mode")]
    pub memory_mode: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    /// 显式声明的翻译 provider 家族（如 deepseek_official / deepseek_compatible /
    /// other）。留空时由 Python 侧按 base_url/model 嗅探兜底。
    #[serde(default)]
    pub provider_family: String,
    #[serde(default)]
    pub start_page: i64,
    #[serde(default = "default_end_page")]
    pub end_page: i64,
    #[serde(default = "default_batch_size")]
    pub batch_size: i64,
    #[serde(default)]
    pub workers: i64,
    #[serde(default = "default_source_lang")]
    pub source_lang: String,
    #[serde(default = "default_target_lang")]
    pub target_lang: String,
    #[serde(default = "default_target_language_name")]
    pub target_language_name: String,
}

impl Default for TranslationInput {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            math_mode: default_math_mode(),
            skip_title_translation: false,
            classify_batch_size: default_classify_batch_size(),
            rule_profile_name: default_rule_profile_name(),
            custom_rules_text: String::new(),
            glossary_id: String::new(),
            glossary_name: String::new(),
            glossary_resource_entry_count: 0,
            glossary_inline_entry_count: 0,
            glossary_overridden_entry_count: 0,
            glossary_entries: Vec::new(),
            context_mode: default_translation_context_mode(),
            glossary_mode: default_translation_glossary_mode(),
            memory_mode: default_translation_memory_mode(),
            api_key: String::new(),
            model: String::new(),
            base_url: String::new(),
            provider_family: String::new(),
            start_page: 0,
            end_page: default_end_page(),
            batch_size: default_batch_size(),
            workers: 0,
            source_lang: default_source_lang(),
            target_lang: default_target_lang(),
            target_language_name: default_target_language_name(),
        }
    }
}

/// 归一化后的翻译语言元数据，供视图层消费。source=auto 或空白归一为 None
/// （客户端回落 OCR 语种/默认展示名）；空白 target 也归一为 None。
#[derive(Debug, Clone, PartialEq)]
pub struct TranslationLanguageMeta {
    pub source_lang: Option<String>,
    pub target_lang: Option<String>,
    pub target_language_name: Option<String>,
}

impl TranslationInput {
    pub fn language_meta(&self) -> TranslationLanguageMeta {
        let non_empty = |value: &str| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        };
        TranslationLanguageMeta {
            source_lang: non_empty(&self.source_lang).filter(|value| value != "auto"),
            target_lang: non_empty(&self.target_lang),
            target_language_name: non_empty(&self.target_language_name),
        }
    }
}

pub fn default_translation_context_mode() -> String {
    "needed".to_string()
}

pub fn default_translation_glossary_mode() -> String {
    "matched".to_string()
}

pub fn default_translation_memory_mode() -> String {
    "matched".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_translation_defaults_to_zh_cn() {
        let value = serde_json::json!({});
        let input: TranslationInput =
            serde_json::from_value(value).expect("empty translation group parses");
        assert_eq!(input.source_lang, "auto");
        assert_eq!(input.target_lang, "zh-CN");
        assert_eq!(input.target_language_name, "简体中文");
    }

    #[test]
    fn translation_keeps_explicit_language_values() {
        let value = serde_json::json!({
            "source_lang": "ja",
            "target_lang": "en",
            "target_language_name": "English"
        });
        let input: TranslationInput = serde_json::from_value(value).expect("language fields parse");
        assert_eq!(input.source_lang, "ja");
        assert_eq!(input.target_lang, "en");
        assert_eq!(input.target_language_name, "English");
    }

    fn language_input(
        source_lang: &str,
        target_lang: &str,
        target_language_name: &str,
    ) -> TranslationInput {
        TranslationInput {
            source_lang: source_lang.to_string(),
            target_lang: target_lang.to_string(),
            target_language_name: target_language_name.to_string(),
            ..TranslationInput::default()
        }
    }

    #[test]
    fn language_meta_normalizes_auto_source_to_none() {
        assert_eq!(
            language_input("auto", "zh-CN", "简体中文").language_meta(),
            TranslationLanguageMeta {
                source_lang: None,
                target_lang: Some("zh-CN".to_string()),
                target_language_name: Some("简体中文".to_string()),
            }
        );
        assert_eq!(
            language_input("", "en", "English").language_meta(),
            TranslationLanguageMeta {
                source_lang: None,
                target_lang: Some("en".to_string()),
                target_language_name: Some("English".to_string()),
            }
        );
        assert_eq!(
            language_input("fr", "", "").language_meta(),
            TranslationLanguageMeta {
                source_lang: Some("fr".to_string()),
                target_lang: None,
                target_language_name: None,
            }
        );
    }
}

// Port of `provider_adapters/paddle/{content_profile,asset_links,markdown_match,rich_content}.py`
// — per-block content/asset/markdown-match trace enrichment.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::super::common::to_string_value;

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<[^>]+>").expect("tag regex"))
}

fn whitespace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").expect("whitespace regex"))
}

fn asset_link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"src=["']([^"']+)["']"#).expect("asset link regex"))
}

/// `to_plain_text` — strip HTML tags, collapse whitespace.
pub fn to_plain_text(text: &str) -> String {
    let without_tags = tag_re().replace_all(text, " ");
    let normalized = whitespace_re().replace_all(&without_tags, " ");
    normalized.trim().to_string()
}

/// `enrich_content_profile` (content_profile.py).
fn enrich_content_profile(metadata: &mut Map<String, Value>, raw_label: &str, text: &str) {
    let label = raw_label.trim().to_lowercase();
    let stripped = text.trim();
    let lowered = stripped.to_lowercase();
    metadata.insert(
        "content_is_rich".to_string(),
        Value::Bool(matches!(label.as_str(), "image" | "table" | "algorithm" | "figure_title")),
    );
    metadata.insert("content_length".to_string(), Value::from(stripped.chars().count()));
    let line_count = if stripped.is_empty() {
        0
    } else {
        stripped
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
    };
    metadata.insert("content_line_count".to_string(), Value::from(line_count));
    let lower_has = |needle: &str| lowered.contains(needle);
    let html_len = Value::from(stripped.chars().count());
    match label.as_str() {
        "table" => {
            metadata.insert("content_format".to_string(), Value::String(if lower_has("<table") { "html_table" } else { "plain_text" }.into()));
            metadata.insert("contains_table_tag".to_string(), Value::Bool(lower_has("<table")));
            metadata.insert("contains_img_tag".to_string(), Value::Bool(lower_has("<img")));
            metadata.insert("html_length".to_string(), html_len);
        }
        "image" => {
            metadata.insert("content_format".to_string(), Value::String(if lower_has("<img") { "html_image" } else { "plain_text" }.into()));
            metadata.insert("contains_img_tag".to_string(), Value::Bool(lower_has("<img")));
            metadata.insert("contains_table_tag".to_string(), Value::Bool(lower_has("<table")));
            metadata.insert("html_length".to_string(), html_len);
        }
        "algorithm" => {
            metadata.insert("content_format".to_string(), Value::String("code_like_text".into()));
            metadata.insert("contains_img_tag".to_string(), Value::Bool(lower_has("<img")));
            metadata.insert("contains_table_tag".to_string(), Value::Bool(lower_has("<table")));
            let looks_like_command = lower_has("python ") || lower_has("bash ") || stripped.contains("scripts/");
            metadata.insert("looks_like_command".to_string(), Value::Bool(looks_like_command));
        }
        "figure_title" => {
            metadata.insert("content_format".to_string(), Value::String(if lower_has("<div") { "html_caption" } else { "plain_text" }.into()));
            metadata.insert("contains_img_tag".to_string(), Value::Bool(lower_has("<img")));
            metadata.insert("contains_table_tag".to_string(), Value::Bool(lower_has("<table")));
        }
        _ => {}
    }
}

/// `enrich_asset_links` (asset_links.py).
fn enrich_asset_links(
    metadata: &mut Map<String, Value>,
    text: &str,
    markdown_images: &Map<String, Value>,
) {
    let stripped = text.trim();
    if let Some(caps) = asset_link_re().captures(stripped) {
        let asset_key = caps.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
        metadata.insert("asset_key".to_string(), Value::String(asset_key.clone()));
        metadata.insert("asset_kind".to_string(), Value::String("markdown_image".into()));
        let asset_url = to_string_value(markdown_images.get(&asset_key));
        metadata.insert("asset_url".to_string(), Value::String(asset_url.clone()));
        metadata.insert("asset_resolved".to_string(), Value::Bool(!asset_url.is_empty()));
    } else {
        metadata.insert("asset_key".to_string(), Value::String(String::new()));
        metadata.insert("asset_kind".to_string(), Value::String(String::new()));
        metadata.insert("asset_url".to_string(), Value::String(String::new()));
        metadata.insert("asset_resolved".to_string(), Value::Bool(false));
    }
}

/// `enrich_markdown_match` (markdown_match.py).
fn enrich_markdown_match(
    metadata: &mut Map<String, Value>,
    text: &str,
    markdown_text: &str,
) {
    let plain_text = to_plain_text(text.trim());
    let match_text: String = plain_text.chars().take(160).collect();
    let match_count = if !match_text.is_empty() && !markdown_text.is_empty() {
        markdown_text.matches(&match_text).count()
    } else {
        0
    };
    metadata.insert("markdown_match_text".to_string(), Value::String(match_text));
    metadata.insert("markdown_match_found".to_string(), Value::Bool(match_count > 0));
    metadata.insert("markdown_match_count".to_string(), Value::from(match_count));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn to_plain_text_strips_tags_and_collapses_whitespace() {
        assert_eq!(to_plain_text("<b>hello</b>  world"), "hello world");
        assert_eq!(to_plain_text("plain"), "plain");
        assert_eq!(to_plain_text(""), "");
    }

    fn metadata_for(label: &str, text: &str, images: Map<String, Value>, markdown: &str) -> Map<String, Value> {
        let mut meta = Map::new();
        enrich_rich_content_trace(&mut meta, label, text, &images, markdown);
        meta
    }

    #[test]
    fn table_profile_detects_html_and_plain() {
        let images = Map::new();
        let html = metadata_for("table", "<table><tr><td>a</td></tr></table>", images.clone(), "");
        assert_eq!(html["content_is_rich"], true);
        assert_eq!(html["content_format"], "html_table");
        assert_eq!(html["contains_table_tag"], true);
        let plain = metadata_for("table", "plain table text", images, "");
        assert_eq!(plain["content_format"], "plain_text");
    }

    #[test]
    fn image_profile_and_asset_resolution() {
        let mut images = Map::new();
        images.insert("fig_1.png".to_string(), Value::String("rendered/fig_1.png".to_string()));
        let meta = metadata_for("image", r#"<img src="fig_1.png" />"#, images, "");
        assert_eq!(meta["content_format"], "html_image");
        assert_eq!(meta["contains_img_tag"], true);
        assert_eq!(meta["asset_key"], "fig_1.png");
        assert_eq!(meta["asset_kind"], "markdown_image");
        assert_eq!(meta["asset_url"], "rendered/fig_1.png");
        assert_eq!(meta["asset_resolved"], true);
    }

    #[test]
    fn algorithm_profile_flags_command() {
        let images = Map::new();
        let meta = metadata_for("algorithm", "python scripts/parse.py", images, "");
        assert_eq!(meta["content_format"], "code_like_text");
        assert_eq!(meta["looks_like_command"], true);
    }

    #[test]
    fn figure_title_profile_and_content_length() {
        let images = Map::new();
        let meta = metadata_for("figure_title", "Figure 1: overview", images, "");
        assert_eq!(meta["content_is_rich"], true);
        assert_eq!(meta["content_length"], 18);
        assert_eq!(meta["content_format"], "plain_text");
    }

    #[test]
    fn markdown_match_counts_occurrences() {
        let images = Map::new();
        let body = "the reading order can be recovered";
        let meta = metadata_for("text", body, images.clone(), &format!("preface {body} {body}"));
        assert_eq!(meta["markdown_match_found"], true);
        assert_eq!(meta["markdown_match_count"], 2);
        let miss = metadata_for("text", "absent sentence here", images, body);
        assert_eq!(miss["markdown_match_found"], false);
        assert_eq!(miss["markdown_match_count"], 0);
    }
}

/// `enrich_rich_content_trace` — content_profile + asset_links + markdown_match.
pub fn enrich_rich_content_trace(
    metadata: &mut Map<String, Value>,
    raw_label: &str,
    text: &str,
    markdown_images: &Map<String, Value>,
    markdown_text: &str,
) {
    enrich_content_profile(metadata, raw_label, text);
    enrich_asset_links(metadata, text, markdown_images);
    enrich_markdown_match(metadata, text, markdown_text);
}

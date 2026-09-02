//! R-2: rebuild the full-book translated markdown from the per-page translated
//! payloads the render bundle consumes, and write it to
//! `<job_root>/md/translated.md` so relative image paths resolve against the
//! same `md/images` dir the OCR markdown (`md/full.md`) uses.
//!
//! Best-effort export: each translated item is rendered with the same
//! inline-content markdown builder the render path uses
//! (`rendering_core::inline_content::markdown::build_markdown_paragraph`);
//! non-textual blocks that carry no protected text are skipped.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::translations::load_translated_pages;

pub const TRANSLATED_MARKDOWN_FILE_NAME: &str = "translated.md";

/// Rebuild the translated markdown into `dest` (creates parents as needed).
pub fn write_translated_markdown(
    translations_dir: &Path,
    translation_manifest: Option<&Path>,
    dest: &Path,
) -> Result<()> {
    let pages = load_translated_pages(translations_dir, translation_manifest)?;
    let mut chunks: Vec<String> = Vec::new();
    for (_page_idx, items) in pages.iter() {
        let page = build_page_markdown(items);
        if !page.is_empty() {
            chunks.push(page);
        }
    }
    if chunks.is_empty() {
        bail!("no translated markdown content to write");
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut content = chunks.join("\n\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    std::fs::write(dest, &content)
        .with_context(|| format!("failed to write translated markdown {}", dest.display()))?;
    Ok(())
}

fn build_page_markdown(items: &[Value]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for item in items {
        let text = rendering_core::inline_content::markdown::build_markdown_paragraph(item);
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "render-rs-translated-md-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_translations(dir: &std::path::Path) {
        fs::write(
            dir.join("translation-manifest.json"),
            serde_json::to_string(&serde_json::json!({
                "schema": "translation_manifest_v1",
                "schema_version": 1,
                "pages": [
                    {"page_index": 0, "path": "page-001.json"},
                    {"page_index": 1, "path": "page-002.json"},
                ],
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("page-001.json"),
            serde_json::to_string(&vec![
                serde_json::json!({
                    "item_id": "p000-b001",
                    "block_kind": "text",
                    "layout_role": "paragraph",
                    "semantic_role": "body",
                    "structure_role": "body",
                    "policy_translate": true,
                    "asset_id": "",
                    "reading_order": 1,
                    "raw_block_type": "text",
                    "normalized_sub_type": "body",
                    "protected_translated_text": "你好世界",
                    "formula_map": [],
                    "math_mode": "placeholder",
                }),
                serde_json::json!({
                    "item_id": "p000-b002",
                    "block_kind": "image",
                    "layout_role": "figure",
                    "semantic_role": "image",
                    "structure_role": "image",
                    "policy_translate": false,
                    "asset_id": "a1",
                    "reading_order": 2,
                    "raw_block_type": "image",
                    "normalized_sub_type": "image",
                }),
            ])
            .unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("page-002.json"),
            serde_json::to_string(&vec![serde_json::json!({
                "item_id": "p001-b001",
                "block_kind": "text",
                "layout_role": "paragraph",
                "semantic_role": "body",
                "structure_role": "body",
                "policy_translate": true,
                "asset_id": "",
                "reading_order": 1,
                "raw_block_type": "text",
                "normalized_sub_type": "body",
                "protected_translated_text": "第二页",
                "formula_map": [],
                "math_mode": "placeholder",
            })])
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn writes_translated_markdown_from_pages() {
        let dir = temp_dir("write");
        write_translations(&dir);
        let dest = dir.join("md").join("translated.md");
        write_translated_markdown(&dir, None, &dest).unwrap();
        let content = fs::read_to_string(&dest).unwrap();
        assert!(content.contains("你好世界"));
        assert!(content.contains("第二页"));
        assert!(content.ends_with('\n'));
    }

    #[test]
    fn fails_without_translated_content() {
        let dir = temp_dir("empty");
        fs::write(
            dir.join("translation-manifest.json"),
            serde_json::to_string(&serde_json::json!({
                "schema": "translation_manifest_v1",
                "pages": [{"page_index": 0, "path": "page-001.json"}],
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("page-001.json"),
            serde_json::to_string(&vec![serde_json::json!({
                "item_id": "p000-b001",
                "block_kind": "image",
                "layout_role": "figure",
                "semantic_role": "image",
                "structure_role": "image",
                "policy_translate": false,
                "asset_id": "a1",
                "reading_order": 1,
                "raw_block_type": "image",
                "normalized_sub_type": "image",
            })])
            .unwrap(),
        )
        .unwrap();
        let err = write_translated_markdown(&dir, None, &dir.join("md").join("translated.md"))
            .expect_err("no text content should fail");
        assert!(err.to_string().contains("no translated markdown content"));
    }
}

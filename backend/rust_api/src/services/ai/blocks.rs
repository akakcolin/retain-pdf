//! 任务产物的块级读取(移植自 retainpdf_ai/blocks.py)。
//! 真相在任务目录:ocr/normalized/document.v1.json(原文块)与
//! translated/page-*.json(译文,按 (page_idx, block_idx) 数字索引对齐)。
//! 只读,不写任何任务目录内容。

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct Block {
    pub page_idx: i64,
    pub block_id: String,
    pub source_text: String,
    pub translated_text: String,
}

fn as_int(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number.as_i64(),
        Some(Value::String(text)) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

pub fn load_job_blocks(job_root: &Path) -> Result<Vec<Block>, AppError> {
    let normalized_path = job_root.join("ocr/normalized/document.v1.json");
    let text = std::fs::read_to_string(&normalized_path).map_err(|err| {
        AppError::internal(format!(
            "read normalized document {}: {err}",
            normalized_path.display()
        ))
    })?;
    let document: Value = serde_json::from_str(&text).map_err(|err| {
        AppError::internal(format!(
            "parse normalized document {}: {err}",
            normalized_path.display()
        ))
    })?;

    let mut translated: HashMap<(i64, i64), String> = HashMap::new();
    let translated_dir = job_root.join("translated");
    if translated_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&translated_dir)
            .map_err(|err| AppError::internal(format!("read translated dir: {err}")))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with("page-") && name.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect();
        entries.sort();
        for path in entries {
            let Ok(page_text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(items) = serde_json::from_str::<Vec<Value>>(&page_text) else {
                continue;
            };
            for item in items {
                let Some(page_idx) = as_int(item.get("page_idx")) else {
                    continue;
                };
                let Some(block_idx) = as_int(item.get("block_idx")) else {
                    continue;
                };
                let text_value = item
                    .get("translated_text")
                    .map(|value| value.as_str().unwrap_or(""))
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !text_value.is_empty() {
                    translated.insert((page_idx, block_idx), text_value);
                }
            }
        }
    }

    let mut blocks = Vec::new();
    if let Some(pages) = document.get("pages").and_then(Value::as_array) {
        for page in pages {
            let page_idx = as_int(page.get("page_index")).unwrap_or(0);
            if let Some(page_blocks) = page.get("blocks").and_then(Value::as_array) {
                for (block_idx, block) in page_blocks.iter().enumerate() {
                    let block_id = block
                        .get("block_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let source_text = block
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let translated_text = translated
                        .get(&(page_idx, block_idx as i64))
                        .cloned()
                        .unwrap_or_default();
                    if block_id.is_empty() || (source_text.is_empty() && translated_text.is_empty())
                    {
                        continue;
                    }
                    blocks.push(Block {
                        page_idx,
                        block_id,
                        source_text,
                        translated_text,
                    });
                }
            }
        }
    }
    Ok(blocks)
}

/// 取某页的块;给定 around_block_id 时以它为中心取窗口。
pub fn read_page_blocks(
    job_root: &Path,
    page_idx: i64,
    around_block_id: &str,
    max_blocks: usize,
) -> Vec<Block> {
    let page_blocks: Vec<Block> = load_job_blocks(job_root)
        .unwrap_or_default()
        .into_iter()
        .filter(|block| block.page_idx == page_idx)
        .collect();
    let max_blocks = max_blocks.max(1);
    if around_block_id.is_empty() {
        return page_blocks.into_iter().take(max_blocks).collect();
    }
    let center = page_blocks
        .iter()
        .position(|block| block.block_id == around_block_id);
    let Some(center) = center else {
        return page_blocks.into_iter().take(max_blocks).collect();
    };
    let half = max_blocks / 2;
    let start = center.saturating_sub(half);
    page_blocks
        .into_iter()
        .skip(start)
        .take(max_blocks)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_job_dir(root: &Path) {
        let normalized = root.join("jobs/job-1/ocr/normalized");
        fs::create_dir_all(&normalized).expect("mkdir normalized");
        fs::write(
            normalized.join("document.v1.json"),
            serde_json::json!({
                "pages": [
                    {
                        "page_index": 2,
                        "blocks": [
                            {"block_id": "p003-b0000", "text": "first block"},
                            {"block_id": "p003-b0001", "text": "second block"},
                        ],
                    }
                ]
            })
            .to_string(),
        )
        .expect("write document");
        let translated = root.join("jobs/job-1/translated");
        fs::create_dir_all(&translated).expect("mkdir translated");
        fs::write(
            translated.join("page-003-deepseek.json"),
            serde_json::json!([
                {"page_idx": "2", "block_idx": "1", "translated_text": "第二个块的译文"}
            ])
            .to_string(),
        )
        .expect("write translated page");
    }

    #[test]
    fn read_page_blocks_aligns_translation_by_numeric_index() {
        let root = std::env::temp_dir().join(format!(
            "ai-blocks-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        write_job_dir(&root);
        let job_root = root.join("jobs/job-1");
        let blocks = read_page_blocks(&job_root, 2, "", 12);
        let ids: Vec<&str> = blocks.iter().map(|block| block.block_id.as_str()).collect();
        assert_eq!(ids, vec!["p003-b0000", "p003-b0001"]);
        assert_eq!(blocks[1].translated_text, "第二个块的译文");
        let windowed = read_page_blocks(&job_root, 2, "p003-b0001", 1);
        let windowed_ids: Vec<&str> = windowed
            .iter()
            .map(|block| block.block_id.as_str())
            .collect();
        assert_eq!(windowed_ids, vec!["p003-b0001"]);
        fs::remove_dir_all(&root).ok();
    }
}

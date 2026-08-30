// Port of `provider_adapters/paddle/continuation.py` — provider continuation
// hints grouped by raw group id, with cross-column-merge suppression.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::super::common::{
    build_provider_continuation_hint, continuation_role_for, py_int, to_string_value,
};
use super::super::defaults::default_block_continuation_hint;

/// `_token` — Python `re.sub(r"[^A-Za-z0-9_.-]+", "-", text).strip("-")`.
fn token(value: Option<&Value>) -> String {
    let text = to_string_value(value).trim().to_string();
    if text.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(text.len());
    let mut pending_dash = false;
    for c in text.chars() {
        let allowed = c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-');
        if allowed {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else if !pending_dash {
            pending_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// `_raw_order` — metadata.raw_block_order as an int (Python `isinstance(value, int)`).
fn raw_order(block: &Value) -> Option<i64> {
    let value = block
        .get("metadata")
        .and_then(Value::as_object)
        .and_then(|m| m.get("raw_block_order"));
    match value {
        Some(Value::Bool(_)) => None,
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => {
            let stripped = s.trim();
            let digit_part = stripped.strip_prefix('-').unwrap_or(stripped);
            if !digit_part.is_empty() && digit_part.chars().all(|c| c.is_ascii_digit()) {
                stripped.parse::<i64>().ok()
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `_group_id` — `provider-paddle-global-{id}` or `provider-paddle-page-{NNN}-group-{id}`.
fn group_id(block: &Value) -> String {
    let metadata = block.get("metadata").and_then(Value::as_object);
    let global_group_id = token(metadata.and_then(|m| m.get("raw_global_group_id")));
    if !global_group_id.is_empty() {
        return format!("provider-paddle-global-{global_group_id}");
    }
    let local_group_id = token(metadata.and_then(|m| m.get("raw_group_id")));
    if !local_group_id.is_empty() {
        let page_index = block.get("page_index").and_then(py_int).unwrap_or(0);
        return format!("provider-paddle-page-{:03}-group-{local_group_id}", page_index + 1);
    }
    String::new()
}

/// `_suppressed_by_cross_column_merge` — truthiness over three provider flags.
fn suppressed_by_cross_column_merge(block: &Value) -> bool {
    let metadata = block.get("metadata").and_then(Value::as_object);
    let get = |key: &str| {
        metadata
            .and_then(|m| m.get(key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    get("provider_cross_column_merge_suspected")
        || get("provider_reading_order_unreliable")
        || get("provider_body_repair_applied")
}

/// `_mark_provider_hint_suppressed`.
fn mark_provider_hint_suppressed(block: &mut Value) {
    let obj = block.as_object_mut().expect("block object");
    let metadata = obj
        .entry("metadata".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let metadata = metadata.as_object_mut().expect("metadata object");
    let body_repair_applied = metadata
        .get("provider_body_repair_applied")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reason = if body_repair_applied {
        "body_repair_applied"
    } else {
        "cross_column_merge_suspected"
    };
    metadata.insert("provider_continuation_suppressed".to_string(), Value::Bool(true));
    metadata.insert(
        "provider_continuation_suppressed_reason".to_string(),
        Value::String(reason.into()),
    );
    metadata.insert("continuation_suppressed".to_string(), Value::Bool(true));
    metadata.insert(
        "continuation_suppressed_reason".to_string(),
        Value::String(reason.into()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn block(page_index: i64, order: i64, raw_order: Option<i64>, group: Option<&str>) -> Value {
        let mut metadata = Map::new();
        if let Some(ro) = raw_order {
            metadata.insert("raw_block_order".into(), Value::from(ro));
        }
        if let Some(g) = group {
            metadata.insert("raw_group_id".into(), Value::String(g.to_string()));
        }
        json!({
            "page_index": page_index,
            "order": order,
            "metadata": metadata,
        })
    }

    #[test]
    fn token_sanitizes_disallowed_runs() {
        assert_eq!(token(None), "");
        assert_eq!(token(Some(&json!("hello world!"))), "hello-world");
        assert_eq!(token(Some(&json!("a  b\tc/d"))), "a-b-c-d");
        assert_eq!(token(Some(&json!("ok_1.a-b"))), "ok_1.a-b");
        assert_eq!(token(Some(&json!("--trim--"))), "trim");
    }

    #[test]
    fn group_id_local_and_global_scopes() {
        let local = block(0, 0, None, Some("sec-1"));
        assert_eq!(group_id(&local), "provider-paddle-page-001-group-sec-1");
        let global = json!({
            "page_index": 2,
            "order": 3,
            "metadata": { "raw_global_group_id": "glob_x" },
        });
        assert_eq!(group_id(&global), "provider-paddle-global-glob_x");
        assert_eq!(group_id(&block(0, 0, None, None)), "");
    }

    #[test]
    fn raw_order_accepts_int_string_rejects_bool() {
        let int_block = json!({"metadata": {"raw_block_order": 5}});
        assert_eq!(raw_order(&int_block), Some(5));
        let str_block = json!({"metadata": {"raw_block_order": "7"}});
        assert_eq!(raw_order(&str_block), Some(7));
        let bool_block = json!({"metadata": {"raw_block_order": true}});
        assert_eq!(raw_order(&bool_block), None);
        let float_block = json!({"metadata": {"raw_block_order": 5.5}});
        assert_eq!(raw_order(&float_block), None);
        let absent = json!({"metadata": {}});
        assert_eq!(raw_order(&absent), None);
    }

    #[test]
    fn intra_page_group_assigns_head_middle_tail() {
        let mut pages = vec![
            json!({"page_index": 0, "blocks": [
                block(0, 0, Some(1), Some("g")),
                block(0, 1, Some(2), Some("g")),
                block(0, 2, Some(3), Some("g")),
            ]}),
        ];
        assign_paddle_continuation_hints(&mut pages);
        let blocks = pages[0]["blocks"].as_array().unwrap();
        assert_eq!(blocks[0]["continuation_hint"]["role"], "head");
        assert_eq!(blocks[0]["continuation_hint"]["scope"], "intra_page");
        assert_eq!(blocks[0]["continuation_hint"]["reading_order"], 1);
        assert_eq!(blocks[0]["continuation_hint"]["group_id"], "provider-paddle-page-001-group-g");
        assert_eq!(blocks[0]["continuation_hint"]["confidence"], 0.98);
        assert_eq!(blocks[1]["continuation_hint"]["role"], "middle");
        assert_eq!(blocks[2]["continuation_hint"]["role"], "tail");
        assert_eq!(blocks[2]["continuation_hint"]["reading_order"], 3);
    }

    #[test]
    fn cross_page_group_scope_and_missing_order_drop() {
        // Cross-page scope requires raw_global_group_id (local group ids embed the
        // page index, so they are always intra-page).
        let global_block = |page_index: i64, order: i64, raw_order: i64| {
            json!({
                "page_index": page_index,
                "order": order,
                "metadata": { "raw_global_group_id": "g", "raw_block_order": raw_order },
            })
        };
        let mut pages = vec![
            json!({"page_index": 0, "blocks": [global_block(0, 0, 1)]}),
            json!({"page_index": 1, "blocks": [global_block(1, 0, 2)]}),
        ];
        assign_paddle_continuation_hints(&mut pages);
        let hint = &pages[0]["blocks"][0]["continuation_hint"];
        assert_eq!(hint["scope"], "cross_page");
        assert_eq!(hint["role"], "head");
        assert_eq!(hint["group_id"], "provider-paddle-global-g");

        let mut dropped = vec![json!({"page_index": 0, "blocks": [
            block(0, 0, None, Some("g")),
            block(0, 1, Some(2), Some("g")),
        ]})];
        assign_paddle_continuation_hints(&mut dropped);
        let hint = &dropped[0]["blocks"][0]["continuation_hint"];
        assert_eq!(hint, &default_block_continuation_hint());
    }

    #[test]
    fn cross_column_merge_suppression_marks_blocks() {
        let mut pages = vec![
            json!({"page_index": 0, "blocks": [
                {
                    "page_index": 0,
                    "order": 0,
                    "metadata": {
                        "raw_group_id": "g",
                        "raw_block_order": 1,
                        "provider_cross_column_merge_suspected": true,
                    },
                },
                block(0, 1, Some(2), Some("g")),
            ]}),
        ];
        assign_paddle_continuation_hints(&mut pages);
        let b0 = pages[0]["blocks"][0].as_object().unwrap();
        let meta = b0["metadata"].as_object().unwrap();
        assert_eq!(meta["provider_continuation_suppressed"], true);
        assert_eq!(meta["provider_continuation_suppressed_reason"], "cross_column_merge_suspected");
        assert_eq!(meta["continuation_suppressed"], true);
        let b1 = pages[0]["blocks"][1].as_object().unwrap();
        assert_eq!(b1["metadata"]["provider_continuation_suppressed"], true);
    }

    #[test]
    fn body_repair_applied_sets_reason() {
        let mut block = json!({
            "page_index": 0,
            "order": 0,
            "metadata": { "provider_body_repair_applied": true },
        });
        mark_provider_hint_suppressed(&mut block);
        let meta = block["metadata"].as_object().unwrap();
        assert_eq!(meta["provider_continuation_suppressed_reason"], "body_repair_applied");
    }
}

/// `assign_paddle_continuation_hints` — group blocks by raw group id and assign
/// provider continuation hints (in place).
pub fn assign_paddle_continuation_hints(pages: &mut [Value]) {
    let mut groups: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
    for (page_idx, page) in pages.iter_mut().enumerate() {
        let Some(blocks) = page.get_mut("blocks").and_then(Value::as_array_mut) else {
            continue;
        };
        for (block_idx, block) in blocks.iter_mut().enumerate() {
            if let Some(obj) = block.as_object_mut() {
                obj.insert("continuation_hint".to_string(), default_block_continuation_hint());
            }
            let gid = group_id(block);
            if gid.is_empty() {
                continue;
            }
            groups.entry(gid).or_default().push((page_idx, block_idx));
        }
    }

    for (group_id, indices) in groups {
        let suppressed = indices
            .iter()
            .any(|(pi, bi)| suppressed_by_cross_column_merge(&pages[*pi]["blocks"][*bi]));
        if suppressed {
            for (pi, bi) in &indices {
                mark_provider_hint_suppressed(&mut pages[*pi]["blocks"][*bi]);
            }
            continue;
        }
        if indices.len() > 1
            && indices
                .iter()
                .any(|(pi, bi)| raw_order(&pages[*pi]["blocks"][*bi]).is_none())
        {
            continue;
        }
        let mut sorted: Vec<(usize, usize)> = indices.clone();
        sorted.sort_by_key(|(pi, bi)| {
            let block = &pages[*pi]["blocks"][*bi];
            (
                block.get("page_index").and_then(py_int).unwrap_or(0),
                raw_order(block).unwrap_or(-1),
                block.get("order").and_then(py_int).unwrap_or(0),
            )
        });
        let mut page_set: BTreeSet<i64> = BTreeSet::new();
        for (pi, _) in &sorted {
            page_set.insert(pages[*pi].get("page_index").and_then(py_int).unwrap_or(0));
        }
        let scope = if page_set.len() > 1 { "cross_page" } else { "intra_page" };
        let size = sorted.len();
        for (index, (pi, bi)) in sorted.iter().enumerate() {
            let block = &mut pages[*pi]["blocks"][*bi];
            let reading_order = raw_order(block).unwrap_or(-1);
            if let Some(obj) = block.as_object_mut() {
                obj.insert(
                    "continuation_hint".to_string(),
                    build_provider_continuation_hint(
                        &group_id,
                        continuation_role_for(index, size),
                        scope,
                        reading_order,
                        0.98,
                    ),
                );
            }
        }
    }
}

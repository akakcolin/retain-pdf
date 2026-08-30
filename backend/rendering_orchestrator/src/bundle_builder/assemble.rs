//! The 15-key `render.bundle.v1` dict, assembled by hand as `serde_json::Value`
//! in the exact key order `build_bundle` emits (the key order is preserved by
//! the `serde_json` `preserve_order` feature so `--dump-bundle` output stays
//! byte-similar to the Python producer). We deliberately do NOT derive
//! `Serialize` on `RenderBundle`/`RedactionItem` — that would drop keys the
//! emitter/redaction DTOs carry.

use std::path::PathBuf;

use serde_json::{json, Map, Value};

pub struct AssembleInputs {
    pub mode: String,
    pub source_pdf: PathBuf,
    pub output_pdf: PathBuf,
    pub work_dir: PathBuf,
    pub font_family: String,
    pub start_page: i32,
    pub end_page: i32,
    pub page_map_indices: Vec<i32>,
    pub translated_pages: Value,
    pub page_specs: Value,
}

pub fn assemble(inputs: AssembleInputs) -> Value {
    let mut map = Map::new();
    map.insert("schema_version".to_string(), json!(super::RENDER_BUNDLE_SCHEMA_VERSION));
    map.insert("mode".to_string(), json!(inputs.mode));
    map.insert("source_pdf".to_string(), json!(inputs.source_pdf.to_string_lossy()));
    map.insert("output_pdf".to_string(), json!(inputs.output_pdf.to_string_lossy()));
    map.insert("work_dir".to_string(), json!(inputs.work_dir.to_string_lossy()));
    map.insert("font_family".to_string(), json!(inputs.font_family));
    map.insert(
        "redaction_strategy".to_string(),
        if inputs.mode == "typst_visual" {
            json!("visual_cover")
        } else {
            Value::Null
        },
    );
    map.insert("precleaned_page_indices".to_string(), json!([]));
    map.insert("visual_profile_fill_map".to_string(), json!({}));
    map.insert(
        "page_map".to_string(),
        json!({ "source_page_indices": inputs.page_map_indices }),
    );
    map.insert("translated_pages".to_string(), inputs.translated_pages);
    map.insert("page_specs".to_string(), inputs.page_specs);
    map.insert("start_page".to_string(), json!(inputs.start_page));
    map.insert("end_page".to_string(), json!(inputs.end_page));
    map.insert("overlay_page_specs".to_string(), Value::Null);
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembles_fifteen_keys_in_order() {
        let value = assemble(AssembleInputs {
            mode: "typst".to_string(),
            source_pdf: PathBuf::from("/src.pdf"),
            output_pdf: PathBuf::from("/rendered/out.pdf"),
            work_dir: PathBuf::from("/rendered/typst/background-book"),
            font_family: "Source Han Serif SC".to_string(),
            start_page: 0,
            end_page: 1,
            page_map_indices: vec![0, 1],
            translated_pages: json!({}),
            page_specs: json!([]),
        });
        let obj = value.as_object().unwrap();
        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "schema_version",
                "mode",
                "source_pdf",
                "output_pdf",
                "work_dir",
                "font_family",
                "redaction_strategy",
                "precleaned_page_indices",
                "visual_profile_fill_map",
                "page_map",
                "translated_pages",
                "page_specs",
                "start_page",
                "end_page",
                "overlay_page_specs",
            ]
        );
        assert_eq!(obj["redaction_strategy"], Value::Null);
        assert_eq!(obj["overlay_page_specs"], Value::Null);
    }
}

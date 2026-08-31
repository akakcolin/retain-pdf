//! R-1: translation manifest + per-page translated-payload loading.
//!
//! Mirror of `runtime/pipeline/translation_loader.py` +
//! `services/translation/core/payload/{manifest,translations,template_contract}.py`
//! for the render-bundle path. Reads the on-disk translation artifacts a job
//! produces:
//!
//! ```text
//! <translations_dir>/translation-manifest.json
//!   {"schema": "translation_manifest_v1",
//!    "pages": [{"page_index": 0, "path": "page-001.json"}, ...]}
//! <translations_dir>/page-001.json
//!   [ {item dict}, ... ]   (one record per translated item)
//! ```
//!
//! `load_translations` validates the strict record contract but deliberately
//! SKIPS the Python-side sanitize/refresh-translation-units/write-back
//! mutations: on pipeline-produced corpus those steps are no-ops (records
//! already carry the derived `translation_unit_*` fields). Tests lock the load
//! to be field-identical against such files; if a corpus file ever diverges,
//! extend the sanitize here rather than silently dropping it.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

pub const TRANSLATION_MANIFEST_FILE_NAME: &str = "translation-manifest.json";
pub const TRANSLATION_MANIFEST_SCHEMA: &str = "translation_manifest_v1";

const REQUIRED_CONTRACT_FIELDS: [&str; 9] = [
    "block_kind",
    "layout_role",
    "semantic_role",
    "structure_role",
    "policy_translate",
    "asset_id",
    "reading_order",
    "raw_block_type",
    "normalized_sub_type",
];

pub fn translation_manifest_path(translations_dir: &Path) -> PathBuf {
    translations_dir.join(TRANSLATION_MANIFEST_FILE_NAME)
}

/// `manifest.load_translation_manifest_file` — schema check, relative payload
/// path resolution, and translations-dir escape guard (duplicate indices and
/// absolute/escaping paths are rejected).
pub fn load_translation_manifest_file(
    manifest_path: &Path,
    translations_dir: Option<&Path>,
) -> Result<BTreeMap<i32, PathBuf>> {
    let base_dir = translations_dir.map(Path::to_path_buf).unwrap_or_else(|| {
        manifest_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    });
    // Resolve the base once (canonical when it exists, e.g. it resolves the
    // `/var` -> `/private/var` symlink on macOS); the escape check below joins
    // the raw path onto THIS resolved base so both sides share one prefix.
    let resolved_base_dir = resolved_path(&base_dir);

    let text = std::fs::read_to_string(manifest_path).map_err(|e| {
        anyhow!(
            "failed to read translation manifest {}: {e}",
            manifest_path.display()
        )
    })?;
    let payload: Value = serde_json::from_str(&text).map_err(|e| {
        anyhow!(
            "invalid translation manifest JSON {}: {e}",
            manifest_path.display()
        )
    })?;
    let obj = payload.as_object().ok_or_else(|| {
        anyhow!(
            "Invalid translation manifest pages: {}",
            manifest_path.display()
        )
    })?;

    let schema = obj
        .get("schema")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if schema != TRANSLATION_MANIFEST_SCHEMA {
        bail!(
            "Unsupported translation manifest schema: {}",
            if schema.is_empty() {
                "<missing>"
            } else {
                schema
            }
        );
    }

    let pages = obj.get("pages").ok_or_else(|| {
        anyhow!(
            "Invalid translation manifest pages: {}",
            manifest_path.display()
        )
    })?;
    let pages = pages.as_array().ok_or_else(|| {
        anyhow!(
            "Invalid translation manifest pages: {}",
            manifest_path.display()
        )
    })?;

    let mut translation_paths: BTreeMap<i32, PathBuf> = BTreeMap::new();
    for page in pages {
        let page_obj = page.as_object().ok_or_else(|| {
            anyhow!(
                "Invalid translation manifest page entry: {}",
                manifest_path.display()
            )
        })?;
        let page_index = as_page_index(page_obj.get("page_index")).ok_or_else(|| {
            anyhow!(
                "Invalid translation manifest page_index: {}",
                manifest_path.display()
            )
        })?;
        let raw_path = page_obj
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if raw_path.is_empty() {
            bail!("Translation manifest page {page_index} is missing path");
        }
        let raw = Path::new(raw_path);
        if raw.is_absolute() {
            bail!("Translation manifest page {page_index} uses absolute payload path: {raw_path}");
        }
        let translation_path = base_dir.join(raw);
        let joined = resolved_base_dir.join(raw);
        let resolved = joined
            .canonicalize()
            .unwrap_or_else(|_| lexically_resolve(&joined));
        if !resolved.starts_with(&resolved_base_dir) {
            bail!(
                "Translation manifest page {page_index} payload path escapes translations_dir: {raw_path}"
            );
        }
        if translation_paths.contains_key(&page_index) {
            bail!("Duplicate translation manifest page index: {page_index}");
        }
        translation_paths.insert(page_index, translation_path);
    }
    Ok(translation_paths)
}

/// `manifest.load_translation_manifest` — manifest path is derived from the
/// translations dir.
pub fn load_translation_manifest(translations_dir: &Path) -> Result<BTreeMap<i32, PathBuf>> {
    let manifest_path = translation_manifest_path(translations_dir);
    load_translation_manifest_file(&manifest_path, Some(translations_dir))
}

/// `translations.load_translations` — read a per-page payload and validate the
/// strict record contract. Sanitize/refresh/write-back mutations are skipped
/// (no-ops on consistent corpus; see module docs).
pub fn load_translations(translation_path: &Path, strict_contract: bool) -> Result<Vec<Value>> {
    let text = std::fs::read_to_string(translation_path).map_err(|e| {
        anyhow!(
            "failed to read translation payload {}: {e}",
            translation_path.display()
        )
    })?;
    let payload: Vec<Value> = serde_json::from_str(&text).map_err(|e| {
        anyhow!(
            "invalid translation payload JSON {}: {e}",
            translation_path.display()
        )
    })?;
    if strict_contract {
        validate_translation_payload_contract(&payload, translation_path)?;
    }
    Ok(payload)
}

/// `template_contract.validate_translation_payload_contract` — every record
/// must be an object carrying the required contract keys.
pub fn validate_translation_payload_contract(
    payload: &[Value],
    translation_path: &Path,
) -> Result<()> {
    for (index, record) in payload.iter().enumerate() {
        let Some(obj) = record.as_object() else {
            bail!(
                "invalid translation payload at {}: record[{index}] is not an object",
                translation_path.display()
            );
        };
        let missing: Vec<&str> = REQUIRED_CONTRACT_FIELDS
            .iter()
            .copied()
            .filter(|key| !obj.contains_key(*key))
            .collect();
        if !missing.is_empty() {
            let item_id = obj
                .get("item_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("record[{index}]"));
            bail!(
                "invalid translation payload at {}: {item_id} missing strict contract fields: {}",
                translation_path.display(),
                missing.join(", ")
            );
        }
    }
    Ok(())
}

/// `translation_loader.load_translated_pages` — load every page listed in the
/// manifest, preserving manifest order.
pub fn load_translated_pages(
    translations_dir: &Path,
    manifest_path: Option<&Path>,
) -> Result<BTreeMap<i32, Vec<Value>>> {
    let resolved_manifest_path = match manifest_path {
        Some(path) => path.to_path_buf(),
        None => translation_manifest_path(translations_dir),
    };
    if !resolved_manifest_path.exists() {
        bail!(
            "Translation manifest not found: {}",
            resolved_manifest_path.display()
        );
    }
    let translation_paths =
        load_translation_manifest_file(&resolved_manifest_path, Some(translations_dir))?;
    let mut translated_pages: BTreeMap<i32, Vec<Value>> = BTreeMap::new();
    for (page_idx, path) in &translation_paths {
        if !path.exists() {
            bail!(
                "Translation manifest entry points to missing file: {}",
                path.display()
            );
        }
        translated_pages.insert(*page_idx, load_translations(path, true)?);
    }
    if translated_pages.is_empty() {
        bail!(
            "No translation pages listed in {}",
            resolved_manifest_path.display()
        );
    }
    Ok(translated_pages)
}

/// `translation_loader.select_translated_pages` — range-select a loaded page
/// map. `end_page < 0` means "through the last listed page".
pub fn select_translated_pages(
    translated_pages: &BTreeMap<i32, Vec<Value>>,
    start_page: i64,
    end_page: i64,
) -> Result<BTreeMap<i32, Vec<Value>>> {
    let start = start_page.max(0) as i32;
    let stop = if end_page < 0 {
        *translated_pages.keys().next_back().unwrap_or(&0)
    } else {
        end_page as i32
    };
    let selected: BTreeMap<i32, Vec<Value>> = translated_pages
        .iter()
        .filter(|(page_idx, _)| **page_idx >= start && **page_idx <= stop)
        .map(|(page_idx, items)| (*page_idx, items.clone()))
        .collect();
    if selected.is_empty() {
        bail!("No translated pages selected in range {start}..{stop}");
    }
    Ok(selected)
}

/// Python `int(page.get("page_index"))` semantics: JSON integer, numeric
/// string, or truncated float.
fn as_page_index(value: Option<&Value>) -> Option<i32> {
    let n = match value? {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i
            } else {
                n.as_f64().map(|f| f as i64)?
            }
        }
        Value::String(s) => s.trim().parse::<i64>().ok()?,
        _ => return None,
    };
    n.try_into().ok()
}

/// Non-strict path resolution: canonicalize when the path exists, otherwise
/// normalize `.`/`..` lexically (mirrors Python `Path.resolve()` strict=False).
fn resolved_path(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|_| lexically_resolve(path))
}

fn lexically_resolve(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let unique = format!(
                "{label}-{}-{:?}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let dir = std::env::temp_dir().join(unique);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample_record(page: i32, block: i32) -> Value {
        serde_json::json!({
            "item_id": format!("p{page:03}-b{block:03}"),
            "page_idx": page,
            "block_kind": "text",
            "layout_role": "paragraph",
            "semantic_role": "body",
            "structure_role": "body",
            "policy_translate": true,
            "asset_id": "",
            "reading_order": block,
            "raw_block_type": "text",
            "normalized_sub_type": "body",
            "source_text": format!("source {page}-{block}"),
        })
    }

    fn write_manifest(translations_dir: &Path, pages: &[(i32, &str)]) -> PathBuf {
        let manifest = translation_manifest_path(translations_dir);
        let value = serde_json::json!({
            "schema": TRANSLATION_MANIFEST_SCHEMA,
            "schema_version": 1,
            "pages": pages
                .iter()
                .map(|(idx, path)| serde_json::json!({"page_index": idx, "path": path}))
                .collect::<Vec<_>>(),
        });
        fs::write(&manifest, serde_json::to_string_pretty(&value).unwrap()).unwrap();
        manifest
    }

    #[test]
    fn loads_manifest_and_pages() {
        let dir = TempDir::new("r1-load");
        let tdir = dir.path();
        let manifest = write_manifest(tdir, &[(0, "page-001.json"), (1, "page-002.json")]);
        fs::write(
            tdir.join("page-001.json"),
            serde_json::to_vec(&vec![sample_record(0, 1), sample_record(0, 2)]).unwrap(),
        )
        .unwrap();
        fs::write(
            tdir.join("page-002.json"),
            serde_json::to_vec(&vec![sample_record(1, 1)]).unwrap(),
        )
        .unwrap();

        let paths = load_translation_manifest(tdir).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[&0], tdir.join("page-001.json"));
        assert_eq!(paths[&1], tdir.join("page-002.json"));

        let pages = load_translated_pages(tdir, Some(&manifest)).unwrap();
        assert_eq!(pages.keys().copied().collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(pages[&0].len(), 2);
        assert_eq!(pages[&1].len(), 1);
    }

    #[test]
    fn selects_page_range() {
        let dir = TempDir::new("r1-select");
        let tdir = dir.path();
        write_manifest(tdir, &[(0, "p0.json"), (1, "p1.json"), (2, "p2.json")]);
        for i in 0..3 {
            fs::write(
                tdir.join(format!("p{i}.json")),
                serde_json::to_vec(&vec![sample_record(i, 1)]).unwrap(),
            )
            .unwrap();
        }
        let pages = load_translated_pages(tdir, None).unwrap();

        let all = select_translated_pages(&pages, 0, -1).unwrap();
        assert_eq!(all.len(), 3);

        let mid = select_translated_pages(&pages, 1, 1).unwrap();
        assert_eq!(mid.keys().copied().collect::<Vec<_>>(), vec![1]);

        let err = select_translated_pages(&pages, 5, 9).unwrap_err();
        assert!(err
            .to_string()
            .contains("No translated pages selected in range"));
    }

    #[test]
    fn rejects_missing_manifest() {
        let dir = TempDir::new("r1-missing-manifest");
        let err = load_translated_pages(dir.path(), None).unwrap_err();
        assert!(err.to_string().contains("Translation manifest not found"));
    }

    #[test]
    fn rejects_escaping_path() {
        let dir = TempDir::new("r1-escape");
        let tdir = dir.path();
        write_manifest(tdir, &[(0, "../outside.json")]);
        let err = load_translation_manifest(tdir).unwrap_err();
        assert!(err.to_string().contains("escapes translations_dir"));
    }

    #[test]
    fn rejects_duplicate_page_index() {
        let dir = TempDir::new("r1-dup");
        let tdir = dir.path();
        let manifest = translation_manifest_path(tdir);
        fs::write(
            &manifest,
            serde_json::to_string(&serde_json::json!({
                "schema": TRANSLATION_MANIFEST_SCHEMA,
                "pages": [
                    {"page_index": 0, "path": "p0.json"},
                    {"page_index": 0, "path": "p0b.json"},
                ],
            }))
            .unwrap(),
        )
        .unwrap();
        let err = load_translation_manifest(tdir).unwrap_err();
        assert!(err
            .to_string()
            .contains("Duplicate translation manifest page index"));
    }

    #[test]
    fn rejects_absolute_payload_path() {
        let dir = TempDir::new("r1-abs");
        let tdir = dir.path();
        let manifest = translation_manifest_path(tdir);
        fs::write(
            &manifest,
            serde_json::to_string(&serde_json::json!({
                "schema": TRANSLATION_MANIFEST_SCHEMA,
                "pages": [{"page_index": 0, "path": "/etc/passwd"}],
            }))
            .unwrap(),
        )
        .unwrap();
        let err = load_translation_manifest(tdir).unwrap_err();
        assert!(err.to_string().contains("uses absolute payload path"));
    }

    #[test]
    fn validates_strict_contract() {
        let dir = TempDir::new("r1-contract");
        let tdir = dir.path();
        let record = serde_json::json!({"item_id": "x", "source_text": "ok"});
        fs::write(
            tdir.join("bad.json"),
            serde_json::to_vec(&vec![record]).unwrap(),
        )
        .unwrap();
        let err = load_translations(&tdir.join("bad.json"), true).unwrap_err();
        assert!(err.to_string().contains("missing strict contract fields"));

        // strict_contract=false skips the check.
        let loaded = load_translations(&tdir.join("bad.json"), false).unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn accepts_string_page_index() {
        let dir = TempDir::new("r1-str-idx");
        let tdir = dir.path();
        let manifest = translation_manifest_path(tdir);
        fs::write(
            &manifest,
            serde_json::to_string(&serde_json::json!({
                "schema": TRANSLATION_MANIFEST_SCHEMA,
                "pages": [{"page_index": "3", "path": "p3.json"}],
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            tdir.join("p3.json"),
            serde_json::to_vec(&vec![sample_record(3, 1)]).unwrap(),
        )
        .unwrap();
        let paths = load_translation_manifest(tdir).unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[&3], tdir.join("p3.json"));
    }
}

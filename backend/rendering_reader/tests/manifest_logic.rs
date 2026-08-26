// Ports of the pure-logic pieces of scripts/devtools/run_golden_flow.py:
// manifest.csv validation, sample-id resolution, item-id normalization, and the
// Typst place() bbox verification. The real rendered overlay only exists after a
// live render (needs API keys / typst), so the bbox verifier is exercised on
// synthetic typst snippets.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use common::repo_root;

// --- ported logic ------------------------------------------------------------

#[derive(Debug)]
struct ManifestRow {
    id: String,
    file: String,
}

const REQUIRED_COLUMNS: [&str; 6] = ["id", "file", "category", "pages", "focus", "notes"];

/// Parse a golden manifest CSV, validating the header and per-row column count.
fn parse_manifest(csv: &str) -> Result<Vec<ManifestRow>, String> {
    let mut lines = csv.lines();
    let header = lines.next().ok_or("empty csv")?;
    let header_cols: Vec<&str> = header.split(',').map(str::trim).collect();
    for req in REQUIRED_COLUMNS {
        if !header_cols.contains(&req) {
            return Err(format!("manifest missing column {req}"));
        }
    }
    let mut rows = Vec::new();
    for (i, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').map(str::trim).collect();
        if cols.len() < REQUIRED_COLUMNS.len() {
            return Err(format!(
                "manifest row {}: expected {} columns, got {}",
                i + 2,
                REQUIRED_COLUMNS.len(),
                cols.len()
            ));
        }
        rows.push(ManifestRow {
            id: cols[0].to_string(),
            file: cols[1].to_string(),
        });
    }
    Ok(rows)
}

/// Validate manifest rows: non-empty unique ids; if `file` is set it must exist
/// under `root` and be a `.pdf`.
fn validate_manifest(rows: &[ManifestRow], root: &Path) -> Result<(), String> {
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut errors: Vec<String> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let lineno = i + 2;
        if row.id.trim().is_empty() {
            errors.push(format!("manifest row {lineno}: missing id"));
        } else if seen.contains_key(&row.id) {
            errors.push(format!("manifest row {lineno}: duplicate id '{}'", row.id));
        }
        seen.insert(row.id.clone(), ());
        if !row.file.trim().is_empty() {
            let path = root.join(&row.file);
            if !path.exists() {
                errors.push(format!("manifest row {lineno}: file not found: {}", row.file));
            } else if path.extension().map(|e| e.to_string_lossy().to_lowercase())
                != Some("pdf".to_string())
            {
                errors.push(format!("manifest row {lineno}: file is not a PDF: {}", row.file));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "golden manifest check failed:\n- {}",
            errors.join("\n- ")
        ))
    }
}

/// Resolve a sample id to its PDF path (existing-file rows only).
fn resolve_sample(rows: &[ManifestRow], root: &Path, id: &str) -> Result<PathBuf, String> {
    let existing: Vec<&ManifestRow> = rows.iter().filter(|r| !r.file.trim().is_empty()).collect();
    let matches: Vec<&&ManifestRow> = existing.iter().filter(|r| r.id == id).collect();
    if matches.is_empty() {
        let known: Vec<&str> = existing.iter().map(|r| r.id.as_str()).collect();
        return Err(format!(
            "unknown golden sample id: {id}; known samples: {}",
            known.join(", ")
        ));
    }
    let path = root.join(&matches[0].file);
    if !path.exists() {
        return Err(format!("golden sample file does not exist: {}", path.display()));
    }
    path.canonicalize().map_err(|e| e.to_string())
}

/// `re.fullmatch(r"p(\d+)-b0*(\d+)")` → `p%03d-b%03d`; unchanged when no match.
fn normalize_id(value: &str) -> String {
    let Some(rest) = value.strip_prefix('p') else {
        return value.to_string();
    };
    let Some(dash) = rest.find("-b") else {
        return value.to_string();
    };
    let (p_part, tail) = rest.split_at(dash);
    let Some(b_part) = tail.strip_prefix("-b") else {
        return value.to_string();
    };
    if p_part.is_empty()
        || b_part.is_empty()
        || !p_part.bytes().all(|b| b.is_ascii_digit())
        || !b_part.bytes().all(|b| b.is_ascii_digit())
    {
        return value.to_string();
    }
    let (Ok(p), Ok(b)) = (p_part.parse::<i64>(), b_part.parse::<i64>()) else {
        return value.to_string();
    };
    format!("p{:03}-b{:03}", p, b)
}

/// Parse `place(top + left, dx: Xpt, dy: Ypt` from a typst snippet.
fn parse_place_xy(snippet: &str) -> Result<(f64, f64), String> {
    const MARKER: &str = "place(top + left, dx: ";
    let start = snippet.find(MARKER).ok_or("cannot parse Typst place()")?;
    let rest = &snippet[start + MARKER.len()..];
    let sep = rest.find("pt, dy: ").ok_or("cannot parse dx")?;
    let dx: f64 = rest[..sep]
        .trim()
        .parse()
        .map_err(|_| format!("bad dx: {:?}", &rest[..sep]))?;
    let tail = &rest[sep + "pt, dy: ".len()..];
    let end = tail.find("pt").ok_or("cannot parse dy")?;
    let dy: f64 = tail[..end]
        .trim()
        .parse()
        .map_err(|_| format!("bad dy: {:?}", &tail[..end]))?;
    Ok((dx, dy))
}

/// Find `item_<id>` in the typst overlay, parse its place(), and assert dx/dy
/// match expected within 0.01pt.
fn verify_typst_bbox(text: &str, item_id: &str, expected_xy: [f64; 2]) -> Result<(f64, f64), String> {
    let token = format!("item_{}", item_id.replace('-', "_"));
    let start = text
        .find(&token)
        .ok_or_else(|| format!("cannot find Typst block for item: {item_id}"))?;
    let end = (start + 1200).min(text.len());
    let (dx, dy) = parse_place_xy(&text[start..end])?;
    if (dx - expected_xy[0]).abs() > 0.01 || (dy - expected_xy[1]).abs() > 0.01 {
        return Err(format!(
            "Typst bbox mismatch for {item_id}: actual=({dx}, {dy}), expected=({}, {})",
            expected_xy[0], expected_xy[1]
        ));
    }
    Ok((dx, dy))
}

// --- tests -------------------------------------------------------------------

fn golden_root() -> PathBuf {
    repo_root()
        .join("resources")
        .join("samples")
        .join("golden-pdfs")
}

fn real_manifest() -> Vec<ManifestRow> {
    let csv = std::fs::read_to_string(golden_root().join("manifest.csv")).unwrap();
    parse_manifest(&csv).unwrap()
}

#[test]
fn test_check_manifest_valid() {
    let rows = real_manifest();
    assert_eq!(rows.len(), 6);
    validate_manifest(&rows, &golden_root()).expect("real manifest must validate");
}

#[test]
fn test_check_manifest_duplicate_id() {
    let csv = "id,file,category,pages,focus,notes\ndup,1.pdf,c,1,f,\ndup,2.pdf,c,1,f,\n";
    let rows = parse_manifest(csv).unwrap();
    let err = validate_manifest(&rows, &golden_root()).unwrap_err();
    assert!(err.contains("duplicate id 'dup'"), "got: {err}");
}

#[test]
fn test_check_manifest_missing_columns() {
    let csv = "id,file,category\na,1.pdf,c\n";
    let err = parse_manifest(csv).unwrap_err();
    assert!(err.contains("missing column"), "got: {err}");
}

#[test]
fn test_check_manifest_missing_id() {
    let csv = "id,file,category,pages,focus,notes\n,1.pdf,c,1,f,\n";
    let rows = parse_manifest(csv).unwrap();
    let err = validate_manifest(&rows, &golden_root()).unwrap_err();
    assert!(err.contains("missing id"), "got: {err}");
}

#[test]
fn test_check_manifest_file_not_found() {
    let csv = "id,file,category,pages,focus,notes\nx,does-not-exist.pdf,c,1,f,\n";
    let rows = parse_manifest(csv).unwrap();
    let err = validate_manifest(&rows, &golden_root()).unwrap_err();
    assert!(err.contains("file not found"), "got: {err}");
}

#[test]
fn test_check_manifest_not_pdf() {
    let csv = "id,file,category,pages,focus,notes\nx,manifest.csv,c,1,f,\n";
    let rows = parse_manifest(csv).unwrap();
    let err = validate_manifest(&rows, &golden_root()).unwrap_err();
    assert!(err.contains("not a PDF"), "got: {err}");
}

#[test]
fn test_sample_pdf_resolves_existing() {
    let rows = real_manifest();
    let path = resolve_sample(&rows, &golden_root(), "editable-paper-formula").unwrap();
    assert_eq!(path.file_name().unwrap(), "1.pdf");
    let path = resolve_sample(&rows, &golden_root(), "pseudo-editable").unwrap();
    assert_eq!(path.file_name().unwrap(), "2.pdf");
}

#[test]
fn test_sample_pdf_unknown_id_errors() {
    let rows = real_manifest();
    let err = resolve_sample(&rows, &golden_root(), "nope").unwrap_err();
    assert!(err.contains("unknown golden sample id"), "got: {err}");
    assert!(err.contains("editable-paper-formula"), "should list known ids: {err}");
}

#[test]
fn test_normalize_id() {
    assert_eq!(normalize_id("p1-b013"), "p001-b013");
    assert_eq!(normalize_id("p001-b3"), "p001-b003");
    assert_eq!(normalize_id("p12-b0007"), "p012-b007");
    assert_eq!(normalize_id("not-an-id"), "not-an-id");
    assert_eq!(normalize_id("p1-x3"), "p1-x3");
}

#[test]
fn test_verify_typst_bbox_parse_and_compare() {
    let text = "item_p001_b013: place(top + left, dx: 12.34pt, dy: 56.78pt, content)";
    let (dx, dy) = verify_typst_bbox(text, "p001-b013", [12.34, 56.78]).expect("in tolerance");
    assert_eq!((dx, dy), (12.34, 56.78));

    let err = verify_typst_bbox(text, "p001-b013", [12.40, 56.78]).unwrap_err();
    assert!(err.contains("mismatch"), "got: {err}");

    let err = verify_typst_bbox("no token here", "p001-b013", [0.0, 0.0]).unwrap_err();
    assert!(err.contains("cannot find"), "got: {err}");

    let text_missing_place = "item_p001_b013 content";
    let err = verify_typst_bbox(text_missing_place, "p001-b013", [0.0, 0.0]).unwrap_err();
    assert!(err.contains("place()"), "got: {err}");
}

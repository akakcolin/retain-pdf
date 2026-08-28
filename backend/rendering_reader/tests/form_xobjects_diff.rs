//! Phase B2-Inc2 pdf_structure_profile form-xobject differential: mupdf-rs
//! `PdfDocument::page_form_xobjects` == fitz `page.get_xobjects()` facts
//! (`[name, xref, bbox]`), recorded by
//! `rendering_writer/differential/gen_pdf_structure_corpus.py` on deterministic
//! synthetic PDFs (plain text, path rect, image, mixed, a single-level Form
//! XObject, 90/180-degree-rotated pages, a cropbox offset, and an item-hit
//! page).
//!
//! Both sides report the `/Resources/XObject` entries that carry a `/BBox` —
//! fitz `get_xobjects()` as `(xref, name, type, bbox)` Do-instances, the native
//! scan as one entry per named form. The corpus only pins single-level forms
//! placed with an identity matrix, where the two agree exactly; nested
//! form-invokes-form instances are a documented divergence (the shim docstring
//! carries the rationale) and are not replayed here. Counts are exact, names and
//! xrefs are exact, bboxes within 0.01 pt.

use std::collections::HashMap;
use std::path::Path;

use base64::Engine;
use mupdf::Document;
use rendering_reader::{PdfDocument, PdfError};
use serde::Deserialize;

const TOL: f64 = 0.01;

#[derive(Deserialize)]
struct FormCorpus {
    schema: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    pdf_b64: String,
    #[allow(dead_code)]
    items_by_page: HashMap<String, serde_json::Value>,
    pages: HashMap<String, PageFacts>,
}

#[derive(Deserialize)]
struct PageFacts {
    #[allow(dead_code)]
    page_width_pt: f64,
    #[allow(dead_code)]
    page_height_pt: f64,
    /// `[[name, xref, [x0, y0, x1, y1]], ...]` from fitz `get_xobjects()`.
    form_xobjects_primitive: Vec<FormEntry>,
}

struct FormEntry {
    name: String,
    xref: i64,
    bbox: [f64; 4],
}

impl<'de> Deserialize<'de> for FormEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw: Vec<serde_json::Value> = Vec::deserialize(deserializer)?;
        if raw.len() != 3 {
            return Err(serde::de::Error::custom("form entry must be [name, xref, bbox]"));
        }
        let name = raw[0].as_str().unwrap_or_default().to_string();
        let xref = raw[1].as_i64().unwrap_or(0);
        let bbox_list = raw[2]
            .as_array()
            .ok_or_else(|| serde::de::Error::custom("bbox must be an array"))?;
        if bbox_list.len() != 4 {
            return Err(serde::de::Error::custom("bbox must have 4 coords"));
        }
        let mut bbox = [0.0f64; 4];
        for (slot, value) in bbox.iter_mut().zip(bbox_list) {
            *slot = value.as_f64().unwrap_or(0.0);
        }
        Ok(FormEntry { name, xref, bbox })
    }
}

fn open_all<T: PdfDocument>(path: &Path) -> Result<T, PdfError> {
    T::open(path)
}

#[test]
fn form_xobjects_replay_matches_fitz() {
    let corpus: FormCorpus =
        serde_json::from_str(include_str!("pdf_structure_corpus.json")).expect("parse corpus");
    assert_eq!(corpus.schema, "retainpdf_pdf_structure_corpus_v1");
    assert!(!corpus.cases.is_empty());

    let dir = std::env::temp_dir().join(format!("rps-psp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");

    for case in &corpus.cases {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(case.pdf_b64.as_bytes())
            .unwrap_or_else(|e| panic!("{} decode: {e}", case.name));
        let path = dir.join(format!("{}.pdf", case.name));
        std::fs::write(&path, bytes).expect("write case pdf");
        let doc = open_all::<Document>(&path).unwrap_or_else(|e| panic!("{} open: {e}", case.name));

        for (idx_str, expected) in &case.pages {
            let idx: i64 = idx_str.parse().unwrap();
            let label = format!("{} p{idx}", case.name);
            let actual = doc.page_form_xobjects(idx);
            assert_eq!(
                actual.len(),
                expected.form_xobjects_primitive.len(),
                "{label}: form count"
            );
            for (n, (info, exp)) in actual
                .iter()
                .zip(expected.form_xobjects_primitive.iter())
                .enumerate()
            {
                assert_eq!(info.name, exp.name, "{label}: form {n} name");
                assert_eq!(info.xref, exp.xref, "{label}: form {n} xref");
                let bbox = [info.bbox.x0, info.bbox.y0, info.bbox.x1, info.bbox.y1];
                for (i, (a, e)) in bbox.iter().zip(exp.bbox.iter()).enumerate() {
                    assert!(
                        (a - e).abs() <= TOL,
                        "{label}: form {n} coord {i} actual={a}, expected={e}, tol={TOL}"
                    );
                }
            }
        }
    }
}

//! `metadata.py::copy_toc` / `copy_toc_for_page_map` — flatten the source
//! outline tree, remap/filter pages, normalize levels, and re-write the
//! outlines via `PdfDocument::set_outlines`.
//!
//! fitz `get_toc()` returns 1-based page numbers and a flat depth-first list;
//! mupdf-rs `Document::outlines()` returns a tree with 0-based destinations.
//! The flatten+remap reproduces the same filtered entries, then the flat list
//! is re-nested by level for `set_outlines`. Destination kind is documented
//! as `/Fit` (fitz writes `/XYZ null null null`); the outline count/titles are
//! what the corpus asserts. `copy_toc` supports the `start_page`/`end_page`
//! range variant (`end_page < 0` = unbounded); `copy_toc_for_page_map` maps
//! source page indices through a `RenderPageMap` (target page = output slot + 1).

use std::collections::HashMap;

use mupdf::pdf::PdfDocument;
use mupdf::document::Location;
use mupdf::link::LinkDestination;
use mupdf::{DestinationKind, Document, Error, Outline};

struct TocEntry {
    level: u8,
    title: String,
    page: u32,
}

fn flatten_outline(outline: &Outline, level: u8, out: &mut Vec<TocEntry>) {
    if let Some(dest) = &outline.dest {
        out.push(TocEntry {
            level,
            title: outline.title.clone(),
            page: dest.loc.page_number,
        });
    }
    for child in &outline.down {
        flatten_outline(child, level + 1, out);
    }
}

/// `metadata.py::_normalize_toc_levels` — first entry is level 1; later levels
/// clamp to `[1, previous + 1]`.
fn normalize_toc_levels(mut entries: Vec<TocEntry>) -> Vec<TocEntry> {
    let mut previous = 0u8;
    for (i, entry) in entries.iter_mut().enumerate() {
        if i == 0 {
            entry.level = 1;
        } else {
            entry.level = entry.level.clamp(1, previous + 1);
        }
        previous = entry.level;
    }
    entries
}

/// Build a nested `Outline` tree from a flat, level-normalized entry list.
fn build_outline_tree(entries: &[TocEntry]) -> Vec<Outline> {
    struct Node {
        level: u8,
        title: String,
        page: u32,
        children: Vec<usize>,
    }
    let mut nodes: Vec<Node> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for entry in entries {
        let idx = nodes.len();
        nodes.push(Node {
            level: entry.level,
            title: entry.title.clone(),
            page: entry.page,
            children: Vec::new(),
        });
        while let Some(&parent) = stack.last() {
            if nodes[parent].level >= entry.level {
                stack.pop();
            } else {
                break;
            }
        }
        if let Some(&parent) = stack.last() {
            nodes[parent].children.push(idx);
        }
        stack.push(idx);
    }
    fn convert(nodes: &[Node], idx: usize) -> Outline {
        let node = &nodes[idx];
        Outline {
            title: node.title.clone(),
            uri: None,
            dest: Some(LinkDestination {
                loc: Location {
                    chapter: 0,
                    page_in_chapter: node.page,
                    page_number: node.page,
                },
                kind: DestinationKind::Fit,
            }),
            down: node.children.iter().map(|&c| convert(nodes, c)).collect(),
        }
    }
    let roots: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.level == 1)
        .map(|(i, _)| i)
        .collect();
    roots.iter().map(|&r| convert(&nodes, r)).collect()
}

/// `copy_toc(source_doc, target_doc)` with a `start_page`/`end_page` range.
/// `end_page < 0` is unbounded (Python `end_page=None`); the default call is
/// `copy_toc(source, target, 0, -1)`.
pub fn copy_toc(
    source: &Document,
    target: &mut PdfDocument,
    start_page: i32,
    end_page: i32,
) -> Result<usize, Error> {
    let outlines = source.outlines()?;
    if outlines.is_empty() {
        return Ok(0);
    }
    let last_source_page = source.page_count()? - 1;
    let target_page_count = target.page_count()?;
    let first = start_page.max(0);
    let last = if end_page < 0 {
        last_source_page
    } else {
        end_page.min(last_source_page)
    };
    if first > last {
        return Ok(0);
    }

    let mut flat: Vec<TocEntry> = Vec::new();
    for root in &outlines {
        flatten_outline(root, 1, &mut flat);
    }

    let mut remapped: Vec<TocEntry> = Vec::new();
    for entry in flat {
        let source_page = entry.page as i32;
        if !(first <= source_page && source_page <= last) {
            continue;
        }
        let target_page = source_page - first + 1;
        if !(1 <= target_page && target_page <= target_page_count) {
            continue;
        }
        remapped.push(TocEntry {
            level: entry.level,
            title: entry.title,
            page: (source_page - first) as u32,
        });
    }

    let remapped = normalize_toc_levels(remapped);
    if remapped.is_empty() {
        return Ok(0);
    }
    let tree = build_outline_tree(&remapped);
    target.set_outlines(&tree)?;
    Ok(remapped.len())
}

/// `copy_toc_for_page_map(source_doc, target_doc, page_map)` — target page =
/// output slot + 1 over `source_page_indices` (fitz `set_toc` 1-based).
pub fn copy_toc_for_page_map(
    source: &Document,
    target: &mut PdfDocument,
    source_page_indices: &[u32],
) -> Result<usize, Error> {
    let outlines = source.outlines()?;
    if outlines.is_empty() {
        return Ok(0);
    }
    let source_page_count = source.page_count()?;
    let mut target_pages_by_source: HashMap<u32, u32> = HashMap::new();
    for (output_idx, source_idx) in source_page_indices.iter().enumerate() {
        let source_page = *source_idx as i32;
        if 0 <= source_page && source_page < source_page_count {
            target_pages_by_source.insert(*source_idx, (output_idx as u32) + 1);
        }
    }
    if target_pages_by_source.is_empty() {
        return Ok(0);
    }
    let target_page_count = target.page_count()?;

    let mut flat: Vec<TocEntry> = Vec::new();
    for root in &outlines {
        flatten_outline(root, 1, &mut flat);
    }

    let mut remapped: Vec<TocEntry> = Vec::new();
    for entry in flat {
        let target_page = match target_pages_by_source.get(&entry.page) {
            Some(&t) => t,
            None => continue,
        };
        if !(1 <= target_page && (target_page as i32) <= target_page_count) {
            continue;
        }
        remapped.push(TocEntry {
            level: entry.level,
            title: entry.title,
            page: target_page - 1,
        });
    }

    let remapped = normalize_toc_levels(remapped);
    if remapped.is_empty() {
        return Ok(0);
    }
    let tree = build_outline_tree(&remapped);
    target.set_outlines(&tree)?;
    Ok(remapped.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_levels_matches_python() {
        let mk = |level: u8, title: &str| TocEntry { level, title: title.to_string(), page: 1 };
        let cases = normalize_toc_levels(vec![mk(3, "a"), mk(5, "b"), mk(1, "c")]);
        let levels: Vec<u8> = cases.iter().map(|e| e.level).collect();
        assert_eq!(levels, vec![1, 2, 1]);
    }

    #[test]
    fn tree_preserves_flat_order() {
        let entries = vec![
            TocEntry { level: 1, title: "one".into(), page: 0 },
            TocEntry { level: 2, title: "one-a".into(), page: 0 },
            TocEntry { level: 1, title: "two".into(), page: 1 },
        ];
        let tree = build_outline_tree(&entries);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].title, "one");
        assert_eq!(tree[0].down.len(), 1);
        assert_eq!(tree[0].down[0].title, "one-a");
        assert_eq!(tree[1].title, "two");
        assert!(tree[1].down.is_empty());
    }
}

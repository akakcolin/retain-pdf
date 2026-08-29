//! Typst stage: emit the whole-book background overlay source from the
//! emitter-side `RenderPageSpec` DTOs, then compile it with the `typst` CLI
//! (mirror of `compiler.py::compile_typst_render_pages_pdf`, including the
//! `--root` = common root of [.typ, .pdf, background pdf]).

use std::env;
use std::path::{Path, PathBuf};

use rendering_output::compile::compile_typst_source;
use rendering_output::compile::resolve_typst_bin;
use rendering_output::compile::CompileContext;
use rendering_output::compile::DEFAULT_COMPILE_TIMEOUT_SECONDS;
use rendering_output::emitter::build_typst_source_from_page_specs;

use crate::bundle::RenderBundle;

pub const COMPILE_STEM: &str = "book-background-overlay";
pub const COMPILE_PHASE: &str = "render_pages";

pub fn run_typst(bundle: &RenderBundle, cleaned_bg_path: &Path) -> anyhow::Result<PathBuf> {
    let work_dir = bundle.work_dir.as_path();
    let emitter_specs = bundle.emitter_page_specs()?;
    let source = build_typst_source_from_page_specs(
        cleaned_bg_path,
        &emitter_specs,
        work_dir,
        &bundle.font_family,
    );

    let ctx = compile_context();
    let typ_path = work_dir.join(format!("{COMPILE_STEM}.typ"));
    let pdf_path = work_dir.join(format!("{COMPILE_STEM}.pdf"));
    let root = common_root(&[typ_path.as_path(), pdf_path.as_path(), cleaned_bg_path]);

    let out = compile_typst_source(
        &source,
        COMPILE_STEM,
        COMPILE_PHASE,
        work_dir,
        Some(&root),
        &[],
        &ctx,
        serde_json::Map::new(),
    )
    .map_err(|e| anyhow::anyhow!("typst compile: {}", e.message()))?;
    Ok(out)
}

/// `compiler.py` CompileContext: `TYPST_BIN` (else which/snap default via
/// `resolve_typst_bin`), the backend fonts dir when it exists, and
/// `RETAIN_PDF_TYPST_FONT_DIRS`.
pub(crate) fn compile_context() -> CompileContext {
    CompileContext {
        typ_bin: resolve_typst_bin(env::var("TYPST_BIN").ok().as_deref(), None),
        timeout_seconds: DEFAULT_COMPILE_TIMEOUT_SECONDS,
        backends_fonts_dir: resolve_backend_fonts_dir(),
        env_font_dirs: env::var("RETAIN_PDF_TYPST_FONT_DIRS").ok(),
    }
}

/// `fonts.BACKEND_FONTS_DIR` (= `backend/fonts`) resolved from env override,
/// the render_rs binary location (`.../backend/target/<profile>/render_rs`),
/// then CWD candidates.
fn resolve_backend_fonts_dir() -> Option<PathBuf> {
    if let Ok(dir) = env::var("RETAIN_PDF_BACKEND_FONTS_DIR") {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return Some(path);
        }
    }
    if let Ok(exe) = env::current_exe() {
        let candidate = exe
            .parent()?
            .parent()?
            .parent()?
            .join("fonts");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    for candidate in ["backend/fonts", "fonts", "../fonts"] {
        let path = PathBuf::from(candidate);
        if path.is_dir() {
            return Some(path);
        }
    }
    None
}

/// `os.path.commonpath` over absolute paths (`compiler.py::_resolved_common_root`).
fn common_root(paths: &[&Path]) -> PathBuf {
    let component_lists: Vec<Vec<String>> = paths
        .iter()
        .map(|p| {
            p.components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect()
        })
        .collect();
    let first = &component_lists[0];
    let mut prefix: Vec<String> = Vec::new();
    'outer: for i in 0..first.len() {
        for other in component_lists.iter().skip(1) {
            if other.get(i).map(|c| c != &first[i]).unwrap_or(true) {
                break 'outer;
            }
        }
        prefix.push(first[i].clone());
    }
    let mut root = PathBuf::from("/");
    for component in prefix {
        root.push(component);
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_root_shared_parent() {
        let root = common_root(&[
            Path::new("/tmp/work/book-background-overlay.typ"),
            Path::new("/tmp/work/book-background-overlay.pdf"),
            Path::new("/tmp/work/book-background-cleaned.pdf"),
        ]);
        assert_eq!(root, PathBuf::from("/tmp/work"));
    }

    #[test]
    fn common_root_none_shared() {
        let root = common_root(&[
            Path::new("/a/out.typ"),
            Path::new("/b/out.pdf"),
        ]);
        assert_eq!(root, PathBuf::from("/"));
    }
}

//! `render_rs` — Rust orchestration worker entry point. `--spec` runs the
//! render flow mirroring `render_only.py` (prints the `output pdf`/`source
//! pdf`/`translations dir` labels, non-zero exit on error); `--extract-text-layer`
//! runs the skip-OCR text-layer extraction mirroring `run_extract_text_layer.py`;
//! `--normalize-ocr` runs the OCR payload normalization (C5-N2, mineru provider);
//! `--dump-bundle` is a hidden
//! C3-N11 differential hook. `--render-page-jpeg` / `--repair-pdf` /
//! `--side-by-side` are the native replacements for the last three fitz-backed
//! derived-artifact paths (see `derived_artifacts.rs`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rendering_orchestrator::derived_artifacts;
use rendering_orchestrator::run;

const USAGE: &str = "usage: render_rs --spec <stage-spec.json> [--dump-bundle <path>] [--extract-text-layer] [--normalize-ocr]
       render_rs --render-page-jpeg <input> <output> <page_index> <width_px> <dpi> <quality>
       render_rs --repair-pdf <input> <output> <max_output_bytes> <max_pages>
       render_rs --side-by-side <source> <translated> <output>";

enum Mode {
    Spec {
        spec: PathBuf,
        extract_text_layer: bool,
        normalize_ocr: bool,
        dump_bundle: Option<PathBuf>,
    },
    RenderPageJpeg {
        input: PathBuf,
        output: PathBuf,
        page_index: i64,
        width_px: u32,
        dpi: u32,
        quality: u8,
    },
    RepairPdf {
        input: PathBuf,
        output: PathBuf,
        max_output_bytes: u64,
        max_pages: u32,
    },
    SideBySide {
        source: PathBuf,
        translated: PathBuf,
        output: PathBuf,
    },
}

fn next_value(iter: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    iter.next().ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_u32(value: &str, flag: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("{flag}: invalid integer: {value}"))
}

fn parse_u64(value: &str, flag: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{flag}: invalid integer: {value}"))
}

fn parse_args() -> Result<Mode, String> {
    let mut spec: Option<PathBuf> = None;
    let mut extract_text_layer = false;
    let mut normalize_ocr = false;
    let mut dump_bundle: Option<PathBuf> = None;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--spec" => {
                let value = next_value(&mut iter, "--spec")?;
                spec = Some(PathBuf::from(value));
            }
            "--extract-text-layer" => {
                extract_text_layer = true;
            }
            // C5-N2: native normalize_ocr worker (mineru provider).
            "--normalize-ocr" => {
                normalize_ocr = true;
            }
            // Hidden C3-N11 differential hook: build the native bundle and exit
            // without running the stages (the orchestrator_bundle_parity smoke
            // and the corpus producer consume this).
            "--dump-bundle" => {
                let value = next_value(&mut iter, "--dump-bundle")?;
                dump_bundle = Some(PathBuf::from(value));
            }
            "--render-page-jpeg" => {
                let input = PathBuf::from(next_value(&mut iter, "--render-page-jpeg input")?);
                let output = PathBuf::from(next_value(&mut iter, "--render-page-jpeg output")?);
                let page_index = next_value(&mut iter, "--render-page-jpeg page_index")?
                    .parse::<i64>()
                    .map_err(|_| "page_index: invalid integer".to_string())?;
                let width_px = parse_u32(
                    &next_value(&mut iter, "--render-page-jpeg width_px")?,
                    "width_px",
                )?;
                let dpi = parse_u32(&next_value(&mut iter, "--render-page-jpeg dpi")?, "dpi")?;
                let quality = u8::try_from(parse_u32(
                    &next_value(&mut iter, "--render-page-jpeg quality")?,
                    "quality",
                )?)
                .map_err(|_| "quality: out of range".to_string())?;
                return Ok(Mode::RenderPageJpeg {
                    input,
                    output,
                    page_index,
                    width_px,
                    dpi,
                    quality,
                });
            }
            "--repair-pdf" => {
                let input = PathBuf::from(next_value(&mut iter, "--repair-pdf input")?);
                let output = PathBuf::from(next_value(&mut iter, "--repair-pdf output")?);
                let max_output_bytes = parse_u64(
                    &next_value(&mut iter, "--repair-pdf max_output_bytes")?,
                    "max_output_bytes",
                )?;
                let max_pages = parse_u32(
                    &next_value(&mut iter, "--repair-pdf max_pages")?,
                    "max_pages",
                )?;
                return Ok(Mode::RepairPdf {
                    input,
                    output,
                    max_output_bytes,
                    max_pages,
                });
            }
            "--side-by-side" => {
                let source = PathBuf::from(next_value(&mut iter, "--side-by-side source")?);
                let translated = PathBuf::from(next_value(&mut iter, "--side-by-side translated")?);
                let output = PathBuf::from(next_value(&mut iter, "--side-by-side output")?);
                return Ok(Mode::SideBySide {
                    source,
                    translated,
                    output,
                });
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let spec = spec.ok_or_else(|| "--spec <path> is required".to_string())?;
    Ok(Mode::Spec {
        spec,
        extract_text_layer,
        normalize_ocr,
        dump_bundle,
    })
}

fn dump_bundle_only(spec_path: &Path, dump_path: &Path) -> anyhow::Result<()> {
    let spec = rendering_orchestrator::spec::RenderStageSpec::load(spec_path)?;
    let mut stats = rendering_orchestrator::native_stats::NativeStats::new();
    let value = rendering_orchestrator::bundle_builder::build_bundle(&spec, &mut stats)?.bundle;
    if let Some(parent) = dump_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(dump_path, serde_json::to_string_pretty(&value)?)?;
    println!("native render bundle written: {}", dump_path.display());
    Ok(())
}

fn report(result: anyhow::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("render_rs: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_spec_mode(
    spec_path: PathBuf,
    extract_text_layer: bool,
    normalize_ocr: bool,
    dump_bundle: Option<PathBuf>,
) -> ExitCode {
    if normalize_ocr {
        return report(
            rendering_orchestrator::normalize::normalize_ocr(Path::new(&spec_path)).map(|_| ()),
        );
    }
    if extract_text_layer {
        return report(
            rendering_orchestrator::extract_text_layer::extract_text_layer(Path::new(&spec_path))
                .map(|_| ()),
        );
    }
    if let Some(dump_path) = dump_bundle {
        return report(dump_bundle_only(Path::new(&spec_path), &dump_path));
    }
    match run(Path::new(&spec_path)) {
        Ok(outcome) => {
            println!("output pdf: {}", outcome.output_pdf.display());
            println!("source pdf: {}", outcome.source_pdf.display());
            println!("translations dir: {}", outcome.translations_dir.display());
            println!("summary: {}", outcome.summary_path.display());
            println!("native stats: {}", outcome.stats_path.display());
            println!("events jsonl: {}", outcome.events_jsonl.display());
            println!("render mode: {}", outcome.mode);
            println!("pages processed: {}", outcome.page_count);
            println!("total time: {:.2}s", outcome.elapsed_seconds);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("render_rs: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    match parse_args() {
        Err(message) => {
            eprintln!("render_rs: {message}");
            ExitCode::from(2)
        }
        Ok(Mode::Spec {
            spec,
            extract_text_layer,
            normalize_ocr,
            dump_bundle,
        }) => run_spec_mode(spec, extract_text_layer, normalize_ocr, dump_bundle),
        Ok(Mode::RenderPageJpeg {
            input,
            output,
            page_index,
            width_px,
            dpi,
            quality,
        }) => report(derived_artifacts::render_page_jpeg(
            &input, &output, page_index, width_px, dpi, quality,
        )),
        Ok(Mode::RepairPdf {
            input,
            output,
            max_output_bytes,
            max_pages,
        }) => report(derived_artifacts::repair_pdf(
            &input,
            &output,
            max_output_bytes,
            max_pages,
        )),
        Ok(Mode::SideBySide {
            source,
            translated,
            output,
        }) => report(derived_artifacts::side_by_side(&source, &translated, &output)),
    }
}

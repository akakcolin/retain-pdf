//! `render_rs` — Rust orchestration worker entry point. `--spec` runs the
//! render flow mirroring `render_only.py` (prints the `output pdf`/`source
//! pdf`/`translations dir` labels, non-zero exit on error); `--extract-text-layer`
//! runs the skip-OCR text-layer extraction mirroring `run_extract_text_layer.py`;
//! `--normalize-ocr` runs the OCR payload normalization (C5-N2, mineru provider);
//! `--dump-bundle` is a hidden
//! C3-N11 differential hook.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rendering_orchestrator::run;

fn parse_args() -> Result<(PathBuf, bool, bool, Option<PathBuf>), String> {
    let mut spec: Option<PathBuf> = None;
    let mut extract_text_layer = false;
    let mut normalize_ocr = false;
    let mut dump_bundle: Option<PathBuf> = None;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--spec" => {
                let value = iter.next().ok_or("--spec requires a value")?;
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
                let value = iter.next().ok_or("--dump-bundle requires a value")?;
                dump_bundle = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                println!(
                    "usage: render_rs --spec <stage-spec.json> [--dump-bundle <path>] [--extract-text-layer] [--normalize-ocr]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let spec = spec.ok_or_else(|| "--spec <path> is required".to_string())?;
    Ok((spec, extract_text_layer, normalize_ocr, dump_bundle))
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

fn main() -> ExitCode {
    match parse_args() {
        Err(message) => {
            eprintln!("render_rs: {message}");
            ExitCode::from(2)
        }
        Ok((spec_path, extract_text_layer, normalize_ocr, dump_bundle)) => {
            if normalize_ocr {
                return match rendering_orchestrator::normalize::normalize_ocr(
                    Path::new(&spec_path),
                ) {
                    Ok(_) => ExitCode::SUCCESS,
                    Err(error) => {
                        eprintln!("render_rs: {error:#}");
                        ExitCode::FAILURE
                    }
                };
            }
            if extract_text_layer {
                return match rendering_orchestrator::extract_text_layer::extract_text_layer(
                    Path::new(&spec_path),
                ) {
                    Ok(_) => ExitCode::SUCCESS,
                    Err(error) => {
                        eprintln!("render_rs: {error:#}");
                        ExitCode::FAILURE
                    }
                };
            }
            if let Some(dump_path) = dump_bundle {
                return match dump_bundle_only(Path::new(&spec_path), &dump_path) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(error) => {
                        eprintln!("render_rs: {error:#}");
                        ExitCode::FAILURE
                    }
                };
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
    }
}

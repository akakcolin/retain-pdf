//! `render_rs --spec <stage-spec.json>` — Rust orchestration entry point.
//! Prints the production stdout labels (`output pdf`, `source pdf`,
//! `translations dir`) mirroring `render_only.py`, non-zero exit on error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rendering_orchestrator::run;

fn parse_args() -> Result<PathBuf, String> {
    let mut spec: Option<PathBuf> = None;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--spec" => {
                let value = iter.next().ok_or("--spec requires a value")?;
                spec = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                println!("usage: render_rs --spec <stage-spec.json>");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    spec.ok_or_else(|| "--spec <path> is required".to_string())
}

fn main() -> ExitCode {
    match parse_args() {
        Err(message) => {
            eprintln!("render_rs: {message}");
            ExitCode::from(2)
        }
        Ok(spec_path) => match run(Path::new(&spec_path)) {
            Ok(outcome) => {
                println!("output pdf: {}", outcome.output_pdf.display());
                println!("source pdf: {}", outcome.source_pdf.display());
                println!("translations dir: {}", outcome.translations_dir.display());
                println!("summary: {}", outcome.summary_path.display());
                println!("render mode: {}", outcome.mode);
                println!("pages processed: {}", outcome.page_count);
                println!("total time: {:.2}s", outcome.elapsed_seconds);
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("render_rs: {error:#}");
                ExitCode::FAILURE
            }
        },
    }
}

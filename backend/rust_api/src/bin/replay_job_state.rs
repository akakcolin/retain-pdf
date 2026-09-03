//! replay_job_state — P2-2 event-sourcing replay audit command.
//!
//! Rebuilds a job's terminal state from its event stream (SQLite `events`
//! table primary, `logs/events.jsonl` fallback) and diffs it against the
//! stored `jobs` row plus the artifact registry. A divergence means the event
//! stream and the durable row disagree.
//!
//! Usage:
//!   replay_job_state --data-root <dir> --job-id <id> [--json]
//!
//! Exit codes:
//!   0 = consistent, or a documented known-gap (recovery path, no terminal event)
//!   1 = divergence found
//!   2 = usage / I/O / event-source error

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::json;

use rust_api::db::Db;
use rust_api::models::{
    JobArtifactRecord, JobEventRecord, JobFailureInfo, JobSnapshot, JobStatusKind, WorkflowKind,
};
use rust_api::services::jobs::replay::{
    diff_rebuilt_vs_stored, expected_terminal_artifacts, rebuild_terminal_state,
    RebuiltTerminalState, ReplayDiff,
};
use rust_api::storage_paths::resolve_data_path;

const EVENTS_FILE_NAME: &str = "events.jsonl";
const USAGE: &str = "usage: replay_job_state --data-root <dir> --job-id <id> [--json]";

struct Args {
    data_root: PathBuf,
    job_id: String,
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Consistent,
    KnownGap,
    Divergence,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let args = parse_args()?;
    let db = Db::new(
        args.data_root.join("db").join("jobs.db"),
        args.data_root.clone(),
    );

    let stored = db
        .get_job(&args.job_id)
        .map_err(|err| format!("load stored job {}: {err:#}", args.job_id))?;
    let stored_finished_at = stored.finished_at.clone();

    let (events, event_source) = load_events(&db, &args.data_root, &stored)?;
    if events.is_empty() {
        return Err(format!("no events found for job {}", args.job_id));
    }

    let rebuilt = rebuild_terminal_state(&events);
    let diffs = diff_rebuilt_vs_stored(&rebuilt, &stored);
    let verdict = classify(&diffs);

    let artifacts = db
        .list_job_artifact_entries(&args.job_id)
        .map_err(|err| format!("load artifact registry: {err:#}"))?;
    let notes = artifact_notes(&rebuilt, &stored.workflow, &artifacts);

    if args.json {
        print_json_report(
            &args,
            &stored,
            &rebuilt,
            &event_source,
            stored_finished_at,
            &diffs,
            &artifacts,
            &notes,
            verdict,
        );
    } else {
        print_text_report(
            &args,
            &stored,
            &rebuilt,
            &event_source,
            stored_finished_at,
            &diffs,
            &artifacts,
            &notes,
            verdict,
        );
    }

    Ok(ExitCode::from(exit_code(&verdict)))
}

fn exit_code(verdict: &Verdict) -> u8 {
    match verdict {
        Verdict::Divergence => 1,
        Verdict::Consistent | Verdict::KnownGap => 0,
    }
}

fn parse_args() -> Result<Args, String> {
    let mut data_root: Option<PathBuf> = None;
    let mut job_id: Option<String> = None;
    let mut json = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-root" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--data-root requires a value".to_string())?;
                data_root = Some(PathBuf::from(value));
            }
            "--job-id" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--job-id requires a value".to_string())?;
                job_id = Some(value);
            }
            "--json" => json = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let data_root = data_root.ok_or_else(|| "--data-root is required".to_string())?;
    let job_id = job_id.ok_or_else(|| "--job-id is required".to_string())?;
    Ok(Args {
        data_root,
        job_id,
        json,
    })
}

fn load_events(
    db: &Db,
    data_root: &Path,
    stored: &JobSnapshot,
) -> Result<(Vec<JobEventRecord>, String), String> {
    let from_db = db
        .list_all_job_events(&stored.job_id)
        .map_err(|err| format!("load events from db: {err:#}"))?;
    if !from_db.is_empty() {
        let count = from_db.len();
        return Ok((from_db, format!("db ({count} events)")));
    }

    let jsonl_path = jsonl_fallback_path(data_root, stored);
    let text = std::fs::read_to_string(&jsonl_path).map_err(|err| {
        format!(
            "no events in db and events.jsonl fallback {} unreadable: {err}",
            jsonl_path.display()
        )
    })?;
    let events = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|err| format!("parse events.jsonl line: {err}"))
        })
        .collect::<Result<Vec<JobEventRecord>, String>>()?;
    let count = events.len();
    Ok((events, format!("events.jsonl ({count} events)")))
}

fn jsonl_fallback_path(data_root: &Path, stored: &JobSnapshot) -> PathBuf {
    if let Some(root) = stored
        .artifacts
        .as_ref()
        .and_then(|item| item.job_root.as_ref())
    {
        if let Ok(dir) = resolve_data_path(data_root, root) {
            let path = dir.join("logs").join(EVENTS_FILE_NAME);
            if path.exists() {
                return path;
            }
        }
    }
    data_root
        .join("jobs")
        .join(&stored.job_id)
        .join("logs")
        .join(EVENTS_FILE_NAME)
}

fn classify(diffs: &[ReplayDiff]) -> Verdict {
    if diffs
        .iter()
        .any(|diff| !matches!(diff, ReplayDiff::KnownGapRecovered { .. }))
    {
        Verdict::Divergence
    } else if diffs
        .iter()
        .any(|diff| matches!(diff, ReplayDiff::KnownGapRecovered { .. }))
    {
        Verdict::KnownGap
    } else {
        Verdict::Consistent
    }
}

/// Registry-level terminal-artifact sanity check. Informational only: a Book
/// job that failed during render legitimately leaves translated_pdf ready, so
/// these notes never flip the exit code by themselves.
fn artifact_notes(
    rebuilt: &RebuiltTerminalState,
    workflow: &WorkflowKind,
    artifacts: &[JobArtifactRecord],
) -> Vec<String> {
    let expected = expected_terminal_artifacts(workflow);
    let ready: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|key| {
            artifacts
                .iter()
                .any(|item| item.artifact_key == *key && item.ready)
        })
        .collect();
    match rebuilt.status {
        JobStatusKind::Succeeded if ready.is_empty() => vec![format!(
            "Succeeded job has no ready expected terminal artifact; expected at least one of {}",
            expected.join(", ")
        )],
        JobStatusKind::Failed | JobStatusKind::Canceled if !ready.is_empty() => vec![format!(
            "{} job still has ready expected artifacts: {}",
            snake(&rebuilt.status),
            ready.join(", ")
        )],
        _ => Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)] // 报告输出参数即展示字段集合，收拢结构体收益低
fn print_text_report(
    args: &Args,
    stored: &JobSnapshot,
    rebuilt: &RebuiltTerminalState,
    event_source: &str,
    stored_finished_at: Option<String>,
    diffs: &[ReplayDiff],
    artifacts: &[JobArtifactRecord],
    notes: &[String],
    verdict: Verdict,
) {
    println!("job replay audit");
    println!("  job_id:          {}", args.job_id);
    println!("  workflow:        {}", workflow_name(&stored.workflow));
    println!("  event source:    {event_source}");
    println!(
        "  events:          {} (first {} last {})",
        rebuilt.event_count,
        rebuilt.first_ts.as_deref().unwrap_or("-"),
        rebuilt.last_ts.as_deref().unwrap_or("-")
    );
    println!(
        "  status:          rebuilt={} stored={}",
        snake(&rebuilt.status),
        snake(&stored.status)
    );
    println!(
        "  terminal stage:  rebuilt={} stored={}",
        display_opt(&rebuilt.terminal_stage),
        display_opt(&stored.stage)
    );
    println!("  stage history:   {}", rebuilt.stage_history.join(" -> "));
    println!(
        "  stage_detail:    rebuilt={} stored={}",
        display_opt(&rebuilt.stage_detail),
        display_opt(&stored.stage_detail)
    );
    println!(
        "  error:           rebuilt={} stored={}",
        quoted(rebuilt.error.as_deref().unwrap_or("")),
        quoted(stored.error.as_deref().unwrap_or(""))
    );
    println!(
        "  failure:         rebuilt={}",
        failure_summary(&rebuilt.failure)
    );
    println!(
        "  finished_at:     rebuilt={} stored={}",
        display_opt(&rebuilt.finished_at),
        display_opt(&stored_finished_at)
    );
    if !artifacts.is_empty() {
        println!("  artifacts:");
        for item in artifacts {
            println!(
                "    {:<28} ready={:<5} {}",
                item.artifact_key, item.ready, item.relative_path
            );
        }
    }
    for note in notes {
        println!("  [artifact] {note}");
    }
    if diffs.is_empty() {
        println!("  diff:           (none)");
    } else {
        for diff in diffs {
            println!("  diff:           {}", diff_line(diff));
        }
    }
    println!("  verdict:        {}", verdict_name(&verdict));
}

#[allow(clippy::too_many_arguments)]
fn print_json_report(
    args: &Args,
    stored: &JobSnapshot,
    rebuilt: &RebuiltTerminalState,
    event_source: &str,
    stored_finished_at: Option<String>,
    diffs: &[ReplayDiff],
    artifacts: &[JobArtifactRecord],
    notes: &[String],
    verdict: Verdict,
) {
    let report = json!({
        "job_id": args.job_id,
        "workflow": workflow_name(&stored.workflow),
        "status": { "stored": snake(&stored.status), "rebuilt": snake(&rebuilt.status) },
        "terminal_stage": { "stored": stored.stage, "rebuilt": rebuilt.terminal_stage },
        "stage_detail": { "stored": stored.stage_detail, "rebuilt": rebuilt.stage_detail },
        "error": { "stored": stored.error, "rebuilt": rebuilt.error },
        "failure": rebuilt.failure,
        "finished_at": { "stored": stored_finished_at, "rebuilt": rebuilt.finished_at },
        "stage_history": rebuilt.stage_history,
        "terminal_event_seen": rebuilt.terminal_event_seen,
        "events": {
            "source": event_source,
            "count": rebuilt.event_count,
            "first_ts": rebuilt.first_ts,
            "last_ts": rebuilt.last_ts,
        },
        "diffs": diffs.iter().map(diff_json).collect::<Vec<_>>(),
        "artifacts": artifacts.iter().map(artifact_json).collect::<Vec<_>>(),
        "artifact_notes": notes,
        "verdict": verdict_name(&verdict),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .unwrap_or_else(|err| format!("{{ \"error\": \"{err}\" }}"))
    );
}

fn diff_json(diff: &ReplayDiff) -> serde_json::Value {
    match diff {
        ReplayDiff::Status { rebuilt, stored } => json!({
            "field": "status", "rebuilt": snake(rebuilt), "stored": snake(stored), "kind": "divergence"
        }),
        ReplayDiff::TerminalStage { rebuilt, stored } => json!({
            "field": "terminal_stage", "rebuilt": rebuilt, "stored": stored, "kind": "divergence"
        }),
        ReplayDiff::StageDetail { rebuilt, stored } => json!({
            "field": "stage_detail", "rebuilt": rebuilt, "stored": stored, "kind": "divergence"
        }),
        ReplayDiff::Error { rebuilt, stored } => json!({
            "field": "error", "rebuilt": rebuilt, "stored": stored, "kind": "divergence"
        }),
        ReplayDiff::FailurePresence { rebuilt, stored } => json!({
            "field": "failure_presence", "rebuilt": rebuilt, "stored": stored, "kind": "divergence"
        }),
        ReplayDiff::FailureCategory { rebuilt, stored } => json!({
            "field": "failure_category", "rebuilt": rebuilt, "stored": stored, "kind": "divergence"
        }),
        ReplayDiff::FailureCode { rebuilt, stored } => json!({
            "field": "failure_code", "rebuilt": rebuilt, "stored": stored, "kind": "divergence"
        }),
        ReplayDiff::FailureSummary { rebuilt, stored } => json!({
            "field": "failure_summary", "rebuilt": rebuilt, "stored": stored, "kind": "divergence"
        }),
        ReplayDiff::KnownGapRecovered { stored_status } => json!({
            "field": "known_gap", "rebuilt": null, "stored": snake(stored_status), "kind": "known_gap"
        }),
    }
}

fn artifact_json(item: &JobArtifactRecord) -> serde_json::Value {
    json!({
        "artifact_key": item.artifact_key,
        "artifact_group": item.artifact_group,
        "ready": item.ready,
        "relative_path": item.relative_path,
        "size_bytes": item.size_bytes,
    })
}

fn diff_line(diff: &ReplayDiff) -> String {
    match diff {
        ReplayDiff::Status { rebuilt, stored } => {
            format!("status rebuilt={} stored={}", snake(rebuilt), snake(stored))
        }
        ReplayDiff::TerminalStage { rebuilt, stored } => format!(
            "terminal_stage rebuilt={} stored={}",
            display_opt(rebuilt),
            display_opt(stored)
        ),
        ReplayDiff::StageDetail { rebuilt, stored } => format!(
            "stage_detail rebuilt={} stored={}",
            display_opt(rebuilt),
            display_opt(stored)
        ),
        ReplayDiff::Error { rebuilt, stored } => format!(
            "error rebuilt={} stored={}",
            quoted(rebuilt),
            quoted(stored)
        ),
        ReplayDiff::FailurePresence { rebuilt, stored } => {
            format!("failure_presence rebuilt={rebuilt} stored={stored}")
        }
        ReplayDiff::FailureCategory { rebuilt, stored } => {
            format!("failure_category rebuilt={rebuilt} stored={stored}")
        }
        ReplayDiff::FailureCode { rebuilt, stored } => {
            format!("failure_code rebuilt={rebuilt} stored={stored}")
        }
        ReplayDiff::FailureSummary { rebuilt, stored } => format!(
            "failure_summary rebuilt={} stored={}",
            quoted(rebuilt),
            quoted(stored)
        ),
        ReplayDiff::KnownGapRecovered { stored_status } => format!(
            "known-gap stored={} has no job_terminal event (recovery path)",
            snake(stored_status)
        ),
    }
}

fn failure_summary(failure: &Option<JobFailureInfo>) -> String {
    match failure {
        Some(failure) => format!(
            "category={} code={} summary={}",
            failure.category,
            failure.failure_code_value(),
            quoted(&failure.summary)
        ),
        None => "(none)".to_string(),
    }
}

fn snake(status: &JobStatusKind) -> &'static str {
    match status {
        JobStatusKind::Queued => "queued",
        JobStatusKind::Running => "running",
        JobStatusKind::Succeeded => "succeeded",
        JobStatusKind::Failed => "failed",
        JobStatusKind::Canceled => "canceled",
    }
}

fn workflow_name(workflow: &WorkflowKind) -> &'static str {
    match workflow {
        WorkflowKind::Book => "book",
        WorkflowKind::Ocr => "ocr",
        WorkflowKind::Translate => "translate",
        WorkflowKind::Render => "render",
    }
}

fn verdict_name(verdict: &Verdict) -> &'static str {
    match verdict {
        Verdict::Consistent => "OK (consistent)",
        Verdict::KnownGap => "OK (known-gap)",
        Verdict::Divergence => "DIVERGENCE",
    }
}

fn display_opt(value: &Option<String>) -> String {
    value
        .as_deref()
        .map(quoted)
        .unwrap_or_else(|| "(none)".to_string())
}

fn quoted(value: &str) -> String {
    format!("{value:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_consistent_when_no_diffs() {
        assert_eq!(classify(&[]), Verdict::Consistent);
    }

    #[test]
    fn classify_known_gap_when_only_known_gap() {
        let diffs = vec![ReplayDiff::KnownGapRecovered {
            stored_status: JobStatusKind::Failed,
        }];
        assert_eq!(classify(&diffs), Verdict::KnownGap);
    }

    #[test]
    fn classify_divergence_when_any_hard_diff() {
        // A known-gap marker alongside a hard diff must still be DIVERGENCE.
        let diffs = vec![
            ReplayDiff::KnownGapRecovered {
                stored_status: JobStatusKind::Succeeded,
            },
            ReplayDiff::Status {
                rebuilt: JobStatusKind::Failed,
                stored: JobStatusKind::Succeeded,
            },
        ];
        assert_eq!(classify(&diffs), Verdict::Divergence);
    }

    #[test]
    fn exit_code_mapping() {
        assert_eq!(exit_code(&Verdict::Consistent), 0);
        assert_eq!(exit_code(&Verdict::KnownGap), 0);
        assert_eq!(exit_code(&Verdict::Divergence), 1);
    }
}

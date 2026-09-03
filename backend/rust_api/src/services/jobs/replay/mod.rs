//! Event-sourced job terminal-state replay (P2-2).
//!
//! Deterministic, pure reducer: fold an ordered [`JobEventRecord`] stream into
//! the terminal state it implies, then diff that projection against the stored
//! [`JobSnapshot`]. A divergence means the event stream and the durable row
//! disagree; the caller (`replay_job_state` bin / CI corpus gate) decides policy.
//!
//! Two documented asymmetries prevent false divergences:
//! - `started_at` cannot be rebuilt (first event ts is `job_created`'s append
//!   time = created_at, not started_at) — timestamps are presence-compared only.
//! - Failure data is normalized on BOTH sides via `with_formal_fields()` before
//!   comparison, because the `failure_classified` payload backfills formal
//!   fields on deserialize but the stored row does not.

use serde_json::Value;

use crate::models::domain::{
    JobFailureInfo, JobSnapshot, JobStageTiming, JobStatusKind, WorkflowKind,
};
use crate::models::JobEventRecord;
use crate::storage_paths::{
    ARTIFACT_KEY_NORMALIZED_DOCUMENT_JSON, ARTIFACT_KEY_PROVIDER_RESULT_JSON,
    ARTIFACT_KEY_TRANSLATED_PDF, ARTIFACT_KEY_TYPST_PDF,
};

/// Terminal state projected from an event stream (seq-ordered fold).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuiltTerminalState {
    pub status: JobStatusKind,
    pub terminal_stage: Option<String>,
    pub stage_detail: Option<String>,
    pub error: Option<String>,
    pub failure: Option<JobFailureInfo>,
    /// Last `job_terminal` event ts; presence-only comparison (see module doc).
    pub finished_at: Option<String>,
    pub stage_history: Vec<String>,
    pub terminal_event_seen: bool,
    pub event_count: usize,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayDiff {
    Status {
        rebuilt: JobStatusKind,
        stored: JobStatusKind,
    },
    TerminalStage {
        rebuilt: Option<String>,
        stored: Option<String>,
    },
    StageDetail {
        rebuilt: Option<String>,
        stored: Option<String>,
    },
    /// Normalized so `None` ≈ `Some("")`.
    Error {
        rebuilt: String,
        stored: String,
    },
    /// Exactly one side carries a classified failure.
    FailurePresence {
        rebuilt: bool,
        stored: bool,
    },
    FailureCategory {
        rebuilt: String,
        stored: String,
    },
    FailureCode {
        rebuilt: String,
        stored: String,
    },
    FailureSummary {
        rebuilt: String,
        stored: String,
    },
    /// Stored reached a terminal status but the stream carries no `job_terminal`
    /// event — the documented gap left by the raw-SQL recovery/cleanup paths
    /// (`db.recover_stale_running_job`, `cleanup_legacy_workflows`). This is a
    /// warning, not a hard divergence.
    KnownGapRecovered {
        stored_status: JobStatusKind,
    },
}

/// Pure fold over events. Input order is defensively re-sorted by `seq` so the
/// projection is deterministic regardless of the caller's ordering.
pub fn rebuild_terminal_state(events: &[JobEventRecord]) -> RebuiltTerminalState {
    let mut events = events.to_vec();
    events.sort_by_key(|event| event.seq);

    let mut status = JobStatusKind::Queued;
    let mut stage_detail: Option<String> = None;
    let mut error: Option<String> = None;
    let mut failure: Option<JobFailureInfo> = None;
    let mut finished_at: Option<String> = None;
    let mut terminal_event_seen = false;
    let mut terminal_event_stage: Option<String> = None;
    let mut last_transition_to_stage: Option<String> = None;
    let mut to_stage_sequence: Vec<String> = Vec::new();
    let mut last_stage_history: Option<Vec<JobStageTiming>> = None;
    let mut fallback_stage: Option<String> = None;

    for event in &events {
        match event.event.as_str() {
            "job_created" => {
                if let Some(value) = event.payload.as_ref().and_then(|p| p.get("status")) {
                    if let Some(parsed) = parse_status(value) {
                        status = parsed;
                    }
                }
            }
            "status_changed" => {
                if let Some(value) = event.payload.as_ref().and_then(|p| p.get("to")) {
                    if let Some(parsed) = parse_status(value) {
                        status = parsed;
                    }
                }
            }
            "job_terminal" => {
                terminal_event_seen = true;
                terminal_event_stage = event.stage.clone();
                if let Some(value) = event.payload.as_ref().and_then(|p| p.get("status")) {
                    if let Some(parsed) = parse_status(value) {
                        status = parsed;
                    }
                }
                finished_at = Some(event.ts.clone());
            }
            "stage_transition" => {
                if let Some(payload) = event.payload.as_ref() {
                    if let Some(value) = payload.get("to_stage").and_then(Value::as_str) {
                        last_transition_to_stage = Some(value.to_string());
                        to_stage_sequence.push(value.to_string());
                    }
                    let history = payload
                        .get("stage_history")
                        .or_else(|| payload.get("runtime").and_then(|r| r.get("stage_history")));
                    if let Some(history) = history {
                        if let Ok(timings) =
                            serde_json::from_value::<Vec<JobStageTiming>>(history.clone())
                        {
                            if !timings.is_empty() {
                                last_stage_history = Some(timings);
                            }
                        }
                    }
                }
            }
            "stage_updated" | "stage_progress" => {}
            "job_error" => {
                let message = event
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("error"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        if event.message.trim().is_empty() {
                            None
                        } else {
                            Some(event.message.clone())
                        }
                    });
                if let Some(message) = message {
                    error = Some(message);
                }
            }
            "failure_classified" => {
                if let Some(payload) = event.payload.as_ref() {
                    if let Some(parsed) = JobFailureInfo::from_json_value(payload) {
                        failure = Some(parsed);
                    }
                }
            }
            _ => {}
        }
        fallback_stage = event.stage.clone();
        if let Some(detail) = event.stage_detail.as_ref() {
            if !detail.trim().is_empty() {
                stage_detail = Some(detail.clone());
            }
        }
    }

    // Terminal stage: on the terminal path the `job_terminal` event's stage is
    // authoritative (completion.rs sets it before persisting); otherwise the
    // last `stage_transition` to_stage; fallback to the max-seq event stage.
    let terminal_stage = if terminal_event_seen {
        terminal_event_stage
            .or(last_transition_to_stage)
            .or(fallback_stage)
    } else {
        last_transition_to_stage.or(fallback_stage)
    };

    // Real flow accumulates stage_history in the runtime carried by the last
    // stage_transition; fallback to the composed to_stage sequence.
    let stage_history = last_stage_history
        .map(|timings| timings.into_iter().map(|timing| timing.stage).collect())
        .unwrap_or(to_stage_sequence);

    RebuiltTerminalState {
        status,
        terminal_stage,
        stage_detail,
        error,
        failure,
        finished_at,
        stage_history,
        terminal_event_seen,
        event_count: events.len(),
        first_ts: events.first().map(|event| event.ts.clone()),
        last_ts: events.last().map(|event| event.ts.clone()),
    }
}

pub fn diff_rebuilt_vs_stored(
    rebuilt: &RebuiltTerminalState,
    stored: &JobSnapshot,
) -> Vec<ReplayDiff> {
    let mut diffs = Vec::new();

    let stored_terminal = matches!(
        stored.status,
        JobStatusKind::Succeeded | JobStatusKind::Failed | JobStatusKind::Canceled
    );

    if stored_terminal && !rebuilt.terminal_event_seen {
        diffs.push(ReplayDiff::KnownGapRecovered {
            stored_status: stored.status.clone(),
        });
        return diffs;
    }

    if rebuilt.status != stored.status {
        diffs.push(ReplayDiff::Status {
            rebuilt: rebuilt.status.clone(),
            stored: stored.status.clone(),
        });
    }

    if rebuilt.terminal_stage != stored.stage {
        diffs.push(ReplayDiff::TerminalStage {
            rebuilt: rebuilt.terminal_stage.clone(),
            stored: stored.stage.clone(),
        });
    }

    // Only meaningful when the stored side carries a non-empty detail;
    // intermediate detail noise on the stream side is not a divergence.
    let stored_has_detail = stored
        .stage_detail
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    if stored_has_detail && rebuilt.stage_detail != stored.stage_detail {
        diffs.push(ReplayDiff::StageDetail {
            rebuilt: rebuilt.stage_detail.clone(),
            stored: stored.stage_detail.clone(),
        });
    }

    if normalized_error(&rebuilt.error) != normalized_error(&stored.error) {
        diffs.push(ReplayDiff::Error {
            rebuilt: normalized_error(&rebuilt.error).to_string(),
            stored: normalized_error(&stored.error).to_string(),
        });
    }

    // Normalize the stored failure the same way the failure_classified payload
    // is normalized on ingest; otherwise formal-field backfill causes a
    // permanent false divergence.
    let stored_failure = stored
        .failure
        .clone()
        .map(JobFailureInfo::with_formal_fields);
    match (&rebuilt.failure, &stored_failure) {
        (Some(rebuilt_failure), Some(stored_failure)) => {
            if rebuilt_failure.category != stored_failure.category {
                diffs.push(ReplayDiff::FailureCategory {
                    rebuilt: rebuilt_failure.category.clone(),
                    stored: stored_failure.category.clone(),
                });
            }
            if rebuilt_failure.failure_code_value() != stored_failure.failure_code_value() {
                diffs.push(ReplayDiff::FailureCode {
                    rebuilt: rebuilt_failure.failure_code_value().to_string(),
                    stored: stored_failure.failure_code_value().to_string(),
                });
            }
            if rebuilt_failure.summary != stored_failure.summary {
                diffs.push(ReplayDiff::FailureSummary {
                    rebuilt: rebuilt_failure.summary.clone(),
                    stored: stored_failure.summary.clone(),
                });
            }
        }
        (rebuilt_failure, stored_failure) => {
            let rebuilt_present = rebuilt_failure.is_some();
            let stored_present = stored_failure.is_some();
            if rebuilt_present != stored_present {
                diffs.push(ReplayDiff::FailurePresence {
                    rebuilt: rebuilt_present,
                    stored: stored_present,
                });
            }
        }
    }

    diffs
}

/// Terminal artifacts a Succeeded job of this workflow must expose in the
/// artifact registry. The bin treats "at least one ready" as satisfied.
pub fn expected_terminal_artifacts(workflow: &WorkflowKind) -> Vec<&'static str> {
    match workflow {
        WorkflowKind::Book | WorkflowKind::Translate => {
            vec![ARTIFACT_KEY_TRANSLATED_PDF, ARTIFACT_KEY_TYPST_PDF]
        }
        WorkflowKind::Ocr => vec![
            ARTIFACT_KEY_PROVIDER_RESULT_JSON,
            ARTIFACT_KEY_NORMALIZED_DOCUMENT_JSON,
        ],
        WorkflowKind::Render => vec![ARTIFACT_KEY_TYPST_PDF],
    }
}

fn parse_status(value: &Value) -> Option<JobStatusKind> {
    match value.as_str() {
        Some("queued") => Some(JobStatusKind::Queued),
        Some("running") => Some(JobStatusKind::Running),
        Some("succeeded") => Some(JobStatusKind::Succeeded),
        Some("failed") => Some(JobStatusKind::Failed),
        Some("canceled") => Some(JobStatusKind::Canceled),
        _ => None,
    }
}

fn normalized_error(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::{JobFailureInfo, JobSnapshot, JobStatusKind, WorkflowKind};
    use crate::models::request::CreateJobInput;
    use serde_json::{json, Value};

    fn status_name(status: &JobStatusKind) -> &'static str {
        match status {
            JobStatusKind::Queued => "queued",
            JobStatusKind::Running => "running",
            JobStatusKind::Succeeded => "succeeded",
            JobStatusKind::Failed => "failed",
            JobStatusKind::Canceled => "canceled",
        }
    }

    /// Locked contract surface: rebuilds must equal the hand-authored
    /// `expected.json` exactly (timestamps excluded — presence-only).
    fn semantic_projection(rebuilt: &RebuiltTerminalState) -> Value {
        json!({
            "status": status_name(&rebuilt.status),
            "terminal_stage": rebuilt.terminal_stage,
            "stage_detail": rebuilt.stage_detail,
            "error": rebuilt.error,
            "failure": rebuilt.failure.as_ref().map(|failure| json!({
                "stage": failure.stage,
                "category": failure.category,
                "failure_code": failure.failure_code_value(),
                "failure_category": failure.failure_category,
                "summary": failure.summary,
                "retryable": failure.retryable,
            })),
            "stage_history": rebuilt.stage_history,
            "terminal_event_seen": rebuilt.terminal_event_seen,
            "event_count": rebuilt.event_count,
        })
    }

    fn event(
        seq: i64,
        event_name: &str,
        payload: Option<Value>,
        stage: Option<&str>,
    ) -> JobEventRecord {
        JobEventRecord {
            job_id: "job-1".to_string(),
            seq,
            ts: format!("2026-09-01T00:00:{seq:02}Z"),
            created_at: String::new(),
            level: "info".to_string(),
            user_stage: None,
            lane: None,
            display_stage: None,
            stage: stage.map(str::to_string),
            substage: None,
            stage_detail: None,
            provider: None,
            provider_stage: None,
            event: event_name.to_string(),
            event_type: Some(event_name.to_string()),
            raw_event_type: Some(event_name.to_string()),
            raw: None,
            progress: None,
            message: String::new(),
            progress_current: None,
            progress_total: None,
            progress_unit: None,
            retry_count: None,
            elapsed_ms: None,
            payload,
        }
    }

    fn timing(stage: &str) -> Value {
        json!({
            "stage": stage,
            "detail": stage,
            "enter_at": "2026-09-01T00:00:00Z",
            "exit_at": null,
            "duration_ms": null,
            "terminal_status": null,
        })
    }

    fn transition(seq: i64, from_stage: &str, to_stage: &str, history: &[&str]) -> JobEventRecord {
        event(
            seq,
            "stage_transition",
            Some(json!({
                "from_stage": from_stage,
                "to_stage": to_stage,
                "stage_history": history.iter().map(|stage| timing(stage)).collect::<Vec<_>>(),
            })),
            Some(to_stage),
        )
    }

    fn status_event(seq: i64, to_status: &str, stage: &str) -> JobEventRecord {
        event(
            seq,
            "status_changed",
            Some(json!({ "from": "queued", "to": to_status })),
            Some(stage),
        )
    }

    #[test]
    fn replay_rebuilds_succeeded_terminal_state() {
        let stream = vec![
            event(
                0,
                "job_created",
                Some(json!({"workflow": "book", "status": "queued", "stage": "queued"})),
                Some("queued"),
            ),
            status_event(1, "running", "running"),
            transition(2, "queued", "running", &["queued", "running"]),
            transition(
                3,
                "running",
                "translating",
                &["queued", "running", "translating"],
            ),
            transition(
                4,
                "translating",
                "rendering",
                &["queued", "running", "translating", "rendering"],
            ),
            status_event(5, "succeeded", "rendering"),
            transition(
                6,
                "rendering",
                "finished",
                &["queued", "running", "translating", "rendering", "finished"],
            ),
            event(
                7,
                "job_terminal",
                Some(json!({"status": "succeeded"})),
                Some("finished"),
            ),
        ];

        let rebuilt = rebuild_terminal_state(&stream);
        assert_eq!(rebuilt.status, JobStatusKind::Succeeded);
        assert_eq!(rebuilt.terminal_stage.as_deref(), Some("finished"));
        assert_eq!(
            rebuilt.stage_history,
            vec!["queued", "running", "translating", "rendering", "finished"]
        );
        assert!(rebuilt.terminal_event_seen);
        assert_eq!(rebuilt.event_count, 8);
        assert_eq!(rebuilt.finished_at.as_deref(), Some("2026-09-01T00:00:07Z"));
    }

    #[test]
    fn replay_rebuilds_failure_from_failure_classified() {
        let stream = vec![
            event(
                0,
                "job_created",
                Some(json!({"workflow": "book", "status": "queued", "stage": "queued"})),
                Some("queued"),
            ),
            status_event(1, "running", "running"),
            transition(2, "queued", "running", &["queued", "running"]),
            transition(
                3,
                "running",
                "translating",
                &["queued", "running", "translating"],
            ),
            event(
                4,
                "job_error",
                Some(json!({"error": "ReadTimeout"})),
                Some("translating"),
            ),
            status_event(5, "failed", "failed"),
            event(
                6,
                "failure_classified",
                Some(json!({
                    "stage": "translating",
                    "category": "upstream_timeout",
                    "summary": "外部服务请求超时",
                    "retryable": true,
                })),
                Some("failed"),
            ),
            event(
                7,
                "job_terminal",
                Some(json!({"status": "failed", "failure_category": "upstream_timeout"})),
                Some("failed"),
            ),
            transition(
                8,
                "translating",
                "failed",
                &["queued", "running", "translating", "failed"],
            ),
        ];

        let rebuilt = rebuild_terminal_state(&stream);
        assert_eq!(rebuilt.status, JobStatusKind::Failed);
        assert_eq!(rebuilt.terminal_stage.as_deref(), Some("failed"));
        assert_eq!(rebuilt.error.as_deref(), Some("ReadTimeout"));
        let failure = rebuilt.failure.expect("failure_classified payload");
        assert_eq!(failure.category, "upstream_timeout");
        assert_eq!(failure.failure_code_value(), "upstream_timeout");
        assert_eq!(failure.summary, "外部服务请求超时");
        assert_eq!(failure.failed_stage_value(), "translating");
        assert_eq!(failure.failure_category.as_deref(), Some("timeout"));
    }

    #[test]
    fn replay_stage_history_falls_back_to_to_stage_sequence() {
        let stream = vec![
            event(
                0,
                "job_created",
                Some(json!({"workflow": "ocr", "status": "queued", "stage": "queued"})),
                Some("queued"),
            ),
            status_event(1, "running", "running"),
            // stage_transition without a stage_history payload → fallback path.
            event(
                2,
                "stage_transition",
                Some(json!({"from_stage": "queued", "to_stage": "running"})),
                Some("running"),
            ),
            event(
                3,
                "stage_transition",
                Some(json!({"from_stage": "running", "to_stage": "normalizing"})),
                Some("normalizing"),
            ),
        ];

        let rebuilt = rebuild_terminal_state(&stream);
        assert_eq!(rebuilt.status, JobStatusKind::Running);
        assert_eq!(rebuilt.terminal_stage.as_deref(), Some("normalizing"));
        assert_eq!(rebuilt.stage_history, vec!["running", "normalizing"]);
    }

    #[test]
    fn replay_known_gap_when_stored_terminal_stream_missing_terminal() {
        let stream = vec![
            event(
                0,
                "job_created",
                Some(json!({"workflow": "book", "status": "queued", "stage": "queued"})),
                Some("queued"),
            ),
            status_event(1, "running", "running"),
            transition(2, "queued", "running", &["queued", "running"]),
        ];

        let mut stored = JobSnapshot::new(
            "job-gap".to_string(),
            CreateJobInput::default(),
            vec!["python".to_string()],
        );
        stored.status = JobStatusKind::Failed;
        stored.stage = Some("failed".to_string());

        let rebuilt = rebuild_terminal_state(&stream);
        assert!(!rebuilt.terminal_event_seen);
        let diffs = diff_rebuilt_vs_stored(&rebuilt, &stored);
        assert_eq!(
            diffs,
            vec![ReplayDiff::KnownGapRecovered {
                stored_status: JobStatusKind::Failed
            }]
        );
    }

    #[test]
    fn replay_diff_detects_status_and_failure_drift() {
        let stream = vec![
            event(
                0,
                "job_created",
                Some(json!({"workflow": "book", "status": "queued", "stage": "queued"})),
                Some("queued"),
            ),
            status_event(1, "running", "running"),
            transition(2, "queued", "running", &["queued", "running"]),
            status_event(3, "succeeded", "finished"),
            event(
                4,
                "job_terminal",
                Some(json!({"status": "succeeded"})),
                Some("finished"),
            ),
        ];

        let mut stored = JobSnapshot::new(
            "job-drift".to_string(),
            CreateJobInput::default(),
            vec!["python".to_string()],
        );
        stored.status = JobStatusKind::Failed;
        stored.stage = Some("failed".to_string());
        stored.stage_detail = None;
        stored.error = Some("boom".to_string());
        stored.failure = Some(JobFailureInfo {
            stage: "render".to_string(),
            category: "render_failed".to_string(),
            code: None,
            failed_stage: None,
            failure_code: None,
            failure_category: None,
            provider_stage: None,
            provider_code: None,
            summary: "渲染失败".to_string(),
            root_cause: None,
            retryable: false,
            upstream_host: None,
            provider: None,
            suggestion: None,
            last_log_line: None,
            raw_excerpt: None,
            raw_error_excerpt: None,
            raw_diagnostic: None,
            ai_diagnostic: None,
        });

        let rebuilt = rebuild_terminal_state(&stream);
        let diffs = diff_rebuilt_vs_stored(&rebuilt, &stored);

        assert!(diffs.contains(&ReplayDiff::Status {
            rebuilt: JobStatusKind::Succeeded,
            stored: JobStatusKind::Failed,
        }));
        assert!(diffs.contains(&ReplayDiff::TerminalStage {
            rebuilt: Some("finished".to_string()),
            stored: Some("failed".to_string()),
        }));
        assert!(diffs.contains(&ReplayDiff::Error {
            rebuilt: String::new(),
            stored: "boom".to_string(),
        }));
        assert!(diffs.contains(&ReplayDiff::FailurePresence {
            rebuilt: false,
            stored: true,
        }));
    }

    #[test]
    fn replay_failure_comparison_normalizes_both_sides() {
        let stream = vec![
            event(
                0,
                "job_created",
                Some(json!({"workflow": "book", "status": "queued", "stage": "queued"})),
                Some("queued"),
            ),
            status_event(1, "failed", "failed"),
            event(
                2,
                "failure_classified",
                Some(json!({
                    "stage": "translation",
                    "category": "upstream_timeout",
                    "summary": "外部服务请求超时",
                    "retryable": true,
                })),
                Some("failed"),
            ),
            event(
                3,
                "job_terminal",
                Some(json!({"status": "failed"})),
                Some("failed"),
            ),
        ];

        // Stored failure is the legacy shape (no formal fields) — both sides must
        // normalize identically so no false divergence is reported.
        let mut stored = JobSnapshot::new(
            "job-fail-norm".to_string(),
            CreateJobInput::default(),
            vec!["python".to_string()],
        );
        stored.status = JobStatusKind::Failed;
        stored.stage = Some("failed".to_string());
        stored.stage_detail = None;
        stored.failure = Some(JobFailureInfo {
            stage: "translation".to_string(),
            category: "upstream_timeout".to_string(),
            code: None,
            failed_stage: None,
            failure_code: None,
            failure_category: None,
            provider_stage: None,
            provider_code: None,
            summary: "外部服务请求超时".to_string(),
            root_cause: None,
            retryable: true,
            upstream_host: None,
            provider: None,
            suggestion: None,
            last_log_line: None,
            raw_excerpt: None,
            raw_error_excerpt: None,
            raw_diagnostic: None,
            ai_diagnostic: None,
        });

        let rebuilt = rebuild_terminal_state(&stream);
        let diffs = diff_rebuilt_vs_stored(&rebuilt, &stored);
        assert_eq!(diffs, Vec::new());
    }

    #[test]
    fn replay_expected_terminal_artifacts_by_workflow() {
        assert_eq!(
            expected_terminal_artifacts(&WorkflowKind::Book),
            vec![ARTIFACT_KEY_TRANSLATED_PDF, ARTIFACT_KEY_TYPST_PDF]
        );
        assert_eq!(
            expected_terminal_artifacts(&WorkflowKind::Translate),
            vec![ARTIFACT_KEY_TRANSLATED_PDF, ARTIFACT_KEY_TYPST_PDF]
        );
        assert_eq!(
            expected_terminal_artifacts(&WorkflowKind::Ocr),
            vec![
                ARTIFACT_KEY_PROVIDER_RESULT_JSON,
                ARTIFACT_KEY_NORMALIZED_DOCUMENT_JSON
            ]
        );
        assert_eq!(
            expected_terminal_artifacts(&WorkflowKind::Render),
            vec![ARTIFACT_KEY_TYPST_PDF]
        );
    }

    #[test]
    fn replay_corpus_matches_golden() {
        let cases: [(&str, &str, &str); 2] = [
            (
                "book_succeeded",
                include_str!("../../../../tests/fixtures/job_replay/book_succeeded.events.jsonl"),
                include_str!("../../../../tests/fixtures/job_replay/book_succeeded.expected.json"),
            ),
            (
                "book_failed",
                include_str!("../../../../tests/fixtures/job_replay/book_failed.events.jsonl"),
                include_str!("../../../../tests/fixtures/job_replay/book_failed.expected.json"),
            ),
        ];

        for (name, events_jsonl, expected_json) in cases {
            let events: Vec<JobEventRecord> = events_jsonl
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| serde_json::from_str(line).expect("parse event"))
                .collect();
            let rebuilt = rebuild_terminal_state(&events);
            let projection = semantic_projection(&rebuilt);
            let expected: Value = serde_json::from_str(expected_json).expect("parse expected");
            assert_eq!(
                projection, expected,
                "replay corpus {name} diverged from golden"
            );
        }
    }
}

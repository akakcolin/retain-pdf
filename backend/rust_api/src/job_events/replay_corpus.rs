//! Committed corpus for the P2-2 event-sourced replay protocol.
//!
//! Synthetic snapshot sequences mirror the real job lifecycle (a MUTATING
//! snapshot, so `stage_history` accumulates the way production does), derived
//! through the production `derive_events`, then serialized to deterministic
//! `JobEventRecord` fixtures. The `#[ignore]` regen tests write the committed
//! `events.jsonl` files; the `replay` gate test rebuilds from those files and
//! compares against hand-authored `expected.json` (no circular verification).

use std::fs;
use std::path::Path;

use crate::models::api::JobEventRecord;
use crate::models::domain::{job_stage_str, JobFailureInfo, JobSnapshot, JobStage, JobStatusKind};
use crate::models::request::CreateJobInput;
use crate::services::jobs::replay::rebuild_terminal_state;

use super::derivation::{
    derive_events, normalize_user_stage, progress_unit_for_event, user_stage_for_event,
    PendingJobEvent,
};

fn new_deterministic_job(job_id: &str) -> JobSnapshot {
    let mut job = JobSnapshot::new(
        job_id.to_string(),
        CreateJobInput::default(),
        vec!["python".to_string()],
    );
    // `JobSnapshot::new` stamps real `now_iso()` into created_at/updated_at and
    // the first stage_history enter_at; pin them so a fresh regen is byte-identical.
    job.record.created_at = "2026-09-01T00:00:00Z".to_string();
    job.record.updated_at = "2026-09-01T00:00:00Z".to_string();
    if let Some(runtime) = job.record.runtime.as_mut() {
        if let Some(first) = runtime.stage_history.first_mut() {
            first.enter_at = "2026-09-01T00:00:00Z".to_string();
        }
    }
    job
}

fn to_record(snapshot: &JobSnapshot, pending: PendingJobEvent, seq: i64) -> JobEventRecord {
    let ts = format!("2026-09-01T00:00:{seq:02}Z");
    JobEventRecord {
        job_id: snapshot.job_id.clone(),
        seq,
        ts: ts.clone(),
        created_at: ts,
        level: pending.level,
        user_stage: pending
            .user_stage
            .clone()
            .map(normalize_user_stage)
            .or_else(|| user_stage_for_event(pending.stage.as_deref())),
        lane: None,
        display_stage: None,
        stage: pending.stage.clone(),
        substage: pending
            .substage
            .clone()
            .or_else(|| pending.provider_stage.clone()),
        stage_detail: pending.stage_detail.clone(),
        provider: pending.provider.clone(),
        provider_stage: pending.provider_stage.clone(),
        event: pending.event.clone(),
        event_type: Some(pending.event.clone()),
        raw_event_type: Some(pending.event.clone()),
        raw: None,
        progress: None,
        message: pending.message,
        progress_current: pending.progress_current,
        progress_total: pending.progress_total,
        progress_unit: pending
            .progress_unit
            .clone()
            .or_else(|| progress_unit_for_event(pending.stage.as_deref(), &pending.event)),
        retry_count: pending.retry_count,
        elapsed_ms: pending.elapsed_ms,
        payload: pending.payload,
    }
}

fn derive_sequence(initial: JobSnapshot, mutations: Vec<JobSnapshot>) -> Vec<JobEventRecord> {
    let all: Vec<JobSnapshot> = std::iter::once(initial).chain(mutations).collect();
    let mut events = Vec::new();
    let mut seq = 0i64;
    for item in derive_events(None, &all[0]) {
        seq += 1;
        events.push(to_record(&all[0], item, seq));
    }
    for pair in all.windows(2) {
        for item in derive_events(Some(&pair[0]), &pair[1]) {
            seq += 1;
            events.push(to_record(&pair[1], item, seq));
        }
    }
    events
}

fn build_book_succeeded() -> (JobSnapshot, Vec<JobEventRecord>) {
    let initial = new_deterministic_job("book-succeeded");

    let mut running = initial.clone();
    running.status = JobStatusKind::Running;
    running.stage = Some(job_stage_str(JobStage::Running).to_string());
    running.stage_detail = Some("正在启动 Python worker".to_string());
    running.started_at = Some("2026-09-01T00:00:00Z".to_string());
    running.updated_at = "2026-09-01T00:00:05Z".to_string();
    running.sync_runtime_state();

    let mut translating = running.clone();
    translating.stage = Some(job_stage_str(JobStage::Translating).to_string());
    translating.stage_detail = Some("正在翻译".to_string());
    translating.updated_at = "2026-09-01T00:00:12Z".to_string();
    translating.sync_runtime_state();

    let mut rendering = translating.clone();
    rendering.stage = Some(job_stage_str(JobStage::Rendering).to_string());
    rendering.stage_detail = Some("正在渲染".to_string());
    rendering.updated_at = "2026-09-01T00:00:20Z".to_string();
    rendering.sync_runtime_state();

    let mut finished = rendering.clone();
    finished.status = JobStatusKind::Succeeded;
    finished.stage = Some(job_stage_str(JobStage::Finished).to_string());
    finished.stage_detail = Some("任务完成".to_string());
    finished.finished_at = Some("2026-09-01T00:00:25Z".to_string());
    finished.updated_at = "2026-09-01T00:00:25Z".to_string();
    finished.sync_runtime_state();

    let events = derive_sequence(
        initial,
        vec![running, translating, rendering, finished.clone()],
    );
    (finished, events)
}

fn book_failed_failure() -> JobFailureInfo {
    JobFailureInfo {
        stage: "translation".to_string(),
        category: "upstream_timeout".to_string(),
        code: None,
        failed_stage: Some("translation".to_string()),
        failure_code: Some("upstream_timeout".to_string()),
        failure_category: Some("timeout".to_string()),
        provider_stage: None,
        provider_code: None,
        summary: "外部服务请求超时".to_string(),
        root_cause: Some("网络超时".to_string()),
        retryable: true,
        upstream_host: Some("api.deepseek.com".to_string()),
        provider: Some("deepseek".to_string()),
        suggestion: Some("稍后重试".to_string()),
        last_log_line: Some("ReadTimeout".to_string()),
        raw_excerpt: Some("ReadTimeout".to_string()),
        raw_error_excerpt: Some("ReadTimeout".to_string()),
        raw_diagnostic: None,
        ai_diagnostic: None,
    }
}

fn build_book_failed() -> (JobSnapshot, Vec<JobEventRecord>) {
    let initial = new_deterministic_job("book-failed");

    let mut running = initial.clone();
    running.status = JobStatusKind::Running;
    running.stage = Some(job_stage_str(JobStage::Running).to_string());
    running.stage_detail = Some("正在启动 Python worker".to_string());
    running.started_at = Some("2026-09-01T00:00:00Z".to_string());
    running.updated_at = "2026-09-01T00:00:05Z".to_string();
    running.sync_runtime_state();

    let mut translating = running.clone();
    translating.stage = Some(job_stage_str(JobStage::Translating).to_string());
    translating.stage_detail = Some("正在翻译".to_string());
    translating.updated_at = "2026-09-01T00:00:12Z".to_string();
    translating.sync_runtime_state();

    let mut failed = translating.clone();
    failed.status = JobStatusKind::Failed;
    failed.stage = Some(job_stage_str(JobStage::Failed).to_string());
    failed.stage_detail = Some("任务失败".to_string());
    failed.error = Some("ReadTimeout".to_string());
    failed.finished_at = Some("2026-09-01T00:00:15Z".to_string());
    failed.updated_at = "2026-09-01T00:00:15Z".to_string();
    failed.replace_failure_info(Some(book_failed_failure()));
    failed.sync_runtime_state();

    let events = derive_sequence(initial, vec![running, translating, failed.clone()]);
    (failed, events)
}

fn write_events_fixture(name: &str, events: &[JobEventRecord]) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/job_replay");
    fs::create_dir_all(&dir).expect("create fixtures dir");
    let path = dir.join(format!("{name}.events.jsonl"));
    let mut lines = String::new();
    for event in events {
        lines.push_str(&serde_json::to_string(event).expect("serialize event"));
        lines.push('\n');
    }
    fs::write(&path, lines).expect("write events fixture");
}

#[test]
#[ignore = "regenerate book_succeeded events fixture; run with --ignored"]
fn regen_book_succeeded_events_fixture() {
    let (_, events) = build_book_succeeded();
    write_events_fixture("book_succeeded", &events);
}

#[test]
#[ignore = "regenerate book_failed events fixture; run with --ignored"]
fn regen_book_failed_events_fixture() {
    let (_, events) = build_book_failed();
    write_events_fixture("book_failed", &events);
}

#[test]
fn round_trip_derive_replay_recovers_terminal_snapshot() {
    let (final_snapshot, events) = build_book_succeeded();
    let rebuilt = rebuild_terminal_state(&events);
    assert_eq!(rebuilt.status, final_snapshot.status);
    assert_eq!(
        rebuilt.terminal_stage.as_deref(),
        final_snapshot.stage.as_deref()
    );
    assert_eq!(
        rebuilt.stage_history.last().map(String::as_str),
        final_snapshot.stage.as_deref()
    );
    assert!(rebuilt.terminal_event_seen);
    assert!(rebuilt.failure.is_none());
    assert_eq!(rebuilt.event_count, 12);

    let (failed_snapshot, failed_events) = build_book_failed();
    let rebuilt = rebuild_terminal_state(&failed_events);
    assert_eq!(rebuilt.status, JobStatusKind::Failed);
    assert_eq!(
        rebuilt.terminal_stage.as_deref(),
        failed_snapshot.stage.as_deref()
    );
    assert_eq!(rebuilt.error.as_deref(), Some("ReadTimeout"));
    let failure = rebuilt.failure.expect("failure_classified payload");
    assert_eq!(failure.category, "upstream_timeout");
    assert_eq!(failure.failure_code_value(), "upstream_timeout");
    assert_eq!(failure.summary, "外部服务请求超时");
    assert!(failure.retryable);
    assert!(rebuilt.terminal_event_seen);
    assert_eq!(rebuilt.event_count, 12);
}

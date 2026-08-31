//! `pipeline_events.jsonl` writer mirroring `services/pipeline_shared/events.py`
//! (`PipelineEventWriter`), so the rust_api live_stage parser
//! (`load_pipeline_events_jsonl`) reads render_rs events with the same record
//! shape the Python render worker wrote. One JSON object per line, UTC ISO-8601
//! timestamps, sequence continuing from the existing file's line count.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{json, Value};

pub const PIPELINE_EVENTS_FILE_NAME: &str = "pipeline_events.jsonl";

const OCR_STAGES: &[&str] = &[
    "ocr_upload",
    "ocr_processing",
    "ocr_result_ready",
    "normalizing",
];
const TRANSLATE_STAGES: &[&str] = &[
    "translation_prepare",
    "translating",
    "translation_batches",
    "continuation_review",
    "page_policies",
    "domain_inference",
    "garbled_repair",
    "agent_repair",
    "final_untranslated_recovery",
];
const RENDER_STAGES: &[&str] = &[
    "render_prepare",
    "render_preprocess",
    "rendering",
    "compile",
    "overlay",
    "saving",
];

/// `events.user_stage_for_stage` — the public user-stage label the live_stage
/// pipeline groups events under.
fn user_stage_for_stage(stage: &str) -> String {
    if OCR_STAGES.contains(&stage) {
        return "ocr".to_string();
    }
    if TRANSLATE_STAGES.contains(&stage) {
        return "translation".to_string();
    }
    if RENDER_STAGES.contains(&stage) {
        return "render".to_string();
    }
    if matches!(stage, "finished" | "done") {
        return "done".to_string();
    }
    String::new()
}

/// `events.semantic_event_type`.
fn semantic_event_type(event_type: &str) -> String {
    match event_type {
        "stage_transition" | "stage_progress" => "progress".to_string(),
        "artifact_published" => "artifact".to_string(),
        "job_terminal" | "failure_classified" => "terminal".to_string(),
        other if other.trim().is_empty() => "event".to_string(),
        other => other.to_string(),
    }
}

/// `datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")` — UTC
/// ISO-8601 with microsecond precision.
fn now_iso() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch");
    let secs = duration.as_secs() as i64;
    let micros = duration.subsec_micros();
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{micros:06}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
    )
}

/// `civil_from_days` (Hinnant) — days since 1970-01-01 to (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Mirror of `services/pipeline_shared/events.py::PipelineEventWriter` — appends
/// one JSON record per line to `<logs_dir>/pipeline_events.jsonl`.
pub struct PipelineEventWriter {
    job_id: String,
    logs_dir: PathBuf,
    seq: u64,
}

impl PipelineEventWriter {
    /// Sequence resumes from the existing file's non-empty line count
    /// (`PipelineEventWriter.__post_init__`), so appending to a worker that
    /// already wrote events keeps `seq` unique.
    pub fn new(job_id: &str, logs_dir: &Path) -> Self {
        let seq = existing_line_count(&logs_dir.join(PIPELINE_EVENTS_FILE_NAME));
        Self {
            job_id: job_id.to_string(),
            logs_dir: logs_dir.to_path_buf(),
            seq,
        }
    }

    pub fn path(&self) -> PathBuf {
        self.logs_dir.join(PIPELINE_EVENTS_FILE_NAME)
    }

    /// Append one event record, mirroring `PipelineEventWriter.emit`. `user_stage`
    /// and `substage` fall back to payload-derived values like the Python writer.
    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &mut self,
        level: &str,
        stage: &str,
        event_type: &str,
        substage: &str,
        message: &str,
        stage_detail: &str,
        progress_current: Option<i64>,
        progress_total: Option<i64>,
        progress_unit: &str,
        elapsed_ms: Option<i64>,
        payload: &Value,
    ) -> Result<Value> {
        std::fs::create_dir_all(&self.logs_dir)?;
        self.seq += 1;
        let ts = now_iso();
        let user_stage = payload
            .get("user_stage")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| user_stage_for_stage(stage));
        let substage_value = {
            let explicit = substage.trim();
            let payload_substage = payload
                .get("substage")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if !explicit.is_empty() {
                explicit.to_string()
            } else if !payload_substage.is_empty() {
                payload_substage.to_string()
            } else {
                String::new()
            }
        };
        let record = json!({
            "job_id": self.job_id,
            "seq": self.seq,
            "ts": ts,
            "created_at": ts,
            "level": if level.trim().is_empty() { "info" } else { level },
            "user_stage": user_stage,
            "stage": stage,
            "substage": substage_value,
            "stage_detail": stage_detail,
            "provider": payload.get("provider").and_then(Value::as_str).unwrap_or(""),
            "provider_stage": payload.get("provider_stage").and_then(Value::as_str).unwrap_or(""),
            "event_type": event_type,
            "semantic_event_type": semantic_event_type(event_type),
            "message": message,
            "progress_current": progress_current,
            "progress_total": progress_total,
            "progress_unit": progress_unit,
            "retry_count": payload.get("retry_count").and_then(Value::as_i64),
            "elapsed_ms": elapsed_ms,
            "payload": payload,
        });
        let mut line = serde_json::to_string(&record)?;
        line.push('\n');
        let mut handle = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())?;
        handle.write_all(line.as_bytes())?;
        Ok(record)
    }

    /// `emit_stage_transition` (level info, no progress, progress_unit "step").
    pub fn emit_transition(&mut self, stage: &str, substage: &str, message: &str) -> Result<Value> {
        self.emit(
            "info",
            stage,
            "stage_transition",
            substage,
            message,
            message,
            None,
            None,
            "step",
            None,
            &json!({}),
        )
    }

    /// `emit_stage_progress` (level info, explicit progress window).
    pub fn emit_progress(
        &mut self,
        stage: &str,
        substage: &str,
        message: &str,
        progress_current: i64,
        progress_total: i64,
        progress_unit: &str,
        payload: &Value,
    ) -> Result<Value> {
        self.emit(
            "info",
            stage,
            "stage_progress",
            substage,
            message,
            message,
            Some(progress_current),
            Some(progress_total),
            progress_unit,
            None,
            payload,
        )
    }
}

fn existing_line_count(path: &Path) -> u64 {
    let Ok(text) = std::fs::read_to_string(path) else {
        return 0;
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .count() as u64
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn unique_logs_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("render-rs-events-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn civil_from_days_matches_known_epochs() {
        // 1970-01-01.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2026-08-31.
        assert_eq!(civil_from_days(20_696), (2026, 8, 31));
        // 2000-02-29 (leap day).
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        // Negative (1969-12-31).
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn now_iso_is_utc_iso8601_with_micros() {
        let value = now_iso();
        assert!(value.ends_with('Z'), "value {value}");
        let body = &value[..value.len() - 1];
        let mut parts = body.split('T');
        let date = parts.next().unwrap();
        let time = parts.next().unwrap();
        let mut date_parts = date.split('-');
        let year: u32 = date_parts.next().unwrap().parse().unwrap();
        assert!((2000..=2100).contains(&year), "year {year}");
        assert_eq!(date_parts.count(), 2);
        assert_eq!(time.len(), 15, "HH:MM:SS.mmmmmm = {time}");
    }

    #[test]
    fn emit_writes_parseable_records_with_shape_parity() {
        let dir = unique_logs_dir("emit");
        let mut writer = PipelineEventWriter::new("job-x", &dir);
        let payload = json!({"user_stage": "render", "progress_unit": "page"});
        writer
            .emit_progress("rendering", "render_pages", "渲染中", 1, 3, "page", &payload)
            .expect("emit");
        writer
            .emit_transition("finished", "", "完成")
            .expect("emit");

        let text = fs::read_to_string(writer.path()).expect("read events");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: Value = serde_json::from_str(lines[0]).expect("parse line 1");
        assert_eq!(first["job_id"], "job-x");
        assert_eq!(first["seq"], 1);
        assert_eq!(first["stage"], "rendering");
        assert_eq!(first["substage"], "render_pages");
        assert_eq!(first["user_stage"], "render");
        assert_eq!(first["semantic_event_type"], "progress");
        assert_eq!(first["progress_current"], 1);
        assert_eq!(first["progress_total"], 3);
        assert_eq!(first["progress_unit"], "page");
        assert!(first["ts"].as_str().unwrap().ends_with('Z'));
        assert_eq!(first["payload"]["progress_unit"], "page");
        let second: Value = serde_json::from_str(lines[1]).expect("parse line 2");
        assert_eq!(second["seq"], 2);
        assert_eq!(second["event_type"], "stage_transition");
        assert_eq!(second["semantic_event_type"], "progress");
        assert_eq!(second["user_stage"], "done");
        assert!(second["substage"].as_str().unwrap().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sequence_resumes_from_existing_lines() {
        let dir = unique_logs_dir("seq");
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(
            dir.join(PIPELINE_EVENTS_FILE_NAME),
            "{}\n\n{}\n",
        )
        .expect("write seed");
        let mut writer = PipelineEventWriter::new("job-x", &dir);
        writer
            .emit_transition("saving", "saving", "保存")
            .expect("emit");
        let text = fs::read_to_string(writer.path()).expect("read");
        let records: Vec<Value> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str::<Value>(line).expect("parse"))
            .collect();
        // Two seed lines + one new record; the writer resumed at seq 3.
        assert_eq!(records.len(), 3);
        assert_eq!(records[2]["seq"], 3);
        assert_eq!(records[2]["message"], "保存");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_payload_user_stage_falls_back_to_stage() {
        let dir = unique_logs_dir("ustage");
        let mut writer = PipelineEventWriter::new("job-x", &dir);
        writer
            .emit_progress("compile", "render_compile", "编译", 1, 1, "step", &json!({}))
            .expect("emit");
        let text = fs::read_to_string(writer.path()).expect("read");
        let record: Value = serde_json::from_str(text.trim()).expect("parse");
        assert_eq!(record["user_stage"], "render");
        assert_eq!(record["substage"], "render_compile");

        let _ = fs::remove_dir_all(&dir);
    }
}

from __future__ import annotations

from pathlib import Path


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
TRANSLATE_ONLY_PIPELINE = REPO_SCRIPTS_ROOT / "services" / "translation" / "entrypoints" / "translate_only_pipeline.py"


def test_translate_only_pipeline_keeps_events_and_diagnostics_protocol() -> None:
    source = TRANSLATE_ONLY_PIPELINE.read_text(encoding="utf-8")

    assert "PipelineEventWriter(" in source
    assert "STDOUT_LABEL_EVENTS_JSONL" in source
    assert 'artifact_key="pipeline_events_jsonl"' in source
    assert 'artifact_key="translation_diagnostics_json"' in source
    assert '"translation_diagnostics.json"' in source
    assert 'infer_provider_capabilities(' in source
    assert 'base_url=args.base_url' in source
    assert 'provider_family=args.provider_family' in source
    assert "normalize_base_url(args.base_url) == normalize_base_url(DEFAULT_BASE_URL)" not in source

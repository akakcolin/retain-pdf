from __future__ import annotations

import json
import sys
from pathlib import Path

REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))

from foundation.shared.job_dirs import ensure_job_dirs
from foundation.shared.job_dirs import resolve_job_dirs
from foundation.shared.stage_specs import NormalizeStageSpec
from foundation.shared.stage_specs import build_stage_invocation_metadata
from foundation.shared.stage_specs import BookStageSpec
from foundation.shared.stage_specs import BOOK_STAGE_SCHEMA_VERSION
from foundation.shared.stage_specs import NORMALIZE_STAGE_SCHEMA_VERSION
from foundation.shared.stage_specs import ProviderStageSpec
from foundation.shared.stage_specs import PROVIDER_STAGE_SCHEMA_VERSION
from foundation.shared.stage_specs import resolve_credential_ref
from foundation.shared.stage_specs import TranslateStageSpec
from foundation.shared.stage_specs import TRANSLATE_STAGE_SCHEMA_VERSION
from foundation.shared.stage_specs import RenderStageSpec
from foundation.shared.stage_specs import RENDER_STAGE_SCHEMA_VERSION
from foundation.config import fonts

def test_translate_stage_spec_loads_and_resolves_env_credential(tmp_path: Path, monkeypatch) -> None:
    job_root = tmp_path / "20260414-translatejob"
    ensure_job_dirs(resolve_job_dirs(job_root))
    source_json = tmp_path / "document.v1.json"
    source_pdf = tmp_path / "source.pdf"
    source_json.write_text("{}", encoding="utf-8")
    source_pdf.write_bytes(b"%PDF-1.4\n")
    spec_path = job_root / "specs" / "translate.spec.json"
    spec_path.parent.mkdir(parents=True, exist_ok=True)
    spec_path.write_text(
        json.dumps(
            {
                "schema_version": TRANSLATE_STAGE_SCHEMA_VERSION,
                "stage": "translate",
                "job": {
                    "job_id": "20260414-translatejob",
                    "job_root": str(job_root),
                    "workflow": "translate",
                },
                "inputs": {
                    "source_json": str(source_json),
                    "source_pdf": str(source_pdf),
                    "layout_json": "",
                },
                "params": {
                    "start_page": 0,
                    "end_page": -1,
                    "batch_size": 8,
                    "workers": 4,
                    "mode": "sci",
                    "skip_title_translation": False,
                    "classify_batch_size": 12,
                    "rule_profile_name": "general_sci",
                    "custom_rules_text": "",
                    "glossary_id": "",
                    "glossary_name": "",
                    "glossary_resource_entry_count": 0,
                    "glossary_inline_entry_count": 0,
                    "glossary_overridden_entry_count": 0,
                    "glossary_entries": [],
                    "model": "deepseek-v4-flash",
                    "base_url": "https://api.deepseek.com/v1",
                    "credential_ref": "env:RETAIN_TRANSLATION_API_KEY",
                },
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )
    monkeypatch.setenv("RETAIN_TRANSLATION_API_KEY", "sk-stage-test")

    spec = TranslateStageSpec.load(spec_path)

    assert spec.stage == "translate"
    assert spec.params.model == "deepseek-v4-flash"
    assert resolve_credential_ref(spec.params.credential_ref) == "sk-stage-test"


def test_translate_stage_spec_defaults_math_mode_to_direct_typst(tmp_path: Path) -> None:
    job_root = tmp_path / "20260414-translatejob-default-math"
    ensure_job_dirs(resolve_job_dirs(job_root))
    source_json = tmp_path / "document.v1.json"
    source_pdf = tmp_path / "source.pdf"
    source_json.write_text("{}", encoding="utf-8")
    source_pdf.write_bytes(b"%PDF-1.4\n")
    spec_path = job_root / "specs" / "translate.spec.json"
    spec_path.parent.mkdir(parents=True, exist_ok=True)
    spec_path.write_text(
        json.dumps(
            {
                "schema_version": TRANSLATE_STAGE_SCHEMA_VERSION,
                "stage": "translate",
                "job": {
                    "job_id": "20260414-translatejob-default-math",
                    "job_root": str(job_root),
                    "workflow": "translate",
                },
                "inputs": {
                    "source_json": str(source_json),
                    "source_pdf": str(source_pdf),
                    "layout_json": "",
                },
                "params": {
                    "model": "deepseek-v4-flash",
                    "base_url": "https://api.deepseek.com/v1",
                    "credential_ref": "",
                },
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )

    spec = TranslateStageSpec.load(spec_path)

    assert spec.params.math_mode == "direct_typst"
    assert spec.params.source_lang == "auto"
    assert spec.params.target_lang == "zh-CN"
    assert spec.params.target_language_name == "简体中文"


def test_translate_stage_spec_reads_explicit_language_keys(tmp_path: Path) -> None:
    job_root = tmp_path / "20260414-translatejob-lang"
    ensure_job_dirs(resolve_job_dirs(job_root))
    source_json = tmp_path / "document.v1.json"
    source_pdf = tmp_path / "source.pdf"
    source_json.write_text("{}", encoding="utf-8")
    source_pdf.write_bytes(b"%PDF-1.4\n")
    spec_path = job_root / "specs" / "translate.spec.json"
    spec_path.parent.mkdir(parents=True, exist_ok=True)
    spec_path.write_text(
        json.dumps(
            {
                "schema_version": TRANSLATE_STAGE_SCHEMA_VERSION,
                "stage": "translate",
                "job": {
                    "job_id": "20260414-translatejob-lang",
                    "job_root": str(job_root),
                    "workflow": "translate",
                },
                "inputs": {
                    "source_json": str(source_json),
                    "source_pdf": str(source_pdf),
                    "layout_json": "",
                },
                "params": {
                    "model": "deepseek-v4-flash",
                    "base_url": "https://api.deepseek.com/v1",
                    "credential_ref": "",
                    "source_lang": "en",
                    "target_lang": "fr",
                    "target_language_name": "Français",
                },
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )

    spec = TranslateStageSpec.load(spec_path)

    assert spec.params.source_lang == "en"
    assert spec.params.target_lang == "fr"
    assert spec.params.target_language_name == "Français"



# Python Pipeline Dependencies

This file is generated from static import scanning under `backend/scripts`.
Regenerate with:
`python backend/scripts/devtools/extract_pipeline_requirements.py --repo-root . --json-out doc/core/python/pipeline_dependencies.json --markdown-out doc/core/python/pipeline_dependencies.md --runtime-req-out doc/core/python/pipeline_runtime_requirements.in --test-req-out doc/core/python/pipeline_test_requirements.in`

## Runtime Python Packages

- `PyMuPDF`
- `requests`
- `urllib3`

## Test-only Python Packages

- `pytest`

## External Commands

- `typst`
  refs: `devtools/architecture_checks/translation_field_writers.py`, `devtools/d5_baseline.py`, `devtools/job_debug_runner.py`, `devtools/replay_translation_item.py`, `devtools/tests/d5_baseline/test_d5_baseline.py`, `devtools/tests/document_schema/test_stage_spec_book.py`

## Package Map

| Import | Package | Runtime | Test | Example refs |
| --- | --- | --- | --- | --- |
| `fitz` | `PyMuPDF` | yes | yes | `services/derived_artifacts/side_by_side_pdf.py`, `services/ocr_provider/paddle_runner.py`, `services/translation/llm/domain_context.py` |
| `pytest` | `pytest` | no | yes | `devtools/tests/d5_baseline/test_d5_baseline.py`, `devtools/tests/document_schema/test_adapters_detection.py`, `devtools/tests/document_schema/test_document_v1_schema_parity.py` |
| `requests` | `requests` | yes | yes | `services/mineru/mineru_api.py`, `services/network/retry.py`, `services/ocr_provider/local_paddlex_wrapper.py` |
| `urllib3` | `urllib3` | yes | no | `services/network/retry.py`, `services/translation/llm/providers/deepseek/transport.py` |

## Existing Requirement Files

- `docker/requirements-app.txt`
- `desktop/requirements-desktop-posix.txt`
- `desktop/requirements-desktop-windows.txt`
- `desktop/requirements-desktop-macos.txt`

## Generated Outputs

- `doc/core/python/pipeline_dependencies.json`
- `doc/core/python/pipeline_dependencies.md`
- `doc/core/python/pipeline_runtime_requirements.in`
- `doc/core/python/pipeline_test_requirements.in`

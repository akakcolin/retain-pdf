from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))

from services.document_schema.validator import (
    SUPPORTED_DOCUMENT_SCHEMA_VERSIONS,
    default_schema_json_path,
    validate_document_payload,
)
from services.document_schema.version import (
    DOCUMENT_SCHEMA_NAME,
    DOCUMENT_SCHEMA_VERSION,
)

DOCUMENT_REQUIRED_KEYS = {
    "schema", "schema_version", "document_id", "source", "page_count", "pages",
    "derived", "markers",
}
PAGE_REQUIRED_KEYS = {"page_index", "width", "height", "unit", "blocks"}
BLOCK_REQUIRED_KEYS = {
    "block_id", "page_index", "order", "geometry", "content", "layout_role",
    "semantic_role", "structure_role", "policy", "provenance", "continuation_hint",
    "metadata", "source",
}

# Locked-in baseline: values valid when the parity lock was introduced. Shrinking
# any enum below this baseline is a breaking contract change.
BASELINE_LAYOUT_ROLES = {
    "title", "heading", "paragraph", "list_item", "caption", "header", "footer",
    "footnote", "page_number", "toc", "unknown",
}
BASELINE_SEMANTIC_ROLES = {
    "body", "abstract", "reference", "metadata", "affiliation", "acknowledgement",
    "table_of_contents", "unknown",
}
BASELINE_BLOCK_TYPES = {"text", "formula", "image", "table", "code", "unknown"}
BASELINE_CONTENT_KINDS = {"text", "image", "table", "formula", "code", "unknown"}


def _schema() -> dict:
    path = default_schema_json_path()
    assert path.is_file(), f"schema not found at {path}"
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def test_default_schema_json_path_points_to_producer_crate() -> None:
    path = default_schema_json_path()
    assert path.is_file()
    assert path.parts[-3:] == ("rendering_orchestrator", "schemas", "document.v1.schema.json")


def test_schema_metadata_matches_version_consts() -> None:
    schema = _schema()
    assert schema["properties"]["schema"]["const"] == DOCUMENT_SCHEMA_NAME
    assert schema["properties"]["schema_version"]["enum"] == [DOCUMENT_SCHEMA_VERSION]
    assert schema["$defs"]["page"]["properties"]["unit"]["const"] == "pt"
    assert SUPPORTED_DOCUMENT_SCHEMA_VERSIONS == (DOCUMENT_SCHEMA_VERSION,)


def test_required_keys_parity() -> None:
    schema = _schema()
    assert set(schema["required"]) == DOCUMENT_REQUIRED_KEYS
    assert set(schema["$defs"]["block"]["required"]) == BLOCK_REQUIRED_KEYS
    assert set(schema["$defs"]["page"]["required"]) == PAGE_REQUIRED_KEYS


def test_role_enums_include_toc_and_table_of_contents() -> None:
    schema = _schema()
    block = schema["$defs"]["block"]["properties"]
    assert "toc" in block["layout_role"]["enum"]
    assert "table_of_contents" in block["semantic_role"]["enum"]


def test_toc_roles_validate_through_python_validator() -> None:
    document = {
        "schema": "normalized_document_v1",
        "schema_version": "1.1",
        "document_id": "parity-toc",
        "doc_id": "parity-toc",
        "source": {
            "provider": "generic_flat_ocr",
            "provider_version": "1.0",
            "raw_files": {"source_json": "/src/layout.json"},
        },
        "page_count": 1,
        "pages": [
            {
                "page_index": 0,
                "width": 600,
                "height": 800,
                "unit": "pt",
                "page": 1,
                "blocks": [
                    {
                        "block_id": "p001-b0000",
                        "page_index": 0,
                        "order": 0,
                        "type": "text",
                        "sub_type": "body",
                        "bbox": [10, 10, 200, 40],
                        "geometry": {"bbox": [10, 10, 200, 40]},
                        "content": {"kind": "text", "text": "Contents"},
                        "text": "Contents",
                        "lines": [],
                        "segments": [],
                        "tags": [],
                        "derived": {"role": "", "by": "", "confidence": 0.0},
                        "layout_role": "toc",
                        "semantic_role": "table_of_contents",
                        "structure_role": "body",
                        "policy": {"translate": True, "translate_reason": "x"},
                        "continuation_hint": {
                            "source": "", "group_id": "", "role": "", "scope": "",
                            "reading_order": -1, "confidence": 0.0,
                        },
                        "metadata": {},
                        "source": {
                            "provider": "generic_flat_ocr",
                            "raw_page_index": 0,
                            "raw_type": "text",
                            "raw_sub_type": "body",
                            "raw_bbox": [10, 10, 200, 40],
                            "raw_text_excerpt": "Contents",
                        },
                        "reading_order": 0,
                        "provenance": {
                            "provider": "generic_flat_ocr",
                            "raw_label": "text",
                            "raw_sub_type": "body",
                            "raw_bbox": [10, 10, 200, 40],
                            "raw_path": "",
                        },
                    }
                ],
            }
        ],
        "assets": {},
        "derived": {"notes": "x"},
        "markers": {},
    }
    validate_document_payload(document)


def test_unknown_schema_version_reports_expected_one_of() -> None:
    document = {
        "schema": "normalized_document_v1",
        "schema_version": "9.9",
        "document_id": None,
        "source": None,
        "page_count": None,
        "pages": None,
        "derived": None,
        "markers": None,
    }
    with pytest.raises(ValueError) as excinfo:
        validate_document_payload(document)
    message = str(excinfo.value)
    assert "expected one of" in message
    assert "9.9" in message


def test_additional_properties_never_closed() -> None:
    def _assert_permissive(node, path: str) -> None:
        if isinstance(node, dict) and node.get("type") == "object":
            assert node.get("additionalProperties") is not False, (
                f"{path}: object must not close additionalProperties=false"
            )
            for key, value in node.get("properties", {}).items():
                _assert_permissive(value, f"{path}.properties.{key}")
            for key, value in node.get("$defs", {}).items():
                _assert_permissive(value, f"{path}.$defs.{key}")
        if isinstance(node, dict) and isinstance(node.get("items"), dict):
            _assert_permissive(node["items"], f"{path}.items")
        if isinstance(node, dict) and isinstance(node.get("additionalProperties"), dict):
            _assert_permissive(node["additionalProperties"], f"{path}.additionalProperties")

    _assert_permissive(_schema(), "$")


def test_required_keys_additive_only() -> None:
    # Adding a required key is breaking (existing documents stop validating),
    # so the schema may only ever drop required keys, never add them.
    baseline_document = {
        "schema", "schema_version", "document_id", "source", "page_count", "pages",
        "derived", "markers",
    }
    baseline_page = {"page_index", "width", "height", "unit", "blocks"}
    baseline_block = {
        "block_id", "page_index", "order", "geometry", "content", "layout_role",
        "semantic_role", "structure_role", "policy", "provenance", "continuation_hint",
        "metadata", "source",
    }

    schema = _schema()
    assert set(schema["required"]).issubset(baseline_document)
    assert set(schema["$defs"]["block"]["required"]).issubset(baseline_block)
    assert set(schema["$defs"]["page"]["required"]).issubset(baseline_page)


def test_enums_additive_only() -> None:
    schema = _schema()
    block = schema["$defs"]["block"]["properties"]
    assert BASELINE_LAYOUT_ROLES.issubset(set(block["layout_role"]["enum"]))
    assert BASELINE_SEMANTIC_ROLES.issubset(set(block["semantic_role"]["enum"]))
    assert BASELINE_BLOCK_TYPES.issubset(set(block["type"]["enum"]))
    assert BASELINE_CONTENT_KINDS.issubset(
        set(schema["$defs"]["content"]["properties"]["kind"]["enum"])
    )

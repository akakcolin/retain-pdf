import sys
from pathlib import Path


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))


from services.document_schema.defaults import default_block_continuation_hint
from devtools.tests.translation._generic_flat_ocr_fixtures import build_generic_flat_ocr_document
from services.translation.core.ocr.json_extractor import extract_text_items

PROVIDER_GENERIC_FLAT_OCR = "generic_flat_ocr"

def test_extract_text_items_only_keeps_primary_body_like_text_blocks() -> None:
    adapted = build_generic_flat_ocr_document(
        payload={
            "provider": PROVIDER_GENERIC_FLAT_OCR,
            "pages": [
                {
                    "width": 300.0,
                    "height": 240.0,
                    "unit": "pt",
                    "blocks": [
                        {
                            "type": "text",
                            "sub_type": "body",
                            "bbox": [0, 0, 140, 20],
                            "text": "Body paragraph",
                            "lines": [{"bbox": [0, 0, 140, 20], "spans": [{"type": "text", "raw_type": "text", "text": "Body paragraph", "bbox": [0, 0, 140, 20]}]}],
                            "segments": [],
                            "tags": [],
                            "derived": {"role": "", "by": "", "confidence": 0.0},
                            "metadata": {},
                        },
                        {
                            "type": "text",
                            "sub_type": "heading",
                            "bbox": [0, 30, 140, 50],
                            "text": "Results",
                            "lines": [{"bbox": [0, 30, 140, 50], "spans": [{"type": "text", "raw_type": "text", "text": "Results", "bbox": [0, 30, 140, 50]}]}],
                            "segments": [],
                            "tags": [],
                            "derived": {"role": "heading", "by": "", "confidence": 0.0},
                            "metadata": {},
                        },
                        {
                            "type": "text",
                            "sub_type": "table_caption",
                            "bbox": [0, 60, 200, 80],
                            "text": "Table 1. Caption text",
                            "lines": [{"bbox": [0, 60, 200, 80], "spans": [{"type": "text", "raw_type": "text", "text": "Table 1. Caption text", "bbox": [0, 60, 200, 80]}]}],
                            "segments": [],
                            "tags": ["caption", "table_caption"],
                            "derived": {"role": "table_caption", "by": "", "confidence": 0.0},
                            "metadata": {},
                        },
                        {
                            "type": "text",
                            "sub_type": "header",
                            "bbox": [0, 90, 200, 110],
                            "text": "Journal Header",
                            "lines": [{"bbox": [0, 90, 200, 110], "spans": [{"type": "text", "raw_type": "text", "text": "Journal Header", "bbox": [0, 90, 200, 110]}]}],
                            "segments": [],
                            "tags": ["skip_translation"],
                            "derived": {"role": "header", "by": "", "confidence": 0.0},
                            "metadata": {},
                        },
                    ],
                }
            ],
        },
        document_id="generic-body-only-doc",
        source_json_path=Path("/tmp/generic-body-only.json"),
    )

    items = extract_text_items(adapted, 0)

    assert [item.text for item in items] == ["Body paragraph", "Results"]
    assert [item.structure_role for item in items] == ["body", "heading"]


def test_extract_text_items_keeps_empty_subtype_plain_text_body_block() -> None:
    adapted = {
        "schema": "normalized_document_v1",
        "schema_version": "1.0.0",
        "document_id": "normalized-empty-subtype-body",
        "source": {"provider": "test", "provider_version": "test", "raw_files": {}},
        "page_count": 1,
        "pages": [
            {
                "page_index": 0,
                "width": 200.0,
                "height": 120.0,
                "unit": "pt",
                "blocks": [
                        {
                            "block_id": "p001-b0000",
                            "page_index": 0,
                            "order": 0,
                            "type": "text",
                            "sub_type": "",
                            "geometry": {"bbox": [0, 0, 150, 20]},
                            "content": {"kind": "text", "text": "Plain normalized body block"},
                            "bbox": [0, 0, 150, 20],
                            "text": "Plain normalized body block",
                            "lines": [
                                {
                                    "bbox": [0, 0, 150, 20],
                                "spans": [
                                    {
                                        "type": "text",
                                        "raw_type": "text",
                                        "text": "Plain normalized body block",
                                        "bbox": [0, 0, 150, 20],
                                    }
                                ],
                            }
                        ],
                            "segments": [],
                            "tags": [],
                            "derived": {"role": "", "by": "", "confidence": 0.0},
                            "layout_role": "paragraph",
                            "semantic_role": "body",
                            "structure_role": "body",
                            "policy": {"translate": True, "translate_reason": "test_explicit_policy:body"},
                            "continuation_hint": default_block_continuation_hint(),
                            "metadata": {},
                        "source": {
                            "provider": "test",
                            "raw_page_index": 0,
                            "raw_type": "text",
                            "raw_sub_type": "",
                            "raw_bbox": [0, 0, 150, 20],
                            "raw_text_excerpt": "Plain normalized body block",
                        },
                    }
                ],
            }
        ],
        "derived": {},
        "markers": {},
    }

    items = extract_text_items(adapted, 0)

    assert [item.text for item in items] == ["Plain normalized body block"]

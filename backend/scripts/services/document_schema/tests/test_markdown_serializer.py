import json
import sys
from pathlib import Path


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))

from services.document_schema.markdown_serializer import serialize_document_to_markdown


def _text_block(text: str, *, order: int, layout_role: str = "paragraph", kind: str = "text") -> dict:
    return {
        "content": {"kind": kind, "text": text},
        "reading_order": order,
        "layout_role": layout_role,
    }


def _write_document(tmp_path: Path, blocks: list[dict]) -> Path:
    document = {
        "schema": "normalized_document_v1",
        "schema_version": "1",
        "document_id": "doc-test",
        "page_count": 1,
        "pages": [{"page": 1, "blocks": blocks}],
    }
    normalized_path = tmp_path / "normalized" / "document.v1.json"
    normalized_path.parent.mkdir(parents=True, exist_ok=True)
    normalized_path.write_text(json.dumps(document), encoding="utf-8")
    return normalized_path


def test_serialize_document_to_markdown_orders_blocks_and_skips_empty(tmp_path: Path) -> None:
    blocks = [
        _text_block("Chapter one", order=0, layout_role="heading"),
        _text_block("", order=1, layout_role="paragraph"),
        _text_block("First paragraph", order=2, layout_role="paragraph"),
        _text_block("item alpha", order=3, layout_role="list_item"),
        _text_block("Title", order=4, layout_role="title"),
    ]
    normalized_path = _write_document(tmp_path, blocks)
    output_path = tmp_path / "md" / "full.md"

    result = serialize_document_to_markdown(normalized_path, output_path)

    assert result == output_path
    content = output_path.read_text(encoding="utf-8")
    assert "## Chapter one" in content
    assert "First paragraph" in content
    assert "- item alpha" in content
    assert "# Title" in content
    # blocks appear in reading order: heading, paragraph, list_item, title
    assert content.index("Chapter one") < content.index("First paragraph") < content.index("item alpha") < content.index("Title")


def test_serialize_document_to_markdown_table_and_code_blocks(tmp_path: Path) -> None:
    table_text = "Name | Age\nAlice | 30\nBob | 25"
    blocks = [
        _text_block(table_text, order=0, kind="table"),
        _text_block("def main():\n    pass", order=1, kind="code"),
    ]
    normalized_path = _write_document(tmp_path, blocks)
    output_path = tmp_path / "md" / "full.md"

    serialize_document_to_markdown(normalized_path, output_path)

    content = output_path.read_text(encoding="utf-8")
    assert "| Name | Age |" in content
    assert "| Alice | 30 |" in content
    assert "```" in content
    assert "def main():" in content


def test_serialize_document_to_markdown_is_idempotent(tmp_path: Path) -> None:
    blocks = [_text_block("Only line", order=0, layout_role="paragraph")]
    normalized_path = _write_document(tmp_path, blocks)
    output_path = tmp_path / "md" / "full.md"

    first = serialize_document_to_markdown(normalized_path, output_path)
    second = serialize_document_to_markdown(normalized_path, output_path)

    assert first == output_path
    assert first.read_text(encoding="utf-8") == second.read_text(encoding="utf-8")

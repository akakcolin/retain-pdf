from __future__ import annotations

import json
from pathlib import Path

from services.document_schema.consumer_reader import (
    block_kind,
    block_layout_role,
    block_reading_order,
    block_text,
    get_pages,
    iter_page_blocks,
)


def serialize_document_to_markdown(normalized_json_path: Path, output_path: Path) -> Path:
    """Fallback: normalized document.v1 JSON -> md/full.md.

    Only runs when the provider did not ship native markdown (paddle/mineru write
    md/full.md themselves; the local command provider does not). Pure stdlib,
    idempotent: overwrites output_path with the full serialized document.
    """
    data = json.loads(normalized_json_path.read_text(encoding="utf-8"))
    page_sections = [_serialize_page(data, page) for page in get_pages(data)]
    sections = [section for section in page_sections if section]
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text("\n\n".join(sections).strip() + "\n", encoding="utf-8")
    return output_path


def _serialize_page(data: dict, page: dict) -> str:
    blocks = sorted(iter_page_blocks(data, page), key=block_reading_order)
    rendered = [_serialize_block(data, block) for block in blocks]
    return "\n\n".join(line for line in rendered if line)


def _serialize_block(data: dict, block: dict) -> str:
    kind = block_kind(block)
    text = block_text(block).strip()
    if kind == "text":
        return _serialize_text_block(block, text)
    if kind == "table":
        return _serialize_table_block(text)
    if kind == "code":
        return f"```\n{text}\n```" if text else ""
    if kind == "formula":
        return f"$$\n{text}\n$$" if text else ""
    # image / unknown blocks carry no markdown-representable text here; skip
    return ""


def _serialize_text_block(block: dict, text: str) -> str:
    if not text:
        return ""
    role = block_layout_role(block)
    if role == "title":
        return f"# {text}"
    if role == "heading":
        return f"## {text}"
    if role == "list_item":
        return f"- {text}"
    return text


def _serialize_table_block(text: str) -> str:
    if not text:
        return ""
    rows: list[list[str]] = []
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        cells = [cell.strip() for cell in stripped.split("|") if cell.strip()]
        if cells:
            rows.append(cells)
    if not rows:
        return text
    column_count = max(len(row) for row in rows)
    rows = [row + [""] * (column_count - len(row)) for row in rows]
    header, *body = rows
    lines = [
        "| " + " | ".join(header) + " |",
        "| " + " | ".join(["---"] * column_count) + " |",
    ]
    lines.extend("| " + " | ".join(row) + " |" for row in body)
    return "\n".join(lines)

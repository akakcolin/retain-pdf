"""Extract the embedded text layer of a PDF into a generic_flat_ocr payload.

This is the "skip OCR" path: instead of calling an OCR provider, the worker
reads the PDF's own text layer and emits a minimal `generic_flat_ocr` JSON
that the document_schema adapter normalizes into document.v1 downstream, so
the translation/render pipeline is unchanged.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.append(str(Path(__file__).resolve().parents[1]))

from foundation.shared.stage_specs import ExtractTextLayerStageSpec
from foundation.shared.structured_errors import run_with_structured_failure
from services.document_schema.providers import PROVIDER_GENERIC_FLAT_OCR
from services.pipeline_shared.io import save_json

DEFAULT_UNIT = "pt"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Extract a PDF's embedded text layer into a generic_flat_ocr document.",
    )
    parser.add_argument("--spec", type=str, default="", help="Path to extract_text_layer stage spec JSON.")
    return parser.parse_args()


def _line_text(line: dict) -> str:
    return " ".join(
        str(span.get("text", "") or "").strip()
        for span in line.get("spans", []) or []
        if str(span.get("text", "") or "").strip()
    ).strip()


def _block_text(block: dict) -> str:
    lines = [_line_text(line) for line in block.get("lines", []) or []]
    return "\n".join(line for line in lines if line)


def build_text_layer_document(source_pdf: Path) -> dict:
    try:
        import fitz  # deferred: desktop bundle prunes pymupdf; this worker is native-default (render_rs)
    except ModuleNotFoundError as exc:
        raise RuntimeError(
            "文本层提取需要 pymupdf，桌面构建未携带；请使用默认的 render_rs 路径"
        ) from exc
    pages_out: list[dict] = []
    total_blocks = 0
    with fitz.open(source_pdf) as document:
        for page_index, page in enumerate(document):
            rect = page.rect
            blocks_out: list[dict] = []
            try:
                text_dict = page.get_text("dict")
            except Exception:
                text_dict = {"blocks": []}
            for block in text_dict.get("blocks", []) or []:
                if block.get("type") != 0:
                    continue
                text = _block_text(block)
                if not text:
                    continue
                bbox = list(block.get("bbox", []) or [0, 0, 0, 0])
                if len(bbox) != 4:
                    continue
                blocks_out.append(
                    {
                        "bbox": [float(value) for value in bbox],
                        "type": "text",
                        "sub_type": "body",
                        "text": text,
                    }
                )
            total_blocks += len(blocks_out)
            pages_out.append(
                {
                    "page_index": page_index,
                    "width": float(rect.width),
                    "height": float(rect.height),
                    "unit": DEFAULT_UNIT,
                    "blocks": blocks_out,
                }
            )
    if total_blocks == 0:
        raise RuntimeError("源 PDF 不含可提取的文本层，无法跳过 OCR；请改用 OCR 识别")
    return {
        "provider": PROVIDER_GENERIC_FLAT_OCR,
        "pages": pages_out,
    }


def main() -> None:
    args = parse_args()
    if not args.spec.strip():
        raise RuntimeError(
            "extract_text_layer worker now requires --spec <extract_text_layer.spec.json>"
        )
    spec = ExtractTextLayerStageSpec.load(Path(args.spec))
    source_pdf = spec.inputs.source_pdf
    output_json = spec.inputs.output_json
    if not source_pdf.exists():
        raise RuntimeError(f"source pdf not found: {source_pdf}")

    document = build_text_layer_document(source_pdf)
    save_json(output_json, document, compact=True)

    page_count = len(document["pages"])
    block_count = sum(len(page.get("blocks", []) or []) for page in document["pages"])
    print(f"job root: {spec.job.job_root}", flush=True)
    print(f"source pdf: {source_pdf}", flush=True)
    print(f"normalized document json: {output_json}", flush=True)
    print("schema version: document.v1", flush=True)
    print(
        "text layer extracted: "
        f"pages={page_count} "
        f"blocks={block_count} "
        f"path={output_json}",
        flush=True,
    )


if __name__ == "__main__":
    run_with_structured_failure(main, default_stage="normalization", provider="text_layer")

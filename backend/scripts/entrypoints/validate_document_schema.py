import argparse
import json
from pathlib import Path
import sys

sys.path.append(str(Path(__file__).resolve().parents[1]))

from services.document_schema import build_validation_report
from services.document_schema import validate_document_path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate a normalized document.v1.json produced by the native worker `render_rs --normalize-ocr`.",
    )
    parser.add_argument("json_path", nargs="?", default="", type=str, help="Path to normalized document.v1.json.")
    parser.add_argument("--write-report", type=str, default="", help="Optional path to save JSON report.")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if not args.json_path.strip():
        raise SystemExit("json_path is required.")

    path = Path(args.json_path).resolve()
    data = validate_document_path(path)
    validation_report = build_validation_report(data)
    report = {
        "mode": "validate",
        "input_path": str(path),
        "validation": validation_report,
    }
    print(f"valid schema: {data['schema']} {data['schema_version']}")
    print(f"document_id: {data['document_id']}")
    print(f"pages: {data['page_count']}")
    print(f"blocks: {validation_report['block_count']}")

    if args.write_report.strip():
        report_path = Path(args.write_report).resolve()
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"report: {report_path}")


if __name__ == "__main__":
    main()

from __future__ import annotations

import argparse
import json
from pathlib import Path

MIB = 1024 * 1024


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Report desktop bundle component sizes from the manifest.",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        required=True,
        help="Path to desktop/app/backend/bundle-manifest.json",
    )
    parser.add_argument(
        "--table-title",
        default="Desktop bundle sizes",
        help="Markdown heading for the table (printed as ## <title>).",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    payload = json.loads(args.manifest.read_text(encoding="utf-8"))
    sizes = payload.get("sizesBytes")
    print(f"## {args.table_title}")
    if not isinstance(sizes, dict) or not sizes:
        print("`sizesBytes` missing from manifest.")
        return
    rows = sorted(sizes.items(), key=lambda item: -item[1])
    print()
    print("| Component | Size (MiB) |")
    print("| --- | ---: |")
    for name, value in rows:
        print(f"| {name} | {value / MIB:.1f} |")


if __name__ == "__main__":
    main()

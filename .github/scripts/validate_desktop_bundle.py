from __future__ import annotations

import argparse
import json
import os
import platform
import shlex
import subprocess
import tempfile
from pathlib import Path


CMARKER_VERSION = "0.1.8"
MITEX_VERSION = "0.2.6"
OUTPUT_LIMIT = 2_000


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate the generated desktop bundle manifest.",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        required=True,
        help="Path to desktop/app/backend/bundle-manifest.json",
    )
    parser.add_argument(
        "--min-fonts",
        type=int,
        default=3,
        help="Minimum number of bundled fonts expected in the manifest.",
    )
    return parser.parse_args()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def command_text(command: list[str]) -> str:
    return " ".join(shlex.quote(part) for part in command)


def output_summary(value: str) -> str:
    value = value.strip()
    if not value:
        return "<empty>"
    if len(value) <= OUTPUT_LIMIT:
        return value
    return f"{value[:OUTPUT_LIMIT]}... <truncated {len(value) - OUTPUT_LIMIT} chars>"


def run_checked(command: list[str], *, env: dict[str, str], cwd: Path) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(
            "command failed while validating desktop Typst bundle\n"
            f"command: {command_text(command)}\n"
            f"exit code: {result.returncode}\n"
            f"stdout: {output_summary(result.stdout)}\n"
            f"stderr: {output_summary(result.stderr)}"
        )
    return result


def is_windows_bundle(payload: dict[str, object]) -> bool:
    target = str(payload.get("targetPlatformName") or payload.get("targetPlatform") or "").lower()
    if target:
        return target in {"windows", "win32"}
    return platform.system() == "Windows"


def typst_executable(backend_root: Path, payload: dict[str, object]) -> Path:
    name = "typst.exe" if is_windows_bundle(payload) else "typst"
    candidate = backend_root / "typst" / "bin" / name
    require(candidate.is_file(), f"bundled Typst executable missing: {candidate}")
    return candidate


def validate_typst_bundle(backend_root: Path, payload: dict[str, object]) -> None:
    typst_bin = typst_executable(backend_root, payload)
    fonts_root = backend_root / "fonts"
    packages_root = backend_root / "typst-packages"
    require(fonts_root.is_dir(), f"bundled fonts directory missing: {fonts_root}")
    require(packages_root.is_dir(), f"bundled Typst packages directory missing: {packages_root}")

    env = os.environ.copy()
    env["TYPST_PACKAGE_PATH"] = str(packages_root)

    run_checked([str(typst_bin), "--version"], env=env, cwd=backend_root)

    source = f"""#import "@preview/cmarker:{CMARKER_VERSION}"
#import "@preview/mitex:{MITEX_VERSION}": mitex
#set text(font: "Source Han Serif SC", lang: "zh")

= RetainPDF Typst smoke

中文字体校验：你好，世界。

内置数学公式：$ integral_0^1 x^2 dif x = 1/3 $

#cmarker.render("**Markdown** package smoke with math: $E = mc^2 + \\\\frac{{a}}{{b}}$", math: mitex)
"""

    with tempfile.TemporaryDirectory(prefix="retainpdf-typst-smoke-") as tmp:
        tmp_root = Path(tmp)
        input_path = tmp_root / "smoke.typ"
        output_path = tmp_root / "smoke.pdf"
        input_path.write_text(source, encoding="utf-8")
        command = [
            str(typst_bin),
            "compile",
            "--font-path",
            str(fonts_root),
            str(input_path),
            str(output_path),
        ]
        run_checked(command, env=env, cwd=backend_root)
        require(output_path.is_file() and output_path.stat().st_size > 0, f"Typst smoke output missing: {output_path}")


def main() -> None:
    args = parse_args()
    manifest_path = args.manifest.resolve()
    backend_root = manifest_path.parent
    payload = json.loads(manifest_path.read_text(encoding="utf-8"))

    require(payload.get("rustApiBinaryBundled") is True, "bundle manifest missing Rust API binary")
    require(payload.get("pythonBundled") is True, "bundle manifest missing bundled Python runtime")
    require(payload.get("typstBundled") is True, "bundle manifest missing Typst runtime")
    require(payload.get("typstPackagesBundled") is True, "bundle manifest missing Typst packages")
    require(bool(payload.get("bundledPythonImportCheck")), "bundle manifest missing Python import check result")
    if payload.get("targetPlatformName") == "mac":
        require(bool(payload.get("bundledPythonHome")), "mac bundle manifest missing Python framework home")

    bundled_fonts = payload.get("bundledFonts") or []
    require(
        isinstance(bundled_fonts, list) and len(bundled_fonts) >= args.min_fonts,
        "bundle manifest missing bundled fonts",
    )

    validate_typst_bundle(backend_root, payload)

    print(f"desktop bundle manifest OK: {manifest_path}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""双端默认值一致性门禁。

跨端共享的默认值(Rust API 与 Python 流水线各持一份字面量)必须满足:

1. 两端各有唯一定义点(Rust: ``src/config/defaults.rs``;Python: 见 ``PARITY_TABLE``);
2. 两端定义值相等——门禁直接解析两端定义行比对,不内置第三份拷贝;
3. 字面量不得出现在其他生产代码中(Rust 剥离 ``#[cfg(test)]`` 段、Python 排除
   测试目录),防止绕行定义点重新硬编码。

新增跨端默认值:在 ``PARITY_TABLE`` 登记一行,字面量只写进两端定义点。
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

SCRIPTS_ROOT = Path(__file__).resolve().parents[1]
BACKEND_ROOT = SCRIPTS_ROOT.parent
RUST_SRC = BACKEND_ROOT / "rust_api" / "src"

RUST_DEFAULTS_FILE = "src/config/defaults.rs"

# name -> (rust 定义点正则, python 定义文件, python 定义点正则)
PARITY_TABLE: dict[str, tuple[str, str, str]] = {
    "deepseek_base_url": (
        r'DEEPSEEK_DEFAULT_BASE_URL:\s*&str\s*=\s*"([^"]+)"',
        "services/translation/llm/providers/deepseek/transport.py",
        r'DEFAULT_BASE_URL\s*=\s*"([^"]+)"',
    ),
    "local_llm_model": (
        r'LOCAL_LLM_DEFAULT_MODEL:\s*&str\s*=\s*"([^"]+)"',
        "services/translation/llm/shared/provider_registry.py",
        r'_env\(\s*"RETAIN_LOCAL_LLM_MODEL",\s*"([^"]+)"',
    ),
    "local_llm_base_url": (
        r'LOCAL_LLM_DEFAULT_BASE_URL:\s*&str\s*=\s*"([^"]+)"',
        "services/translation/llm/shared/provider_registry.py",
        r'_env\(\s*"RETAIN_LOCAL_LLM_BASE_URL",\s*"([^"]+)"',
    ),
}


def rust_production_text(path: Path) -> str:
    """剥离 #[cfg(test)] 之后的测试段,只留生产代码。"""
    return path.read_text(encoding="utf-8").split("\n#[cfg(test)]", 1)[0]


def iter_rust_production_sources() -> list[tuple[Path, str]]:
    items: list[tuple[Path, str]] = []
    for path in sorted(RUST_SRC.rglob("*.rs")):
        if path.name.startswith("._"):  # macOS 网络卷 AppleDouble 元数据
            continue
        rel_parts = path.relative_to(RUST_SRC).parts
        # api_tests/ 目录与 tests.rs 文件整体由上级 #[cfg(test)] 挂载,
        # 文件内没有内联 cfg 段可剥离,按测试代码排除。
        if "api_tests" in rel_parts or path.stem in {"tests", "test"}:
            continue
        items.append((path, rust_production_text(path)))
    return items


def iter_python_production_sources() -> list[tuple[Path, str]]:
    items: list[tuple[Path, str]] = []
    for path in sorted(SCRIPTS_ROOT.rglob("*.py")):
        parts = set(path.relative_to(SCRIPTS_ROOT).parts)
        if parts & {"tests", "test", "__pycache__"} or path.name.startswith("._"):
            continue
        items.append((path, path.read_text(encoding="utf-8")))
    return items


def extract(pattern: str, text: str, where: str, errors: list[str]) -> str | None:
    matches = re.findall(pattern, text)
    if len(matches) != 1:
        errors.append(f"{where}: expected exactly 1 definition match, got {len(matches)}")
        return None
    return matches[0]


def main() -> int:
    errors: list[str] = []
    rust_sources = iter_rust_production_sources()
    python_sources = iter_python_production_sources()

    for name, (rust_pattern, python_def_file, python_pattern) in PARITY_TABLE.items():
        rust_def_rel = Path(RUST_DEFAULTS_FILE)
        rust_def_path = BACKEND_ROOT / "rust_api" / RUST_DEFAULTS_FILE
        python_def_path = SCRIPTS_ROOT / python_def_file

        rust_value = extract(
            rust_pattern,
            rust_def_path.read_text(encoding="utf-8"),
            f"{name} @ rust_api/{RUST_DEFAULTS_FILE}",
            errors,
        )
        python_value = extract(
            python_pattern,
            python_def_path.read_text(encoding="utf-8"),
            f"{name} @ scripts/{python_def_file}",
            errors,
        )
        if rust_value is None or python_value is None:
            continue
        if rust_value != python_value:
            errors.append(
                f"{name}: rust 定义值 {rust_value!r} != python 定义值 {python_value!r}"
            )
            continue

        literal = rust_value
        for path, text in rust_sources:
            if literal not in text:
                continue
            occurrences = text.count(literal)
            if path.relative_to(BACKEND_ROOT / "rust_api") == rust_def_rel:
                if occurrences != 1:
                    errors.append(
                        f"{name}: 定义点 {path} 内字面量出现 {occurrences} 次,应为 1 次"
                    )
            else:
                errors.append(
                    f"{name}: 字面量 {literal!r} 出现在非定义点 {path.relative_to(BACKEND_ROOT)} "
                    f"(生产代码,共 {occurrences} 处);请引用 config::defaults 常量"
                )
        for path, text in python_sources:
            if literal not in text:
                continue
            if path == python_def_path:
                continue
            errors.append(
                f"{name}: 字面量 {literal!r} 出现在非定义点 "
                f"{path.relative_to(SCRIPTS_ROOT)};请从定义模块导入常量"
            )

    if errors:
        print("default parity check failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    print(f"default parity check passed ({len(PARITY_TABLE)} defaults consistent across rust/python)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

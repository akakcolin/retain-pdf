#!/usr/bin/env python3
"""TP-018 整书性能/质量回归：公式密集合成用例的前后对照。

原始问题书 job ``20260412152452-4abf73`` 已不在本地（无目录 / PDF / DB 行），无法
复现；回归集因此改用同源公式密集**合成**用例
（``promptfoo/fixtures/tp018/*.json``，frozen ``translation_case_bundle_v1`` 格式，
可直接喂给 ``replay_translation_case_artifact`` 重放）。

``record`` 跑一遍用例并把观测值写进基线 ``tp018_baseline.json``
（schema ``retainpdf_tp018_baseline_v1``）；``check`` 再跑一遍并断言不回归：

- ``windowed`` 路由条目数不高于基线；
- 总耗时不超过 ``基线 * --time-tolerance``（默认 1.0，留给机器抖动）；
- 协议壳 / 空翻译 / 英文残留 / placeholder 丢失 / failed 均不高于基线。

``--results-file`` 可喂入预先录制的 replay 输出（离线、不需要 API key），也是
pytest 验证 record→check 闭环的入口。渲染级差异不在离线门禁内：翻译丢 placeholder
是渲染产物损坏的最直接前兆，这里以 ``placeholder_regressions`` 作为渲染安全代理。
"""
from __future__ import annotations

import argparse
import json
import sys
import tempfile
import time
from datetime import date
from pathlib import Path


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[1]
if str(REPO_SCRIPTS_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_SCRIPTS_ROOT))

from services.translation.llm.validation.english_residue import (
    looks_like_mixed_english_residue_output,
)
from services.translation.llm.validation.english_residue import (
    looks_like_untranslated_english_output,
)
from services.translation.llm.validation.english_residue import unit_source_text
from services.translation.llm.validation.placeholder_tokens import FORMULA_TOKEN_RE
from services.translation.llm.validation.protocol_shell import looks_like_protocol_shell_output


SCHEMA = "retainpdf_tp018_baseline_v1"
DEFAULT_CASES_DIR = Path(__file__).resolve().parent / "promptfoo" / "fixtures" / "tp018"
DEFAULT_BASELINE = Path(__file__).resolve().parent / "tp018_baseline.json"
WINDOWED_MARKER = "windowed"
SEGMENTED_MARKER = "segmented"
HEAVY_SPLIT_MARKER = "heavy_formula_split"

# 逐项断言：metric -> 比较方向（"le" 表示观测值不得超过基线）
LOWER_IS_BETTER = (
    "windowed_cases",
    "protocol_shell",
    "empty_translations",
    "english_residue",
    "placeholder_regressions",
    "failed_cases",
)


def _route_path(replay_output: dict) -> list[str]:
    result = dict(replay_output.get("replay_result") or {})
    diagnostics = dict(result.get("translation_diagnostics") or {})
    return [
        str(part or "").strip()
        for part in (diagnostics.get("route_path") or [])
        if str(part or "").strip()
    ]


def placeholder_regression(source_text: str, translated_text: str) -> bool:
    """译文保留了部分公式 placeholder、却丢掉了另一些 = 渲染必然缺公式。

    只查"丢一部分"这一明确信号；译文一个 placeholder 都没有时无法与 direct_typst
    数学转换区分，不报。
    """
    source_ids = set(FORMULA_TOKEN_RE.findall(source_text or ""))
    if not source_ids:
        return False
    translated_ids = set(FORMULA_TOKEN_RE.findall(translated_text or ""))
    if not translated_ids:
        return False
    return bool(source_ids - translated_ids)


def analyze_case(replay_output: dict) -> dict:
    """把一次 replay 输出压成回归指标。纯函数，离线可测。"""
    result = dict(replay_output.get("replay_result") or {})
    item = dict(replay_output.get("saved_item") or {})
    route_path = _route_path(replay_output)
    translated_text = str(result.get("translated_text") or "")
    source_text = unit_source_text(item)
    return {
        "item_id": str(replay_output.get("item_id") or item.get("item_id") or ""),
        "route_path": route_path,
        "windowed": WINDOWED_MARKER in route_path,
        "segmented": SEGMENTED_MARKER in route_path,
        "heavy_formula_split": HEAVY_SPLIT_MARKER in route_path,
        "final_status": str(result.get("final_status") or ""),
        "protocol_shell": looks_like_protocol_shell_output(translated_text),
        "empty_translation": bool(item.get("should_translate", True))
        and not translated_text.strip(),
        "english_residue": bool(
            looks_like_untranslated_english_output(item, translated_text, target_lang="zh")
            or looks_like_mixed_english_residue_output(item, translated_text, target_lang="zh")
        ),
        "placeholder_regression": placeholder_regression(source_text, translated_text),
        "elapsed_seconds": round(float(replay_output.get("elapsed_seconds") or 0.0), 3),
    }


def summarize(per_case: list[dict]) -> dict:
    return {
        "case_count": len(per_case),
        "windowed_cases": sum(1 for case in per_case if case["windowed"]),
        "segmented_cases": sum(1 for case in per_case if case["segmented"]),
        "heavy_formula_split_cases": sum(1 for case in per_case if case["heavy_formula_split"]),
        "protocol_shell": sum(1 for case in per_case if case["protocol_shell"]),
        "empty_translations": sum(1 for case in per_case if case["empty_translation"]),
        "english_residue": sum(1 for case in per_case if case["english_residue"]),
        "placeholder_regressions": sum(
            1 for case in per_case if case["placeholder_regression"]
        ),
        "failed_cases": sum(1 for case in per_case if case["final_status"] == "failed"),
        "total_seconds": round(sum(case["elapsed_seconds"] for case in per_case), 3),
    }


def evaluate(
    baseline_observed: dict,
    observed: dict,
    *,
    time_tolerance: float = 1.0,
) -> tuple[list[tuple[str, str, str, str]], bool]:
    """观测值 vs 基线观测值。返回 ``(rows, ok)``，row 为 (metric, 基线, 当前, 状态)。"""
    rows: list[tuple[str, str, str, str]] = []
    ok = True
    for metric in LOWER_IS_BETTER:
        base = baseline_observed.get(metric)
        current = observed.get(metric)
        if base is None or current is None:
            rows.append((metric, "N/A", "N/A", "PENDING"))
            continue
        status = "PASS" if current <= base else "FAIL"
        rows.append((metric, str(base), str(current), status))
        ok = ok and current <= base

    base_count = baseline_observed.get("case_count")
    current_count = observed.get("case_count")
    if base_count is None or current_count is None:
        rows.append(("case_count", "N/A", "N/A", "PENDING"))
    else:
        status = "PASS" if current_count >= base_count else "FAIL"
        rows.append(("case_count", f">= {base_count}", str(current_count), status))
        ok = ok and current_count >= base_count

    base_seconds = baseline_observed.get("total_seconds")
    current_seconds = observed.get("total_seconds")
    if base_seconds is None or current_seconds is None:
        rows.append(("total_seconds", "N/A", "N/A", "PENDING"))
    else:
        budget = round(float(base_seconds) * time_tolerance, 3)
        status = "PASS" if current_seconds <= budget else "FAIL"
        rows.append(("total_seconds", f"<= {budget}", f"{current_seconds}", status))
        ok = ok and current_seconds <= budget
    return rows, ok


def print_rows(rows: list[tuple[str, str, str, str]]) -> None:
    width = max(len(row[0]) for row in rows)
    for name, target, current, status in rows:
        print(f"{name:<{width}}  基线 {target:<10}  当前 {current:<10}  {status}")


def _load_case_outputs(cases_dir: Path, results_file: Path | None) -> list[dict]:
    if results_file is not None:
        payload = json.loads(results_file.read_text(encoding="utf-8"))
        outputs = payload.get("cases") if isinstance(payload, dict) else payload
        if not isinstance(outputs, list):
            raise ValueError(f"results file must contain a list or {{'cases': [...]}}: {results_file}")
        return [dict(item) for item in outputs]

    from devtools.replay_translation_item import replay_translation_case_artifact
    from foundation.config import paths

    outputs: list[dict] = []
    # 命中翻译单元缓存会直接返回译文、丢掉 route_path 诊断，路由指标随之变成
    # 缓存状态的函数。回归测量必须每次真打模型，故隔离到一次性临时缓存目录。
    with tempfile.TemporaryDirectory(prefix="tp018-cache-") as cache_dir:
        original_cache_dir = paths.TRANSLATION_UNIT_CACHE_DIR
        paths.TRANSLATION_UNIT_CACHE_DIR = Path(cache_dir)
        try:
            for path in sorted(cases_dir.glob("*.json")):
                started = time.perf_counter()
                output = dict(replay_translation_case_artifact(path))
                output["elapsed_seconds"] = round(time.perf_counter() - started, 3)
                outputs.append(output)
                print(f"replayed {path.name}: {output.get('item_id')}", flush=True)
        finally:
            paths.TRANSLATION_UNIT_CACHE_DIR = original_cache_dir
    return outputs


def _per_case_by_id(per_case: list[dict]) -> dict[str, dict]:
    return {str(case.get("item_id") or ""): case for case in per_case}


def _save_results(path: Path | None, outputs: list[dict]) -> None:
    if path is None:
        return
    path.write_text(
        json.dumps({"cases": outputs}, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    print(f"wrote {path}")


def record(
    baseline_path: Path,
    *,
    cases_dir: Path,
    results_file: Path | None,
    time_tolerance: float,
    save_results: Path | None = None,
) -> int:
    outputs = _load_case_outputs(cases_dir, results_file)
    _save_results(save_results, outputs)
    per_case = [analyze_case(output) for output in outputs]
    baseline = {
        "schema": SCHEMA,
        "updated_at": date.today().isoformat(),
        "note": (
            "样本为公式密集同源合成用例（原始 job 20260412152452-4abf73 已不可复现）；"
            "record 写 before 观测，check 断言 windowed/耗时/检测器不回归。"
        ),
        "targets": {"time_tolerance": time_tolerance},
        "observed": summarize(per_case),
        "cases": _per_case_by_id(per_case),
    }
    baseline_path.write_text(
        json.dumps(baseline, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"wrote {baseline_path}")
    print_rows(
        [
            (metric, "-", str(value), "-")
            for metric, value in sorted(baseline["observed"].items())
        ]
    )
    return 0


def check(
    baseline_path: Path,
    *,
    cases_dir: Path,
    results_file: Path | None,
    time_tolerance: float | None,
    save_results: Path | None = None,
) -> int:
    baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
    if baseline.get("schema") != SCHEMA:
        raise ValueError(f"baseline schema mismatch: {baseline.get('schema')!r}")
    baseline_observed = dict(baseline.get("observed") or {})
    if not baseline_observed:
        raise ValueError(f"baseline has no observed values; run `record` first: {baseline_path}")

    tolerance = (
        time_tolerance
        if time_tolerance is not None
        else float((baseline.get("targets") or {}).get("time_tolerance") or 1.0)
    )
    outputs = _load_case_outputs(cases_dir, results_file)
    _save_results(save_results, outputs)
    per_case = [analyze_case(output) for output in outputs]
    observed = summarize(per_case)
    rows, ok = evaluate(baseline_observed, observed, time_tolerance=tolerance)

    print(f"TP-018 回归对照  baseline={baseline_path}  updated_at={baseline.get('updated_at')}")
    print_rows(rows)

    baseline_cases = dict(baseline.get("cases") or {})
    for item_id, case in sorted(_per_case_by_id(per_case).items()):
        before = dict(baseline_cases.get(item_id) or {})
        if not before:
            continue
        regressed = [
            metric
            for metric in ("windowed", "protocol_shell", "empty_translation", "english_residue", "placeholder_regression")
            if case.get(metric) and not before.get(metric)
        ]
        if regressed:
            print(f"  {item_id}: 新增 {', '.join(regressed)}")
    print(f"结果: {'PASS' if ok else 'FAIL'}")
    return 0 if ok else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="TP-018 整书回归：记录与对照")
    subparsers = parser.add_subparsers(dest="command", required=True)
    for name, help_text in (("record", "跑用例并写基线"), ("check", "跑用例并对照基线")):
        sub = subparsers.add_parser(name, help=help_text)
        sub.add_argument("--cases-dir", type=Path, default=DEFAULT_CASES_DIR)
        sub.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
        sub.add_argument(
            "--results-file",
            type=Path,
            help="预先录制的 replay 输出（离线，不需要 API key）",
        )
        sub.add_argument(
            "--time-tolerance",
            type=float,
            default=None,
            help="check 的耗时预算倍数（默认取基线 targets，缺失按 1.0）",
        )
        sub.add_argument(
            "--save-results",
            type=Path,
            help="把本次 replay 输出落盘，供后续 --results-file 离线复跑",
        )

    args = parser.parse_args(argv)
    if args.command == "record":
        return record(
            args.baseline,
            cases_dir=args.cases_dir,
            results_file=args.results_file,
            time_tolerance=1.0 if args.time_tolerance is None else args.time_tolerance,
            save_results=args.save_results,
        )
    return check(
        args.baseline,
        cases_dir=args.cases_dir,
        results_file=args.results_file,
        time_tolerance=args.time_tolerance,
        save_results=args.save_results,
    )


if __name__ == "__main__":
    raise SystemExit(main())

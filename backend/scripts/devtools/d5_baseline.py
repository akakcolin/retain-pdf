#!/usr/bin/env python3
"""D5 目标度量基线：记录与对照（桌面体积 / Python 渲染进程 / 渲染耗时 / native 命中率）。

基线定义在 `d5_baseline.json`（schema ``retainpdf_d5_baseline_v1``）：

- ``record`` 从真实来源刷新 observed 参考值（桌面体积来自
  bundle-manifest 的 ``sizesBytes.total``，缺失时对 ``--desktop-dir``
  递归求和——镜像 ``prepare-app.mjs`` 的 ``dirSize`` 语义；运行期三类来自
  Prometheus text：``--base-url`` 实时抓取 /metrics 或 ``--metrics-file``
  读离线 dump）。
- ``check`` 对照当前观测值逐项判定并退出 0/1（PENDING 未观测不算失败，
  阈值类类别数据缺失时不阻断）。

四类指标语义：

- 桌面体积：发布包体积，max 兜底 python/typst 运行时回潮。
- python_renderer：``retainpdf_render_jobs_total{renderer=python}`` 是否为 0
  （C3 验收：渲染不再 spawn python3）。
- render_elapsed：``retainpdf_render_elapsed_seconds_sum/count`` 均值。
- native_hit_ratio：``retainpdf_native_hit_ratio{subsystem=...}`` 每子系统命中率。

本模块 stdlib-only，便于在桌面打包机与 CI 上直接运行。
"""
from __future__ import annotations

import argparse
import json
import re
import stat
import urllib.request
from datetime import date
from pathlib import Path

SCHEMA = "retainpdf_d5_baseline_v1"
DEFAULT_BASELINE = Path(__file__).resolve().parent / "d5_baseline.json"
MIB = 1024 * 1024

_METRIC_RE = re.compile(
    r"^([A-Za-z_:][A-Za-z0-9_:]*)(?:\{([^}]*)\})?\s+"
    r"(-?[0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)\s*$"
)


# --------------------------------------------------------------------------
# Prometheus text 解析
# --------------------------------------------------------------------------
def parse_metrics(text: str) -> dict[str, list[tuple[dict[str, str], float]]]:
    """Parse Prometheus exposition lines into ``name -> [(labels, value)]``."""
    out: dict[str, list[tuple[dict[str, str], float]]] = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        match = _METRIC_RE.match(line)
        if not match:
            continue
        name, label_text, value_text = match.groups()
        labels: dict[str, str] = {}
        if label_text:
            for pair in label_text.split(","):
                key, _, value = pair.partition("=")
                labels[key.strip()] = value.strip().strip('"')
        out.setdefault(name, []).append((labels, float(value_text)))
    return out


def series(
    parsed: dict[str, list[tuple[dict[str, str], float]]],
    name: str,
    **filter_labels: str,
) -> list[float]:
    """Values of `name` whose labels match every `filter_labels` pair."""
    return [
        value
        for labels, value in parsed.get(name, [])
        if all(labels.get(key) == expected for key, expected in filter_labels.items())
    ]


def python_renderer_count(
    parsed: dict[str, list[tuple[dict[str, str], float]]],
) -> int | None:
    """Python renderer job count, or None when no render jobs are recorded.

    An absent `retainpdf_render_jobs_total` metric means "no data", not "no
    python"; the empty-parsed / no-render-history case stays PENDING rather
    than falsely asserting absence.
    """
    if "retainpdf_render_jobs_total" not in parsed:
        return None
    return int(sum(series(parsed, "retainpdf_render_jobs_total", renderer="python")))


def render_elapsed_mean(parsed: dict[str, list[tuple[dict[str, str], float]]]) -> float | None:
    count = series(parsed, "retainpdf_render_elapsed_seconds_count")
    total = series(parsed, "retainpdf_render_elapsed_seconds_sum")
    if not count or not total or count[0] == 0:
        return None
    return total[0] / count[0]


def native_hit_ratios(parsed: dict[str, list[tuple[dict[str, str], float]]]) -> dict[str, float]:
    return {
        labels["subsystem"]: value
        for labels, value in parsed.get("retainpdf_native_hit_ratio", [])
        if "subsystem" in labels
    }


# --------------------------------------------------------------------------
# 桌面体积
# --------------------------------------------------------------------------
def _dir_size(root: Path, rel: str) -> int:
    """Recursive regular-file byte sum, mirroring prepare-app.mjs dirSize.

    Symlinked entries are skipped (lstat) so linked-in files are not
    double-counted; a missing path sums to 0.
    """
    target = (root / rel).resolve() if rel else root
    if not target.is_dir():
        return 0
    total = 0
    stack = [target]
    while stack:
        directory = stack.pop()
        for entry in directory.iterdir():
            try:
                lstat = entry.lstat()
            except OSError:
                continue
            if stat.S_ISDIR(lstat.st_mode):
                stack.append(entry)
            elif stat.S_ISREG(lstat.st_mode):
                total += entry.stat().st_size
    return total


def desktop_size_bytes(
    manifest_path: Path | None,
    desktop_dir: Path | None,
) -> int | None:
    """Bundle size from the manifest's ``sizesBytes.total``, else dir sum."""
    if manifest_path is not None and manifest_path.is_file():
        try:
            payload = json.loads(manifest_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            payload = {}
        total = (payload.get("sizesBytes") or {}).get("total")
        if isinstance(total, int):
            return total
    if desktop_dir is not None and desktop_dir.is_dir():
        return _dir_size(desktop_dir, "")
    return None


# --------------------------------------------------------------------------
# 观测聚合
# --------------------------------------------------------------------------
def observe(
    *,
    manifest_path: Path | None,
    desktop_dir: Path | None,
    parsed: dict[str, list[tuple[dict[str, str], float]]],
) -> dict[str, object]:
    """Collect the four categories into one observed snapshot."""
    return {
        "desktop_bytes": desktop_size_bytes(manifest_path, desktop_dir),
        "python_renderer_count": python_renderer_count(parsed),
        "render_elapsed_mean_seconds": render_elapsed_mean(parsed),
        "native_ratios": native_hit_ratios(parsed),
    }


def read_metrics(metrics_file: Path | None, base_url: str | None) -> str | None:
    if metrics_file is not None and metrics_file.is_file():
        return metrics_file.read_text(encoding="utf-8")
    if base_url:
        url = base_url.rstrip("/") + "/metrics"
        with urllib.request.urlopen(url, timeout=10) as response:  # noqa: S310
            return response.read().decode("utf-8")
    return None


# --------------------------------------------------------------------------
# 对照
# --------------------------------------------------------------------------
def _mib(value: int) -> str:
    return f"{value / MIB:.1f} MiB"


def evaluate(baseline: dict, observed: dict) -> tuple[list[tuple[str, str, str, str]], bool]:
    """Compare `observed` against `baseline` targets.

    Returns ``(rows, ok)``; each row is ``(name, target, current, status)``
    with status in PASS / FAIL / PENDING. Missing observations are PENDING,
    never FAIL.
    """
    rows: list[tuple[str, str, str, str]] = []
    ok = True

    desktop_max = baseline["desktop"]["max_bytes"]
    desktop_cur = observed.get("desktop_bytes")
    if desktop_cur is None:
        rows.append(("desktop_bytes", f"<= {_mib(desktop_max)}", "N/A", "PENDING"))
    elif desktop_cur <= desktop_max:
        rows.append(("desktop_bytes", f"<= {_mib(desktop_max)}", _mib(desktop_cur), "PASS"))
    else:
        rows.append(("desktop_bytes", f"<= {_mib(desktop_max)}", _mib(desktop_cur), "FAIL"))
        ok = False

    target_absent = bool(baseline["python_renderer"]["target_absent"])
    python_cur = observed.get("python_renderer_count")
    if python_cur is None:
        rows.append(("python_renderer_absent", str(target_absent), "N/A", "PENDING"))
    elif (python_cur == 0) == target_absent:
        rows.append(("python_renderer_absent", str(target_absent), str(python_cur), "PASS"))
    else:
        rows.append(("python_renderer_absent", str(target_absent), str(python_cur), "FAIL"))
        ok = False

    elapsed_max = baseline["render_elapsed"]["max_mean_seconds"]
    elapsed_cur = observed.get("render_elapsed_mean_seconds")
    if elapsed_cur is None:
        rows.append(("render_elapsed_mean", f"<= {elapsed_max} s", "N/A", "PENDING"))
    elif elapsed_cur <= elapsed_max:
        rows.append(("render_elapsed_mean", f"<= {elapsed_max} s", f"{elapsed_cur:.3f} s", "PASS"))
    else:
        rows.append(("render_elapsed_mean", f"<= {elapsed_max} s", f"{elapsed_cur:.3f} s", "FAIL"))
        ok = False

    ratio_min = baseline["native_hit_ratio"]["min"]
    subsystems = baseline["native_hit_ratio"].get("subsystems", [])
    observed_ratios = observed.get("native_ratios", {})
    native_ok = True
    for subsystem in subsystems:
        ratio = observed_ratios.get(subsystem)
        if ratio is None:
            rows.append((f"native_hit_ratio {subsystem}", f">= {ratio_min:.3f}", "N/A", "PENDING"))
            continue
        if ratio >= ratio_min:
            rows.append(
                (f"native_hit_ratio {subsystem}", f">= {ratio_min:.3f}", f"{ratio:.3f}", "PASS")
            )
        else:
            rows.append(
                (f"native_hit_ratio {subsystem}", f">= {ratio_min:.3f}", f"{ratio:.3f}", "FAIL")
            )
            native_ok = False
    if not native_ok:
        ok = False

    return rows, ok


def print_rows(rows: list[tuple[str, str, str, str]]) -> None:
    width = max(len(row[0]) for row in rows)
    for name, target, current, status in rows:
        print(f"{name:<{width}}  {target:<14}  {current:<12}  {status}")


def check(
    baseline_path: Path,
    *,
    manifest_path: Path | None,
    desktop_dir: Path | None,
    metrics_file: Path | None,
    base_url: str | None,
) -> int:
    baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
    if baseline.get("schema") != SCHEMA:
        raise ValueError(f"baseline schema mismatch: {baseline.get('schema')!r}")
    metrics_text = read_metrics(metrics_file, base_url)
    parsed = parse_metrics(metrics_text) if metrics_text is not None else {}
    observed = observe(
        manifest_path=manifest_path,
        desktop_dir=desktop_dir,
        parsed=parsed,
    )
    rows, ok = evaluate(baseline, observed)
    print(f"D5 目标度量基线对照  baseline={baseline_path}  updated_at={baseline.get('updated_at')}")
    print_rows(rows)
    statuses = {row[3] for row in rows}
    print(f"结果: {'PASS' if ok else 'FAIL'}  ({statuses})")
    return 0 if ok else 1


# --------------------------------------------------------------------------
# 记录
# --------------------------------------------------------------------------
def _default_baseline() -> dict:
    """Fresh target template used only when the baseline file is absent."""
    return {
        "schema": SCHEMA,
        "updated_at": date.today().isoformat(),
        "desktop": {
            "source": "bundle-manifest.json:sizesBytes.total",
            "max_bytes": 200 * MIB,
            "observed_bytes": None,
            "observed_source": None,
            "note": "max 兜底 python/typst 运行时回潮（pythonBundled=false 下回潮即 +100MiB+）",
        },
        "python_renderer": {
            "target_absent": True,
            "observed_count": None,
            "note": "C3 验收：渲染不再 spawn python3；render_jobs_total{renderer=python} 应为 0",
        },
        "render_elapsed": {
            "max_mean_seconds": 60.0,
            "observed_mean_seconds": None,
            "observed_source": None,
            "note": "max 兜底异常卡顿；正常值随文档规模波动",
        },
        "native_hit_ratio": {
            "min": 0.9,
            "subsystems": [
                "source",
                "background",
                "typst",
                "layout",
                "layout_payload",
                "visual_profile",
                "pdf_structure_profile",
                "analysis",
                "source_cleanup_planning",
            ],
            "observed": {},
            "note": "D1 双实现清零后各子系统应趋近 1.0；IN_MEMORY_PAGE 为合法回退会拉低",
        },
    }


def record(
    baseline_path: Path,
    *,
    manifest_path: Path | None,
    desktop_dir: Path | None,
    metrics_file: Path | None,
    base_url: str | None,
) -> int:
    baseline = _default_baseline()
    if baseline_path.is_file():
        loaded = json.loads(baseline_path.read_text(encoding="utf-8"))
        # Deep-merge a partial/older baseline over the default template so every
        # section exists and existing targets are preserved.
        for key, value in loaded.items():
            if isinstance(baseline.get(key), dict) and isinstance(value, dict):
                baseline[key].update(value)
            else:
                baseline[key] = value
    metrics_text = read_metrics(metrics_file, base_url)
    parsed = parse_metrics(metrics_text) if metrics_text is not None else {}
    observed = observe(
        manifest_path=manifest_path,
        desktop_dir=desktop_dir,
        parsed=parsed,
    )
    desktop = observed["desktop_bytes"]
    if desktop is not None:
        baseline["desktop"]["observed_bytes"] = desktop
        from_manifest = False
        if manifest_path is not None and manifest_path.is_file():
            try:
                payload = json.loads(manifest_path.read_text(encoding="utf-8"))
                from_manifest = isinstance((payload.get("sizesBytes") or {}).get("total"), int)
            except (OSError, json.JSONDecodeError):
                pass
        baseline["desktop"]["observed_source"] = (
            f"bundle-manifest sizesBytes.total={desktop}"
            if from_manifest
            else f"desktop_dir recursive sum={desktop}"
        )
    python_count = observed["python_renderer_count"]
    if python_count is not None:
        baseline["python_renderer"]["observed_count"] = python_count
    elapsed = observed["render_elapsed_mean_seconds"]
    if elapsed is not None:
        baseline["render_elapsed"]["observed_mean_seconds"] = round(elapsed, 3)
        baseline["render_elapsed"]["observed_source"] = "metrics render_elapsed sum/count"
    ratios = observed["native_ratios"]
    if ratios:
        baseline["native_hit_ratio"]["observed"] = {k: round(v, 3) for k, v in ratios.items()}
    baseline["updated_at"] = date.today().isoformat()
    baseline_path.write_text(
        json.dumps(baseline, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"wrote {baseline_path}")
    return 0


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------
def _add_common_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--metrics-file", type=Path, help="离线 Prometheus text dump（/metrics）")
    parser.add_argument("--base-url", help="实时抓取 http://<host>:<port>/metrics")
    parser.add_argument(
        "--desktop-manifest",
        type=Path,
        default=None,
        help="desktop/app/backend/bundle-manifest.json（sizesBytes.total）",
    )
    parser.add_argument(
        "--desktop-dir",
        type=Path,
        default=None,
        help="目录递归求和兜底（manifest 无 sizesBytes 时）",
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="D5 目标度量基线：记录与对照")
    subparsers = parser.add_subparsers(dest="command", required=True)

    check_parser = subparsers.add_parser("check", help="对照当前观测与基线目标")
    _add_common_args(check_parser)
    check_parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)

    record_parser = subparsers.add_parser("record", help="从真实来源刷新 observed 参考值")
    _add_common_args(record_parser)
    record_parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)

    args = parser.parse_args(argv)
    if args.command == "check":
        return check(
            args.baseline,
            manifest_path=args.desktop_manifest,
            desktop_dir=args.desktop_dir,
            metrics_file=args.metrics_file,
            base_url=args.base_url,
        )
    return record(
        args.baseline,
        manifest_path=args.desktop_manifest,
        desktop_dir=args.desktop_dir,
        metrics_file=args.metrics_file,
        base_url=args.base_url,
    )


if __name__ == "__main__":
    raise SystemExit(main())

import json
import sys
import tempfile
import unittest
from pathlib import Path


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))

from devtools import tp018_regression as tp018
from services.translation.llm.shared.control_context import SegmentationPolicy
from services.translation.llm.shared.orchestration.heavy_formula import heavy_formula_split_reason
from services.translation.llm.shared.orchestration.segment_risk import formula_segment_translation_route
from services.translation.llm.shared.orchestration.segment_risk import formula_segment_window_count
from services.translation.llm.validation.english_residue import unit_source_text


FIXTURES_DIR = REPO_SCRIPTS_ROOT / "devtools" / "promptfoo" / "fixtures" / "tp018"
BASELINE_PATH = REPO_SCRIPTS_ROOT / "devtools" / "tp018_baseline.json"
RECORDED_PATH = REPO_SCRIPTS_ROOT / "devtools" / "tp018_recorded.json"

# fixture 文件名 -> 期望的公式路由形态。离线断言，不依赖任何 API key。
EXPECTED_ROUTES = {
    "tp018-inline-abbrev": "single",
    "tp018-fragmented-formula": "single",
    "tp018-formula-window-12": "single",
    "tp018-heavy-split": "heavy",
    "tp018-plain-prose": "none",
}


def _load_fixture(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def _fixture_item(bundle: dict) -> dict:
    payload = (bundle.get("replay_input") or {}).get("page_payload") or []
    return payload[0] if payload else {}


def _replay_output(
    item_id: str,
    translated_text: str,
    *,
    route_path: list[str] | None = None,
    source_text: str = "",
    final_status: str = "translated",
    elapsed_seconds: float = 0.1,
) -> dict:
    return {
        "item_id": item_id,
        "elapsed_seconds": elapsed_seconds,
        "saved_item": {
            "item_id": item_id,
            "should_translate": True,
            "block_type": "text",
            "raw_block_type": "text",
            "block_kind": "text",
            "layout_role": "paragraph",
            "semantic_role": "body",
            "structure_role": "body",
            "normalized_sub_type": "body",
            "metadata": {"structure_role": "body"},
            "protected_source_text": source_text,
            "translation_unit_protected_source_text": source_text,
        },
        "replay_result": {
            "translated_text": translated_text,
            "final_status": final_status,
            "translation_diagnostics": {"route_path": list(route_path or [])},
        },
    }


class Tp018FixtureTests(unittest.TestCase):
    def test_fixtures_are_frozen_bundles_with_placeholder_math_mode(self):
        paths = sorted(FIXTURES_DIR.glob("*.json"))
        self.assertEqual(len(paths), len(EXPECTED_ROUTES))
        for path in paths:
            bundle = _load_fixture(path)
            replay_input = bundle.get("replay_input") or {}
            spec = replay_input.get("spec") or {}
            item = _fixture_item(bundle)
            self.assertEqual(bundle.get("schema"), "translation_case_bundle_v1", path.name)
            self.assertEqual(spec.get("math_mode"), "placeholder", path.name)
            self.assertEqual(item.get("math_mode"), "placeholder", path.name)
            self.assertTrue(item.get("should_translate"), path.name)
            self.assertEqual(item.get("item_id"), replay_input.get("item_id"), path.name)
            self.assertEqual(spec.get("credential_ref"), "env:RETAIN_TRANSLATION_API_KEY", path.name)

    def test_formula_fixtures_are_placeholder_dense(self):
        for name in EXPECTED_ROUTES:
            if name == "tp018-plain-prose":
                continue
            item = _fixture_item(_load_fixture(FIXTURES_DIR / f"{name}.json"))
            placeholders = tp018.FORMULA_TOKEN_RE.findall(unit_source_text(item))
            self.assertGreaterEqual(len(placeholders), 4, name)

    def test_fixture_routing_matches_intent_offline(self):
        policy = SegmentationPolicy()
        context = type("_Ctx", (), {"segmentation_policy": policy})()
        for name, expected in EXPECTED_ROUTES.items():
            item = _fixture_item(_load_fixture(FIXTURES_DIR / f"{name}.json"))
            if expected == "heavy":
                self.assertTrue(
                    heavy_formula_split_reason(item, context=context),
                    f"{name} should trigger heavy formula split",
                )
                continue
            self.assertEqual(
                formula_segment_translation_route(item, policy=policy),
                expected,
                name,
            )

    def test_windowed_fixtures_have_multiple_windows(self):
        policy = SegmentationPolicy()
        for name in ("tp018-fragmented-formula", "tp018-formula-window-12"):
            item = _fixture_item(_load_fixture(FIXTURES_DIR / f"{name}.json"))
            self.assertGreater(
                formula_segment_window_count(item, policy=policy),
                1,
                f"{name} should exercise the windowed segment path",
            )


class Tp018AnalyzerTests(unittest.TestCase):
    def test_analyze_case_flags_protocol_shell_and_empty_translation(self):
        shell = tp018.analyze_case(
            _replay_output("a", '{"translated_text": "x"}', source_text="hello world")
        )
        self.assertTrue(shell["protocol_shell"])
        empty = tp018.analyze_case(_replay_output("b", "   ", source_text="hello world"))
        self.assertTrue(empty["empty_translation"])

    def test_analyze_case_flags_english_residue_when_output_copies_source(self):
        source = (
            "The catalytic pathway is discussed at length in the surrounding narrative "
            "and the measured outcomes are reported without any translation."
        )
        case = tp018.analyze_case(_replay_output("c", source, source_text=source))
        self.assertTrue(case["english_residue"])

    def test_placeholder_regression_only_flags_partial_placeholder_loss(self):
        source = "term <f1-a11/> and term <f2-b22/> are defined."
        self.assertFalse(tp018.placeholder_regression(source, "术语 <f1-a11/> 和 <f2-b22/> 被定义。"))
        self.assertFalse(tp018.placeholder_regression(source, "术语和术语被定义。"))
        self.assertTrue(tp018.placeholder_regression(source, "术语 <f1-a11/> 被定义。"))

    def test_analyze_case_reads_route_path_from_diagnostics(self):
        case = tp018.analyze_case(
            _replay_output(
                "d",
                "译文",
                route_path=["block_level", "segmented", "windowed", "failed"],
                final_status="failed",
            )
        )
        self.assertTrue(case["windowed"])
        self.assertTrue(case["segmented"])
        self.assertFalse(case["heavy_formula_split"])
        self.assertEqual(case["final_status"], "failed")

    def test_evaluate_passes_equal_and_fails_regression(self):
        baseline = {"windowed_cases": 0, "total_seconds": 10.0}
        rows, ok = tp018.evaluate(baseline, dict(baseline))
        self.assertTrue(ok)
        _, ok = tp018.evaluate(baseline, {"windowed_cases": 1, "total_seconds": 10.0})
        self.assertFalse(ok)
        _, ok = tp018.evaluate(baseline, {"windowed_cases": 0, "total_seconds": 11.0})
        self.assertFalse(ok)
        _, ok = tp018.evaluate(
            baseline, {"windowed_cases": 0, "total_seconds": 11.0}, time_tolerance=1.2
        )
        self.assertTrue(ok)
        self.assertTrue(any(status == "PASS" for _, _, _, status in rows))

    def test_evaluate_marks_missing_metrics_pending(self):
        rows, ok = tp018.evaluate({"windowed_cases": 0}, {"windowed_cases": 0})
        self.assertTrue(ok)
        self.assertIn(("total_seconds", "N/A", "N/A", "PENDING"), rows)


class Tp018RoundTripTests(unittest.TestCase):
    def _write_results(self, path: Path, outputs: list[dict]) -> None:
        path.write_text(json.dumps({"cases": outputs}, ensure_ascii=False), encoding="utf-8")

    def test_record_then_check_round_trip(self):
        outputs = [
            _replay_output("x", "译文 <f1-a11/>", source_text="term <f1-a11/>", elapsed_seconds=1.0),
            _replay_output("y", "译文", source_text="prose", elapsed_seconds=2.0),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            results = Path(tmp) / "results.json"
            baseline = Path(tmp) / "baseline.json"
            self._write_results(results, outputs)

            self.assertEqual(
                tp018.record(
                    baseline,
                    cases_dir=FIXTURES_DIR,
                    results_file=results,
                    time_tolerance=1.0,
                ),
                0,
            )
            saved = json.loads(baseline.read_text(encoding="utf-8"))
            self.assertEqual(saved["schema"], tp018.SCHEMA)
            self.assertEqual(saved["observed"]["case_count"], 2)

            self.assertEqual(
                tp018.check(
                    baseline,
                    cases_dir=FIXTURES_DIR,
                    results_file=results,
                    time_tolerance=None,
                ),
                0,
            )

            regressed = [
                _replay_output(
                    "x",
                    '{"translated_text": "x"}',
                    source_text="term <f1-a11/>",
                    elapsed_seconds=9.0,
                ),
                outputs[1],
            ]
            self._write_results(results, regressed)
            self.assertEqual(
                tp018.check(
                    baseline,
                    cases_dir=FIXTURES_DIR,
                    results_file=results,
                    time_tolerance=None,
                ),
                1,
            )

            # 空 results 文件不能让门禁空转通过：case_count 低于基线即失败。
            self._write_results(results, [])
            self.assertEqual(
                tp018.check(
                    baseline,
                    cases_dir=FIXTURES_DIR,
                    results_file=results,
                    time_tolerance=None,
                ),
                1,
            )

    def test_committed_baseline_reproduces_from_recorded_results(self):
        self.assertTrue(BASELINE_PATH.exists(), "run `tp018_regression.py record` first")
        self.assertTrue(RECORDED_PATH.exists(), "run `tp018_regression.py record --save-results`")
        baseline = json.loads(BASELINE_PATH.read_text(encoding="utf-8"))
        recorded = json.loads(RECORDED_PATH.read_text(encoding="utf-8"))
        fixture_ids = {
            _fixture_item(_load_fixture(path))["item_id"]
            for path in FIXTURES_DIR.glob("*.json")
        }
        self.assertEqual(set(baseline.get("cases") or {}), fixture_ids)
        self.assertEqual(
            {str(case.get("item_id") or "") for case in recorded.get("cases") or []},
            fixture_ids,
        )
        self.assertEqual(
            tp018.check(
                BASELINE_PATH,
                cases_dir=FIXTURES_DIR,
                results_file=RECORDED_PATH,
                time_tolerance=None,
            ),
            0,
        )


if __name__ == "__main__":
    unittest.main()

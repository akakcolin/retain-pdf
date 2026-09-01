from __future__ import annotations

import json
import sys
from pathlib import Path

REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))

from services.document_schema.contract_v1 import (  # noqa: E402
    _build_layout_role,
    _build_semantic_role,
    _build_structure_role,
)

# Same shared fixture the Rust integration test (tests/role_vectors.rs) asserts
# against. `parents[4]` == repo `backend/`; the fixture lives in the
# rendering_orchestrator crate so both sides read one file.
FIXTURE_PATH = (
    Path(__file__).resolve().parents[4]
    / "rendering_orchestrator"
    / "tests"
    / "fixtures"
    / "role_vectors.json"
)


def _corpus() -> dict:
    with open(FIXTURE_PATH, encoding="utf-8") as handle:
        return json.load(handle)


def test_fixture_exists_and_schema_is_current() -> None:
    assert FIXTURE_PATH.is_file(), f"role vectors fixture not found at {FIXTURE_PATH}"
    corpus = _corpus()
    assert corpus["schema"] == "retainpdf_role_vector_v1"
    assert corpus["schema_version"] == 1


def test_python_role_builders_agree_with_shared_golden() -> None:
    corpus = _corpus()
    cases = corpus["cases"]
    assert len(cases) == 13, "one case per role-decision branch"
    for case in cases:
        block = case["block"]
        expected = case["expected"]
        layout = _build_layout_role(block)
        semantic = _build_semantic_role(block, layout_role=layout)
        structure = _build_structure_role(
            block,
            layout_role=layout,
            semantic_role=semantic,
        )
        assert layout == expected["layout_role"], f"{case['id']}: layout_role"
        assert semantic == expected["semantic_role"], f"{case['id']}: semantic_role"
        assert structure == expected["structure_role"], f"{case['id']}: structure_role"

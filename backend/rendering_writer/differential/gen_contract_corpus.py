#!/usr/bin/env python3
"""D3 bridge-contract corpus generator.

Builds a synthetic two-page PDF, runs the REAL production bundle producer
`run_render_delegate.build_bundle` (render-source prep + document analysis +
prepared-page spec build + visual-profile fill map) plus the bridge-facing
serializers `_page_spec_to_dict` / `render_page_spec_to_bridge`, and records
the six bridge boundaries that cross the Python->Rust wire:

  * emitter_page_specs       (`_page_spec_to_dict` list, full 29-key blocks)
  * redaction_page_specs     (`render_page_spec_to_bridge` list)
  * translated_pages         (page index -> RedactionItem dicts)
  * fill_map                 (item_id -> [r, g, b])
  * precleaned_page_indices  (sorted int list)
  * render_bundle            (the whole `build_bundle` dict)

Each case records its portable shape tree (serialized from the Python TypedDict
via `bridge_shapes.shape_to_schema`) next to the payload, so the Rust strict
gate `contract_strict_diff.rs` never duplicates the shape definitions. The
generator calls `assert_keys_match` on every payload first — a producer key
drift fails loudly here, before the corpus is ever written.

Deterministic: fixed synthetic content, no RNG, sort_keys + indent.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python \
        ../rendering_writer/differential/gen_contract_corpus.py
"""

import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)
sys.path.insert(0, _HERE)

import fitz  # noqa: E402

import orchestrator_parity  # noqa: E402
import smoke_end_to_end_parity as gate  # noqa: E402

from entrypoints.run_render_delegate import build_bundle  # noqa: E402
from foundation.shared.job_dirs import ensure_job_dirs  # noqa: E402
from foundation.shared.job_dirs import resolve_job_dirs  # noqa: E402
from foundation.shared.stage_specs import RenderStageSpec  # noqa: E402
from services.rendering.contracts import bridge_shapes as B  # noqa: E402
from services.rendering.layout.page_specs import build_render_page_specs  # noqa: E402
from services.rendering.source.background._native import (  # noqa: E402
    render_page_spec_to_bridge,
)

# The strict gate lives in rendering_orchestrator/tests (it needs the emitter +
# bundle DTOs), so the corpus is written next to it.
OUT_PATH = os.path.abspath(
    os.path.join(_HERE, "..", "..", "rendering_orchestrator", "tests", "contract_corpus.json")
)

CORPUS_SCHEMA = "retainpdf_contract_corpus_v1"


def _translated_pages() -> dict[int, list[dict]]:
    """Production-shaped items satisfying the translation payload strict
    contract (reuses the orchestrator-parity fixture builder)."""
    return {
        0: [orchestrator_parity._contract_item("p001-b001", 0, 1, [40.0, 40.0, 360.0, 80.0], "你好世界")],
        1: [orchestrator_parity._contract_item("p002-b001", 1, 1, [40.0, 40.0, 360.0, 80.0], "再见世界")],
    }


def _arr(elem: dict) -> dict:
    return {"type": "arr", "elem": elem}


def _map(value: dict) -> dict:
    return {"type": "map", "value": value}


def _write_job_fixture(job_root: Path, source_pdf: Path, translated_pages: dict[int, list[dict]]) -> RenderStageSpec:
    job_dirs = ensure_job_dirs(resolve_job_dirs(job_root))
    source_path = job_dirs.source_dir / "source.pdf"
    shutil.copyfile(source_pdf, source_path)

    for page_idx, items in translated_pages.items():
        (job_dirs.translated_dir / f"page-{page_idx}.json").write_text(
            json.dumps(items, ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
    manifest = {
        "schema": "translation_manifest_v1",
        "schema_version": "translation_manifest_v1",
        "pages": [
            {"page_index": page_idx, "page_number": page_idx + 1, "path": f"page-{page_idx}.json"}
            for page_idx in sorted(translated_pages)
        ],
    }
    (job_dirs.translated_dir / "translation-manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )

    spec = {
        "schema_version": "render.stage.v1",
        "stage": "render",
        "job": {
            "job_id": "contract-corpus",
            "job_root": str(job_root),
            "workflow": "render",
        },
        "inputs": {
            "source_pdf": str(source_path),
            "translations_dir": str(job_dirs.translated_dir),
            "translation_manifest": str(job_dirs.translated_dir / "translation-manifest.json"),
        },
        "params": {
            "start_page": 0,
            "end_page": -1,
            "render_mode": "typst",
            "compile_workers": 0,
            "typst_font_family": "Source Han Serif SC",
            "pdf_compress_dpi": 0,
            "translated_pdf_name": "out.pdf",
            "body_font_size_factor": 1.0,
            "body_leading_factor": 1.0,
            "inner_bbox_shrink_x": 0.0,
            "inner_bbox_shrink_y": 0.0,
            "inner_bbox_dense_shrink_x": 0.0,
            "inner_bbox_dense_shrink_y": 0.0,
            "font_unify_mode": "role_min",
            "source_cleanup_strategy": "pikepdf_text_strip",
            "model": "",
            "base_url": "",
            "credential_ref": "",
        },
    }
    spec_path = job_root / "spec.json"
    spec_path.write_text(json.dumps(spec, ensure_ascii=False, indent=2), encoding="utf-8")
    return RenderStageSpec.load(spec_path)


def _normalize_bundle_paths(bundle: dict, job_root: Path) -> dict:
    """Replace the volatile temp job-root prefix in artifact paths with a stable
    placeholder so regenerating the corpus is byte-deterministic. `mkdtemp`
    returns `/var/...` but producers emit `Path.resolve()`d `/private/var/...`,
    so match either form."""
    out = dict(bundle)
    roots = {str(job_root), os.path.realpath(str(job_root))}
    for key in ("source_pdf", "output_pdf", "work_dir"):
        raw = out.get(key)
        if not isinstance(raw, str):
            continue
        for root in roots:
            if raw.startswith(root):
                out[key] = "<job_root>" + raw[len(root):]
                break
    return out


def _boundary_cases(bundle: dict, job_root: Path) -> list[dict]:
    emitter_value = bundle["page_specs"]

    # Rebuild the same RenderPageSpec objects `build_bundle` consumed so the
    # redaction bridge producer runs on real specs (deterministic inputs).
    redaction_objects = build_render_page_specs(
        source_pdf_path=Path(bundle["source_pdf"]),
        translated_pages=bundle["translated_pages"],
        prepared=True,
    )
    redaction_value = [render_page_spec_to_bridge(spec) for spec in redaction_objects]

    translated_value = bundle["translated_pages"]
    fill_value = bundle["visual_profile_fill_map"]
    precleaned_value = bundle["precleaned_page_indices"]
    bundle_value = _normalize_bundle_paths(bundle, job_root)

    for spec in emitter_value:
        B.assert_keys_match(spec, B.EmitterPageSpecShape)
    for spec in redaction_value:
        B.assert_keys_match(spec, B.RedactionPageSpecShape)
    for items in translated_value.values():
        for item in items:
            B.assert_keys_match(item, B.RedactionItemShape)
    B.assert_keys_match(fill_value, B.FillMapShape)
    B.assert_keys_match(precleaned_value, B.PrecleanedPageIndicesShape)
    B.assert_keys_match(bundle_value, B.RenderBundleShape)

    bundle_shape = B.shape_to_schema(B.RenderBundleShape)
    return [
        {
            "name": "emitter_page_specs",
            "shape": _arr(B.shape_to_schema(B.EmitterPageSpecShape)),
            "value": emitter_value,
        },
        {
            "name": "redaction_page_specs",
            "shape": _arr(B.shape_to_schema(B.RedactionPageSpecShape)),
            "value": redaction_value,
        },
        {
            "name": "translated_pages",
            "shape": bundle_shape["fields"]["translated_pages"],
            "value": translated_value,
        },
        {"name": "fill_map", "shape": B.shape_to_schema(B.FillMapShape), "value": fill_value},
        {
            "name": "precleaned_page_indices",
            "shape": B.shape_to_schema(B.PrecleanedPageIndicesShape),
            "value": precleaned_value,
        },
        {"name": "render_bundle", "shape": bundle_shape, "value": bundle_value},
    ]


def main() -> int:
    job_root = Path(tempfile.mkdtemp(prefix="retainpdf-contract-corpus-"))
    source_pdf = job_root / "source.pdf"
    gate._build_source_pdf(source_pdf)
    translated_pages = _translated_pages()
    try:
        spec = _write_job_fixture(job_root, source_pdf, translated_pages)
        bundle = build_bundle(spec)
        cases = _boundary_cases(bundle, job_root)
    finally:
        shutil.rmtree(job_root, ignore_errors=True)

    corpus = {"schema": CORPUS_SCHEMA, "cases": cases}
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    for case in cases:
        print(
            f"{case['name']:<24} value bytes={len(json.dumps(case['value']))}"
        )
    print(f"wrote {OUT_PATH} with {len(cases)} boundaries")
    return 0


if __name__ == "__main__":
    sys.exit(main())

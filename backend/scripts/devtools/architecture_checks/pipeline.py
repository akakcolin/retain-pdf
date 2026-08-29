#!/usr/bin/env python3
from __future__ import annotations

import sys

from devtools.architecture_checks.common import ArchitectureCheckSyntaxError
from devtools.architecture_checks.entrypoints import check_entrypoint_stable_imports
from devtools.architecture_checks.entrypoints import check_stage_spec_contract_checker
from devtools.architecture_checks.fitz_imports import check_fitz_import_allowlist
from devtools.architecture_checks.providers import check_ocr_provider_boundaries
from devtools.architecture_checks.providers import check_pipeline_provider_leaks
from devtools.architecture_checks.providers import check_service_provider_raw_leaks
from devtools.architecture_checks.rendering import check_render_pipeline_facade_boundary
from devtools.architecture_checks.rendering import check_rendering_internal_boundaries
from devtools.architecture_checks.translation import check_devtools_translation_internal_usage
from devtools.architecture_checks.translation import check_translation_internal_boundaries
from devtools.architecture_checks.translation import check_translation_pipeline_facade_boundary
from devtools.architecture_checks.translation import check_translation_public_surface_usage
from devtools.architecture_checks.translation import check_translation_rendering_separation
from devtools.architecture_checks.translation import check_translation_worker_protocol
from devtools.architecture_checks.translation_field_writers import check_translation_payload_field_writers


def main() -> int:
    errors: list[str] = []
    try:
        check_pipeline_provider_leaks(errors)
        check_service_provider_raw_leaks(errors)
        check_entrypoint_stable_imports(errors)
        check_ocr_provider_boundaries(errors)
        check_translation_worker_protocol(errors)
        check_stage_spec_contract_checker(errors)
        check_translation_pipeline_facade_boundary(errors)
        check_translation_public_surface_usage(errors)
        check_devtools_translation_internal_usage(errors)
        check_render_pipeline_facade_boundary(errors)
        check_rendering_internal_boundaries(errors)
        check_translation_rendering_separation(errors)
        check_translation_internal_boundaries(errors)
        check_translation_payload_field_writers(errors)
        check_fitz_import_allowlist(errors)
    except ArchitectureCheckSyntaxError as exc:
        errors.append(str(exc))
    if errors:
        print("pipeline architecture check failed:", file=sys.stderr)
        for item in errors:
            print(f"- {item}", file=sys.stderr)
        return 1
    print("pipeline architecture check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

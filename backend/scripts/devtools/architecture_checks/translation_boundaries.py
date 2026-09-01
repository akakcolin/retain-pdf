from __future__ import annotations

from devtools.architecture_checks.common import SCRIPTS_ROOT
from devtools.architecture_checks.common import imported_from_symbols
from devtools.architecture_checks.common import imported_modules
from devtools.architecture_checks.common import module_allowed
from devtools.architecture_checks.common import read_text
from devtools.architecture_checks.common import rel
from devtools.architecture_checks.common import scan_py_files
from devtools.architecture_checks.translation_rules import TRANSLATE_ONLY_ENTRYPOINT
from devtools.architecture_checks.translation_rules import TRANSLATION_ALLOWED_ROOT_DIRS
from devtools.architecture_checks.translation_rules import TRANSLATION_ALLOWED_ROOT_FILES
from devtools.architecture_checks.translation_rules import TRANSLATION_LAYER_IMPORT_EXCEPTIONS
from devtools.architecture_checks.translation_rules import TRANSLATION_LAYER_IMPORT_RULES
from devtools.architecture_checks.translation_rules import TRANSLATION_REMOVED_COMPAT_IMPORTS
from devtools.architecture_checks.translation_rules import TRANSLATION_ROOT
from devtools.architecture_checks.translation_rules import TRANSLATION_SHARED_COMPAT_IMPORTS
from devtools.architecture_checks.translation_rules import TRANSLATION_WORKFLOW_ALLOWED_DIRS
from devtools.architecture_checks.translation_rules import TRANSLATION_WORKFLOW_ALLOWED_FILES
from devtools.architecture_checks.translation_rules import TRANSLATION_WORKFLOW_PRIVATE_IMPORT_EXCEPTIONS
from devtools.architecture_checks.translation_rules import TRANSLATION_WORKFLOW_SUBPACKAGE_RULES
from devtools.architecture_checks.translation_rules import translation_layer_for


def check_translation_internal_boundaries(errors: list[str]) -> None:
    for path in TRANSLATION_ROOT.iterdir():
        if path.name == "__pycache__" or path.name.startswith("._"):
            continue  # AppleDouble resource-fork companions are not Python
        if path.is_dir() and path.name not in TRANSLATION_ALLOWED_ROOT_DIRS:
            errors.append(
                f"services/translation/{path.name}: unexpected translation root directory; update architecture rules or move it into a named layer"
            )
        if path.is_file() and path.name not in TRANSLATION_ALLOWED_ROOT_FILES:
            errors.append(
                f"services/translation/{path.name}: unexpected translation root file; place new code inside entrypoints/workflow/core/services/llm/artifacts."
            )

    workflow_root = TRANSLATION_ROOT / "workflow"
    for path in workflow_root.iterdir():
        if path.name.startswith("._"):
            continue  # AppleDouble resource-fork companions are not Python
        if path.is_dir() and path.name not in TRANSLATION_WORKFLOW_ALLOWED_DIRS:
            errors.append(
                f"{rel(path)}: unexpected workflow directory; use batching/legacy/phases/scheduling or update architecture rules"
            )
        if path.is_file() and path.name not in TRANSLATION_WORKFLOW_ALLOWED_FILES:
            errors.append(
                f"{rel(path)}: unexpected workflow root file; place implementation in phases/scheduling/batching/legacy"
            )

    for path in scan_py_files(workflow_root):
        try:
            parts = path.relative_to(workflow_root).parts
        except ValueError:
            continue
        if len(parts) < 2:
            continue
        subpackage = parts[0]
        allowed_prefixes = TRANSLATION_WORKFLOW_SUBPACKAGE_RULES.get(subpackage)
        if allowed_prefixes is None:
            continue
        for module in imported_modules(path):
            if not module.startswith(("services.translation.", "services.pipeline_shared.")):
                continue
            if module_allowed(module, allowed_prefixes):
                continue
            errors.append(
                f"{rel(path)}: workflow/{subpackage} must not import '{module}' directly"
            )

    private_import_prefix = "services.translation.workflow."
    for path in scan_py_files(workflow_root):
        rel_to_translation = path.relative_to(TRANSLATION_ROOT)
        exception_prefixes = TRANSLATION_WORKFLOW_PRIVATE_IMPORT_EXCEPTIONS.get(rel_to_translation, ())
        for module, symbol in imported_from_symbols(path):
            if not module.startswith(private_import_prefix):
                continue
            if not symbol.startswith("_"):
                continue
            full_name = f"{module}.{symbol}"
            if any(full_name.startswith(prefix) for prefix in exception_prefixes):
                continue
            errors.append(
                f"{rel(path)}: do not import private workflow symbol '{full_name}' across modules; expose a public helper or keep it local"
            )

    public_init = TRANSLATION_ROOT / "public" / "__init__.py"
    public_text = read_text(public_init)
    forbidden_public_eager_imports = (
        "from services.translation.",
        "import services.translation.",
    )
    for item in forbidden_public_eager_imports:
        if item in public_text:
            errors.append(
                f"{rel(public_init)}: public facade must stay lazy; register exports in _EXPORTS instead of eager import '{item}'"
            )
            break

    forbidden_runtime_imports = (
        "from runtime.pipeline",
        "import runtime.pipeline",
    )
    for path in scan_py_files(SCRIPTS_ROOT):
        for module in imported_modules(path):
            if module_allowed(module, TRANSLATION_REMOVED_COMPAT_IMPORTS):
                errors.append(
                    f"{rel(path)}: removed translation compat import '{module}'; use the real core/entrypoints/llm path"
                )
                break

    for path in scan_py_files(TRANSLATION_ROOT):
        if path == TRANSLATE_ONLY_ENTRYPOINT:
            continue
        if translation_layer_for(path) == "workflow":
            continue
        text = read_text(path)
        rel_path = rel(path)
        for item in forbidden_runtime_imports:
            if item in text:
                errors.append(
                    f"{rel_path}: translation internals must not import runtime.pipeline directly"
                )
                break

    for path in scan_py_files(TRANSLATION_ROOT / "llm" / "providers"):
        text = read_text(path)
        rel_path = rel(path)
        forbidden = (
            "from services.translation.workflow",
            "import services.translation.workflow",
            "from services.translation.services.policy",
            "import services.translation.services.policy",
            "from runtime.pipeline",
            "import runtime.pipeline",
        )
        for item in forbidden:
            if item in text:
                errors.append(
                    f"{rel_path}: provider modules must stay transport-only and must not import workflow/policy/runtime"
                )
                break

    for path in scan_py_files(TRANSLATION_ROOT / "core" / "payload"):
        text = read_text(path)
        rel_path = rel(path)
        forbidden = (
            "from services.translation.llm",
            "import services.translation.llm",
            "from services.translation.workflow",
            "import services.translation.workflow",
            "from services.translation.workflow.batching",
            "import services.translation.workflow.batching",
            "from services.translation.services.fast_path",
            "import services.translation.services.fast_path",
            "from services.translation.services.results",
            "import services.translation.services.results",
            "from services.translation.services.memory",
            "import services.translation.services.memory",
            "from runtime.pipeline",
            "import runtime.pipeline",
        )
        for item in forbidden:
            if item in text:
                errors.append(
                    f"{rel_path}: payload layer must remain data construction/application only and must not import execution/cache/provider layers"
                )
                break

    for path in scan_py_files(TRANSLATION_ROOT):
        layer = translation_layer_for(path)
        if layer is None:
            continue
        allowed_prefixes = TRANSLATION_LAYER_IMPORT_RULES[layer]
        exception_prefixes = TRANSLATION_LAYER_IMPORT_EXCEPTIONS.get(path.relative_to(TRANSLATION_ROOT), ())
        for module in imported_modules(path):
            if not module.startswith("services.translation."):
                continue
            if module_allowed(module, TRANSLATION_SHARED_COMPAT_IMPORTS):
                continue
            if module_allowed(module, allowed_prefixes) or module_allowed(module, exception_prefixes):
                continue
            errors.append(
                f"{rel(path)}: translation layer '{layer}' must not import '{module}' directly"
            )

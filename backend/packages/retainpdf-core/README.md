# retainpdf-core

`retainpdf-core` is the first packaging layer for the RetainPDF Python
pipeline. It exposes the existing `backend/scripts` packages through an
editable Python package without moving production code yet.

Install from the repository root:

```bash
python -m pip install -e backend/packages/retainpdf-core
```

Smoke-check imports:

```bash
python -c "import foundation, runtime, services"
```

Production worker commands registered by this package:

```bash
retainpdf-run-translate-only
retainpdf-validate-document-schema
```

The Rust API may continue using the existing script paths while this package
boundary is hardened. Later phases can switch workers from script paths to
console commands or `python -m entrypoints...`.

To let the Rust API launch installed worker commands instead of direct script
paths, install this package into the Python environment used by `PYTHON_BIN`
and start Rust API with:

```bash
RUST_API_PYTHON_ENTRYPOINT_MODE=console
```

The default remains `script`, so local and desktop bundles that still rely on
`RUST_API_SCRIPTS_DIR` keep working.

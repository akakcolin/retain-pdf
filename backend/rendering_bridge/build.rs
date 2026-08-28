//! Emits the macOS `-undefined dynamic_lookup` linker flag required to link a
//! pyo3 extension module without libpython. pyo3 0.29 no longer emits this
//! automatically; `pyo3_build_config::add_extension_module_link_args` is the
//! documented way for a crate's own build script to supply it.

fn main() {
    pyo3_build_config::add_extension_module_link_args();
}

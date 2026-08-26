//! Port of `output/typst/shared.py::escape_typst_string`.

/// Escape backslash, double-quote and newline for embedding a string inside
/// Typst source (order matters: backslash first so the others stay literal).
pub fn escape_typst_string(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

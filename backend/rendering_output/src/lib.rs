//! Port of the production Python **Typst output layer**
//! (`services/rendering/output/typst/`): turn render page-specs into Typst
//! source, compile it with the `typst` CLI, and merge the result onto a base
//! PDF. Verified against production via golden-replay differential tests.

pub mod block_config;
pub mod block_fields;
pub mod block_fit;
pub mod block_markup;
pub mod block_renderer;
pub mod compile;
pub mod dto;
pub mod emitter;
pub mod escape;
pub mod fit_helpers;
pub mod formula_safety;
pub mod inline_passthrough;
pub mod latex_normalizer;
pub mod merge;
pub mod source_builder;
pub mod source_pages;
pub mod pyre;
pub mod text_analysis;
pub mod text_tokens;
pub mod util;

//! Ports of backend/scripts/services/rendering/source_cleanup/pdf/ pure logic.
//!
//! `content_stream` provides the lexer/serializer so operands arrive normalized
//! to `pdf_math::Operand`; the decision modules consume only that enum. PDF
//! iteration and writing (pikepdf) are Phase 5C scope.

pub mod constants;
pub mod content_stream;
pub mod hit_test;
pub mod path_removal;
pub mod pdf_math;
pub mod planning;
pub mod stream_state;
pub mod text_ops;
pub mod text_removal;

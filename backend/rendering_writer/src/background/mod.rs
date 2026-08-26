//! Background fill/cleanup logic, port of `source/background/`.
//!
//! Phase 5D-6: the pure pixel/geometry logic (`config`, `sampling`, `patch`,
//! `fill`) that needs no mupdf/fitz bindings — everything operates on raw
//! pixel buffers and `[x0, y0, x1, y1]` rects. Phase 5D-7 adds the mupdf
//! bindings (`detect`, `extract`, `image_route`, `stage`) on top.

pub mod config;
pub mod detect;
pub mod extract;
pub mod fill;
pub mod image_route;
pub mod patch;
pub mod redaction;
pub mod sampling;
pub mod stage;
pub mod toc;

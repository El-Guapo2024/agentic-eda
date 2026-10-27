//! INACTIVE (2026-10-28): replaced by the C++ binding to KiCad's vendored
//! Clipper2 2.0.1 (`crate::cpp`, built from `third_party/clipper2` by
//! `build.rs`); kept for a future all-Rust build. Turn it back on for every
//! caller with the `rust-port` cargo feature (off by default). It still
//! compiles in every build and `tests/differential.rs` runs it against the
//! binding on every `cargo test`, so it does not rot.
//!
//! A faithful Rust port of Clipper2's `Clipper64` engine (the only
//! numeric/PolyTree flavor KiCad's `SHAPE_POLY_SET` ever instantiates), its
//! `ClipperOffset` path-offsetting module, `RectClip64` and `SimplifyPath`,
//! ported from KiCad's vendored snapshot of Clipper2
//! (`thirdparty/clipper2/Clipper2Lib`), not reimplemented from memory -- see
//! each module's doc comment for the specific source files and the handful
//! of deliberate, documented scope cuts (no `ClipperD` double-precision
//! path, no `ReuseableDataContainer64`, no `DeltaCallback64`, no
//! `RectClipLines64`: none of these have a call site anywhere in KiCad's
//! `pcbnew`/`libs/kimath`). The Z callback (`USINGZ`) is ported.

pub mod engine;
pub mod offset;
pub mod rectclip;
pub mod simplify;

pub use engine::Clipper64;
pub use offset::ClipperOffset;
pub use rectclip::{rect_clip, RectClip64};
pub use simplify::{simplify_path, simplify_paths};

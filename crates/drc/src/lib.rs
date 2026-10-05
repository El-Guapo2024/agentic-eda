//! The geometry core other crates build on: KiCad's shapes and collision
//! (`kimath`), the flattened board (`board`), clearance and rule resolution
//! (`constraints`, `pcbexpr`), a spatial index (`rtree`), the zone-fill driver
//! (`fill`, over `eda_zone_filler`) and the stroke font (`stroke_font`).
//!
//! The interactive router (`eda_pns`), the zone filler, live connectivity
//! (`eda_connectivity`), the `.kicad_pcb` exporter and the renderer all use it.
//!
//! This crate is **not** a design-rule checker any more. DRC, ERC, plots and
//! every other batch job KiCad already does are kicad-cli's, run on the
//! exported design (`eda_kicad_engine`, docs/ARCHITECTURE.md "Engines"); our
//! own checks, the ones KiCad does not have, are `eda_lint`'s. The crate keeps
//! its name for now.

pub mod board;
pub mod constraints;
pub mod fill;
pub mod kimath;
pub mod pcbexpr;
pub mod rtree;
pub mod stroke_font;

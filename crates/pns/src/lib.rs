//! A port of KiCad's interactive push-and-shove PCB router (`PNS`,
//! `pcbnew/router/*`) to Rust, for the React studio's route tool.
//!
//! See `PARITY.md` (this crate's root) for exactly which KiCad files each
//! module ports, what's faithful, and what's deliberately simplified or
//! out of scope.
//!
//! ## Architecture
//! - [`node::Node`] is the router's "world" -- every item plus the joint
//!   graph connecting them, built from the project's own IR
//!   ([`from_ir::build_node`]) and queryable for collisions.
//! - [`line_placer::LinePlacer`] drives interactive single-track routing
//!   (the X tool): head/tail, posture, via placement, end snapping.
//! - [`walkaround`] and [`shove`] are the two obstacle-resolution
//!   strategies a placer (or the dragger) can call into.
//! - [`dragger::Dragger`] drags an existing segment/via, keeping its
//!   connections, by reusing the same shove/walkaround machinery.
//! - [`router::Router`] is the top-level session object the HTTP API
//!   layer (`crates/cli/src/studio.rs`) drives: one per in-progress
//!   route/drag, holding the committed [`node::Node`] plus whatever
//!   transient preview state the current operation needs.
//!
//! Everything here is pure computation: no I/O, no knowledge of the HTTP
//! layer or the JSON `Cmd` wire format. `crates/cli/src/studio.rs` is the
//! only place that imports both this crate and `eda_ops`.

pub mod diff_pair;
pub mod direction45;
pub mod dragger;
pub mod from_ir;
pub mod hull;
pub mod index;
pub mod item;
pub mod joint;
pub mod layer;
pub mod line;
pub mod line_placer;
pub mod meander;
pub mod node;
pub mod optimizer;
pub mod router;
pub mod settings;
pub mod shove;
pub mod line_walk;
pub mod walkaround;

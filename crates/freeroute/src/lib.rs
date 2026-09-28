//! A port of FreeRouting's routing core onto our own board model.
//!
//! FreeRouting is a gridless, shape-based autorouter: free space is kept as
//! exact 45-degree polygons, searched as rooms and the doors between them,
//! and existing traces can be pushed aside to make room. Our own router
//! (`eda-router`) rasterises the board into cells instead. This crate ports
//! FreeRouting's algorithms -- not its file formats, GUI or Java plumbing --
//! so it reads and writes `design.json` directly, with no DSN round-trip.
//!
//! Ported phase by phase, each checked before the next is built on it:
//! geometry kernel -> spatial index -> board model -> room maze search ->
//! push-and-shove -> optimiser. Each is checked against FreeRouting v1.9
//! itself: `parity/` holds Java harnesses that dump what FreeRouting does
//! on a board, and `tests/` replays those dumps against the port.
//!
//! [`dump::read_board`] reads a board as FreeRouting loaded it;
//! [`autoroute::maze::MazeSearch`] searches a connection on it.
//!
//! Licence: GPL-3.0, inherited from FreeRouting. See Cargo.toml.

pub mod geometry;
pub mod searchtree;
pub mod room;
pub mod door;
pub mod routing;
pub mod rules;
pub mod model;
pub mod board;
pub mod dump;
pub mod autoroute;

//! KiCad's IPC API through `kicad-cli api-server`: the second kicad-cli lane (docs/ARCHITECTURE.md, "Engines").
//!
//! The per-run lane (the rest of this crate) starts a kicad-cli for every DRC, ERC and export, and each run pays KiCad's start-up. This lane keeps one
//! `kicad-cli api-server` up ([`server`]) and sends it requests: the protobuf messages of KiCad's API ([`proto`], generated from `api/proto`) over an nng
//! `req0` socket ([`sp`]). It does what the per-run commands cannot: import a Specctra session ([`specctra`]: `ImportSpecctraSession`, which has no
//! `kicad-cli pcb import` counterpart), and run board and schematic export jobs without a process start each ([`jobs`]).
//!
//! It is optional by design. A kicad-cli without `api-server` (the Mac's 10.99.0-5334, KiCad 9), one whose server does not know a command
//! (`ImportSpecctraSession` is "Since: 11.0"), or `EDA_KICAD_API=0` leaves every caller on its other path: the per-run kicad-cli for exports, our
//! own session import (`eda_kicad::specctra`, INACTIVE otherwise) for Specctra. [`server::ApiError::falls_back`] says when.
//!
//! An export takes the lane only where it pays: the studio's long-lived process ([`long_lived_process`]) or `EDA_KICAD_API` set true; a one-shot CLI
//! command runs kicad-cli per job, since a server start costs more than the job saves (`exports_use_server`).
//!
//! The server does not run a DRC or an ERC (`InjectDrcError` only, no `RunDrc`), so those stay per-run.

pub mod jobs;
pub mod proto;
pub mod server;
pub mod sp;
pub mod specctra;

pub use server::{exports_use_server, long_lived_process, shutdown, unavailable, with_server, ApiError, Client};

//! Specctra DSN export and SES import (`pcbnew/specctra_import_export`), File > Export > Specctra DSN... and File > Import > Specctra Session...
//! (`pcbnew.EditorControl.exportSpecctraDSN`, `pcbnew.EditorControl.importSpecctraSession`).
//!
//! This is an exporter of our own, next to kicad-cli's, and an approved exception to "kicad-cli is the only exporter" (docs/ARCHITECTURE.md): kicad-cli has no DSN or SES
//! command, and the studio's autorouter (`crates/freeroute`) works on `design.json` directly and is left alone -- the DSN and the session are for outside autorouters such
//! as FreeRouting.
//!
//! The port is `SPECCTRA_DB`'s, file by file:
//! - [`format`]  `OUTPUTFORMATTER` and the number formats;
//! - [`model`]   the element tree of `specctra.h` with its `Format` functions (the writer);
//! - [`lexer`]   `DSNLEXER` in Specctra mode;
//! - [`parse`]   `doPCB`, `doSESSION` and the `do..` functions below them (`LoadPCB`, `LoadSESSION`).

pub mod format;
pub mod lexer;
pub mod model;
pub mod parse;

#[cfg(test)]
mod tests;

//! Specctra DSN export and SES import (`pcbnew/specctra_import_export`), File > Export > Specctra DSN... and File > Import > Specctra Session...
//! (`pcbnew.EditorControl.exportSpecctraDSN`, `pcbnew.EditorControl.importSpecctraSession`).
//!
//! The session import is ours, an approved exception to "kicad-cli is the only engine" (docs/ARCHITECTURE.md): kicad-cli has no SES command. The DSN export is
//! kicad-cli's (`kicad-cli pcb export specctra`) where the kicad-cli found has the command; [`export_dsn`] is the fallback for one that has not, deprecated and
//! to be deleted once every machine has it (see its doc for what goes with it). The studio's autorouter (`crates/freeroute`) works on `design.json` directly and
//! is left alone -- the DSN and the session are for outside autorouters such as FreeRouting.
//!
//! The port is `SPECCTRA_DB`'s, file by file:
//! - [`format`]  `OUTPUTFORMATTER` and the number formats;
//! - [`model`]   the element tree of `specctra.h` with its `Format` functions (the writer);
//! - [`lexer`]   `DSNLEXER` in Specctra mode;
//! - [`parse`]   `doPCB`, `doSESSION` and the `do..` functions below them (`LoadPCB`, `LoadSESSION`);
//! - [`export`]  `FromBOARD`, and [`session`] `FromSESSION` (`specctra_import.cpp`), which `eda_ops`' `Cmd::Specctra` applies to the design.

pub mod board;
pub mod export;
pub mod format;
pub mod geom;
pub mod lexer;
pub mod model;
pub mod parse;
pub mod session;

#[cfg(test)]
mod tests;

use eda_model::units::Nm;
use eda_model::ir::{Design, Shape};
use eda_model::{CheckResult, ConstraintModel};

/// The text of a DSN design and what the export had to say besides: the warnings `FromBOARD` gives (`wxLogWarning`), such as a malformed board outline.
#[derive(Debug, Clone)]
pub struct DsnExport {
    pub text: String,
    pub warnings: Vec<String>,
}

/// What `host_version` says: the KiCad this port follows.
pub const HOST_VERSION: &str = "10.99.0";

/// `PCB_EDIT_FRAME::ExportSpecctraFile` for a design: the board file the studio derives from it (`export_kicad_pcb`), read as KiCad reads a board, written as `FromBOARD`
/// and `ExportPCB` write it. `pcb_name` is the file's name, the design's name in the file (`aNameChange`). An error is what KiCad's dialog shows after "Unable to export,
/// please fix and try again".
///
/// DEPRECATED (fallback for kicad-cli without pcb export specctra; delete once every machine has it). The studio and `eda board export specctra` take
/// `kicad-cli pcb export specctra` where the kicad-cli found has it (`crates/cli/src/specctra_api.rs`); this is what a machine whose kicad-cli has not (10.99.0-5334
/// and older) gets instead. Deleting it takes [`export`], [`DsnExport`], [`shape_in_nm`] and the writer's tests with it; the session import keeps [`lexer`],
/// [`parse`], [`model`], [`format`], [`geom`], [`board`] and [`session`].
pub fn export_dsn(design: &Design, model: &ConstraintModel, pcb_name: &str) -> Result<DsnExport, Vec<CheckResult>> {
    let meta = crate::ExportMeta { date: "", title: "" };
    let pcb_text = crate::export_kicad_pcb(design, model, &meta)?;
    let mut board = board::BoardView::parse(&pcb_text).map_err(|e| vec![CheckResult::fail("specctra.board", "board", e)])?;

    // `BuiltBoardOutlines`: the Edge.Cuts items in the order the board file has them, chained into the outline and its cutouts -- in nanometres, at the board's
    // `m_MaxError` and `GetOutlinesChainingEpsilon()` (`BOARD::GetBoardPolygonOutlines`), so a curve's corners are where KiCad puts them, not rounded to micrometres.
    let outline = {
        let mut shapes: Vec<Shape> = Vec::new();
        if let Some(d) = &design.drawings {
            let mut edge: Vec<&Shape> = d.shapes.iter().filter(|s| eda_model::outline::is_edge_cuts(s)).collect();
            edge.sort_by(|a, b| a.id().cmp(b.id()));
            shapes.extend(edge.into_iter().cloned());
        }
        if !eda_model::outline::outline_is_shapes(design) {
            if let Some(pl) = &design.placement {
                let n = pl.outline.len();
                if n >= 2 {
                    for i in 0..n {
                        shapes.push(Shape::Segment { id: String::new(), layer: "Edge.Cuts".into(), stroke_width: Nm::ZERO, filled: false, start: pl.outline[i], end: pl.outline[(i + 1) % n] });
                    }
                }
            }
        }
        let max_error = model.board.max_error_nm;
        let mut outline = eda_drc::outline::build_board_polygon_outlines(&shapes, max_error.as_f64(), eda_model::outline::CHAINING_EPSILON_NM, true);
        // "Make polygon strictly simple to avoid issues (especially in 3D viewer)".
        outline.polys.simplify();
        outline
    };

    let b = &model.board;
    let facts_of = |net: &str| -> export::ClassFacts {
        export::ClassFacts {
            name: b.effective_class(net).name,
            track_width: b.width_of(net).get(),
            clearance: b.clearance_of(net).get(),
            via_diameter: b.via_diameter_of(net).get(),
            via_drill: b.via_drill_of(net).get(),
        }
    };
    let default_class = export::ClassFacts { name: "Default".into(), track_width: b.track_width.get(), clearance: b.clearance.get(), via_diameter: b.via_diameter.get(), via_drill: b.via_drill.get() };
    let settings = export::Settings {
        pcb_name: pcb_name.to_string(),
        host_version: HOST_VERSION.to_string(),
        outline: &outline,
        hole_clearance: b.hole_clearance_nm.get(),
        max_error: b.max_error_nm.get(),
        default_class,
        class_of_net: &facts_of,
    };
    let (pcb, warnings) = export::from_board(&mut board, &settings).map_err(|e| vec![CheckResult::fail("specctra.export", "board", e.0)])?;
    Ok(DsnExport { text: pcb.to_text(), warnings })
}


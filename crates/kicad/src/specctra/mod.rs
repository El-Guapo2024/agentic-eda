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

use eda_model::ir::{Design, Point, Shape};
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
                        shapes.push(Shape::Segment { id: String::new(), layer: "Edge.Cuts".into(), stroke_width: 0, filled: false, start: pl.outline[i], end: pl.outline[(i + 1) % n] });
                    }
                }
            }
        }
        let shapes: Vec<Shape> = shapes.iter().map(shape_in_nm).collect();
        let max_error = model.board.max_error_um * 1000;
        let mut outline = eda_drc::outline::build_board_polygon_outlines(&shapes, max_error as f64, eda_model::outline::CHAINING_EPSILON_UM * 1000, true);
        // "Make polygon strictly simple to avoid issues (especially in 3D viewer)".
        outline.polys.simplify();
        outline
    };

    let b = &model.board;
    let um = |v: i64| v * 1000;
    let facts_of = |net: &str| -> export::ClassFacts {
        export::ClassFacts {
            name: b.effective_class(net).name,
            track_width: um(b.width_of(net)),
            clearance: um(b.clearance_of(net)),
            via_diameter: um(b.via_diameter_of(net)),
            via_drill: um(b.via_drill_of(net)),
        }
    };
    let default_class = export::ClassFacts { name: "Default".into(), track_width: um(b.track_width), clearance: um(b.clearance), via_diameter: um(b.via_diameter), via_drill: um(b.via_drill) };
    let settings = export::Settings {
        pcb_name: pcb_name.to_string(),
        host_version: HOST_VERSION.to_string(),
        outline: &outline,
        hole_clearance: um(b.hole_clearance_um),
        max_error: um(b.max_error_um),
        default_class,
        class_of_net: &facts_of,
    };
    let (pcb, warnings) = export::from_board(&mut board, &settings).map_err(|e| vec![CheckResult::fail("specctra.export", "board", e.0)])?;
    Ok(DsnExport { text: pcb.to_text(), warnings })
}

/// A shape of the design (micrometres) in the nanometres of the board KiCad reads: the derived file writes every coordinate and width of it exactly.
fn shape_in_nm(s: &Shape) -> Shape {
    let n = |p: &Point| Point { x: p.x * 1000, y: p.y * 1000 };
    let mut s = s.clone();
    match &mut s {
        Shape::Segment { start, end, stroke_width, .. } | Shape::Rect { start, end, stroke_width, .. } => {
            (*start, *end, *stroke_width) = (n(start), n(end), *stroke_width * 1000);
        }
        Shape::Circle { center, end, stroke_width, .. } => (*center, *end, *stroke_width) = (n(center), n(end), *stroke_width * 1000),
        Shape::Arc { start, mid, end, stroke_width, .. } => (*start, *mid, *end, *stroke_width) = (n(start), n(mid), n(end), *stroke_width * 1000),
        Shape::Polygon { pts, stroke_width, .. } => {
            pts.iter_mut().for_each(|p| *p = n(p));
            *stroke_width *= 1000;
        }
        Shape::Bezier { start, c1, c2, end, stroke_width, .. } => (*start, *c1, *c2, *end, *stroke_width) = (n(start), n(c1), n(c2), n(end), *stroke_width * 1000),
    }
    s
}

//! The body of a part, for the 3D view's fallback boxes.
//!
//! When the real KiCad render (`/api/board.glb`) is not there yet -- the export takes seconds, or it failed -- the 3D tab draws each part as a plain box.
//! The courtyard is the wrong size for that box: it is the body plus a clearance margin plus the pads' reach (a 0603 resistor's courtyard is three times
//! as wide as the part). KiCad draws a part's body on `F.Fab`, so the box is sized from that: the bounding box of the footprint's `F.Fab` graphics, and
//! the courtyard only for a footprint that has none.
//!
//! Where a footprint's `F.Fab` comes from, first match wins, for each of the part's `footprint` / `package` names:
//!   1. the project library's entry of that name (the one the Footprint Editor edits);
//!   2. a library of the board's footprint library tables (KiCad's installed ones, or the board's own), when the name is a `Lib:Name` id;
//!   3. the KiCad footprint the generic package name stands for (`eda_model::footprint::kicad_footprint_for`: `0603` on an `R` is
//!      `Resistor_SMD:R_0603_1608Metric`, ...), read from the installed library.
//!
//! Library footprints are parsed once and kept, against the file's modification time.

use crate::library_index::{modified, Kind, LibSource};
use eda_model::footprint::{kicad_footprint_of_part, to_board};
use eda_model::ir::{Design, FootprintInstance, Point, Shape, Nm};
use eda_model::Part;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

/// The footprint libraries a board's parts are looked up in: read when a part first needs one, then kept for the request.
pub struct Libs<'a> {
    dir: Option<&'a Path>,
    set: std::cell::OnceCell<Arc<dyn LibSource>>,
}

impl<'a> Libs<'a> {
    /// The footprint libraries of the board in `dir` (its library tables).
    pub fn of_board(dir: &'a Path) -> Libs<'a> {
        Libs { dir: Some(dir), set: std::cell::OnceCell::new() }
    }

    /// Footprint libraries given outright (a folder of `.pretty` libraries, in the tests).
    #[cfg(test)]
    pub fn of_source(src: Arc<dyn LibSource>) -> Libs<'static> {
        let set = std::cell::OnceCell::new();
        let _ = set.set(src);
        Libs { dir: None, set }
    }

    fn get(&self) -> &Arc<dyn LibSource> {
        self.set.get_or_init(|| match self.dir {
            Some(dir) => crate::library_tables::libs(dir, Kind::Footprint),
            None => Arc::new(PathBuf::new()),
        })
    }
}

/// `(x0, y0, x1, y1)` in µm.
pub type Rect = (Nm, Nm, Nm, Nm);

/// The bounding box of the `F.Fab` graphics, in the footprint's own frame. Strokes are not widened (a fab line is 0.1 mm); an arc is its three points.
pub fn fab_bbox(graphics: &[Shape]) -> Option<Rect> {
    let mut pts: Vec<Point> = Vec::new();
    for g in graphics.iter().filter(|g| g.layer() == "F.Fab") {
        match g {
            Shape::Segment { start, end, .. } | Shape::Rect { start, end, .. } => pts.extend([*start, *end]),
            Shape::Arc { start, mid, end, .. } => pts.extend([*start, *mid, *end]),
            Shape::Circle { center, end, .. } => {
                let r = Nm::from_raw_shim((((end.x - center.x).raw_shim() as f64).hypot((end.y - center.y).raw_shim() as f64)).round() as i64);
                pts.extend([Point { x: center.x - r, y: center.y - r }, Point { x: center.x + r, y: center.y + r }]);
            }
            Shape::Polygon { pts: p, .. } => pts.extend(p.iter().copied()),
            Shape::Bezier { start, c1, c2, end, .. } => pts.extend([*start, *c1, *c2, *end]),
        }
    }
    let (first, rest) = pts.split_first()?;
    let init = (first.x, first.y, first.x, first.y);
    let bb = rest.iter().fold(init, |b, p| (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y)));
    // A degenerate box (one fab line) is no body.
    ((bb.2 - bb.0) > Nm::ZERO && (bb.3 - bb.1) > Nm::ZERO).then_some(bb)
}

/// Each library footprint's body box with the modification time of the file it was read from.
type BodyCache = Mutex<HashMap<PathBuf, (Option<SystemTime>, Option<Rect>)>>;

fn installed_cache() -> &'static BodyCache {
    static CACHE: OnceLock<BodyCache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The `F.Fab` box of a library footprint (`Lib:Name`), read once for as long as its file is the same.
fn installed_fab_bbox(libs: &Libs, id: &str) -> Option<Rect> {
    let (lib, item) = id.split_once(':')?;
    if item.is_empty() || item.starts_with('.') || item.contains(['/', '\\']) || item.contains("..") {
        return None;
    }
    let file = libs.get().lib_path(Kind::Footprint, lib).ok()?.join(format!("{item}.kicad_mod"));
    let stamp = modified(&file)?;
    if let Some((s, hit)) = installed_cache().lock().unwrap_or_else(PoisonError::into_inner).get(&file) {
        if *s == Some(stamp) {
            return *hit;
        }
    }
    let found = std::fs::read_to_string(&file).ok().and_then(|text| fab_bbox(&eda_kicad::parse_library_footprint(&text).ok()?.footprint.graphics));
    installed_cache().lock().unwrap_or_else(PoisonError::into_inner).insert(file, (Some(stamp), found));
    found
}

/// The part's body box in its footprint's own frame, or `None` when no `F.Fab` is known for it (the caller then uses the courtyard).
pub fn local_body(libs: &Libs, design: &Design, part: &Part) -> Option<Rect> {
    for name in [part.footprint.as_deref(), part.package.as_deref()].into_iter().flatten() {
        if let Some(lib_fp) = design.footprint_library.as_ref().and_then(|l| l.by_name(name)) {
            if let Some(b) = fab_bbox(&lib_fp.graphics) {
                return Some(b);
            }
        }
        if name.contains(':') {
            if let Some(b) = installed_fab_bbox(libs, name) {
                return Some(b);
            }
        }
    }
    installed_fab_bbox(libs, &kicad_footprint_of_part(part)?.id())
}

/// The body box in board space: the four corners of the local box through the placement (rotation, bottom-side mirror, translation), boxed again.
pub fn placed_body(libs: &Libs, design: &Design, part: &Part, fp: &FootprintInstance) -> Option<Rect> {
    let (x0, y0, x1, y1) = local_body(libs, design, part)?;
    let corners = [to_board(fp, (x0, y0)), to_board(fp, (x1, y0)), to_board(fp, (x1, y1)), to_board(fp, (x0, y1))];
    let xs = corners.iter().map(|c| c.x);
    let ys = corners.iter().map(|c| c.y);
    Some((xs.clone().min()?, ys.clone().min()?, xs.max()?, ys.max()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Provenance, Side};
    use eda_model::units::mm;

    fn design(footprint_library: Option<eda_model::ir::FootprintLibrarySection>) -> Design {
        Design {
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        }
    }

    fn seg(layer: &str, a: (Nm, Nm), b: (Nm, Nm)) -> Shape {
        Shape::Segment { id: String::new(), layer: layer.into(), stroke_width: mm(0.1), filled: false, start: Point { x: a.0, y: a.1 }, end: Point { x: b.0, y: b.1 } }
    }

    #[test]
    fn the_body_is_the_fab_graphics_box_not_the_silkscreen_or_courtyard() {
        let g = vec![seg("F.Fab", (mm(-0.8), mm(-0.4)), (mm(0.8), mm(-0.4))), seg("F.Fab", (mm(0.8), mm(-0.4)), (mm(0.8), mm(0.4))), seg("F.SilkS", (mm(-3.0), mm(-3.0)), (mm(3.0), mm(3.0))), seg("F.CrtYd", (mm(-2.0), mm(-1.0)), (mm(2.0), mm(1.0)))];
        assert_eq!(fab_bbox(&g), Some((mm(-0.8), mm(-0.4), mm(0.8), mm(0.4))));
        assert_eq!(fab_bbox(&[seg("F.SilkS", (Nm::ZERO, Nm::ZERO), (mm(1.0), mm(1.0)))]), None, "no fab graphics, no body: the caller falls back to the courtyard");
        assert_eq!(fab_bbox(&[seg("F.Fab", (Nm::ZERO, Nm::ZERO), (mm(1.0), Nm::ZERO))]), None, "a single line is not a body");
    }

    #[test]
    fn a_circle_and_a_polygon_count_too() {
        let c = Shape::Circle { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, center: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(1.5), y: Nm::ZERO } };
        assert_eq!(fab_bbox(&[c]), Some((mm(-1.5), mm(-1.5), mm(1.5), mm(1.5))));
        let p = Shape::Polygon { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, pts: vec![Point { x: mm(-1.95), y: mm(-4.95) }, Point { x: mm(1.95), y: mm(-4.95) }, Point { x: mm(1.95), y: mm(4.95) }, Point { x: mm(-1.95), y: mm(4.95) }] };
        assert_eq!(fab_bbox(&[p]), Some((mm(-1.95), mm(-4.95), mm(1.95), mm(4.95))));
    }

    #[test]
    fn a_project_library_entry_of_that_name_is_the_body_before_anything_installed() {
        let mut fp = eda_model::ir::LibraryFootprint::new_empty("0603");
        fp.graphics.push(Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, start: Point { x: mm(-0.5), y: mm(-0.25) }, end: Point { x: mm(0.5), y: mm(0.25) } });
        let d = design(Some(eda_model::ir::FootprintLibrarySection { footprints: vec![fp], ..Default::default() }));
        let part = Part { reference: "R1".into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![], body_nm: None, symbol: None, datasheet: None, edge: None };
        assert_eq!(local_body(&Libs::of_source(Arc::new(PathBuf::new())), &d, &part), Some((mm(-0.5), mm(-0.25), mm(0.5), mm(0.25))), "the entry the Footprint Editor edits, not the KiCad 0603 it was copied from");
    }

    #[test]
    fn the_placed_body_follows_rotation_and_position() {
        let fp = FootprintInstance { id: "R1".into(), at: Point { x: mm(10.0), y: mm(5.0) }, rot: 90_000, side: Side::Top, label: Default::default() };
        // A 1600 x 800 body turned a quarter turn is 800 wide and 1600 tall about the part's position.
        let corners = [to_board(&fp, (mm(-0.8), mm(-0.4))), to_board(&fp, (mm(0.8), mm(0.4)))];
        assert_eq!(corners[0].x.min(corners[1].x), Nm::from_raw_shim(10_000_000 - 400_000));
        assert_eq!(corners[0].y.min(corners[1].y), Nm::from_raw_shim(5_000_000 - 800_000));
    }

    /// The brief's four parts, with KiCad.app installed: their bodies are the real footprints' F.Fab boxes.
    #[test]
    fn the_four_brief_parts_get_their_real_bodies_when_kicad_is_installed() {
        if !eda_kicad::default_footprint_library_root().is_dir() {
            return;
        }
        let part = |r: &str, pkg: &str, v: Option<&str>| Part { reference: r.into(), mpn: None, lcsc: None, value: v.map(String::from), package: Some(pkg.into()), footprint: Some(pkg.into()), pins: vec![], body_nm: None, symbol: None, datasheet: None, edge: None };
        let design = design(None);
        let libs = Libs::of_source(Arc::new(eda_kicad::default_footprint_library_root()));
        let size = |p: Part| local_body(&libs, &design, &p).map(|b| (b.2 - b.0, b.3 - b.1));
        // The fab outline of R_0603_1608Metric is 1.6 x 0.826 mm (the file draws the body slightly taller than the 0.8 mm nominal).
        let near = |got: Option<(Nm, Nm)>, w: Nm, h: Nm| got.is_some_and(|(gw, gh)| (gw - w).abs() <= mm(0.06) && (gh - h).abs() <= mm(0.06));
        assert!(near(size(part("R1", "0603", Some("330"))), mm(1.6), mm(0.8)), "R_0603_1608Metric: a 1.6 x 0.8 mm body");
        assert!(near(size(part("D1", "0603", Some("LED"))), mm(1.6), mm(0.8)), "LED_0603_1608Metric");
        let soic = size(part("U1", "SOIC-16", None)).expect("SOIC-16 has a fab outline");
        assert!((soic.0 - mm(3.9)).abs() <= mm(0.2) && (soic.1 - mm(9.9)).abs() <= mm(0.2), "SOIC-16_3.9x9.9mm: {soic:?}");
        let header = size(part("J1", "PINHEADER-4", None)).expect("a 1x04 header has a fab outline");
        assert!(header.1 >= mm(10.0) && header.0 <= mm(3.0), "PinHeader_1x04_P2.54mm_Vertical is about 2.5 x 10 mm: {header:?}");
    }
}

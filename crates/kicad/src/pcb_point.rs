//! A board's points (`(point (at x y) (size s) (layer ..) (uuid ..))`, KiCad's `PCB_POINT`) read and written: `PCB_IO_KICAD_SEXPR_PARSER::parsePCB_POINT` and
//! `PCB_IO_KICAD_SEXPR::format( const PCB_POINT* )`, for [`eda_model::pcb_items::PcbPoint`]. A footprint's own `(point ..)` is part of the footprint (`FootprintExtra::tokens`).
//! The parser takes `at`, `size`, `layer` and `uuid` and nothing else -- no `locked` -- and the writer writes the same four.

use std::fmt::Write as _;

use eda_model::ir::Point;
use eda_model::pcb_items::{PcbPoint, DEFAULT_POINT_SIZE_NM};

use crate::sexpr::{self, Sexpr};
use crate::{mm, mm_to_um, sexpr_str};

/// One `(point ..)` of a board. A token the item leaves out keeps what `PCB_POINT( nullptr )` starts with: the origin, 1 mm, `F.Cu`.
pub(crate) fn read(item: &[Sexpr]) -> PcbPoint {
    let at = sexpr::find(item, "at").and_then(|a| Some(Point { x: mm_to_um(sexpr::num(a, 1)?), y: mm_to_um(sexpr::num(a, 2)?) })).unwrap_or_default();
    let size_nm = sexpr::find(item, "size").and_then(|s| sexpr::num(s, 1)).map_or(DEFAULT_POINT_SIZE_NM, mm_to_um);
    let layer = sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.Cu").to_string();
    PcbPoint { id: String::new(), layer, at, size_nm }
}

/// Every `(point ..)` of a board, with the uuid of each in `refs`.
pub(crate) fn read_all(root: &[Sexpr], refs: &mut Vec<crate::import_items::ItemRef>) -> Vec<PcbPoint> {
    let mut out = Vec::new();
    for item in sexpr::find_all(root, "point") {
        out.push(read(item));
        // the file has no lock for a point: whatever the item says, it is not locked here
        let mut r = crate::import_items::ItemRef::of(item);
        r.locked = false;
        refs.push(r);
    }
    out
}

/// One point as `format( const PCB_POINT* )` writes it; returns its uuid.
pub(crate) fn write(out: &mut String, p: &PcbPoint) -> String {
    let uuid = crate::duid_for(&format!("point:{}", p.id), &p.id);
    writeln!(out, "\t(point (at {} {}) (size {}) (layer {}) (uuid \"{uuid}\"))", mm(p.at.x), mm(p.at.y), mm(p.size_nm), sexpr_str(&p.layer)).unwrap();
    uuid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_is_read_as_parse_pcb_point_reads_it_and_written_as_format_writes_it() {
        let tree = sexpr::parse(r#"(kicad_pcb (point (at 12.5 -3) (size 2) (layer "F.SilkS") (uuid "11111111-1111-1111-1111-111111111111")) (point (layer "B.Cu")))"#).unwrap();
        let mut refs = Vec::new();
        let points = read_all(tree.as_list().unwrap(), &mut refs);
        assert_eq!(points.len(), 2);
        assert_eq!((points[0].at, points[0].size_nm, points[0].layer.as_str()), (Point { x: 12_500, y: -3_000 }, 2_000, "F.SilkS"));
        assert_eq!((points[1].at, points[1].size_nm, points[1].layer.as_str()), (Point { x: 0, y: 0 }, 1_000, "B.Cu"), "what the point leaves out is what PCB_POINT starts with");
        assert_eq!(refs[0].uuid.as_deref(), Some("11111111-1111-1111-1111-111111111111"));
        let mut text = String::new();
        let uuid = write(&mut text, &points[0]);
        assert_eq!(text, format!("\t(point (at 12.5 -3) (size 2) (layer \"F.SilkS\") (uuid \"{uuid}\"))\n"));
        let back = read(sexpr::parse(&text).unwrap().as_list().unwrap());
        assert_eq!(back, points[0].clone());
    }

    #[test]
    fn a_locked_point_is_not_locked_here_because_the_file_form_has_no_lock() {
        let tree = sexpr::parse(r#"(kicad_pcb (point (at 1 1) (size 1) (layer "F.Cu") (locked yes)))"#).unwrap();
        let mut refs = Vec::new();
        read_all(tree.as_list().unwrap(), &mut refs);
        assert!(!refs[0].locked);
    }
}

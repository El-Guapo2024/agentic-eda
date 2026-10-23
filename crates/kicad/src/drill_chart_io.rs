//! KiCad 10's drill charts, drill maps and drill symbol profile read and written (KiCad 25ec2335, file version 20260901): `PCB_IO_KICAD_SEXPR_PARSER::parseDrillSymbolProfile`,
//! `parseGeneratedTable` with `parseGeneratedTableExtra`, `parsePCB_DRILL_MAP` and `parseDrillSpanBody`, and `PCB_IO_KICAD_SEXPR::formatDrillSymbolProfile`,
//! `format( const PCB_GENERATED_TABLE* )` with `formatGeneratedTableExtras` and `formatTableData`, and `format( const PCB_DRILL_MAP* )`, for [`eda_model::drill_chart`].
//!
//! A chart's table is read and written by [`crate::pcb_table`]: `formatTableData` is the same for a chart as for any table.

use std::fmt::Write as _;

use eda_model::drill_chart::{
    is_documentation_layer, DrillChart, DrillChartColumn, DrillChartColumnId, DrillChartFilter, DrillGroupKey, DrillMap, DrillMarkMode, DrillMarkPolicy, DrillSpan, DrillSymbolAssignment, DrillSymbolProfile, TableAlign,
    TableUnits,
};
use eda_model::ir::Point;
use eda_model::table::Table;

use crate::import_items::ItemRef;
use crate::sexpr::{self, Sexpr};
use crate::{mm, mm_to_nm, sexpr_str};

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

/// `parseBool` of `(name yes|no)`.
fn bool_of(l: &[Sexpr]) -> bool {
    sexpr::txt(l, 1).is_none_or(|v| v != "no")
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- the profile

/// `parseDrillSymbolProfile`: every grouping key off, then what the file names; what it leaves out keeps the default.
pub(crate) fn read_profile(root: &[Sexpr]) -> Option<DrillSymbolProfile> {
    let node = sexpr::find(root, "setup").and_then(|s| sexpr::find(s, "drill_symbol_profile"))?;
    let mut p = DrillSymbolProfile { group_by: Vec::new(), ..Default::default() };
    for l in node.iter().skip(1).filter_map(Sexpr::as_list) {
        match sexpr::tag(l) {
            Some("name") => p.name = sexpr::txt(l, 1).unwrap_or_default().to_string(),
            Some("group_by") => {
                for k in l.iter().skip(1).filter_map(Sexpr::text).filter_map(DrillGroupKey::from_token) {
                    p.set_grouped_by(k, true);
                }
            }
            Some("default_marks") => {
                if let Some(policy) = sexpr::txt(l, 1).and_then(DrillMarkPolicy::from_token) {
                    p.mark_policy = policy;
                }
            }
            Some("size") => p.size_nm = sexpr::num(l, 1).map(mm_to_nm).unwrap_or(p.size_nm),
            Some("width") => p.width_nm = sexpr::num(l, 1).map(mm_to_nm).unwrap_or(p.width_nm),
            Some("freeze_assignments") => p.freeze_assignments = bool_of(l),
            Some("assignment") => {
                let mut key = String::new();
                let mut a = DrillSymbolAssignment::default();
                for c in l.iter().skip(1).filter_map(Sexpr::as_list) {
                    match sexpr::tag(c) {
                        Some("key") => key = sexpr::txt(c, 1).unwrap_or_default().to_string(),
                        Some("mark") => {
                            // `DrillMarkModeFromToken` leaves the mode as it was (a shape) for a word it does not know.
                            a.mode = sexpr::txt(c, 1).and_then(DrillMarkMode::from_token).unwrap_or(a.mode);
                            match a.mode {
                                DrillMarkMode::Shape => a.shape_index = sexpr::num(c, 2).unwrap_or(0.0) as i32,
                                DrillMarkMode::Letter => a.letter = sexpr::txt(c, 2).unwrap_or_default().to_string(),
                                DrillMarkMode::SizeText => {}
                            }
                        }
                        Some("descr") => a.description = sexpr::txt(c, 1).unwrap_or_default().to_string(),
                        _ => {}
                    }
                }
                if !key.is_empty() {
                    p.assignments.insert(key, a);
                }
            }
            _ => {}
        }
    }
    Some(p)
}

/// `formatDrillSymbolProfile`: nothing for the default profile ("A board nobody has configured writes nothing"), else every setting and the assignments in key order, on
/// one line of the setup.
pub(crate) fn write_profile(out: &mut String, p: &DrillSymbolProfile) {
    if *p == DrillSymbolProfile::default() {
        return;
    }
    out.push_str("\t\t(drill_symbol_profile");
    if !p.name.is_empty() {
        let _ = write!(out, " (name {})", sexpr_str(&p.name));
    }
    out.push_str(" (group_by");
    for k in &p.group_by {
        let _ = write!(out, " {}", k.token());
    }
    out.push(')');
    let _ = write!(out, " (default_marks {}) (size {}) (width {}) (freeze_assignments {})", p.mark_policy.token(), mm(p.size_nm), mm(p.width_nm), yes(p.freeze_assignments));
    for (key, a) in &p.assignments {
        let _ = write!(out, " (assignment (key {}) (mark {}", sexpr_str(key), a.mode.token());
        match a.mode {
            DrillMarkMode::Shape => {
                let _ = write!(out, " {}", a.shape_index);
            }
            DrillMarkMode::Letter => {
                let _ = write!(out, " {}", sexpr_str(&a.letter));
            }
            DrillMarkMode::SizeText => {}
        }
        out.push(')');
        if !a.description.is_empty() {
            let _ = write!(out, " (descr {})", sexpr_str(&a.description));
        }
        out.push(')');
    }
    out.push_str(")\n");
}

// --------------------------------------------------------------------------------------------------------------------------------------------------- the chart

/// `parseGeneratedTable` and `parseGeneratedTableExtra` for one `(drill_chart ..)`: the table (`None` when [`crate::pcb_table::read`] cannot hold it, or KiCad would refuse it:
/// no column, not on a documentation layer) and what makes it a chart, its table id still to be given.
pub(crate) fn read_chart(item: &[Sexpr]) -> Option<(Table, DrillChart)> {
    let table = crate::pcb_table::read(item)?;
    if !is_documentation_layer(&table.layer) {
        return None;
    }
    let mut c = DrillChart { columns: Vec::new(), ..DrillChart::new("") };
    for l in item.iter().skip(1).filter_map(Sexpr::as_list) {
        match sexpr::tag(l) {
            Some("units") => {
                if let Some(u) = sexpr::txt(l, 1).and_then(TableUnits::from_token) {
                    c.units = u;
                }
            }
            // `SetPrecision`: clamped to 0..6.
            Some("precision") => c.precision = (sexpr::num(l, 1).unwrap_or(3.0) as i32).clamp(0, 6),
            Some("column") => {
                // An id the schema does not know is dropped (`Validate`), as is a repeat.
                let mut col: Option<DrillChartColumn> = None;
                let (mut name, mut align, mut width) = (None, None, 0);
                for f in l.iter().skip(1).filter_map(Sexpr::as_list) {
                    match sexpr::tag(f) {
                        Some("id") => col = sexpr::txt(f, 1).and_then(DrillChartColumnId::from_token).map(DrillChartColumn::default_of),
                        Some("name") => name = sexpr::txt(f, 1).map(str::to_string),
                        Some("justify") => align = sexpr::txt(f, 1).and_then(TableAlign::from_token),
                        Some("width") => width = sexpr::num(f, 1).map(mm_to_nm).unwrap_or(0),
                        _ => {}
                    }
                }
                if let Some(mut col) = col {
                    if let Some(n) = name {
                        col.heading = n;
                    }
                    if let Some(a) = align {
                        col.align = a;
                    }
                    col.width_nm = width.clamp(0, 1_000_000_000); // `GENERATED_TABLE_MAX_COLUMN_WIDTH`: 1000 mm
                    if !c.columns.iter().any(|x| x.id == col.id) {
                        c.columns.push(col);
                    }
                }
            }
            Some("row_keys") => {
                for k in l.iter().skip(1).filter_map(Sexpr::as_list).filter(|k| sexpr::tag(k) == Some("key")) {
                    if let (Some(row), Some(key)) = (sexpr::num(k, 1), sexpr::txt(k, 2)) {
                        c.row_keys.insert(row as u32, key.to_string());
                    }
                }
            }
            Some("filter") => {
                let mut f = DrillChartFilter::default();
                for x in l.iter().skip(1).filter_map(Sexpr::as_list) {
                    let v = bool_of(x);
                    match sexpr::tag(x) {
                        Some("plated") => f.plated = v,
                        Some("npth") => f.non_plated = v,
                        Some("vias") => f.vias = v,
                        Some("slots") => f.slots = v,
                        Some("backdrill") => f.backdrills = v,
                        Some("castellated") => f.castellated = v,
                        _ => {}
                    }
                }
                c.filter = f;
            }
            Some("totals") => c.show_totals = bool_of(l),
            Some("row_shapes") => {
                for x in l.iter().skip(1).filter_map(Sexpr::as_list) {
                    match sexpr::tag(x) {
                        Some("column") => c.symbol_column = sexpr::num(x, 1).unwrap_or(-1.0) as i32,
                        Some("shape") => {
                            if let (Some(row), Some(shape)) = (sexpr::num(x, 1), sexpr::num(x, 2)) {
                                c.row_shapes.insert(row as u32, shape as i32);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    // "if( aTable->Columns().empty() ) aTable->Columns() = schema.Defaults();"
    if c.columns.is_empty() {
        c.columns = DrillChartColumn::defaults();
    }
    Some((table, c))
}

/// Every `(drill_chart ..)` of the board, in file order: the charts this model holds (their tables, and the refs of the tables), and the text of the others.
pub(crate) fn read_charts(root: &[Sexpr], refs: &mut Vec<ItemRef>) -> (Vec<(Table, DrillChart)>, Vec<String>) {
    let mut out = Vec::new();
    let mut raw = Vec::new();
    for item in sexpr::find_all(root, "drill_chart") {
        match read_chart(item) {
            Some(tc) => {
                out.push(tc);
                refs.push(ItemRef::of(item));
            }
            None => raw.push(sexpr::to_text(&Sexpr::List(item.to_vec()))),
        }
    }
    (out, raw)
}

/// `format( const PCB_GENERATED_TABLE* )`: the uuid, the lock and the layer, then what the reader would not arrive at on its own -- units and precision other than the schema's,
/// each column with its heading and alignment when they are not the schema's and its width when it has one, the row keys, the filter when it is not the default, the totals when
/// they are off, the row shapes -- and the table's own data. Returns the uuid.
pub(crate) fn write_chart(out: &mut String, t: &Table, c: &DrillChart, locked: bool) -> String {
    let uuid = crate::pcb_table::table_uuid(t);
    let _ = write!(out, "\t(drill_chart (uuid \"{uuid}\")");
    if locked {
        out.push_str(" (locked yes)");
    }
    let _ = write!(out, " (layer {})", sexpr_str(&t.layer));
    if c.units != TableUnits::Mm {
        let _ = write!(out, " (units {})", c.units.token());
    }
    if c.precision != 3 {
        let _ = write!(out, " (precision {})", c.precision);
    }
    for col in &c.columns {
        let (token, heading, align, _, _) = col.id.def();
        let _ = write!(out, " (column (id {token})");
        if col.heading != heading {
            let _ = write!(out, " (name {})", sexpr_str(&col.heading));
        }
        if col.align != align {
            let _ = write!(out, " (justify {})", col.align.token());
        }
        if col.width_nm > 0 {
            let _ = write!(out, " (width {})", mm(col.width_nm));
        }
        out.push(')');
    }
    if !c.row_keys.is_empty() {
        out.push_str(" (row_keys");
        for (row, key) in &c.row_keys {
            let _ = write!(out, " (key {row} {})", sexpr_str(key));
        }
        out.push(')');
    }
    if !c.filter.is_default() {
        let f = &c.filter;
        let _ = write!(
            out,
            " (filter (plated {}) (npth {}) (vias {}) (slots {}) (backdrill {}) (castellated {}))",
            yes(f.plated),
            yes(f.non_plated),
            yes(f.vias),
            yes(f.slots),
            yes(f.backdrills),
            yes(f.castellated)
        );
    }
    if !c.show_totals {
        out.push_str(" (totals no)");
    }
    if c.symbol_column >= 0 && !c.row_shapes.is_empty() {
        let _ = write!(out, " (row_shapes (column {})", c.symbol_column);
        for (row, shape) in &c.row_shapes {
            let _ = write!(out, " (shape {row} {shape})");
        }
        out.push(')');
    }
    let _ = write!(out, " (column_count {})", t.col_count());
    crate::pcb_table::write_body(out, t, true);
    uuid
}

// ----------------------------------------------------------------------------------------------------------------------------------------------------- the map

/// `parsePCB_DRILL_MAP`: what the file leaves out keeps what `PCB_DRILL_MAP( board )` starts with (the mark size of the board's profile); `None` for a map KiCad refuses ("not on
/// a documentation layer").
pub(crate) fn read_map(item: &[Sexpr], profile_size_nm: i64) -> Option<(DrillMap, ItemRef)> {
    let mut m = DrillMap::new("", profile_size_nm);
    for l in item.iter().skip(1).filter_map(Sexpr::as_list) {
        match sexpr::tag(l) {
            Some("layer") => m.layer = sexpr::txt(l, 1).unwrap_or_default().to_string(),
            Some("offset") => m.offset = Point { x: sexpr::num(l, 1).map(mm_to_nm).unwrap_or(0), y: sexpr::num(l, 2).map(mm_to_nm).unwrap_or(0) },
            // `SetSymbolSize`: at least one.
            Some("size") => m.size_nm = sexpr::num(l, 1).map(mm_to_nm).unwrap_or(m.size_nm).max(1),
            Some("span") => {
                m.span = match sexpr::txt(l, 1) {
                    Some("all") | None => None,
                    Some(start) => Some(DrillSpan {
                        start: start.to_string(),
                        end: sexpr::txt(l, 2).unwrap_or_default().to_string(),
                        backdrill: l.iter().skip(3).any(|x| x.text() == Some("backdrill")),
                        npth: l.iter().skip(3).any(|x| x.text() == Some("npth")),
                    }),
                }
            }
            Some("outline_slots") => m.outline_slots = bool_of(l),
            Some("guide_cross") => m.guide_cross = bool_of(l),
            Some("custom_property") => m.custom_properties.push(sexpr::to_text(&Sexpr::List(l.to_vec()))),
            _ => {}
        }
    }
    if !is_documentation_layer(&m.layer) {
        return None;
    }
    Some((m, ItemRef::of(item)))
}

/// Every `(drill_map ..)` of the board, in file order.
pub(crate) fn read_maps(root: &[Sexpr], refs: &mut Vec<ItemRef>) -> Vec<DrillMap> {
    let size = read_profile(root).unwrap_or_default().size_nm;
    let mut out = Vec::new();
    for item in sexpr::find_all(root, "drill_map") {
        if let Some((m, r)) = read_map(item, size) {
            out.push(m);
            refs.push(r);
        }
    }
    out
}

/// `format( const PCB_DRILL_MAP* )`; returns the uuid.
pub(crate) fn write_map(out: &mut String, m: &DrillMap, locked: bool) -> String {
    let uuid = crate::duid_for(&format!("drill_map:{}", m.id), &m.id);
    let _ = write!(out, "\t(drill_map (uuid \"{uuid}\")");
    if locked {
        out.push_str(" (locked yes)");
    }
    let _ = write!(out, " (layer {}) (offset {} {}) (size {})", sexpr_str(&m.layer), mm(m.offset.x), mm(m.offset.y), mm(m.size_nm));
    match &m.span {
        None => out.push_str(" (span all)"),
        Some(s) => {
            let _ = write!(out, " (span {} {}", sexpr_str(&s.start), sexpr_str(&s.end));
            if s.backdrill {
                out.push_str(" backdrill");
            }
            if s.npth {
                out.push_str(" npth");
            }
            out.push(')');
        }
    }
    let _ = write!(out, " (outline_slots {}) (guide_cross {})", yes(m.outline_slots), yes(m.guide_cross));
    for p in &m.custom_properties {
        let _ = write!(out, " {p}");
    }
    out.push_str(")\n");
    uuid
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHART: &str = r#"(drill_chart (uuid "5383ee38-a9d4-45be-94ed-27ce389fc0fc") (layer "Dwgs.User") (precision 2)
        (column (id symbol) (width 16)) (column (id drill_diameter) (name "Drill (mm)") (width 16))
        (row_keys (key 1 "v1|d300000|s-|p1|L0-31|k0|m0:0:0:0/0:0:0:0"))
        (filter (plated yes) (npth no) (vias yes) (slots no) (backdrill no) (castellated yes)) (totals no) (row_shapes (column 0) (shape 1 0))
        (column_count 2) (border (external yes) (header yes) (stroke (width 0.15) (type default))) (separators (rows yes) (cols yes) (stroke (width 0.15) (type default)))
        (column_widths 16 16) (row_heights 2.5 2.5)
        (cells (table_cell "SYM" (start 25 55) (end 41 57.5) (margins 0.5 0.5 0.5 0.5) (span 1 1) (layer "Dwgs.User") (uuid "99387d49-57d3-4660-baa7-f3ad07732326") (effects (font (size 1 1))) (knockout no))
               (table_cell "Drill (mm)" (start 41 55) (end 57 57.5) (margins 0.5 0.5 0.5 0.5) (span 1 1) (layer "Dwgs.User") (uuid "fc4462cc-4b0e-4886-a30e-1616975bd730") (effects (font (size 1 1)) (justify right)) (knockout no))
               (table_cell "" (start 25 57.5) (end 41 60) (margins 0.5 0.5 0.5 0.5) (span 1 1) (layer "Dwgs.User") (uuid "d13e5eb9-e868-4d77-9ed1-2b1cad79a015") (effects (font (size 1 1))) (knockout no))
               (table_cell "0.30" (start 41 57.5) (end 57 60) (margins 0.5 0.5 0.5 0.5) (span 1 1) (layer "Dwgs.User") (uuid "569ea1f9-a60c-4da0-963f-8c4648b0cf6a") (effects (font (size 1 1)) (justify right)) (knockout no))))"#;

    #[test]
    fn a_chart_reads_and_writes_back_what_it_says() {
        let tree = sexpr::parse(CHART).unwrap();
        let (t, c) = read_chart(tree.as_list().unwrap()).unwrap();
        assert_eq!((t.col_count(), t.row_count(), t.layer.as_str()), (2, 2, "Dwgs.User"));
        assert_eq!(c.precision, 2);
        assert_eq!(c.columns.iter().map(|x| (x.id, x.heading.as_str(), x.width_nm)).collect::<Vec<_>>(), vec![(DrillChartColumnId::Symbol, "SYM", 16_000_000), (DrillChartColumnId::DrillDiameter, "Drill (mm)", 16_000_000)]);
        assert_eq!((c.filter.non_plated, c.filter.slots, c.show_totals, c.symbol_column), (false, false, false, 0));
        assert_eq!(c.row_shapes.get(&1), Some(&0));
        let mut out = String::new();
        write_chart(&mut out, &t, &c, false);
        let again = sexpr::parse(out.trim()).unwrap();
        let (t2, c2) = read_chart(again.as_list().unwrap()).unwrap();
        assert_eq!((t2, c2), (t, c));
        assert!(out.contains("(precision 2) (column (id symbol) (width 16)) (column (id drill_diameter) (name \"Drill (mm)\") (width 16))"), "{out}");
        assert!(out.contains("(totals no) (row_shapes (column 0) (shape 1 0)) (column_count 2)"), "{out}");
    }

    #[test]
    fn a_map_and_a_profile_read_and_write_back() {
        let tree = sexpr::parse(r#"(drill_map (uuid "b8c07edd-23fd-4bb5-b51d-dcd8f5d85461") (layer "Dwgs.User") (offset 50 0) (size 1.5) (span "F.Cu" "B.Cu") (outline_slots yes) (guide_cross no))"#).unwrap();
        let (m, _) = read_map(tree.as_list().unwrap(), 1_500_000).unwrap();
        assert_eq!((m.offset, m.span.as_ref().map(|s| s.end.as_str())), (Point { x: 50_000_000, y: 0 }, Some("B.Cu")));
        let mut out = String::new();
        write_map(&mut out, &DrillMap { id: "dmap_1".into(), ..m.clone() }, false);
        assert!(out.contains("(layer \"Dwgs.User\") (offset 50 0) (size 1.5) (span \"F.Cu\" \"B.Cu\") (outline_slots yes) (guide_cross no))"), "{out}");
        let bad = sexpr::parse(r#"(drill_map (layer "F.Cu"))"#).unwrap();
        assert!(read_map(bad.as_list().unwrap(), 1_500_000).is_none(), "copper is no documentation layer");

        let board = sexpr::parse(
            r#"(kicad_pcb (setup (drill_symbol_profile (group_by size slot plating span operation post_machining) (default_marks shapes_then_letters) (size 1.5) (width 0.15) (freeze_assignments yes)
                (assignment (key "v1|d1143000|s-|p1|L0-31|k0|m0:0:0:0/0:0:0:0") (mark shape 1)) (assignment (key "b") (mark letter "AB") (descr "x")))))"#,
        )
        .unwrap();
        let p = read_profile(board.as_list().unwrap()).unwrap();
        assert_eq!(p.group_by, DrillSymbolProfile::default().group_by);
        assert_eq!(p.assignments.len(), 2);
        assert_eq!(p.assignments["b"], DrillSymbolAssignment { mode: DrillMarkMode::Letter, shape_index: 0, letter: "AB".into(), description: "x".into() });
        let mut out = String::new();
        write_profile(&mut out, &p);
        assert!(out.contains(r#"(assignment (key "b") (mark letter "AB") (descr "x"))"#), "{out}");
        let mut none = String::new();
        write_profile(&mut none, &DrillSymbolProfile::default());
        assert!(none.is_empty(), "the default profile is not written");
    }
}

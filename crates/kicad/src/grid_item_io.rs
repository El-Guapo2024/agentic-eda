//! A board's grid items (`(grid_item xy|polar (at ..) (extent ..) (spacing ..) ..)`, KiCad 10's `PCB_GRID_ITEM`, file version 20260728) read and written:
//! `PCB_IO_KICAD_SEXPR_PARSER::parsePCB_GRID_ITEM` and `PCB_IO_KICAD_SEXPR::format( const PCB_GRID_ITEM* )` of KiCad 25ec2335, for [`eda_model::grid_item::GridItem`].

use eda_model::units::Nm;
use std::fmt::Write as _;

use eda_model::grid_item::{GridAffects, GridItem, GridKind};
use eda_model::ir::Point;

use crate::sexpr::{self, Sexpr};
use crate::{fmt_double_2_str, mm, mm_to_nm, sexpr_str};

/// `parsePCB_GRID_ITEM`: the children in the file's order -- the type word first, as the writer puts it, because `extent` and `spacing` mean a length and an angle on a polar grid
/// and two lengths on a cartesian one. What the item leaves out keeps what `PCB_GRID_ITEM( nullptr )` starts with; every value goes through the item's setters.
pub(crate) fn read(item: &[Sexpr]) -> GridItem {
    let mut g = GridItem::new(Point { x: Nm::ZERO, y: Nm::ZERO });
    let um = |l: &[Sexpr], i: usize| sexpr::num(l, i).map(mm_to_nm).unwrap_or(Nm::ZERO);
    for child in item.iter().skip(1) {
        match child {
            Sexpr::Atom(a) if a == "xy" => g.kind = GridKind::Cartesian,
            Sexpr::Atom(a) if a == "polar" => g.kind = GridKind::Polar,
            Sexpr::List(l) => match sexpr::tag(l) {
                Some("at") => g.at = Point { x: um(l, 1), y: um(l, 2) },
                Some("spacing") => match g.kind {
                    GridKind::Polar => {
                        g.spacing.x = um(l, 1).max(Nm::from_raw_shim(1));
                        g.phi_spacing_deg = sexpr::num(l, 2).unwrap_or(0.0).abs().max(0.001);
                    }
                    GridKind::Cartesian => g.spacing = Point { x: um(l, 1).max(Nm::from_raw_shim(1)), y: um(l, 2).max(Nm::from_raw_shim(1)) },
                },
                Some("extent") => match g.kind {
                    GridKind::Polar => {
                        g.extent.x = um(l, 1).abs();
                        g.phi_extent_deg = sexpr::num(l, 2).unwrap_or(0.0).abs().max(0.001);
                    }
                    GridKind::Cartesian => g.extent = Point { x: um(l, 1).abs(), y: um(l, 2).abs() },
                },
                Some("angle") => g.angle = eda_model::grid_item::normalize_deg(sexpr::num(l, 1).unwrap_or(0.0)),
                Some("priority") => g.priority = (sexpr::num(l, 1).unwrap_or(1.0) as i64).max(0) as u32,
                Some("tick_interval") => g.tick_interval = (sexpr::num(l, 1).unwrap_or(0.0) as i64).max(0) as u32,
                // `(affects (cursor yes|no) (routing yes|no) (placement yes|no))`: what the form names, the rest off (`aff.SetAll( false )` first).
                Some("affects") => {
                    let on = |name: &str| sexpr::find(l, name).and_then(|f| sexpr::txt(f, 1)).is_some_and(|v| v == "yes");
                    g.affects = GridAffects { cursor: on("cursor"), routing: on("routing"), placement: on("placement") };
                }
                Some("custom_property") => {
                    if let (Some(k), Some(v)) = (sexpr::txt(l, 1), sexpr::txt(l, 2)) {
                        g.custom_properties.push((k.to_string(), v.to_string()));
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    if g.priority == 0 {
        g.priority = 1;
    }
    g
}

/// Every `(grid_item ..)` of a board, with the uuid and lock of each in `refs`.
pub(crate) fn read_all(root: &[Sexpr], refs: &mut Vec<crate::import_items::ItemRef>) -> Vec<GridItem> {
    let mut out = Vec::new();
    for item in sexpr::find_all(root, "grid_item") {
        out.push(read(item));
        refs.push(crate::import_items::ItemRef::of(item));
    }
    out
}

/// `KICAD_FORMAT::FormatCustomProperties`: `(custom_property "key" "value")` for each.
pub(crate) fn custom_properties(props: &[(String, String)]) -> String {
    props.iter().map(|(k, v)| format!(" (custom_property {} {})", sexpr_str(k), sexpr_str(v))).collect()
}

/// One grid item as `format( const PCB_GRID_ITEM* )` writes it, on one line; returns its uuid. The type comes before `extent` and `spacing`, whose second number is an angle
/// on a polar grid (`EDA_UNIT_UTILS::FormatAngle`, ten significant digits); `angle` only when the grid is turned; `priority` always; `tick_interval` when there is one; `affects`
/// always, its three words.
pub(crate) fn write(out: &mut String, g: &GridItem, locked: bool) -> String {
    let uuid = crate::duid_for(&format!("grid_item:{}", g.id), &g.id);
    write!(out, "\t(grid_item {} (at {} {})", g.kind.token(), mm(g.at.x), mm(g.at.y)).unwrap();
    match g.kind {
        GridKind::Polar => write!(out, " (extent {} {}) (spacing {} {})", mm(g.extent.x), fmt_double_2_str(g.phi_extent_deg), mm(g.spacing.x), fmt_double_2_str(g.phi_spacing_deg)).unwrap(),
        GridKind::Cartesian => write!(out, " (extent {} {}) (spacing {} {})", mm(g.extent.x), mm(g.extent.y), mm(g.spacing.x), mm(g.spacing.y)).unwrap(),
    }
    if g.angle != 0.0 {
        write!(out, " (angle {})", fmt_double_2_str(g.angle)).unwrap();
    }
    write!(out, " (priority {})", g.priority).unwrap();
    if g.tick_interval > 0 {
        write!(out, " (tick_interval {})", g.tick_interval).unwrap();
    }
    let yn = |b: bool| if b { "yes" } else { "no" };
    write!(out, " (affects (cursor {}) (routing {}) (placement {}))", yn(g.affects.cursor), yn(g.affects.routing), yn(g.affects.placement)).unwrap();
    if locked {
        out.push_str(" (locked yes)");
    }
    writeln!(out, " (uuid \"{uuid}\"){})", custom_properties(&g.custom_properties)).unwrap();
    uuid
}

#[cfg(test)]
mod tests {
    use eda_model::units::Nm;
    use super::*;

    #[test]
    fn a_grid_item_is_read_as_parse_pcb_grid_item_reads_it_and_written_as_format_writes_it() {
        let tree = sexpr::parse(
            r#"(kicad_pcb
  (grid_item xy (at 100 50) (extent 4 2.5) (spacing 0.5 0.25) (angle 30) (priority 3) (tick_interval 5) (affects (cursor yes) (routing no) (placement yes)) (locked yes) (uuid "11111111-1111-4111-8111-111111111111") (custom_property "role" "via field"))
  (grid_item polar (at 0 0) (extent 10 90) (spacing 1 15) (priority 1) (affects (cursor yes) (routing yes) (placement yes)) (uuid "22222222-2222-4222-8222-222222222222"))
  (grid_item xy (at 1 1) (priority 0))
)"#,
        )
        .unwrap();
        let mut refs = Vec::new();
        let grids = read_all(tree.as_list().unwrap(), &mut refs);
        let g = &grids[0];
        assert_eq!((g.kind, g.at, g.extent, g.spacing, g.angle, g.priority, g.tick_interval), (GridKind::Cartesian, Point { x: Nm::from_raw_shim(100_000_000), y: Nm::from_raw_shim(50_000_000) }, Point { x: Nm::from_raw_shim(4_000_000), y: Nm::from_raw_shim(2_500_000) }, Point { x: Nm::from_raw_shim(500_000), y: Nm::from_raw_shim(250_000) }, 30.0, 3, 5));
        assert_eq!(g.affects, GridAffects { cursor: true, routing: false, placement: true });
        assert_eq!(g.custom_properties, vec![("role".to_string(), "via field".to_string())]);
        assert!(refs[0].locked);
        let p = &grids[1];
        assert_eq!((p.kind, p.extent.x, p.phi_extent_deg, p.spacing.x, p.phi_spacing_deg), (GridKind::Polar, Nm::from_raw_shim(10_000_000), 90.0, Nm::from_raw_shim(1_000_000), 15.0));
        assert_eq!((grids[2].priority, grids[2].extent, grids[2].spacing), (1, Point { x: Nm::from_raw_shim(5_000_000), y: Nm::from_raw_shim(5_000_000) }, Point { x: Nm::from_raw_shim(1_000_000), y: Nm::from_raw_shim(1_000_000) }), "priority 0 is 1, and the rest the constructor's");

        let mut text = String::new();
        let uuid = write(&mut text, g, true);
        assert_eq!(
            text,
            format!("\t(grid_item xy (at 100 50) (extent 4 2.5) (spacing 0.5 0.25) (angle 30) (priority 3) (tick_interval 5) (affects (cursor yes) (routing no) (placement yes)) (locked yes) (uuid \"{uuid}\") (custom_property \"role\" \"via field\"))\n")
        );
        let mut polar = String::new();
        write(&mut polar, p, false);
        assert!(polar.starts_with("\t(grid_item polar (at 0 0) (extent 10 90) (spacing 1 15) (priority 1) (affects (cursor yes) (routing yes) (placement yes)) (uuid "), "{polar}");
        assert_eq!(read(sexpr::parse(&text).unwrap().as_list().unwrap()), g.clone());
    }
}

//! KiCad 10's via stitching generators (`(generated (type via_stitch) ..)`, `PCB_VIA_STITCH`) read and written -- see [`eda_model::via_stitch`].
//!
//! Ported from `PCB_IO_KICAD_SEXPR::format( const PCB_GENERATOR* )`, `PCB_IO_KICAD_SEXPR_PARSER::parseGENERATOR` and `PCB_VIA_STITCH::GetProperties` / `SetProperties` /
//! `GetTemplateItems` of KiCad 25ec2335. The properties are written in the order of the `std::map` that holds them, a number with ten significant digits (`{:.10g}`, lengths
//! in millimetres), a point as `(key (xy x y))`, a line as `(key (pts ..))`, the excluded cells as `(excluded_grid_cells (cells (ij c r) ..))`, a string quoted; then the
//! template via, the members (the uuids the vias were written with, sorted) and the custom properties.

use eda_model::units::Nm;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use eda_model::ir::{Design, Point};
use eda_model::via_stitch::{StitchLayout, StitchMode, ViaStitch, ViaTemplate};

use crate::pcb_generated::{fmt_g10, line_prop, point_prop, prop_line, prop_num, prop_point, prop_str};
use crate::sexpr::{self, Sexpr};
use crate::{mm, mm_to_nm, sexpr_str};

/// The template via of a generator: `(templates (template (name "via") (via ..)))`.
fn read_template(g: &[Sexpr]) -> ViaTemplate {
    let via = sexpr::find(g, "templates")
        .into_iter()
        .flat_map(|t| sexpr::find_all(t, "template"))
        .find(|t| sexpr::find(t, "name").and_then(|n| sexpr::txt(n, 1)) == Some("via"))
        .and_then(|t| sexpr::find(t, "via"));
    let Some(via) = via else { return ViaTemplate::default() };
    let um = |name: &str| sexpr::find(via, name).and_then(|f| sexpr::num(f, 1)).map(mm_to_nm).unwrap_or(Nm::ZERO);
    let layers = sexpr::find(via, "layers");
    ViaTemplate {
        size_nm: um("size"),
        drill_nm: um("drill"),
        from_layer: layers.and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.Cu").to_string(),
        to_layer: layers.and_then(|l| sexpr::txt(l, 2)).unwrap_or("B.Cu").to_string(),
        text: sexpr::to_text(&Sexpr::List(via.to_vec())),
    }
}

/// `PCB_VIA_STITCH::SetProperties` on one `(generated ..)`, its members named by the design's via ids (`via_ids`: file uuid -> id).
fn read(g: &[Sexpr], via_ids: &BTreeMap<String, String>) -> ViaStitch {
    let mut v = ViaStitch::new(prop_line(g, "outline"));
    v.layer = prop_str(g, "layer").unwrap_or("F.Cu").to_string();
    v.name = prop_str(g, "name").unwrap_or("Via Stitching").to_string();
    v.pitch_nm = prop_num(g, "pitch").map(mm_to_nm).unwrap_or(Nm::ZERO);
    v.layout = prop_num(g, "layout").map(|n| StitchLayout::from_index(n as i64)).unwrap_or_default();
    v.mode = prop_num(g, "mode").map(|n| StitchMode::from_index(n as i64)).unwrap_or_default();
    v.seed = prop_num(g, "seed").map(|n| n as i64 as u32).unwrap_or(0);
    v.origin_offset = Point { x: prop_num(g, "origin_offset_x").map(mm_to_nm).unwrap_or(Nm::ZERO), y: prop_num(g, "origin_offset_y").map(mm_to_nm).unwrap_or(Nm::ZERO) };
    v.origin = prop_point(g, "origin").unwrap_or_default();
    v.net = prop_str(g, "net_name").unwrap_or("").to_string();
    v.guarded_net = prop_str(g, "guarded_net_name").unwrap_or("").to_string();
    v.excluded_cells = sexpr::find(g, "excluded_grid_cells")
        .and_then(|e| sexpr::find(e, "cells"))
        .map(|c| sexpr::find_all(c, "ij").filter_map(|ij| Some((sexpr::num(ij, 1)? as i64, sexpr::num(ij, 2)? as i64))).collect())
        .unwrap_or_default();
    v.excluded_positions = prop_line(g, "excluded_positions");
    v.via = read_template(g);
    v.member_ids = sexpr::find(g, "members").map(|m| m.iter().skip(1).filter_map(Sexpr::text).filter_map(|u| via_ids.get(u).cloned()).collect()).unwrap_or_default();
    for p in sexpr::find_all(g, "custom_property") {
        if let (Some(k), Some(val)) = (sexpr::txt(p, 1), sexpr::txt(p, 2)) {
            v.custom_properties.push((k.to_string(), val.to_string()));
        }
    }
    v
}

/// The via stitchings of an imported design, their members the vias the importer made (`refs.vias[i]` is the file uuid of the design's `i`th via), and the file uuid of each
/// (for the locks and groups that name them) in `refs`.
pub(crate) fn attach(design: &mut Design, root: &[Sexpr], refs: &mut crate::import_items::Refs) {
    let via_ids: BTreeMap<String, String> = design.routing.iter().flat_map(|rt| refs.vias.iter().zip(&rt.vias)).filter_map(|(r, v)| Some((r.uuid.clone()?, v.id.clone()))).collect();
    let mut out = Vec::new();
    for g in sexpr::find_all(root, "generated").filter(|g| prop_str(g, "type") == Some("via_stitch")) {
        out.push(read(g, &via_ids));
        refs.board_items.via_stitches.push(crate::import_items::ItemRef::of(g));
    }
    if out.is_empty() {
        return;
    }
    let dr = design.drawings.get_or_insert_with(Default::default);
    dr.board_items.via_stitches = out;
    dr.assign_missing_ids();
}

/// The template via as `format( PCB_TRACK )` writes the free via `PCB_VIA_STITCH`'s constructor makes (`SetIsFree( true )`, on no net), or the file's own with its size and drill
/// as they are now.
pub(crate) fn template_text(t: &ViaTemplate, uuid: &str) -> String {
    if t.text.is_empty() {
        return format!("(via (at 0 0) (size {}) (drill {}) (layers {} {}) (free yes) (net \"\") (uuid \"{uuid}\"))", mm(t.size_nm), mm(t.drill_nm), sexpr_str(&t.from_layer), sexpr_str(&t.to_layer));
    }
    let Ok(tree) = sexpr::parse(&t.text) else { return t.text.clone() };
    let Some(list) = tree.as_list() else { return t.text.clone() };
    let items: Vec<Sexpr> = list
        .iter()
        .map(|child| match child.as_list().and_then(sexpr::tag) {
            Some("size") => sexpr::parse(&format!("(size {})", mm(t.size_nm))).unwrap_or_else(|_| child.clone()),
            Some("drill") => sexpr::parse(&format!("(drill {})", mm(t.drill_nm))).unwrap_or_else(|_| child.clone()),
            // The span the dialog's From / To set (`m_viaTemplate->SetLayerPair`), so the template and the vias made from it agree.
            Some("layers") if child.as_list().is_some_and(|l| sexpr::txt(l, 1) != Some(t.from_layer.as_str()) || sexpr::txt(l, 2) != Some(t.to_layer.as_str())) => {
                sexpr::parse(&format!("(layers {} {})", sexpr_str(&t.from_layer), sexpr_str(&t.to_layer))).unwrap_or_else(|_| child.clone())
            }
            _ => child.clone(),
        })
        .collect();
    sexpr::to_text(&Sexpr::List(items))
}

/// The `(generated ..)` blocks of the board's via stitchings: `written` says which uuid each via of the design was written as, `uuid_of` names a generator, `locked` says whether
/// it is.
pub(crate) fn write_all(out: &mut String, stitches: &[ViaStitch], written: &BTreeMap<String, Vec<String>>, uuid_of: &dyn Fn(&ViaStitch) -> String, locked: &dyn Fn(&str) -> bool) {
    let mut sorted: Vec<&ViaStitch> = stitches.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    for v in sorted {
        let mut members: Vec<String> = v.member_ids.iter().flat_map(|m| written.get(m).cloned().unwrap_or_default()).collect();
        members.sort();
        members.dedup();
        let uuid = uuid_of(v);
        writeln!(out, "\t(generated\n\t\t(uuid \"{uuid}\")\n\t\t(type via_stitch)\n\t\t(name {})\n\t\t(layer {})", sexpr_str(&v.name), sexpr_str(&v.layer)).unwrap();
        if locked(&v.id) {
            writeln!(out, "\t\t(locked yes)").unwrap();
        }
        // `GetProperties`, in the map's (byte) order of its keys.
        if !v.excluded_cells.is_empty() {
            let cells: Vec<String> = v.excluded_cells.iter().map(|(c, r)| format!("(ij {c} {r})")).collect();
            writeln!(out, "\t\t(excluded_grid_cells (cells {}))", cells.join(" ")).unwrap();
        }
        if !v.excluded_positions.is_empty() {
            line_prop(out, "excluded_positions", &v.excluded_positions);
        }
        if !v.guarded_net.is_empty() {
            writeln!(out, "\t\t(guarded_net_name {})", sexpr_str(&v.guarded_net)).unwrap();
        }
        writeln!(out, "\t\t(layout {})", v.layout.index()).unwrap();
        writeln!(out, "\t\t(mode {})", v.mode.index()).unwrap();
        if !v.net.is_empty() {
            writeln!(out, "\t\t(net_name {})", sexpr_str(&v.net)).unwrap();
        }
        point_prop(out, "origin", v.origin);
        writeln!(out, "\t\t(origin_offset_x {})", fmt_g10(v.origin_offset.x.raw_shim() as f64 / 1e6)).unwrap();
        writeln!(out, "\t\t(origin_offset_y {})", fmt_g10(v.origin_offset.y.raw_shim() as f64 / 1e6)).unwrap();
        if v.outline.len() >= 3 {
            line_prop(out, "outline", &v.outline);
        }
        writeln!(out, "\t\t(pitch {})", fmt_g10(v.pitch_nm.raw_shim() as f64 / 1e6)).unwrap();
        writeln!(out, "\t\t(seed {})", v.seed as i32).unwrap();
        let template_uuid = crate::duid_for(&format!("via_stitch:{}:template", v.id), &v.id);
        writeln!(out, "\t\t(templates (template (name \"via\") {}))", template_text(&v.via, &template_uuid)).unwrap();
        write!(out, "\t\t(members").unwrap();
        for m in &members {
            write!(out, " \"{m}\"").unwrap();
        }
        writeln!(out, "){}\n\t)", crate::grid_item_io::custom_properties(&v.custom_properties)).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::Nm;
    use super::*;

    const GEN: &str = r#"(kicad_pcb (generated (uuid "130c265f-9b69-4b91-90aa-258955818fd2") (type via_stitch) (name "Via Stitching") (layer "F.Cu") (layout 1) (mode 0) (net_name "GND")
        (origin (xy -0.871109 -0.419811)) (origin_offset_x 0.25) (origin_offset_y 0) (outline (pts (xy 106.3 92.8) (xy 119.8 92.8) (xy 119.907549 104.2002) (xy 106.407694 104.000202)))
        (excluded_grid_cells (cells (ij 3 4) (ij -1 2))) (pitch 1) (seed 7)
        (templates (template (name "via") (via (at 0 0) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (free yes) (net "") (uuid "b3482f35-c371-4e19-8d52-5caeb32238dd"))))
        (members "04288bcc-34bb-42f3-875a-f6ee14bb69e9" "0a1a7d70-b336-4d7f-81fc-80ed532ef75a")))"#;

    #[test]
    fn a_stitching_is_read_as_set_properties_reads_it_and_written_as_format_writes_it() {
        let tree = sexpr::parse(GEN).unwrap();
        let g = sexpr::find(tree.as_list().unwrap(), "generated").unwrap();
        let ids: BTreeMap<String, String> = [("04288bcc-34bb-42f3-875a-f6ee14bb69e9".to_string(), "via_a".to_string())].into_iter().collect();
        let v = read(g, &ids);
        assert_eq!((v.layout, v.mode, v.net.as_str(), v.pitch_nm, v.seed, v.origin_offset), (StitchLayout::Staggered, StitchMode::Stitch, "GND", Nm::from_mm(1.0), 7, Point { x: Nm::from_mm(0.25), y: Nm::ZERO }));
        assert_eq!(v.outline.len(), 4);
        assert_eq!(v.excluded_cells, vec![(3, 4), (-1, 2)]);
        assert_eq!((v.via.size_nm, v.via.drill_nm, v.via.from_layer.as_str()), (Nm::from_mm(0.6), Nm::from_mm(0.3), "F.Cu"));
        assert_eq!(v.member_ids, vec!["via_a".to_string()], "a member that is not a via of the board is not one");

        let written: BTreeMap<String, Vec<String>> = [("via_a".to_string(), vec!["u-1".to_string()])].into_iter().collect();
        let mut out = String::new();
        write_all(&mut out, std::slice::from_ref(&v), &written, &|_| "gen-uuid".into(), &|_| false);
        let order: Vec<&str> = ["excluded_grid_cells", "(layout 1)", "(mode 0)", "(net_name \"GND\")", "(origin\n", "(origin_offset_x 0.25)", "(origin_offset_y 0)", "(outline\n", "(pitch 1)", "(seed 7)", "(templates", "(members \"u-1\")"].to_vec();
        let at: Vec<usize> = order.iter().map(|k| out.find(k).unwrap_or_else(|| panic!("{k} in {out}"))).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{out}");
        assert!(out.contains("(via (at 0 0) (size 0.6) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") (free yes) (net \"\") (uuid \"b3482f35-c371-4e19-8d52-5caeb32238dd\"))"), "{out}");
        // An edited template keeps the rest of the file's via.
        let edited = ViaTemplate { size_nm: Nm::from_mm(0.8), drill_nm: Nm::from_mm(0.4), ..v.via.clone() };
        assert!(template_text(&edited, "x").contains("(size 0.8) (drill 0.4) (layers \"F.Cu\" \"B.Cu\") (free yes)"));
        // and a new span is the template's too (the dialog's From / To).
        let blind = ViaTemplate { from_layer: "F.Cu".into(), to_layer: "In1.Cu".into(), ..v.via.clone() };
        assert!(template_text(&blind, "x").contains("(layers \"F.Cu\" \"In1.Cu\") (free yes)"), "{}", template_text(&blind, "x"));
        assert_eq!(template_text(&ViaTemplate { size_nm: Nm::from_mm(0.6), drill_nm: Nm::from_mm(0.3), ..Default::default() }, "t"), "(via (at 0 0) (size 0.6) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") (free yes) (net \"\") (uuid \"t\"))");
    }
}

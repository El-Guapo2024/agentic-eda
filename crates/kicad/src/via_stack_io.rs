//! KiCad 10's microvia stacks (`(generated (type via_stack) ..)`, `PCB_VIA_STACK`) read and written -- see [`eda_model::via_stack`].
//!
//! Ported from `PCB_IO_KICAD_SEXPR::format( const PCB_GENERATOR* )`, `PCB_IO_KICAD_SEXPR_PARSER::parseGENERATOR` and `PCB_VIA_STACK::GetProperties` / `SetProperties` of
//! KiCad 25ec2335. The properties are written in the order of the `std::map` that holds them (`capped`, `end_layer`, `filled`, `hops`, `origin`, `pitch`, `preset`,
//! `start_layer`, `style`, `use_netclass`, `via_drill`, `via_size`), a length in millimetres with ten significant digits (`set_iu`, `{:.10g}`), a flag as `yes`/`no`
//! (`FormatBool`), a point as `(origin (xy x y))`, the hops as `(hops (pts ..))`, a string quoted; then the members (the uuids the vias and traces were written with,
//! sorted) and the custom properties. "A microvia stack with no members is the same kind of ghost [as a tuning pattern with none]": neither written nor read.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use eda_model::ir::Design;
use eda_model::units::Nm;
#[cfg(test)]
use eda_model::ir::Point;
use eda_model::via_stack::{MicroviaStack, ViaStackStyle};

use crate::pcb_generated::{fmt_g10, line_prop, point_prop, prop_line, prop_num, prop_point, prop_str};
use crate::sexpr::{self, Sexpr};
use crate::{mm_to_nm, sexpr_str};

/// `layerFromName`: "Resolve a canonical copper layer name back to its id without needing a board" -- a copper layer's name, else the fallback.
fn copper_or(name: Option<&str>, fallback: &str) -> String {
    name.filter(|n| eda_model::layers::is_copper_layer_name(n)).unwrap_or(fallback).to_string()
}

/// `(key yes|no)`: `get_to( key, bool )` of a property the parser read as a symbol.
fn prop_flag(g: &[Sexpr], key: &str) -> Option<bool> {
    prop_str(g, key).map(|v| v == "yes" || v == "true")
}

/// `PCB_VIA_STACK::SetProperties` on one `(generated ..)`, its members named by the design's ids (`ids`: file uuid -> id of a via or a track).
fn read(g: &[Sexpr], ids: &BTreeMap<String, String>) -> MicroviaStack {
    let mut s = MicroviaStack::new(prop_point(g, "origin").unwrap_or_default());
    s.name = prop_str(g, "name").unwrap_or(MicroviaStack::DISPLAY_NAME).to_string();
    // the generator's `(layer ..)` first (`PCB_GENERATOR`'s), then `start_layer` (`SetStartLayer( layerFromName( name, m_startLayer ) )`)
    let layer = copper_or(prop_str(g, "layer"), "F.Cu");
    s.start_layer = copper_or(prop_str(g, "start_layer"), &layer);
    s.end_layer = copper_or(prop_str(g, "end_layer"), "In1.Cu");
    s.style = prop_str(g, "style").map(ViaStackStyle::from_word).unwrap_or_default();
    s.filled = prop_flag(g, "filled").unwrap_or(true);
    s.capped = prop_flag(g, "capped").unwrap_or(false);
    s.use_netclass = prop_flag(g, "use_netclass").unwrap_or(false);
    s.preset = prop_str(g, "preset").unwrap_or("").to_string();
    let len = |key: &str| prop_num(g, key).map(mm_to_nm).unwrap_or(Nm::ZERO);
    s.pitch_nm = len("pitch");
    s.via_size_nm = len("via_size");
    s.via_drill_nm = len("via_drill");
    s.hops = sexpr::find(g, "hops").map(|_| prop_line(g, "hops"));
    s.member_ids = sexpr::find(g, "members").map(|m| m.iter().skip(1).filter_map(Sexpr::text).filter_map(|u| ids.get(u).cloned()).collect()).unwrap_or_default();
    for p in sexpr::find_all(g, "custom_property") {
        if let (Some(k), Some(val)) = (sexpr::txt(p, 1), sexpr::txt(p, 2)) {
            s.custom_properties.push((k.to_string(), val.to_string()));
        }
    }
    s
}

/// The microvia stacks of an imported design, their members the vias and tracks the importer made (`refs.vias[i]` / `refs.tracks[i]` are the file uuids of the design's
/// `i`th via / track), and the file uuid of each (for the locks and groups that name them) in `refs`. A stack none of whose members is on the board is dropped, as
/// `parseGENERATOR` drops one with no `(members ..)` ("ignore member-less microvia stacks").
pub(crate) fn attach(design: &mut Design, root: &[Sexpr], refs: &mut crate::import_items::Refs) {
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    if let Some(rt) = &design.routing {
        ids.extend(refs.vias.iter().zip(&rt.vias).filter_map(|(r, v)| Some((r.uuid.clone()?, v.id.clone()))));
        ids.extend(refs.tracks.iter().zip(&rt.tracks).filter_map(|(r, t)| Some((r.uuid.clone()?, t.id.clone()))));
    }
    let mut out = Vec::new();
    for g in sexpr::find_all(root, "generated").filter(|g| prop_str(g, "type") == Some(MicroviaStack::GENERATOR_TYPE)) {
        let s = read(g, &ids);
        if s.member_ids.is_empty() {
            continue;
        }
        out.push(s);
        refs.board_items.via_stacks.push(crate::import_items::ItemRef::of(g));
    }
    if out.is_empty() {
        return;
    }
    let dr = design.drawings.get_or_insert_with(Default::default);
    dr.board_items.via_stacks = out;
    dr.assign_missing_ids();
}

/// The uuid a stack is written with.
pub(crate) fn uuid_of(s: &MicroviaStack) -> String {
    crate::duid_for(&format!("generated:{}", s.id), &s.id)
}

/// The `(generated ..)` blocks of the board's microvia stacks: `written` says which uuids each item of the design was written as, `locked` whether a stack is. A stack with
/// no member written is left out (`format( PCB_GENERATOR )`: `if( aGenerator->GetItems().empty() && .. "via_stack" ) return;`).
pub(crate) fn write_all(out: &mut String, stacks: &[MicroviaStack], written: &BTreeMap<String, Vec<String>>, locked: &dyn Fn(&str) -> bool) {
    let mut sorted: Vec<&MicroviaStack> = stacks.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    for s in sorted {
        let mut members: Vec<String> = s.member_ids.iter().flat_map(|m| written.get(m).cloned().unwrap_or_default()).collect();
        if members.is_empty() {
            continue;
        }
        members.sort();
        members.dedup();
        let yn = |b: bool| if b { "yes" } else { "no" };
        let len = |n: Nm| fmt_g10(n.mm());
        // `(layer ..)` is the start layer: `SetStartLayer` sets the generator's layer too
        writeln!(out, "\t(generated\n\t\t(uuid \"{}\")\n\t\t(type via_stack)\n\t\t(name {})\n\t\t(layer {})", uuid_of(s), sexpr_str(&s.name), sexpr_str(&s.start_layer)).unwrap();
        if locked(&s.id) {
            writeln!(out, "\t\t(locked yes)").unwrap();
        }
        writeln!(out, "\t\t(capped {})", yn(s.capped)).unwrap();
        writeln!(out, "\t\t(end_layer {})", sexpr_str(&s.end_layer)).unwrap();
        writeln!(out, "\t\t(filled {})", yn(s.filled)).unwrap();
        if let Some(hops) = &s.hops {
            line_prop(out, "hops", hops);
        }
        point_prop(out, "origin", s.origin);
        writeln!(out, "\t\t(pitch {})", len(s.pitch_nm)).unwrap();
        writeln!(out, "\t\t(preset {})", sexpr_str(&s.preset)).unwrap();
        writeln!(out, "\t\t(start_layer {})", sexpr_str(&s.start_layer)).unwrap();
        writeln!(out, "\t\t(style {})", sexpr_str(s.style.word())).unwrap();
        writeln!(out, "\t\t(use_netclass {})", yn(s.use_netclass)).unwrap();
        writeln!(out, "\t\t(via_drill {})", len(s.via_drill_nm)).unwrap();
        writeln!(out, "\t\t(via_size {})", len(s.via_size_nm)).unwrap();
        write!(out, "\t\t(members").unwrap();
        for m in &members {
            write!(out, " \"{m}\"").unwrap();
        }
        writeln!(out, "){}\n\t)", crate::grid_item_io::custom_properties(&s.custom_properties)).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two stacks of KiCad 25ec2335's `qa/data/pcbnew/via_stacks.kicad_pcb`.
    const GEN: &str = r#"(kicad_pcb
	(generated (uuid "06e04b75-4d2b-42d9-b957-17eeb4bbba74") (type via_stack) (name "Microvia Stack") (layer "F.Cu") (capped yes) (end_layer "In2.Cu") (filled yes)
		(origin (xy 20 20)) (pitch 0) (preset "HDI 1+N+1") (start_layer "F.Cu") (style "stacked") (use_netclass no) (via_drill 0.1) (via_size 0.25)
		(members "3489aebc-fcb5-43a2-8a7d-fa4869f8f90b" "e64e83bb-b9fd-40a4-82a1-4e2680099f9c"))
	(generated (uuid "6b256304-2daa-4e85-a4e2-1c3184456dfa") (type via_stack) (name "Microvia Stack") (layer "In1.Cu") (capped no) (end_layer "In3.Cu") (filled no)
		(hops (pts (xy 40 20) (xy 40.45 20))) (origin (xy 40 20)) (pitch 0.45) (preset "Staggered 0.45") (start_layer "In1.Cu") (style "staggered") (use_netclass no)
		(via_drill 0.1) (via_size 0.25) (members "5f37f62b-46be-4bf1-b36d-1180d87d62cc" "c3911982-d214-49b3-90b2-acb99400f738" "cef5bd56-1e2a-48d1-8676-f66068d03504")))"#;

    #[test]
    fn a_stack_is_read_as_set_properties_reads_it_and_written_as_format_writes_it() {
        let tree = sexpr::parse(GEN).unwrap();
        let gens: Vec<&[Sexpr]> = sexpr::find_all(tree.as_list().unwrap(), "generated").collect();
        let ids: BTreeMap<String, String> = [("3489aebc-fcb5-43a2-8a7d-fa4869f8f90b", "via_b"), ("e64e83bb-b9fd-40a4-82a1-4e2680099f9c", "via_a"), ("5f37f62b-46be-4bf1-b36d-1180d87d62cc", "via_c"), ("c3911982-d214-49b3-90b2-acb99400f738", "trk_a")]
            .into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        let stacked = read(gens[0], &ids);
        assert_eq!((stacked.start_layer.as_str(), stacked.end_layer.as_str(), stacked.style, stacked.filled, stacked.capped), ("F.Cu", "In2.Cu", ViaStackStyle::Stacked, true, true));
        assert_eq!((stacked.via_size_nm, stacked.via_drill_nm, stacked.preset.as_str(), stacked.hops.is_none()), (Nm::from_mm(0.25), Nm::from_mm(0.1), "HDI 1+N+1", true));
        assert_eq!(stacked.member_ids, ["via_b", "via_a"]);
        let staggered = read(gens[1], &ids);
        assert_eq!((staggered.style, staggered.pitch_nm, staggered.filled), (ViaStackStyle::Staggered, Nm::from_mm(0.45), false));
        assert_eq!(staggered.hops.as_deref(), Some(&[Point { x: Nm::from_mm(40.0), y: Nm::from_mm(20.0) }, Point { x: Nm::from_mm(40.45), y: Nm::from_mm(20.0) }][..]));
        assert_eq!(staggered.member_ids, ["via_c", "trk_a"], "a member that is not on the board is not one");

        let written: BTreeMap<String, Vec<String>> = [("via_c", "u-2"), ("trk_a", "u-1")].into_iter().map(|(a, b)| (a.to_string(), vec![b.to_string()])).collect();
        let mut out = String::new();
        write_all(&mut out, &[MicroviaStack { id: "vsk_1".into(), ..staggered }], &written, &|_| false);
        let order = ["(type via_stack)", "(name \"Microvia Stack\")", "(layer \"In1.Cu\")", "(capped no)", "(end_layer \"In3.Cu\")", "(filled no)", "(hops\n", "(xy 40 20) (xy 40.45 20)", "(origin\n", "(pitch 0.45)", "(preset \"Staggered 0.45\")", "(start_layer \"In1.Cu\")", "(style \"staggered\")", "(use_netclass no)", "(via_drill 0.1)", "(via_size 0.25)", "(members \"u-1\" \"u-2\")"];
        let at: Vec<usize> = order.iter().map(|k| out.find(k).unwrap_or_else(|| panic!("{k} in {out}"))).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{out}");
        // a stack with nothing written is a ghost: not written
        let mut none = String::new();
        write_all(&mut none, &[MicroviaStack { id: "vsk_2".into(), ..stacked }], &BTreeMap::new(), &|_| false);
        assert!(none.is_empty());
    }
}

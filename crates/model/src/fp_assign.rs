//! Which part of the board is which: the references the intent named, and the footprint each part is on.
//!
//! A part's reference is the key of everything about it -- its pose, its routing, its fields, its schematic symbol -- and the intent (the
//! frozen `ConstraintModel` the board was started from) names the parts by the references it was written with. Renaming a part
//! (`Edit Reference`, Annotate, Increment Annotations, Reannotate) cannot rewrite the intent, so the design keeps the answer beside the edits it
//! already keeps there: [`PartRename`], one entry per intent part that has another reference now, laid over the model each time the board is
//! loaded ([`apply_part_renames`], called by `crates/cli/src/board.rs::build_model`) the way `Design::nets` and the rules overlay are. The model the
//! rest of the program sees then has the part under its current reference: nothing is left under the old one.
//!
//! The same goes for the footprint a part is on. The intent names one (`Part::footprint`, or the package a built-in footprint stands for), and a board
//! changes it: Change Footprint, Update Footprints from Library, a new Footprint field on the symbol (`board_netlist_updater.cpp` `replaceFootprint`).
//! [`FootprintAssignment`] is that answer per instance, in `DrawingsSection::footprint_assignments` beside `footprint_edits`: the name of the footprint the part
//! is on and, when the board holds its own copy of it, the footprint itself (a KiCad board holds one for every footprint; here an instance without an
//! assignment follows the library or the intent live). [`apply_footprint_assignments`] lays the names over the model, `board::fold_footprint_edits` the
//! copies.
//!
//! Both overlays are additive: a design without them reads as "every part has the reference and the footprint the intent gave it".

use crate::footprint::Footprint;
use crate::ir::{Design, DrawingsSection};
use crate::{ConstraintModel, PlacementRule};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One intent part that has another reference now: `from` is the reference the intent names, `to` the one the part has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartRename {
    pub from: String,
    pub to: String,
}

/// A part's footprint on the board, when it is not the one the intent names: what `Cmd::ExchangeFootprints` leaves (Change Footprint, Update Footprint from Library,
/// the symbol's Footprint field followed). One entry per part, sorted by `reference`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintAssignment {
    /// The part's reference (`FootprintInstance::id`).
    pub reference: String,
    /// The footprint's name: `Library:Name`, or a bare name.
    pub footprint: String,
    /// The footprint as this instance has it, when the board keeps a copy: pads, graphics, courtyard and models, with what the exchange kept of the old footprint's
    /// texts and models laid in. Absent: the part is on whatever the model resolves under `footprint` (the intent's, a library's, a built-in's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<Footprint>,
}

impl Design {
    /// The assignment of the part `reference`, if the board gave it one.
    pub fn footprint_assignment(&self, reference: &str) -> Option<&FootprintAssignment> {
        self.drawings.as_ref()?.footprint_assignments.iter().find(|a| a.reference == reference)
    }
}

impl DrawingsSection {
    /// Make `assignment` the part's (replacing its predecessor), keeping the list sorted by reference.
    pub fn set_footprint_assignment(&mut self, assignment: FootprintAssignment) {
        self.footprint_assignments.retain(|a| a.reference != assignment.reference);
        self.footprint_assignments.push(assignment);
        self.footprint_assignments.sort_by(|a, b| a.reference.cmp(&b.reference));
    }
}

/// Lay the design's [`FootprintAssignment`]s over the model: each part named is on the footprint the board assigned it. (The copies of a footprint, where the board
/// keeps one, are laid in by `board::fold_footprint_edits`, which also lays the pad edits over them.) A part the model lacks is skipped.
pub fn apply_footprint_assignments(design: &Design, model: &mut ConstraintModel) {
    let Some(dr) = design.drawings.as_ref() else { return };
    for a in &dr.footprint_assignments {
        if let Some(part) = model.parts.iter_mut().find(|p| p.reference == a.reference) {
            part.footprint = Some(a.footprint.clone());
        }
    }
}

/// The reference an id names, when it is the id of a part or of something that belongs to one: `R1` itself, a pad (`R1.2`, `R1.2#2`) or a field
/// (`R1:Reference`). A reference holds no `.` and no `:`.
pub fn reference_of_id(id: &str) -> &str {
    id.split(['.', ':']).next().unwrap_or(id)
}

/// `id` with the reference it names (see [`reference_of_id`]) replaced by the one `map` gives it; `None` when `map` has nothing for that reference.
pub fn renamed_id(id: &str, map: &BTreeMap<String, String>) -> Option<String> {
    let head = reference_of_id(id);
    map.get(head).map(|new| format!("{new}{}", &id[head.len()..]))
}

/// A reference no intent names: the `?` of a symbol that is not annotated yet (`R?`).
pub fn is_placeholder(reference: &str) -> bool {
    reference.contains('?')
}

impl ConstraintModel {
    /// Rename parts, all at once (so two parts can swap): the part itself, the pins of the nets that name it (`U1.3`), the clusters and the placement
    /// rules that name it, and what the loader keyed by it before this (the instance footprints and the installed art).
    pub fn rename_parts(&mut self, map: &BTreeMap<String, String>) {
        if map.is_empty() {
            return;
        }
        let new = |r: &str| map.get(r).cloned();
        for part in &mut self.parts {
            if let Some(n) = new(&part.reference) {
                part.reference = n;
            }
        }
        for net in &mut self.nets {
            let mut changed = false;
            for pin in &mut net.pins {
                if let Some(n) = renamed_id(pin, map) {
                    *pin = n;
                    changed = true;
                }
            }
            if changed {
                net.pins.sort();
                net.pins.dedup();
            }
        }
        for cluster in &mut self.clusters {
            if let Some(n) = new(&cluster.anchor) {
                cluster.anchor = n;
            }
            for m in &mut cluster.members {
                if let Some(n) = new(m.as_str()) {
                    *m = n;
                }
            }
        }
        let rename_all = |refs: &mut Vec<String>| {
            for r in refs.iter_mut() {
                if let Some(n) = new(r.as_str()) {
                    *r = n;
                }
            }
        };
        for rule in &mut self.placement_rules {
            match rule {
                PlacementRule::Proximity { a, b, .. } | PlacementRule::Separation { a, b, .. } => {
                    for r in [a, b] {
                        if let Some(n) = new(r.as_str()) {
                            *r = n;
                        }
                    }
                }
                PlacementRule::Keepout { refs, .. } | PlacementRule::ThermalGroup { refs } => rename_all(refs),
            }
        }
        self.instance_footprints = rekey(std::mem::take(&mut self.instance_footprints), map);
        self.installed_art = rekey(std::mem::take(&mut self.installed_art), map);
    }
}

/// `old` with each key that `map` renames under its new name, all at once.
fn rekey<V>(old: BTreeMap<String, V>, map: &BTreeMap<String, String>) -> BTreeMap<String, V> {
    old.into_iter().map(|(k, v)| (map.get(&k).cloned().unwrap_or(k), v)).collect()
}

/// The renames of `design` as a map `from -> to`.
pub fn rename_map(design: &Design) -> BTreeMap<String, String> {
    design.drawings.as_ref().map(|d| d.renamed_parts.iter().map(|r| (r.from.clone(), r.to.clone())).collect()).unwrap_or_default()
}

/// Lay the design's [`PartRename`]s over the model the intent gave (`crates/cli/src/board.rs::load` calls it right after the intent is read, before
/// anything is keyed by a reference). A rename whose `from` the model has no part for (the part was a symbol drawn here, or the intent changed) is
/// skipped.
pub fn apply_part_renames(design: &Design, model: &mut ConstraintModel) {
    let map = rename_map(design);
    // a part the model does not have is no rename: its symbol makes it, under the reference it has
    let map: BTreeMap<String, String> = map.into_iter().filter(|(from, _)| model.part(from).is_some()).collect();
    model.rename_parts(&map);
}

/// The list of renames after `map` was applied to a design whose list was `entries`: an intent part that was renamed before and is renamed again
/// keeps its one entry (`from` is the intent's reference, `to` the newest); a rename back to the intent's own reference drops the entry; a part
/// the list never had is added when `is_intent_part` says the intent names it. The list is sorted by `from`.
pub fn compose_renames(entries: &[PartRename], map: &BTreeMap<String, String>, is_intent_part: impl Fn(&str) -> bool) -> Vec<PartRename> {
    let mut out: Vec<PartRename> = Vec::new();
    let mut moved: Vec<&str> = Vec::new();
    for e in entries {
        let to = map.get(&e.to).cloned().unwrap_or_else(|| e.to.clone());
        if map.contains_key(&e.to) {
            moved.push(e.to.as_str());
        }
        out.push(PartRename { from: e.from.clone(), to });
    }
    for (old, new) in map {
        if moved.contains(&old.as_str()) || is_placeholder(old) || !is_intent_part(old) {
            continue;
        }
        out.push(PartRename { from: old.clone(), to: new.clone() });
    }
    out.retain(|e| e.from != e.to);
    out.sort_by(|a, b| a.from.cmp(&b.from));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cluster, Net, Part, Pin, PinKind};

    fn part(r: &str) -> Part {
        Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: None, symbol: None, datasheet: None, pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }], body_nm: None, edge: None }
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn an_id_names_its_reference_before_the_first_dot_or_colon() {
        assert_eq!(reference_of_id("R1"), "R1");
        assert_eq!(reference_of_id("R1.2"), "R1");
        assert_eq!(reference_of_id("R1.2#2"), "R1");
        assert_eq!(reference_of_id("R1:Reference"), "R1");
        let m = map(&[("R1", "R9")]);
        assert_eq!(renamed_id("R1", &m).as_deref(), Some("R9"));
        assert_eq!(renamed_id("R1.2#2", &m).as_deref(), Some("R9.2#2"));
        assert_eq!(renamed_id("R1:Vendor:code", &m).as_deref(), Some("R9:Vendor:code"));
        assert_eq!(renamed_id("R10", &m), None, "R10 is not R1");
        assert_eq!(renamed_id("trk_abcdef", &m), None);
    }

    #[test]
    fn the_model_has_the_part_under_its_new_reference_everywhere_and_two_parts_can_swap() {
        let mut model = ConstraintModel { parts: vec![part("R1"), part("R2"), part("U1")], ..Default::default() };
        model.nets = vec![Net { name: "A".into(), pins: vec!["R1.1".into(), "U1.3".into()] }, Net { name: "B".into(), pins: vec!["R2.1".into()] }];
        model.clusters = vec![Cluster { anchor: "U1".into(), members: vec!["R1".into(), "R2".into()], orientation: Default::default() }];
        model.placement_rules = vec![PlacementRule::Proximity { a: "R1".into(), b: "U1".into(), max_mm: 3.0, reason: None }, PlacementRule::ThermalGroup { refs: vec!["R2".into(), "U1".into()] }];
        model.rename_parts(&map(&[("R1", "R2"), ("R2", "R1"), ("U1", "U7")]));
        assert_eq!(model.parts.iter().map(|p| p.reference.as_str()).collect::<Vec<_>>(), ["R2", "R1", "U7"], "swapped, not chained");
        assert_eq!(model.nets[0].pins, ["R2.1", "U7.3"]);
        assert_eq!(model.nets[1].pins, ["R1.1"]);
        assert_eq!((model.clusters[0].anchor.as_str(), model.clusters[0].members.as_slice()), ("U7", ["R2".to_string(), "R1".to_string()].as_slice()));
        assert!(matches!(&model.placement_rules[0], PlacementRule::Proximity { a, b, .. } if a == "R2" && b == "U7"));
        assert!(matches!(&model.placement_rules[1], PlacementRule::ThermalGroup { refs } if refs == &["R1", "U7"]));
    }

    #[test]
    fn a_rename_of_a_part_the_model_lacks_is_skipped() {
        let mut design: Design = serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap();
        design.drawings = Some(crate::ir::DrawingsSection { renamed_parts: vec![PartRename { from: "R1".into(), to: "R5".into() }, PartRename { from: "R9".into(), to: "R6".into() }], ..Default::default() });
        let mut model = ConstraintModel { parts: vec![part("R1")], ..Default::default() };
        apply_part_renames(&design, &mut model);
        assert_eq!(model.parts[0].reference, "R5");
        assert_eq!(model.parts.len(), 1);
    }

    #[test]
    fn renames_compose_through_a_second_rename_and_vanish_on_the_way_back() {
        let intent = |r: &str| r == "R1" || r == "R2" || r == "U1";
        let first = compose_renames(&[], &map(&[("R1", "R5")]), intent);
        assert_eq!(first, [PartRename { from: "R1".into(), to: "R5".into() }]);
        // R5 is not an intent part, but it is R1's new reference: the entry moves on
        let second = compose_renames(&first, &map(&[("R5", "R7")]), intent);
        assert_eq!(second, [PartRename { from: "R1".into(), to: "R7".into() }]);
        // and back to the intent's own reference
        assert!(compose_renames(&second, &map(&[("R7", "R1")]), intent).is_empty());
        // a swap in one step
        let swap = compose_renames(&[], &map(&[("R1", "R2"), ("R2", "R1")]), intent);
        assert_eq!(swap, [PartRename { from: "R1".into(), to: "R2".into() }, PartRename { from: "R2".into(), to: "R1".into() }]);
        // a part drawn here, and a symbol nobody numbered, are no intent parts
        assert!(compose_renames(&[], &map(&[("C99", "C100"), ("R?", "R3")]), intent).is_empty());
    }
}

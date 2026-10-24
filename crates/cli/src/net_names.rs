//! KiCad's names for the nets of a board made here (the user's decision of 2026-10-23, `docs/parity/findings/p1-p2-footprints.md` F1).
//!
//! A board made from an intent has the intent's net names (`PA7`, `LED1`); the schematic derived from it has the names KiCad gives what is drawn:
//! `/LED channels (D1–D8)/PA7` for a hierarchical label inside that sheet, `/IN` for a local label on the root, `Net-(D1-Pad1)` for a net no label names,
//! `GND` for a power symbol (`SCH_CONNECTION::recacheName`, `SCH_PIN::GetDefaultNetName`, as `eda_engine::nets` traces them). KiCad's Update PCB from
//! Schematic (F8) gives every pad the schematic's name (`BOARD_NETLIST_UPDATER::updateComponentPadConnections`), so a board that keeps the intent's would
//! be renamed, net by net, the first time it is opened in KiCad. So the board, the studio and every check use KiCad's names, and there is no second name
//! for display: [`kicad_names`] traces the board's schematic -- the stored one, else the one derived from the intent -- and [`apply`] gives each net of
//! the model the name of the drawn net its pins are on. The intent's own name stays what the schematic is drawn from
//! (`ConstraintModel::intent_net_names`: a label shows `PA7`).
//!
//! A design that still holds the intent's names in its own sections (tracks, vias, zones, its net list: a board routed before this) is renamed as it is
//! read ([`rename_design`]): once, since a name that is a net's own is never renamed again.
//!
//! A board read from a KiCad file keeps the names the file gave: they are KiCad's already, and our tracer is the one that could be wrong.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use eda_model::ir::Design;
use eda_model::ConstraintModel;

/// Whether the design's nets came from a KiCad file (a `.kicad_pcb`, whose footprints keep what the file said; a `.kicad_sch` read whole), which named
/// them already.
fn named_by_a_kicad_file(design: &Design) -> bool {
    design.drawings.as_ref().is_some_and(|d| !d.footprint_extras.is_empty())
        || design.schematic.as_ref().is_some_and(|s| s.imported_from_kicad)
        || design.sheet_contents.iter().flat_map(|c| c.values()).any(|s| s.imported_from_kicad)
}

/// What the board's schematic draws, as KiCad reads it: its nets under KiCad's names, and what each footprint takes from its symbol
/// (`eda_kicad::symbol_links`).
#[derive(Clone, Default)]
pub(crate) struct Drawn {
    pub nets: Vec<eda_engine::nets::TracedNet>,
    pub links: BTreeMap<String, eda_model::SymbolLink>,
}

/// The derived schematics' readings, by what they are derived from: deriving a schematic lays out every sheet, which a long-running server should not
/// do again for a design it has seen.
static DERIVED: Mutex<Option<BTreeMap<[u8; 32], Drawn>>> = Mutex::new(None);

/// What the schematic of `design` draws: the stored schematic, else the one the intent derives (`board::derived_schematic`). Empty when the design has
/// no parts to draw or the derivation fails.
pub(crate) fn drawn(design: &Design, model: &ConstraintModel) -> Drawn {
    let read = |d: &mut Design| -> Drawn {
        let links = eda_kicad::symbol_links(d, model);
        Drawn { nets: crate::board::trace_kicad_names(d, model), links }
    };
    if design.schematic.is_some() {
        return read(&mut design.clone());
    }
    // what the derivation reads: the parts, the nets, the libraries' symbols, the clusters, the modules a placement recorded
    let key = {
        let modules = design.placement.as_ref().map(|p| p.modules.as_slice()).unwrap_or(&[]);
        let bytes = serde_json::to_vec(&(&model.parts, &model.nets, &model.symbols, &model.symbol_sim_fields, &model.clusters, &model.placement_rules, &model.intent_net_names, modules)).unwrap_or_default();
        *blake3::hash(&bytes).as_bytes()
    };
    if let Some(hit) = DERIVED.lock().ok().and_then(|c| c.as_ref().and_then(|m| m.get(&key).cloned())) {
        return hit;
    }
    let Ok(derived) = crate::board::derived_schematic(design, model) else { return Drawn::default() };
    let mut d = design.clone();
    d.schematic = derived.schematic;
    d.sheet_contents = derived.sheet_contents;
    if d.bus_aliases.is_empty() {
        d.bus_aliases = derived.bus_aliases;
    }
    let out = read(&mut d);
    if let Ok(mut cache) = DERIVED.lock() {
        let map = cache.get_or_insert_with(BTreeMap::new);
        // a long-running server sees many revisions of a few boards
        if map.len() > 256 {
            map.clear();
        }
        map.insert(key, out.clone());
    }
    out
}

/// For each net of `model` that KiCad names otherwise, its name -> KiCad's: the name of the drawn net most of its pins are on (the larger overlap first,
/// then the name). A drawn name two nets would take goes to the one with more pins on it; a net with no pin on the schematic, or one the drawing shows
/// only in pieces, keeps its name.
pub(crate) fn kicad_names(design: &Design, model: &ConstraintModel, drawn: &Drawn) -> BTreeMap<String, String> {
    if named_by_a_kicad_file(design) || model.nets.is_empty() {
        return BTreeMap::new();
    }
    let drawn = &drawn.nets;
    let drawn_of: BTreeMap<&str, &str> = drawn.iter().flat_map(|t| t.pins.iter().map(move |p| (p.as_str(), t.name.as_str()))).collect();
    // (overlap, net, drawn name), the best claims first
    let mut claims: Vec<(usize, &str, &str)> = Vec::new();
    for n in &model.nets {
        let mut overlap: BTreeMap<&str, usize> = BTreeMap::new();
        for p in &n.pins {
            if let Some(&d) = drawn_of.get(p.as_str()) {
                *overlap.entry(d).or_default() += 1;
            }
        }
        // a drawing that shows the net in pieces (labels that do not quite touch their pins) names none of them: the net is named by a drawn net that
        // has most of its pins, two at least (or its one pin)
        if let Some((d, c)) = overlap.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0))) {
            if 2 * c > n.pins.len() && (c >= 2 || n.pins.len() == 1) {
                claims.push((c, n.name.as_str(), d));
            }
        }
    }
    claims.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    let mut taken: BTreeSet<&str> = BTreeSet::new();
    let mut named: BTreeMap<&str, &str> = BTreeMap::new();
    for (_, net, d) in claims {
        if named.contains_key(net) || !taken.insert(d) {
            continue;
        }
        named.insert(net, d);
    }
    // a net keeps its name when another net that keeps its own has the name it would take
    let kept: BTreeSet<&str> = model.nets.iter().map(|n| n.name.as_str()).filter(|n| !named.contains_key(n)).collect();
    named.into_iter().filter(|(net, d)| net != d && !kept.contains(d)).map(|(net, d)| (net.to_string(), d.to_string())).collect()
}

/// The model with KiCad's names (`renames`, from [`kicad_names`]): its nets, the net classes, pours and custom rules that named them (a class whose
/// patterns matched the old name gets the new one as a pattern of its own, as a label's net class does; a rule's `'OLD'` is `'NEW'`), and the way back to
/// the intent's names for the drawing. An impedance target's pattern is left as it is.
pub(crate) fn apply(model: &mut ConstraintModel, renames: &BTreeMap<String, String>) {
    if renames.is_empty() {
        return;
    }
    for n in model.nets.iter_mut() {
        if let Some(new) = renames.get(&n.name) {
            // what the schematic is drawn from: the name the intent gave, through any rename before this one
            let intent = model.intent_net_names.remove(&n.name).unwrap_or_else(|| n.name.clone());
            model.intent_net_names.insert(new.clone(), intent);
            n.name = new.clone();
        }
    }
    for class in model.board.net_classes.iter_mut() {
        let mut add: Vec<String> = Vec::new();
        for (old, new) in renames {
            let matched = |name: &str| class.nets.iter().any(|p| eda_model::net_pattern::pattern_matches(p, name));
            if matched(old) && !matched(new) {
                add.push(eda_kicad::exact_pattern(new));
            }
        }
        class.nets.extend(add);
    }
    for pour in model.board.pours.iter_mut() {
        if let Some(new) = renames.get(&pour.net) {
            pour.net = new.clone();
        }
    }
    // a custom rule that compares a net's name (`A.NetName == 'PA7'`) names it by its new name, in the rules text kicad-cli reads and in the rules parsed
    // from it (only there: `A.NetClass == 'HV'` names a class, whatever net is called HV)
    let requote = |text: &str| -> String {
        let mut out = text.to_string();
        for (old, new) in renames {
            for op in ["==", "!="] {
                for gap in ["", " "] {
                    out = out.replace(&format!("NetName{gap}{op}{gap}'{old}'"), &format!("NetName{gap}{op}{gap}'{new}'"));
                }
            }
        }
        out
    };
    if let Some(text) = model.board.custom_rules_text.as_mut() {
        *text = requote(text);
    }
    for rule in model.board.custom_rules.iter_mut() {
        if let Some(c) = rule.condition.as_mut() {
            *c = requote(c);
        }
    }
}

/// The design's own sections with KiCad's names (`renames`): the copper (tracks, vias, zones, a shape on a net), the via tenting, the pads of a footprint
/// that lives only on the board, and the net list a schematic edit stored. Only a name that is no net's own any more is renamed. Whether anything changed.
pub(crate) fn rename_design(design: &mut Design, model: &ConstraintModel, renames: &BTreeMap<String, String>) -> bool {
    if renames.is_empty() {
        return false;
    }
    let current: BTreeSet<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
    let mut changed = false;
    let mut rename = |net: &mut String| {
        if !current.contains(net.as_str()) {
            if let Some(new) = renames.get(net.as_str()) {
                *net = new.clone();
                changed = true;
            }
        }
    };
    if let Some(r) = design.routing.as_mut() {
        r.tracks.iter_mut().for_each(|t| rename(&mut t.net));
        r.vias.iter_mut().for_each(|v| rename(&mut v.net));
        r.zones.iter_mut().for_each(|z| rename(&mut z.net));
    }
    if let Some(d) = design.drawings.as_mut() {
        for s in d.board_items.shape_styles.iter_mut() {
            if let Some(n) = s.net.as_mut() {
                rename(n);
            }
        }
        d.board_items.via_stitches.iter_mut().for_each(|v| rename(&mut v.net));
        d.via_tenting.iter_mut().for_each(|v| rename(&mut v.net));
        for bp in d.board_parts.iter_mut() {
            bp.pad_nets.iter_mut().for_each(|(_, n)| rename(n));
        }
    }
    if let Some(nets) = design.nets.as_mut() {
        nets.iter_mut().for_each(|n| rename(&mut n.name));
    }
    changed
}

/// The copper of a net a command renamed: every net of `before` whose pins are, all of them and no others, a net of `after` under another name, when
/// no net of `after` has the old name any more. A net that was split or joined keeps the name its copper has (the board says so).
pub(crate) fn follow_renames(design: &mut Design, before: &[eda_model::Net], after: &[eda_model::Net]) {
    let pins = |n: &eda_model::Net| -> Vec<String> {
        let mut p = n.pins.clone();
        p.sort();
        p.dedup();
        p
    };
    let now: BTreeSet<&str> = after.iter().map(|n| n.name.as_str()).collect();
    let by_pins: BTreeMap<Vec<String>, &str> = after.iter().filter(|n| !n.pins.is_empty()).map(|n| (pins(n), n.name.as_str())).collect();
    let renames: BTreeMap<String, String> = before
        .iter()
        .filter(|n| !now.contains(n.name.as_str()))
        .filter_map(|n| by_pins.get(&pins(n)).map(|new| (n.name.clone(), new.to_string())))
        .collect();
    if renames.is_empty() {
        return;
    }
    let model = ConstraintModel { nets: after.to_vec(), ..Default::default() };
    rename_design(design, &model, &renames);
}

#[cfg(test)]
mod tests {
    use crate::board::tests::{scratch, setup};
    use crate::board::{load, save};

    /// A board made from an intent whose net SIG is drawn as a wire (no label): KiCad calls it `/SIG`, and so does everything here -- the model,
    /// a track the design had on SIG, the net class and the custom rule the intent wrote for SIG. The power nets keep their names; the drawing is made
    /// from the intent's name.
    #[test]
    fn a_board_from_an_intent_takes_kicads_names_and_what_named_the_old_one_follows() {
        let dir = scratch("kicad_names_follow");
        setup(&dir);
        let intent = dir.join("intent.yaml");
        let mut model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
        model.nets.push(eda_model::Net { name: "SIG".into(), pins: vec![] });
        // U1.1-U2.1 is SIG now, GND is gone; VCC stays a power net
        model.nets.retain(|n| n.name != "GND");
        model.nets.iter_mut().find(|n| n.name == "SIG").unwrap().pins = vec!["U1.1".into(), "U2.1".into()];
        model.board.net_classes.push(eda_model::NetClass { name: "Fast".into(), nets: vec!["SIG".into()], track_width: Some(300_000), clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
        model.board.custom_rules_text = Some("(version 1)\n(rule \"sig\" (condition \"A.NetName == 'SIG'\") (constraint length (min 10mm)))\n".into());
        model.board.custom_rules = vec![eda_model::CustomRule { name: "sig".into(), constraint_type: "length".into(), min: Some(10_000_000), max: None, opt: None, layer: None, severity: None, condition: Some("A.NetName == 'SIG'".into()) }];
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
        // a design routed before: its track is on the intent's SIG
        let (_, mut design, _) = load(&dir).unwrap();
        design
            .routing
            .get_or_insert_with(|| eda_model::ir::RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() })
            .tracks
            .push(eda_model::ir::Track { id: "trk_sig".into(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: 200_000, pts: vec![eda_model::ir::Point { x: 4_000_000, y: 5_000_000 }, eda_model::ir::Point { x: 14_000_000, y: 5_000_000 }], arc_mid_offset: None });
        save(&dir, &design).unwrap();

        let (_, design, model) = load(&dir).unwrap();
        let sig = model.nets.iter().find(|n| n.pins.contains(&"U1.1".to_string())).unwrap();
        assert_eq!(sig.name, "/SIG", "{:?}", model.nets);
        assert_eq!(model.intent_net_names.get("/SIG").map(String::as_str), Some("SIG"), "the drawing is made from the intent's name");
        assert!(model.nets.iter().any(|n| n.name == "VCC"), "a power net keeps its name: {:?}", model.nets);
        assert_eq!(design.routing.as_ref().unwrap().tracks.iter().find(|t| t.id == "trk_sig").unwrap().net, "/SIG", "the track follows its net");
        assert_eq!(model.board.class_of("/SIG").map(|c| c.name.as_str()), Some("Fast"), "the class the intent put SIG in has it");
        assert_eq!(model.board.custom_rules[0].condition.as_deref(), Some("A.NetName == '/SIG'"));
        assert!(model.board.custom_rules_text.as_deref().unwrap().contains("A.NetName == '/SIG'"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

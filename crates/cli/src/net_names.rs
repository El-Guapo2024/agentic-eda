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
    // A custom rule that compares a net's name (`A.NetName == 'PA7'`, `A.NetName == 'USB_D*'`) or a pair's (`A.inDiffPair('USB_D')`) still takes the
    // nets it took: the comparison is widened to the new names it no longer matches, in the rules text kicad-cli reads and in the rules parsed from it.
    if let Some(text) = model.board.custom_rules_text.as_mut() {
        *text = widen_rule_text(text, renames);
    }
    for rule in model.board.custom_rules.iter_mut() {
        if let Some(c) = rule.condition.as_mut() {
            *c = widen_rule_text(c, renames);
        }
    }
    // An impedance target's pattern: a target of its own for each renamed net it matched and matches no more.
    let mut targets = Vec::new();
    for t in &model.impedance_targets {
        for (old, new) in renames {
            if wild_compare(&t.net_pattern, old) && !wild_compare(&t.net_pattern, new) {
                targets.push(eda_model::ImpedanceTarget { net_pattern: eda_kicad::exact_pattern(new), ..t.clone() });
            }
        }
    }
    model.impedance_targets.extend(targets);
}

/// `WildCompareString( pattern, name, false )` as `VALUE::EqualTo` compares a string (`common/libeval/numeric_evaluator.cpp`, `pcbexpr_evaluator.cpp`):
/// `*` and `?` wildcards, the case ignored.
fn wild_compare(pattern: &str, name: &str) -> bool {
    eda_model::glob_match(&pattern.to_lowercase(), &name.to_lowercase())
}

/// `text` (a rules file, or one condition) with each `X.NetName ==|!= '<pattern>'` and `X.inDiffPair('<pattern>')` that matched a renamed net's old
/// name and does not match its new one widened to the new one: `(A.NetName == 'PA7' || A.NetName == '/LED/PA7')`, `!=` with `&&`, a pair by its base
/// name (`BOARD::MatchDpSuffix`).
pub(crate) fn widen_rule_text(text: &str, renames: &BTreeMap<String, String>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    while i < chars.len() {
        // `<item>.NetName` or `<item>.inDiffPair(` at i
        let rest: String = chars[i..chars.len().min(i + 12)].iter().collect();
        let field = if rest.starts_with(".NetName") && !chars.get(i + 8).is_some_and(|c| ident(*c)) {
            Some(("NetName", 8))
        } else if rest.starts_with(".inDiffPair") {
            Some(("inDiffPair", 11))
        } else {
            None
        };
        let Some((field, flen)) = field else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        // the item before the dot: `A`, `B`, `AB`
        let mut start = i;
        while start > 0 && ident(chars[start - 1]) {
            start -= 1;
        }
        let item: String = chars[start..i].iter().collect();
        let mut j = i + flen;
        let skip = |j: &mut usize| {
            while *j < chars.len() && chars[*j].is_whitespace() {
                *j += 1;
            }
        };
        skip(&mut j);
        let op = if field == "NetName" {
            let two: String = chars[j..chars.len().min(j + 2)].iter().collect();
            if two != "==" && two != "!=" {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            j += 2;
            two
        } else {
            if chars.get(j) != Some(&'(') {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            j += 1;
            String::new()
        };
        skip(&mut j);
        if chars.get(j) != Some(&'\'') {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let lit_start = j + 1;
        let Some(len) = chars[lit_start..].iter().position(|c| *c == '\'') else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        let lit: String = chars[lit_start..lit_start + len].iter().collect();
        let mut end = lit_start + len + 1;
        if field == "inDiffPair" {
            skip(&mut end);
            if chars.get(end) != Some(&')') {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            end += 1;
        }
        // the new names this comparison no longer takes
        let mut extra: Vec<String> = Vec::new();
        for (old, new) in renames {
            let (old, new) = if field == "inDiffPair" {
                let (o, n) = (eda_drc::pcbexpr::match_dp_suffix(old), eda_drc::pcbexpr::match_dp_suffix(new));
                if o.0 == 0 || n.0 == 0 {
                    continue;
                }
                (o.2, n.2)
            } else {
                (old.clone(), new.clone())
            };
            if wild_compare(&lit, &old) && !wild_compare(&lit, &new) && !extra.contains(&new) {
                extra.push(new);
            }
        }
        let original: String = chars[start..end].iter().collect();
        // the item was already copied to `out`: take it back
        out.truncate(out.len() - item.len());
        if extra.is_empty() {
            out.push_str(&original);
        } else {
            let one = |name: &str| if field == "NetName" { format!("{item}.NetName {op} '{name}'") } else { format!("{item}.inDiffPair('{name}')") };
            let joiner = if op == "!=" { " && " } else { " || " };
            let parts: Vec<String> = std::iter::once(original.clone()).chain(extra.iter().map(|n| one(n))).collect();
            out.push('(');
            out.push_str(&parts.join(joiner));
            out.push(')');
        }
        i = end;
    }
    out
}

/// The intent's name of every net of the model the intent has a net of exactly those pins for, under another name, when no rename of this load recorded one
/// already: a design whose stored net list has KiCad's names (stored by an earlier load or a schematic edit) still draws its labels with the intent's
/// names, and a section that still holds an intent's name (a snapshot an undo brought back) is renamed by it ([`rename_design`]).
pub(crate) fn remember_intent_names(model: &mut ConstraintModel, intent_nets: &[eda_model::Net]) {
    let pins = |n: &eda_model::Net| -> Vec<String> {
        let mut p = n.pins.clone();
        p.sort();
        p.dedup();
        p
    };
    let current: BTreeSet<String> = model.nets.iter().map(|n| n.name.clone()).collect();
    let by_pins: BTreeMap<Vec<String>, &str> = intent_nets.iter().filter(|n| !n.pins.is_empty() && !current.contains(&n.name)).map(|n| (pins(n), n.name.as_str())).collect();
    let taken: BTreeSet<String> = model.intent_net_names.values().cloned().collect();
    let mut add: Vec<(String, String)> = Vec::new();
    for n in &model.nets {
        if model.intent_net_names.contains_key(&n.name) {
            continue;
        }
        if let Some(intent) = by_pins.get(&pins(n)).filter(|i| **i != n.name && !taken.contains(**i)) {
            add.push((n.name.clone(), intent.to_string()));
        }
    }
    model.intent_net_names.extend(add);
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

/// Whether `p` is inside the polygon `outline` (even-odd).
fn inside(outline: &[eda_model::ir::Point], p: eda_model::ir::Point) -> bool {
    let mut odd = false;
    for i in 0..outline.len() {
        let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x as f64 + (p.y - a.y) as f64 * (b.x - a.x) as f64 / (b.y - a.y) as f64;
            if (p.x as f64) < x {
                odd = !odd;
            }
        }
    }
    odd
}

/// The copper on a net the design no longer has, given the net of the pads it reaches: a rename that no name of this load records (a part renamed, so a
/// net named after its pin, `Net-(U1-Pad1)`, is `Net-(U5-Pad1)` now; a net the drawing renamed while no list was stored). For each name of the copper
/// that is no net's, the pads its tracks and vias touch: when they are all on one net, the copper takes that net's name (as KiCad's connectivity gives
/// copper the net of the pads it joins). Copper that reaches no pad, or pads of several nets, keeps the name it has, and the board says so.
pub(crate) fn heal_stale_nets(design: &mut Design, model: &ConstraintModel) {
    let current: BTreeSet<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
    let Some(routing) = design.routing.as_ref() else { return };
    let stale: BTreeSet<String> = routing.tracks.iter().map(|t| t.net.clone()).chain(routing.vias.iter().map(|v| v.net.clone())).chain(routing.zones.iter().map(|z| z.net.clone())).filter(|n| !n.is_empty() && !current.contains(n.as_str())).collect();
    if stale.is_empty() {
        return;
    }
    let net_of: BTreeMap<&str, &str> = model.nets.iter().flat_map(|n| n.pins.iter().map(move |p| (p.as_str(), n.name.as_str()))).collect();
    // every placed pad, with the net of its pin
    let mut pads: Vec<(eda_model::ir::Point, (i64, i64), &str)> = Vec::new();
    for fp in design.placement.iter().flat_map(|p| p.footprints.iter()) {
        let Some(part) = model.part(&fp.id) else { continue };
        for pad in eda_model::footprint::placed_pads(model, part, fp).unwrap_or_default() {
            if let Some(net) = net_of.get(format!("{}.{}", fp.id, pad.number).as_str()) {
                pads.push((pad.center, pad.size, net));
            }
        }
    }
    let on_pad = |p: eda_model::ir::Point| -> Option<&str> {
        pads.iter().find(|(c, size, _)| {
            let r = size.0.max(size.1) / 2;
            (p.x - c.x).abs() <= r && (p.y - c.y).abs() <= r
        }).map(|(_, _, n)| *n)
    };
    let mut renames: BTreeMap<String, String> = BTreeMap::new();
    for name in stale {
        let points = routing.tracks.iter().filter(|t| t.net == name).flat_map(|t| t.pts.iter().copied()).chain(routing.vias.iter().filter(|v| v.net == name).map(|v| v.at));
        let mut found: BTreeSet<&str> = points.filter_map(on_pad).collect();
        // a zone of the name: the pads inside its outline
        for z in routing.zones.iter().filter(|z| z.net == name && z.outline.len() >= 3) {
            found.extend(pads.iter().filter(|(c, _, _)| inside(&z.outline, *c)).map(|(_, _, n)| *n));
        }
        if let [only] = found.into_iter().collect::<Vec<_>>()[..] {
            renames.insert(name, only.to_string());
        }
    }
    let model = ConstraintModel { nets: model.nets.clone(), ..Default::default() };
    rename_design(design, &model, &renames);
}

#[cfg(test)]
mod tests {
    use crate::board::tests::{scratch, setup};
    use crate::board::{load, save};
    use std::collections::BTreeSet;

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
        assert_eq!(model.board.custom_rules[0].condition.as_deref(), Some("(A.NetName == 'SIG' || A.NetName == '/SIG')"));
        assert!(model.board.custom_rules_text.as_deref().unwrap().contains("(A.NetName == 'SIG' || A.NetName == '/SIG')"));

        // Its drawing and its net list stored under KiCad's names (a schematic edit stores both), and a track on the intent's SIG brought back by an Undo
        // of a board-only step (the snapshot is the file as it was): the track takes the net's name all the same.
        let (_, mut design, model) = load(&dir).unwrap();
        let derived = crate::board::derived_schematic(&design, &model).unwrap();
        design.schematic = derived.schematic;
        design.nets = Some(model.nets.clone());
        let mut old = design.routing.as_ref().unwrap().tracks[0].clone();
        old.id = "trk_old".into();
        old.net = "SIG".into();
        design.routing.as_mut().unwrap().tracks.push(old);
        save(&dir, &design).unwrap();
        let (_, design, model) = load(&dir).unwrap();
        assert!(model.nets.iter().any(|n| n.name == "/SIG"), "{:?}", model.nets);
        assert_eq!(design.routing.as_ref().unwrap().tracks.iter().find(|t| t.id == "trk_old").unwrap().net, "/SIG");
        assert_eq!(model.intent_net_names.get("/SIG").map(String::as_str), Some("SIG"), "the stored list's net is drawn with the intent's name");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The intent's nets USB_DP and USB_DN of the test board, drawn as one wire each between U1 and U2: each gets a label (`eda_engine::net_labels`),
    /// so KiCad names them `/USB_DP` and `/USB_DN` and they are still a differential pair (`BOARD::MatchDpSuffix`) for the router, the tuner and the DRC.
    #[test]
    fn a_pair_the_intent_draws_with_wires_alone_is_still_a_pair() {
        let dir = scratch("kicad_names_pair");
        setup(&dir);
        let intent = dir.join("intent.yaml");
        let mut model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
        model.nets = vec![eda_model::Net { name: "USB_DP".into(), pins: vec!["U1.1".into(), "U2.1".into()] }, eda_model::Net { name: "USB_DN".into(), pins: vec!["U1.2".into(), "U2.2".into()] }];
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
        let (_, _, model) = load(&dir).unwrap();
        let mut names: Vec<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["/USB_DN", "/USB_DP"]);
        assert_eq!(eda_pns::diff_pair::dp_coupled_net_name("/USB_DP").as_deref(), Some("/USB_DN"));
        assert_eq!(eda_drc::pcbexpr::match_dp_suffix("/USB_DN").0, -1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A routed board stays routed through the edits that rename its nets: a part renamed, a pin added to a net in the intent, and copper a design kept
    /// under a name KiCad gives no net any more (`Net-(U1-Pad1)`, the name of a net no label drove, before the part was renamed) -- every track, via
    /// and zone is on a net the design has (`heal_stale_nets`: the copper takes the net of the pads it reaches).
    #[test]
    fn every_track_via_and_zone_stays_on_a_net_through_renames() {
        use eda_model::ir::Point;
        let p = |x: i64, y: i64| Point { x: x * 1_000_000, y: y * 1_000_000 };
        let dir = scratch("kicad_names_routed");
        setup(&dir);
        let intent = dir.join("intent.yaml");
        let mut model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
        model.nets = vec![eda_model::Net { name: "SIG".into(), pins: vec!["U1.1".into(), "U2.1".into()] }, eda_model::Net { name: "VCC".into(), pins: vec!["U1.2".into(), "U2.2".into()] }];
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
        let on_nets = |dir: &std::path::Path, what: &str| {
            let (_, design, model) = load(dir).unwrap();
            let known: BTreeSet<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
            let r = design.routing.as_ref().unwrap();
            for net in r.tracks.iter().map(|t| &t.net).chain(r.vias.iter().map(|v| &v.net)).chain(r.zones.iter().map(|z| &z.net)) {
                assert!(known.contains(net.as_str()), "{what}: copper on {net}, which is no net of {known:?}");
            }
            (r.tracks.len(), r.vias.len(), r.zones.len())
        };
        let sig = load(&dir).unwrap().2.nets.iter().find(|n| n.pins.contains(&"U1.1".to_string())).unwrap().name.clone();
        assert_eq!(sig, "/SIG");
        for cmd in [
            eda_ops::Cmd::AddTrack { net: sig.clone(), layer: "F.Cu".into(), width: 200_000, pts: vec![p(4, 5), p(4, 10), p(14, 10), p(14, 5)] },
            eda_ops::Cmd::AddVia { net: sig.clone(), x: 4_000_000, y: 10_000_000, drill: 300_000, diameter: 600_000, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() },
            eda_ops::Cmd::AddZone { net: sig.clone(), layer: "B.Cu".into(), outline: vec![p(13, 4), p(15, 4), p(15, 6), p(13, 6)] },
        ] {
            crate::board::step(&dir, cmd, false, "test").unwrap();
        }
        assert_eq!(on_nets(&dir, "routed"), (1, 1, 1));
        // (the rename on the sheet U1 is drawn on, which the first schematic edit stores)
        let (_, design, model) = load(&dir).unwrap();
        let derived = crate::board::derived_schematic(&design, &model).unwrap();
        let rename = eda_ops::Cmd::RenameSymbol { id: "U1".into(), new_id: "U5".into() };
        let root = derived.schematic.as_ref().unwrap();
        let cmd = match root.sheets.iter().find(|sh| derived.sheet_contents.as_ref().is_some_and(|c| c.get(&sh.file).is_some_and(|s| s.symbols.iter().any(|x| x.id == "U1")))) {
            Some(sheet) => eda_ops::Cmd::OnSheet { sheet: sheet.id.clone(), cmd: Box::new(rename) },
            None => rename,
        };
        crate::board::step(&dir, cmd, false, "test").unwrap();
        on_nets(&dir, "U1 renamed U5");
        // a pin added to SIG in the intent (a part the board has not placed)
        let mut model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
        let mut u3 = model.parts[0].clone();
        u3.reference = "U3".into();
        model.parts.push(u3);
        model.nets[0].pins.push("U3.1".into());
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
        on_nets(&dir, "U3.1 added to SIG");
        // copper a design kept under a name no net has now: it takes the net of the pads it reaches
        let (_, mut design, _) = load(&dir).unwrap();
        for t in design.routing.as_mut().unwrap().tracks.iter_mut() {
            t.net = "Net-(U1-Pad1)".into();
        }
        for v in design.routing.as_mut().unwrap().vias.iter_mut() {
            v.net = "Net-(U1-Pad1)".into();
        }
        design.routing.as_mut().unwrap().zones[0].net = "SIG_OLD".into();
        save(&dir, &design).unwrap();
        on_nets(&dir, "stale names");
        let _ = std::fs::remove_dir_all(&dir);
    }


    /// A rule's net comparison takes the nets it took (`VALUE::EqualTo`: `WildCompareString`, wildcards and case ignored): widened to the new names,
    /// with or without spaces, a wildcard, another case, `!=` and a pair's base name; a comparison of a class or one that still matches is left alone.
    #[test]
    fn a_rule_that_named_a_net_takes_it_under_its_new_name() {
        let renames: std::collections::BTreeMap<String, String> = [("PA7", "/LED/PA7"), ("USB_DP", "/USB_DP"), ("USB_DN", "/USB_DN"), ("HV", "/HV")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let w = |t: &str| super::widen_rule_text(t, &renames);
        assert_eq!(w("A.NetName == 'PA7'"), "(A.NetName == 'PA7' || A.NetName == '/LED/PA7')");
        assert_eq!(w("A.NetName=='pa7' && B.Type == 'Pad'"), "(A.NetName=='pa7' || A.NetName == '/LED/PA7') && B.Type == 'Pad'");
        assert_eq!(w("AB.NetName != 'USB_D*'"), "(AB.NetName != 'USB_D*' && AB.NetName != '/USB_DN' && AB.NetName != '/USB_DP')");
        assert_eq!(w("A.inDiffPair('USB_D')"), "(A.inDiffPair('USB_D') || A.inDiffPair('/USB_D'))");
        assert_eq!(w("A.NetClass == 'HV'"), "A.NetClass == 'HV'", "a class is a class");
        assert_eq!(w("A.NetName == '*PA7'"), "A.NetName == '*PA7'", "a pattern that still matches");
        assert_eq!(w("(rule x (condition \"A.NetName == 'PA7'\"))"), "(rule x (condition \"(A.NetName == 'PA7' || A.NetName == '/LED/PA7')\"))");
    }

}

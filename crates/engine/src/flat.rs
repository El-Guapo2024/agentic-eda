//! How a flat schematic hangs its rails and its labelled nets on the pins.
//!
//! `derive_schematic` lays the parts out and routes the wires between them; a net that is a rail (or too long to wire) is drawn at each of
//! its pins instead. Here that is done the way a module sheet does it (`hier::items`): a power symbol or a label stands two cells
//! (2.54 mm) off its pin's end, on a wire, outward, its glyph and its text running on away from the part; pins side by side on one rail
//! share a symbol; a `PWR_FLAG` stands on the pin's end and the symbol or label that shares the pin is three cells off, clear of it. What
//! hangs on a pin steers clear of the other parts, the wires already routed and what hangs on the other pins, by a longer stub.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use eda_model::ir::{NetLabel, NoConnect, Point, PowerSymbol, Wire};
use eda_model::{ConstraintModel, Part, PinKind};

use crate::hier::items::{outward_rot, Items, RailPin};
use crate::hier::kit::{nc_rect, power_rect, Placed, Rect, G};
use crate::{pin_index, resolve_pin_ref, split_pin_ref, MAX_MERGED_STUB_BENDS, MAX_MERGED_STUB_LEN_UM};

/// The nets that need a `PWR_FLAG` (a power or ground pin on them and no power output, a name that reads as a rail counting as a power
/// input), each with the pin the flag goes on: the net's own first (sorted) pin.
fn flag_anchors(model: &ConstraintModel, parts_by_ref: &HashMap<&str, &Part>, power_style_nets: &BTreeSet<String>, placed: &BTreeMap<String, Placed>, wires: &[Wire]) -> Vec<(String, String)> {
    let mut nets = model.nets.clone();
    nets.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out = Vec::new();
    for net in &nets {
        // Seeded `true` for a net that earned power/ground-symbol treatment by name alone: every pin on it is drawn as a `power_in`-typed
        // symbol whatever its own `PinKind`, so it needs a driver like one a real `Power`/`Ground`-kind pin put there.
        let mut has_power_in = power_style_nets.contains(&net.name);
        let mut has_power_out = false;
        for pin_ref in &net.pins {
            let Some((_, _, pin)) = resolve_pin_ref(pin_ref, parts_by_ref) else { continue };
            let is_out = pin.kind == PinKind::Power && pin.name.as_deref().unwrap_or("").to_ascii_uppercase().contains("OUT");
            match pin.kind {
                PinKind::Power if is_out => has_power_out = true,
                PinKind::Power | PinKind::Ground => has_power_in = true,
                _ => {}
            }
        }
        if !has_power_in || has_power_out {
            continue;
        }
        let mut pins = net.pins.clone();
        pins.sort();
        // the net's own first (sorted) pin, unless a wire is routed out of it that turns before the flag is clear of it
        let clear_of_wire = |pin_ref: &str| -> bool {
            let (r, num) = split_pin_ref(pin_ref);
            let Some((tip, side)) = placed.get(r).and_then(|p| p.tip(num)) else { return true };
            let (dx, dy) = crate::hier::kit::side_dir(side);
            wires.iter().filter(|w| w.net == net.name).all(|w| {
                let first = w.pts.first().copied();
                let last = w.pts.last().copied();
                let leaves = |from: Point, next: Option<&Point>| -> bool {
                    if from != tip {
                        return true;
                    }
                    let Some(n) = next else { return true };
                    (n.x - tip.x).signum() == dx && (n.y - tip.y).signum() == dy && (n.x - tip.x).abs() + (n.y - tip.y).abs() >= 3 * G
                };
                leaves(first.unwrap_or(tip), w.pts.get(1)) && leaves(last.unwrap_or(tip), w.pts.len().checked_sub(2).and_then(|i| w.pts.get(i)))
            })
        };
        let pick = pins.iter().find(|p| clear_of_wire(p)).or_else(|| pins.first()).cloned();
        if let Some(pin) = pick {
            out.push((net.name.clone(), pin));
        }
    }
    out
}

fn thin(a: Point, b: Point) -> Rect {
    Rect::new(a.x.min(b.x) - 100, a.y.min(b.y) - 100, a.x.max(b.x) + 100, a.y.max(b.y) + 100)
}

/// Draw the rails and the labelled nets of a flat schematic: returns the labels and the power symbols (power symbols numbered `#PWR..`, the
/// flags `#FLG..`) and how many of them found no place clear of the rest, and adds the wires they hang on to `wires`.
///
/// `positions` is where each part's box corner is; `label_nets` the nets drawn at every pin (`true` for a rail, `false` for a signal
/// net that spans blocks or ran over the wire budget).
#[allow(clippy::too_many_arguments)]
pub fn hang_nets(
    model: &ConstraintModel,
    parts_by_ref: &HashMap<&str, &Part>,
    positions: &BTreeMap<String, (i64, i64)>,
    wires: &mut Vec<Wire>,
    no_connects: &[NoConnect],
    label_nets: &[(String, Vec<String>, bool)],
    power_style_nets: &BTreeSet<String>,
) -> (Vec<NetLabel>, Vec<PowerSymbol>, usize, BTreeSet<String>) {
    let mut items = Items::default();
    let mut placed: BTreeMap<String, Placed> = BTreeMap::new();
    for part in &model.parts {
        let Some(&(x, y)) = positions.get(&part.reference) else { continue };
        let lib_id = model.lib_id_of(part);
        let resolved = model.real_symbol_of(&lib_id, part);
        let mut p = Placed::new(part, &lib_id, resolved, part.value.as_deref().unwrap_or(""), part.package.as_deref().unwrap_or(""));
        p.x = x;
        p.y = y;
        // the pieces of what the part draws, not the box that holds them all: a stub may pass between its body and its fields
        let mut pieces = vec![p.body_rect()];
        pieces.extend(p.field_rects());
        pieces.extend(p.pin_text_rects());
        for r in pieces {
            items.keepouts.push((part.reference.clone(), r));
            items.rects.push(r);
        }
        placed.insert(part.reference.clone(), p);
    }
    // what is already drawn: the routed wires and the no-connect flags
    let mut routed: Vec<(Rect, String)> = Vec::new();
    for w in wires.iter() {
        for pair in w.pts.windows(2) {
            items.hung.push(thin(pair[0], pair[1]));
            routed.push((thin(pair[0], pair[1]), w.net.clone()));
        }
    }
    let drawn_before = items.hung.len();
    for n in no_connects {
        items.hung.push(nc_rect(n.at));
    }
    let tip_of = |pin_ref: &str| -> Option<(Point, eda_layout::Side)> {
        let (r, num) = split_pin_ref(pin_ref);
        placed.get(r)?.tip(num)
    };

    // the flags first: each stands on its pin's end, so everything else keeps clear of it
    let anchors = flag_anchors(model, parts_by_ref, power_style_nets, &placed, wires);
    let mut flags: Vec<PowerSymbol> = Vec::new();
    let mut flag_pins: BTreeSet<String> = BTreeSet::new();
    for (net, pin_ref) in &anchors {
        let Some((tip, side)) = tip_of(pin_ref) else { continue };
        let rot = outward_rot(true, side);
        items.reserved.push((pin_ref.clone(), power_rect(tip, rot, net, "power:PWR_FLAG")));
        flag_pins.insert(pin_ref.clone());
        flags.push(PowerSymbol { id: String::new(), lib_id: "power:PWR_FLAG".to_string(), at: tip, rot, net: net.clone(), pin: String::new() });
    }

    let mut label_nets: Vec<&(String, Vec<String>, bool)> = label_nets.iter().collect();
    label_nets.sort();
    label_nets.dedup();

    // the rails: a power symbol at each pin, pins side by side sharing one
    let mut rails: Vec<RailPin> = Vec::new();
    for (net, pin_refs, is_power) in &label_nets {
        if !*is_power {
            continue;
        }
        for pin_ref in pin_refs {
            let Some((tip, side)) = tip_of(pin_ref) else { continue };
            rails.push(RailPin { flag: flag_pins.contains(pin_ref), pin_ref: pin_ref.clone(), tip, side, net: net.clone() });
        }
    }
    rails.sort_by(|a, b| (&a.net, &a.pin_ref).cmp(&(&b.net, &b.pin_ref)));
    rails.dedup_by(|a, b| a.net == b.net && a.pin_ref == b.pin_ref);
    // one part at a time: pins share a symbol only when they are on the same part
    let mut rails_of: BTreeMap<String, Vec<RailPin>> = BTreeMap::new();
    for r in rails {
        rails_of.entry(split_pin_ref(&r.pin_ref).0.to_string()).or_default().push(r);
    }
    for (_, rails) in rails_of {
        items.rail_pins(rails);
    }

    // the labelled nets, a label at each pin; adjacent pins of one net on one part share one
    for (net, pin_refs, is_power) in &label_nets {
        if *is_power {
            continue;
        }
        // group this net's pins by part, since two labels only merge when they belong to the same symbol
        let mut by_part: BTreeMap<&str, Vec<(String, usize)>> = BTreeMap::new();
        for pin_ref in pin_refs {
            let (part_ref, pin_num) = split_pin_ref(pin_ref);
            let Some(part) = parts_by_ref.get(part_ref) else { continue };
            let Some(p) = placed.get(part_ref) else { continue };
            if let Some(port_idx) = p.b.pin_port[pin_index(part, pin_num)] {
                by_part.entry(part_ref).or_default().push((pin_ref.clone(), port_idx));
            }
        }
        for (part_ref, mut pins) in by_part {
            // port indices run side by side within one contiguous block per side (`geometry::build_ports`), so consecutive integers on
            // one side are neighbouring pins
            pins.sort_by_key(|(_, idx)| *idx);
            let node = &placed[part_ref].b.node;
            let mut runs: Vec<Vec<(String, usize)>> = Vec::new();
            for p in pins {
                let same_side = |a: usize, b: usize| node.ports[a].side == node.ports[b].side;
                match runs.last_mut() {
                    Some(run) if p.1 == run.last().unwrap().1 + 1 && same_side(run.last().unwrap().1, p.1) => run.push(p),
                    _ => runs.push(vec![p]),
                }
            }
            for run in runs {
                let tips: Vec<(String, Point, eda_layout::Side)> = run.iter().filter_map(|(pin_ref, _)| tip_of(pin_ref).map(|(t, s)| (pin_ref.clone(), t, s))).collect();
                if tips.is_empty() {
                    continue;
                }
                let stub_len: i64 = tips.windows(2).map(|w| (w[0].1.x - w[1].1.x).abs() + (w[0].1.y - w[1].1.y).abs()).sum();
                let stub_bends = tips.len().saturating_sub(2);
                if tips.len() == 1 || stub_len > MAX_MERGED_STUB_LEN_UM || stub_bends > MAX_MERGED_STUB_BENDS {
                    // a single pin, or a run too wide to read as one local jumper: a label at each, no wire between them
                    for (pin_ref, tip, side) in &tips {
                        let len = if flag_pins.contains(pin_ref) { 3 * G } else { 2 * G };
                        items.labelled(pin_ref, *tip, *side, net, false, len);
                    }
                } else {
                    let side = tips[0].2;
                    let pins: Vec<(String, Point)> = tips.iter().map(|(r, t, _)| (r.clone(), *t)).collect();
                    items.label_run(&pins, side, net);
                }
            }
        }
    }

    // the routed nets that a hung thing or a flag could not keep clear of: the caller draws them with labels instead
    let mut blockers: BTreeSet<String> = BTreeSet::new();
    let hung_now = &items.hung[drawn_before..];
    for (r, net) in &routed {
        if hung_now.iter().chain(items.reserved.iter().map(|(_, k)| k)).any(|h| h.overlaps(r)) {
            blockers.insert(net.clone());
        }
    }

    wires.extend(std::mem::take(&mut items.wires));
    // numbered in the order the rails were asked for (by net, then pin), as before, then the flags
    let mut power: Vec<PowerSymbol> = std::mem::take(&mut items.power);
    power.sort_by(|a, b| (&a.net, &a.pin).cmp(&(&b.net, &b.pin)));
    for (i, p) in power.iter_mut().enumerate() {
        p.id = format!("#PWR{:02}", i + 1);
    }
    let mut n = power.len();
    for mut f in flags {
        n += 1;
        f.id = format!("#FLG{n:02}");
        power.push(f);
    }
    (std::mem::take(&mut items.labels), power, items.failures, blockers)
}

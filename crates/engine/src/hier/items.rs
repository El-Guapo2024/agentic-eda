//! What a layout emits: the drawn items of one block or one sheet, with the rectangles they occupy.

use std::collections::BTreeMap;

use eda_model::ir::{LabelKind, LabelShape, NetLabel, NoConnect, Point, PowerSymbol, SymbolInstance, Wire};
use eda_model::ConstraintModel;
use eda_model::PinKind;

use super::kit::{label_rect, nc_rect, power_rect, side_dir, snap_down, Placed, Rect, G};
use crate::geometry::is_power_or_ground_net_name;
use eda_layout::Side;
use eda_model::modules::{is_ground_net_name, is_rail_net};

/// What the existing schematic knew about its symbols, kept when a flat sheet is reorganized: a symbol's identity (reference),
/// fields and library symbol survive; only its place changes.
#[derive(Debug, Clone, Default)]
pub struct Keep {
    pub symbols: BTreeMap<String, SymbolInstance>,
    pub user_fields: BTreeMap<String, BTreeMap<String, String>>,
    pub erc_exclusions: Vec<eda_model::ir::ErcExclusion>,
    pub erc_pin_map: Option<eda_model::ir::ErcPinMap>,
    pub title_block: Option<eda_model::ir::TitleBlock>,
    /// References of the symbols that were locked (`SchExtras::locked`); the lock goes with the symbol to its new sheet.
    pub locked: Vec<String>,
}

/// How a net is drawn inside one module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetClass {
    /// A supply or ground rail: a power symbol at every pin.
    Rail,
    /// A net with pins in other modules too: a hierarchical label (and a sheet pin on the root).
    Cross,
    /// A net wholly inside the module that is not a rail.
    Internal,
}

/// The whole design's nets, classified once.
pub struct Ctx<'a> {
    pub model: &'a ConstraintModel,
    pub keep: &'a Keep,
    /// net name -> every pin on it that is drawn (no-connect pins left out), sorted.
    pub net_pins: BTreeMap<String, Vec<String>>,
    pub rail: BTreeMap<String, bool>,
    /// "REF.PIN" -> net name.
    pub net_of_pin: BTreeMap<String, String>,
}

impl<'a> Ctx<'a> {
    pub fn new(model: &'a ConstraintModel, keep: &'a Keep) -> Ctx<'a> {
        let mut net_pins: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut net_of_pin: BTreeMap<String, String> = BTreeMap::new();
        let mut rail = BTreeMap::new();
        for n in &model.nets {
            let mut pins: Vec<String> = n
                .pins
                .iter()
                .filter(|p| {
                    let (r, num) = p.split_once('.').unwrap_or((p.as_str(), ""));
                    model.part(r).and_then(|part| part.pins.iter().find(|x| x.number == num)).is_some_and(|x| x.kind != PinKind::Nc)
                })
                .cloned()
                .collect();
            pins.sort();
            pins.dedup();
            if pins.len() < 2 {
                continue;
            }
            rail.insert(n.name.clone(), is_rail_net(model, n) || is_power_or_ground_net_name(&n.name));
            for p in &pins {
                net_of_pin.insert(p.clone(), n.name.clone());
            }
            net_pins.insert(n.name.clone(), pins);
        }
        Ctx { model, keep, net_pins, rail, net_of_pin }
    }

    pub fn is_rail(&self, net: &str) -> bool {
        self.rail.get(net).copied().unwrap_or(false)
    }

    /// supply 0, signal 1, ground 2: which end of a chain goes on top.
    pub fn rank(&self, net: Option<&str>) -> u8 {
        match net {
            Some(n) if self.is_rail(n) => {
                if is_ground_net_name(n) {
                    2
                } else {
                    0
                }
            }
            _ => 1,
        }
    }
}

/// One module's view of the nets: what each net is drawn as here.
pub struct View {
    pub refs: std::collections::BTreeSet<String>,
    pub class: BTreeMap<String, NetClass>,
    /// For a [`NetClass::Cross`] net: the pin that carries the hierarchical label; every other pin of the net here gets a local one.
    pub carrier: BTreeMap<String, String>,
    /// The nets that leave the module, in the order the module's pins first meet them.
    pub crossing: Vec<String>,
}

impl View {
    pub fn new(ctx: &Ctx, refs: &[String]) -> View {
        let set: std::collections::BTreeSet<String> = refs.iter().cloned().collect();
        let mut class = BTreeMap::new();
        for (net, pins) in &ctx.net_pins {
            let inside: Vec<&String> = pins.iter().filter(|p| set.contains(p.split('.').next().unwrap_or(""))).collect();
            if inside.is_empty() {
                continue;
            }
            let c = if ctx.is_rail(net) {
                NetClass::Rail
            } else if inside.len() < pins.len() {
                NetClass::Cross
            } else {
                NetClass::Internal
            };
            class.insert(net.clone(), c);
        }
        View { refs: set, class, carrier: BTreeMap::new(), crossing: Vec::new() }
    }

    pub fn class_of(&self, net: &str) -> Option<NetClass> {
        self.class.get(net).copied()
    }

    /// Give each crossing net its carrier, in the order `pins` (the module's pins in drawing order) meet them.
    pub fn assign_carriers(&mut self, ctx: &Ctx, pins_in_order: &[String]) {
        for p in pins_in_order {
            let Some(net) = ctx.net_of_pin.get(p) else { continue };
            if self.class.get(net) == Some(&NetClass::Cross) && !self.carrier.contains_key(net) {
                self.carrier.insert(net.clone(), p.clone());
                self.crossing.push(net.clone());
            }
        }
    }
}

/// Everything drawn for one block or sheet.
#[derive(Debug, Clone, Default)]
pub struct Items {
    pub symbols: Vec<SymbolInstance>,
    pub wires: Vec<Wire>,
    pub labels: Vec<NetLabel>,
    pub power: Vec<PowerSymbol>,
    pub ncs: Vec<NoConnect>,
    /// "REF.PIN" -> where its wire meets it.
    pub tips: BTreeMap<String, Point>,
    /// Everything drawn, as rectangles.
    pub rects: Vec<Rect>,
    /// The symbol keepouts alone, by reference, so a later part can steer clear of an earlier one.
    pub keepouts: Vec<(String, Rect)>,
    /// The box of the part a block is built around, kept through every move so the sheet can centre it.
    pub core: Option<Rect>,
}

fn shift(p: &mut Point, dx: i64, dy: i64) {
    p.x += dx;
    p.y += dy;
}

impl Items {
    pub fn extent(&self) -> Option<Rect> {
        let mut r = super::kit::union_all(self.rects.iter().copied());
        for w in &self.wires {
            for p in &w.pts {
                let pr = Rect { x0: p.x, y0: p.y, x1: p.x, y1: p.y };
                r = Some(r.map_or(pr, |e| e.union(pr)));
            }
        }
        r
    }

    pub fn translate(&mut self, dx: i64, dy: i64) {
        for s in &mut self.symbols {
            shift(&mut s.at, dx, dy);
        }
        for w in &mut self.wires {
            for p in &mut w.pts {
                shift(p, dx, dy);
            }
        }
        for l in &mut self.labels {
            shift(&mut l.at, dx, dy);
        }
        for p in &mut self.power {
            shift(&mut p.at, dx, dy);
        }
        for n in &mut self.ncs {
            shift(&mut n.at, dx, dy);
        }
        for t in self.tips.values_mut() {
            shift(t, dx, dy);
        }
        for r in &mut self.rects {
            *r = r.translate(dx, dy);
        }
        for (_, r) in &mut self.keepouts {
            *r = r.translate(dx, dy);
        }
        self.core = self.core.map(|c| c.translate(dx, dy));
    }

    /// Move so the extent's top-left sits at the origin (on the grid, never past it).
    pub fn normalize(&mut self) {
        if let Some(e) = self.extent() {
            self.translate(-snap_down(e.x0), -snap_down(e.y0));
        }
    }

    pub fn append(&mut self, o: Items) {
        self.symbols.extend(o.symbols);
        self.wires.extend(o.wires);
        self.labels.extend(o.labels);
        self.power.extend(o.power);
        self.ncs.extend(o.ncs);
        self.tips.extend(o.tips);
        self.rects.extend(o.rects);
        self.keepouts.extend(o.keepouts);
        self.core = self.core.or(o.core);
    }

    // ------------------------------------------------------------------------------------------- emitters

    /// The placed symbol itself, its pin tips, and a no-connect flag on every `nc` pin.
    pub fn symbol(&mut self, ctx: &Ctx, p: &Placed) {
        let kept = ctx.keep.symbols.get(&p.part.reference);
        self.symbols.push(SymbolInstance {
            id: p.part.reference.clone(),
            at: p.at(),
            rot: p.rot(),
            mirrored: false,
            mirror_y: false,
            lib_id: p.lib_id.clone(),
            unit: 1,
            value: kept.map(|k| k.value.clone()).unwrap_or_else(|| p.part.value.clone().unwrap_or_default()),
            footprint: kept.map(|k| k.footprint.clone()).unwrap_or_else(|| p.part.footprint.clone().unwrap_or_default()),
            datasheet: kept.map(|k| k.datasheet.clone()).unwrap_or_else(|| p.part.datasheet.clone().unwrap_or_default()),
            // the attributes the user set (Do Not Populate, left out of the BOM / board / simulation) go with the part
            dnp: kept.is_some_and(|k| k.dnp),
            exclude_from_bom: kept.is_some_and(|k| k.exclude_from_bom),
            exclude_from_board: kept.is_some_and(|k| k.exclude_from_board),
            exclude_from_sim: kept.is_some_and(|k| k.exclude_from_sim),
        });
        let keep = p.keepout();
        self.rects.push(keep);
        self.keepouts.push((p.part.reference.clone(), keep));
        for pin in &p.part.pins {
            if let Some((tip, _)) = p.tip(&pin.number) {
                self.tips.insert(format!("{}.{}", p.part.reference, pin.number), tip);
            }
        }
        // `nc` pins: a flag on the point the engine reports for them.
        let sym = self.symbols.last().cloned().unwrap();
        for (number, at) in crate::placed::pin_points(&sym, &p.part, p.resolved.as_ref()) {
            let Some(pin) = p.part.pins.iter().find(|x| x.number == number) else { continue };
            if pin.kind == PinKind::Nc {
                self.ncs.push(NoConnect { id: String::new(), at, pin: format!("{}.{}", p.part.reference, number) });
                self.rects.push(nc_rect(at));
            }
        }
    }

    pub fn wire(&mut self, net: &str, pins: Vec<String>, pts: Vec<Point>) {
        self.wires.push(Wire { id: String::new(), net: net.to_string(), pins, pts, bus: false });
    }

    /// A power symbol on a pin tip; the glyph points away from the part.
    pub fn power_at(&mut self, pin_ref: &str, tip: Point, side: Side, net: &str) {
        let up_glyph = !is_ground_net_name(net);
        // A ground glyph hangs down and a supply glyph rises; where that would run back into the part, turn it half a turn.
        let rot = match (side, up_glyph) {
            (Side::Top, false) | (Side::Bottom, true) => 180_000,
            _ => 0,
        };
        let glyph_up = if rot == 0 { up_glyph } else { !up_glyph };
        self.power.push(PowerSymbol { id: String::new(), lib_id: power_lib_id(net), at: tip, rot, net: net.to_string(), pin: pin_ref.to_string() });
        self.rects.push(power_rect(tip, net, glyph_up));
    }

    /// A wire from a pin tip straight out `len`, ending in a label.
    pub fn labelled(&mut self, pin_ref: &str, tip: Point, side: Side, net: &str, hierarchical: bool, len: i64) {
        let (dx, dy) = side_dir(side);
        let end = Point { x: tip.x + dx * len, y: tip.y + dy * len };
        self.wire(net, vec![pin_ref.to_string()], vec![tip, end]);
        let kind = if hierarchical { LabelKind::Hierarchical { shape: LabelShape::Bidirectional } } else { LabelKind::Local };
        self.labels.push(NetLabel { id: String::new(), net: net.to_string(), at: end, kind });
        self.rects.push(label_rect(end, (dx, dy), net, hierarchical));
    }

    /// Draw whatever `pin_ref`'s net needs at its tip: a power symbol, or a wire to a label.
    pub fn connect(&mut self, ctx: &Ctx, view: &View, pin_ref: &str, tip: Point, side: Side) {
        let Some(net) = ctx.net_of_pin.get(pin_ref) else { return };
        match view.class_of(net) {
            Some(NetClass::Rail) => self.power_at(pin_ref, tip, side, net),
            Some(NetClass::Cross) => {
                let carrier = view.carrier.get(net).is_some_and(|c| c == pin_ref);
                self.labelled(pin_ref, tip, side, net, carrier, 2 * G)
            }
            Some(NetClass::Internal) => self.labelled(pin_ref, tip, side, net, false, 2 * G),
            None => {}
        }
    }
}

/// `"power:GND"` for a ground rail, `"power:<NAME>"` for any other (the engine's own rule).
pub fn power_lib_id(net: &str) -> String {
    let upper = net.to_ascii_uppercase();
    if upper.starts_with("GND") || upper.starts_with("AGND") || upper.starts_with("DGND") {
        "power:GND".to_string()
    } else {
        format!("power:{net}")
    }
}

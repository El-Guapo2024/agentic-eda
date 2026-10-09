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

/// The room kept around what hangs on a pin (micrometres), and how many cells a stub may be made longer to find it.
const HANG_GAP: i64 = 150;
const MAX_PUSH: i64 = 24;

/// A pin on a supply or ground rail, waiting to be drawn (`Items::rail_pins`).
#[derive(Debug, Clone)]
pub struct RailPin {
    pub pin_ref: String,
    pub tip: Point,
    pub side: Side,
    pub net: String,
    /// The net's `PWR_FLAG` goes on this pin's end: the stub is long enough to take it and still leave the rail's symbol clear of it.
    pub flag: bool,
}

fn side_index(s: Side) -> u8 {
    match s {
        Side::Top => 0,
        Side::Bottom => 1,
        Side::Left => 2,
        Side::Right => 3,
    }
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
    /// The pins that carry a `PWR_FLAG` (the first pin of every net that needs one).
    pub flag_pins: std::collections::BTreeSet<String>,
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
        let mut ctx = Ctx { model, keep, net_pins, rail, net_of_pin, flag_pins: Default::default() };
        ctx.flag_pins = super::flag_nets(&ctx).into_iter().filter_map(|(_, pins)| pins.into_iter().next()).collect();
        ctx
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
    /// "REF.PIN" -> the side of its part the pin is on (the way a wire leaves it).
    pub tip_sides: BTreeMap<String, Side>,
    /// Everything drawn, as rectangles.
    pub rects: Vec<Rect>,
    /// The symbol keepouts alone, by reference, so a later part can steer clear of an earlier one.
    pub keepouts: Vec<(String, Rect)>,
    /// What hangs off the pins (stubs, power symbols, labels), so a later one steers clear of an earlier one on the next pin.
    pub hung: Vec<Rect>,
    /// What stands on a pin's end (a flag), by the pin: everything hung on another pin steers clear of it.
    pub reserved: Vec<(String, Rect)>,
    /// How many things found no place clear of the rest (they hang where they would first have): the layout had too little room.
    pub failures: usize,
    /// The box of the part a block is built around, kept through every move so the sheet can centre it.
    pub core: Option<Rect>,
}

/// The box of a wire from `a` to `b`: the line and a hair either side.
fn wire_rect(a: Point, b: Point) -> Rect {
    Rect::new(a.x.min(b.x) - 200, a.y.min(b.y) - 200, a.x.max(b.x) + 200, a.y.max(b.y) + 200)
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
        for r in &mut self.hung {
            *r = r.translate(dx, dy);
        }
        for (_, r) in &mut self.reserved {
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
        self.tip_sides.extend(o.tip_sides);
        self.rects.extend(o.rects);
        self.keepouts.extend(o.keepouts);
        self.hung.extend(o.hung);
        self.reserved.extend(o.reserved);
        self.failures += o.failures;
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
            if let Some((tip, side)) = p.tip(&pin.number) {
                self.tips.insert(format!("{}.{}", p.part.reference, pin.number), tip);
                self.tip_sides.insert(format!("{}.{}", p.part.reference, pin.number), side);
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

    /// Is the thing at `end` (its box `item`), joined to the pin at `tip` by a wire, clear of everything hung on the other pins and of the
    /// other parts? The pin's own part is not in its way: the wire leaves it.
    fn hangs_clear(&self, pin_ref: &str, tip: Point, end: Point, item: Rect) -> bool {
        self.rect_clear(pin_ref, item.inflate(HANG_GAP)) && self.rect_clear(pin_ref, wire_rect(tip, end))
    }

    /// Does `r` keep clear of everything hung so far, of what stands on other pins' ends and of the parts other than the pin's own?
    fn rect_clear(&self, pin_ref: &str, r: Rect) -> bool {
        let own = pin_ref.split('.').next().unwrap_or("");
        self.hung.iter().all(|h| !h.overlaps(&r)) && self.keepouts.iter().all(|(who, k)| who == own || !k.overlaps(&r)) && self.reserved.iter().all(|(pin, k)| pin == pin_ref || !k.overlaps(&r))
    }

    /// Where a wire from the pin at `tip` (on `side` of its part) may go to hang something whose box at `end`, facing `dir`, is `item_at(end, dir)`:
    /// straight out from `min` cells, else out `min` cells and along, either way, as far as it takes. Returns the wire's points and the way the thing at
    /// its end faces; the straight stub of `min` cells when nothing is clear.
    fn hang_route(&self, pin_ref: &str, tip: Point, side: Side, min: i64, item_at: impl Fn(Point, (i64, i64)) -> Rect) -> (Vec<Point>, (i64, i64), bool) {
        let (dx, dy) = side_dir(side);
        let out = |p: Point, d: (i64, i64), c: i64| Point { x: p.x + d.0 * c * G, y: p.y + d.1 * c * G };
        for c in min..=min + MAX_PUSH {
            let end = out(tip, (dx, dy), c);
            if self.hangs_clear(pin_ref, tip, end, item_at(end, (dx, dy))) {
                return (vec![tip, end], (dx, dy), true);
            }
        }
        // out `min` cells, then along the row of pins either way
        for k in 2..=MAX_PUSH {
            for sign in [1, -1] {
                let perp = (-dy * sign, dx * sign);
                let turn = out(tip, (dx, dy), min);
                let end = out(turn, perp, k);
                if self.rect_clear(pin_ref, wire_rect(tip, turn)) && self.hangs_clear(pin_ref, turn, end, item_at(end, perp)) {
                    return (vec![tip, turn, end], perp, true);
                }
            }
        }
        (vec![tip, out(tip, (dx, dy), min)], (dx, dy), false)
    }

    /// Remember the wire from `tip` to `end` and the box of what it ends in.
    fn hang(&mut self, tip: Point, end: Point, item: Rect) {
        let wire = Rect::new(tip.x.min(end.x) - 200, tip.y.min(end.y) - 200, tip.x.max(end.x) + 200, tip.y.max(end.y) + 200);
        self.rects.push(wire);
        self.hung.push(wire);
        self.rects.push(item);
        self.hung.push(item);
    }

    /// A power symbol at `at`, its stem running on out along `side` (away from the part): KiCad's way of drawing one on a pin that is not on
    /// the top or the bottom, where a symbol hung across the pins would run over the next pin's wire. A ground glyph hangs down at rest and
    /// a supply glyph rises; turned so that the way it points is outward.
    pub fn power_symbol_at(&mut self, pin_ref: &str, at: Point, side: Side, net: &str) {
        let rot = outward_rot(!is_ground_net_name(net), side);
        let lib_id = power_lib_id(net);
        self.power.push(PowerSymbol { id: String::new(), lib_id: lib_id.clone(), at, rot, net: net.to_string(), pin: pin_ref.to_string() });
        let r = power_rect(at, rot, net, &lib_id);
        self.rects.push(r);
        self.hung.push(r);
    }

    /// A power symbol off a pin: a wire two cells long out of the tip, the symbol on its end; three when a flag stands on the tip, so the flag's
    /// glyph and the symbol's do not touch; longer, or bent along the row of pins, when what hangs on the pin next to it is in the way.
    pub fn power_at(&mut self, pin_ref: &str, tip: Point, side: Side, net: &str, flag: bool) {
        let lib_id = power_lib_id(net);
        let rises = !is_ground_net_name(net);
        let (pts, dir, found) = self.hang_route(pin_ref, tip, side, if flag { 3 } else { 2 }, |end, d| power_rect(end, outward_rot(rises, side_of_dir(d)), net, &lib_id));
        self.failures += usize::from(!found);
        self.wire(net, vec![pin_ref.to_string()], pts.clone());
        for pair in pts.windows(2) {
            let wire = Rect::new(pair[0].x.min(pair[1].x) - 200, pair[0].y.min(pair[1].y) - 200, pair[0].x.max(pair[1].x) + 200, pair[0].y.max(pair[1].y) + 200);
            self.rects.push(wire);
            self.hung.push(wire);
        }
        self.power_symbol_at(pin_ref, *pts.last().expect("a route has points"), side_of_dir(dir), net);
    }

    /// The rail pins of one part, drawn the way a person draws them: pins of one rail that sit side by side on one side of the part (at most three
    /// cells apart, so no other pin is between them) share one power symbol. Each has a stub three cells long, the stubs are joined across their ends, and the symbol sits at the end
    /// that leaves its glyph clear of the join (a supply glyph rises, so it takes the top-most pin of a side, a ground glyph hangs, so the bottom-most;
    /// on the top or bottom of a part, where the join is horizontal, the right-most, since the painter writes the net's name to the right of the
    /// symbol). The stubs take the symbols clear of the part's own texts -- the painter writes the reference above the part and the value and
    /// footprint below it (`Placed::field_rects`) -- and two symbols never write their names over one another. A pin alone keeps its symbol on its tip.
    pub fn rail_pins(&mut self, pins: Vec<RailPin>) {
        let mut groups: BTreeMap<(u8, String), Vec<RailPin>> = BTreeMap::new();
        for p in pins {
            groups.entry((side_index(p.side), p.net.clone())).or_default().push(p);
        }
        for ((_, net), mut group) in groups {
            let side = group[0].side;
            let horizontal = matches!(side, Side::Top | Side::Bottom);
            group.sort_by_key(|p| if horizontal { p.tip.x } else { p.tip.y });
            // runs of pins side by side
            let mut runs: Vec<Vec<RailPin>> = Vec::new();
            for p in group {
                let axis = |q: &RailPin| if horizontal { q.tip.x } else { q.tip.y };
                match runs.last_mut() {
                    Some(run) if axis(&p) - axis(run.last().expect("a run has a pin")) <= 3 * G => run.push(p),
                    _ => runs.push(vec![p]),
                }
            }
            for run in runs {
                if let [only] = run.as_slice() {
                    self.power_at(&only.pin_ref, only.tip, only.side, &net, only.flag);
                    continue;
                }
                let (dx, dy) = side_dir(side);
                let anchor = if horizontal || is_ground_net_name(&net) { run.len() - 1 } else { 0 };
                // the stubs, three cells at least, as long as it takes for the join, the symbol and its name to clear what is already hung
                let lib_id = power_lib_id(&net);
                let rot = outward_rot(!is_ground_net_name(&net), side);
                let ends_at = |c: i64| -> Vec<Point> { run.iter().map(|p| Point { x: p.tip.x + dx * c * G, y: p.tip.y + dy * c * G }).collect() };
                let found = (3..=3 + MAX_PUSH).find(|&c| {
                    let ends = ends_at(c);
                    run.iter().zip(&ends).all(|(p, e)| self.rect_clear(&p.pin_ref, wire_rect(p.tip, *e)))
                        && ends.windows(2).all(|w| self.rect_clear(&run[0].pin_ref, wire_rect(w[0], w[1])))
                        && self.rect_clear(&run[0].pin_ref, power_rect(ends[anchor], rot, &net, &lib_id).inflate(HANG_GAP))
                });
                self.failures += usize::from(found.is_none());
                let cells = found.unwrap_or(3);
                let ends = ends_at(cells);
                for (i, p) in run.iter().enumerate() {
                    self.wire(&net, vec![p.pin_ref.clone()], vec![p.tip, ends[i]]);
                    let w = Rect::new(p.tip.x.min(ends[i].x) - 200, p.tip.y.min(ends[i].y) - 200, p.tip.x.max(ends[i].x) + 200, p.tip.y.max(ends[i].y) + 200);
                    self.rects.push(w);
                    self.hung.push(w);
                    if i + 1 < run.len() {
                        self.wire(&net, Vec::new(), vec![ends[i], ends[i + 1]]);
                        let j = Rect::new(ends[i].x.min(ends[i + 1].x) - 200, ends[i].y.min(ends[i + 1].y) - 200, ends[i].x.max(ends[i + 1].x) + 200, ends[i].y.max(ends[i + 1].y) + 200);
                        self.rects.push(j);
                        self.hung.push(j);
                    }
                }
                self.power_symbol_at(&run[anchor].pin_ref, ends[anchor], side, &net);
            }
        }
    }

    /// A wire from a pin tip straight out `len` (more, or bent along the row of pins, when what hangs on the pin next to it is in the way), ending in a
    /// label.
    pub fn labelled(&mut self, pin_ref: &str, tip: Point, side: Side, net: &str, hierarchical: bool, len: i64) {
        let (pts, dir, found) = self.hang_route(pin_ref, tip, side, len / G, |end, d| label_rect(end, d, net, hierarchical));
        self.failures += usize::from(!found);
        let end = *pts.last().expect("a route has points");
        self.wire(net, vec![pin_ref.to_string()], pts.clone());
        let kind = if hierarchical { LabelKind::Hierarchical { shape: LabelShape::Bidirectional } } else { LabelKind::Local };
        self.labels.push(NetLabel { id: String::new(), net: net.to_string(), at: end, kind });
        for pair in pts.windows(2) {
            let wire = Rect::new(pair[0].x.min(pair[1].x) - 200, pair[0].y.min(pair[1].y) - 200, pair[0].x.max(pair[1].x) + 200, pair[0].y.max(pair[1].y) + 200);
            self.rects.push(wire);
            self.hung.push(wire);
        }
        let item = label_rect(end, dir, net, hierarchical);
        self.rects.push(item);
        self.hung.push(item);
    }

    /// Adjacent pins of one net on one side of a part, drawn as one: each has a stub three cells long, the stubs are joined across their ends, and
    /// one label stands on a tail out of the middle of the join, its text running on outward -- so two labels of one net never crowd each other.
    pub fn label_run(&mut self, pins: &[(String, Point)], side: Side, net: &str) {
        let (dx, dy) = side_dir(side);
        let at = |p: Point, c: i64| Point { x: p.x + dx * c * G, y: p.y + dy * c * G };
        // the stubs, three cells at least, as long as it takes for the join and the label on its tail to clear what is already hung
        let own = pins[0].0.clone();
        let fits = |c: i64| -> bool {
            let ends: Vec<Point> = pins.iter().map(|(_, t)| at(*t, c)).collect();
            let n = ends.len();
            let mid = if n % 2 == 1 { ends[n / 2] } else { Point { x: (ends[n / 2 - 1].x + ends[n / 2].x) / 2, y: (ends[n / 2 - 1].y + ends[n / 2].y) / 2 } };
            let tail_end = at(mid, 2);
            pins.iter().zip(&ends).all(|((r, t), e)| self.rect_clear(r, wire_rect(*t, *e)))
                && ends.windows(2).all(|w| self.rect_clear(&own, wire_rect(w[0], w[1])))
                && self.hangs_clear(&own, mid, tail_end, label_rect(tail_end, (dx, dy), net, false))
        };
        let found = (3..=3 + MAX_PUSH).find(|&c| fits(c));
        self.failures += usize::from(found.is_none());
        let stub = found.unwrap_or(3);
        let ends: Vec<Point> = pins.iter().map(|(_, t)| at(*t, stub)).collect();
        let n = ends.len();
        // the join's middle: a pin's own stub end, or halfway between the two middle ones
        let mid = if n % 2 == 1 { ends[n / 2] } else { Point { x: (ends[n / 2 - 1].x + ends[n / 2].x) / 2, y: (ends[n / 2 - 1].y + ends[n / 2].y) / 2 } };
        let tail = (2..=2 + MAX_PUSH).find(|&c| self.hangs_clear(&own, mid, at(mid, c), label_rect(at(mid, c), (dx, dy), net, false))).unwrap_or(2);
        let end = at(mid, tail);
        let mut join: Vec<Point> = ends.clone();
        if n % 2 == 0 {
            join.insert(n / 2, mid);
        }
        for ((pin_ref, tip), e) in pins.iter().zip(&ends) {
            self.wire(net, vec![pin_ref.clone()], vec![*tip, *e]);
            let w = Rect::new(tip.x.min(e.x) - 200, tip.y.min(e.y) - 200, tip.x.max(e.x) + 200, tip.y.max(e.y) + 200);
            self.rects.push(w);
            self.hung.push(w);
        }
        for pair in join.windows(2) {
            self.wire(net, Vec::new(), vec![pair[0], pair[1]]);
            let w = Rect::new(pair[0].x.min(pair[1].x) - 200, pair[0].y.min(pair[1].y) - 200, pair[0].x.max(pair[1].x) + 200, pair[0].y.max(pair[1].y) + 200);
            self.rects.push(w);
            self.hung.push(w);
        }
        self.wire(net, Vec::new(), vec![mid, end]);
        self.labels.push(NetLabel { id: String::new(), net: net.to_string(), at: end, kind: LabelKind::Local });
        self.hang(mid, end, label_rect(end, (dx, dy), net, false));
    }

    /// Draw whatever `pin_ref`'s net needs at its tip: a power symbol, or a wire to a label.
    pub fn connect(&mut self, ctx: &Ctx, view: &View, pin_ref: &str, tip: Point, side: Side) {
        let Some(net) = ctx.net_of_pin.get(pin_ref) else { return };
        // a flag on the pin's end takes the first two cells of the wire: the label stands clear of it
        let flag = ctx.flag_pins.contains(pin_ref);
        let len = if flag { 3 * G } else { 2 * G };
        match view.class_of(net) {
            Some(NetClass::Rail) => self.power_at(pin_ref, tip, side, net, flag),
            Some(NetClass::Cross) => {
                let carrier = view.carrier.get(net).is_some_and(|c| c == pin_ref);
                self.labelled(pin_ref, tip, side, net, carrier, len)
            }
            Some(NetClass::Internal) => self.labelled(pin_ref, tip, side, net, false, len),
            None => {}
        }
    }
}

/// The side of a part a wire leaving along `dir` leaves from.
pub fn side_of_dir(dir: (i64, i64)) -> Side {
    match dir {
        (1, 0) => Side::Right,
        (-1, 0) => Side::Left,
        (0, -1) => Side::Top,
        _ => Side::Bottom,
    }
}

/// The turn that makes a power glyph run on out along `side`: a ground glyph hangs down at rest and a supply glyph (or a flag) rises; the engine's
/// `rot` turns clockwise on the sheet, so a quarter takes "down" to "left".
pub fn outward_rot(rises_at_rest: bool, side: Side) -> u32 {
    match (rises_at_rest, side) {
        (false, Side::Bottom) | (true, Side::Top) => 0,
        (false, Side::Top) | (true, Side::Bottom) => 180_000,
        (false, Side::Left) | (true, Side::Right) => 90_000,
        (false, Side::Right) | (true, Side::Left) => 270_000,
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

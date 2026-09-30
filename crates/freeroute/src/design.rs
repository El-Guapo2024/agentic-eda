//! Boards from `design.json`, and routes back into it.
//!
//! A placed design -- its footprints' pads, its nets, its stackup and
//! rules -- becomes the board FreeRouting builds from the Specctra DSN
//! KiCad would write for it: micrometres at resolution 10, so a board unit
//! is 0.1 µm. [`to_dsn`] writes that DSN, so FreeRouting itself can be
//! asked what board it makes of a design; the parity tests hold
//! [`board_from_design`] to its answer, field for field.
//!
//! Each placed footprint becomes an image of its own with its pads already
//! in board coordinates, placed at the origin: rotation and side are the
//! model's own transform, the one every other stage uses, and FreeRouting
//! is left nothing to turn or mirror.
//!
//! The board is the one the routing gates judge, so what the router makes
//! of it passes them: every pad is the rectangle the gates measure it by,
//! the outline keeps copper the gates' edge clearance away, the strips
//! between adjacent SMD pads of a part and the refdes labels are
//! keep-outs, as the gates want them, and traces keep their class's
//! width into narrow pins. Every clearance carries a small margin for the
//! rounding to whole µm on the way back.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use eda_model::footprint::{placed_pads, placed_refdes_box, PlacedPad};
use eda_model::ir::{Design, Point as IrPoint, RoutingSection, Side, Track, Via as IrVia};
use eda_model::{BoardRules, ConstraintModel};

use crate::autoroute::batch::{autoroute_item, autoroute_passes, autoroute_passes_with, pass_items, remove_pass_tails, PassSummary, RouteResult};
use crate::autoroute::optimize::{optimize_board, OptSummary};
use crate::board::PadShape;
use crate::board::AreaShape;
use crate::geometry::{Circle, Direction, FloatPoint, IntBox, IntPoint, Line, PolygonShape, TileShape};
use crate::model::{AreaKind, AutorouteSettings, Board, ExitRestriction, FixedState, Item, ItemKind, Layer, Net, NetClass, Padstack, Rules, ViaInfo};
use crate::routing::RoutingBoard;
use crate::rules::ClearanceMatrix;

/// Board units per micrometre.
pub const UNITS_PER_UM: i64 = 10;

/// `Limits.CRIT_INT`: FreeRouting scales down a board reaching a fifth of it.
const CRIT_INT: i64 = 33_554_432;

/// `BoardOutline.HALF_WIDTH`.
const OUTLINE_HALF_WIDTH: i64 = 100;

/// Added to every clearance, µm: routes go back to the design rounded to
/// the µm, which moves a corner up to half a µm each way, and the routing
/// gates allow nothing under their rule.
const ROUNDING_MARGIN_UM: i64 = 2;

/// The copper-to-edge clearance the routing gate holds a board to, µm.
const EDGE_CLEARANCE_UM: i64 = 500;

/// Passes of rescue for what FreeRouting's own passes leave, each search
/// keeping [`RESCUE_MARGIN`] more room round the trace than it needs.
const RESCUE_PASSES: i32 = 10;

/// Board units: 5 µm. FreeRouting's search can take a gap with no room to
/// spare that its insertion then refuses, and in batch it takes the same
/// gap every pass; a micrometre to spare is not enough to stop it.
const RESCUE_MARGIN: i64 = 50;

/// How much dearer a trace is on an outer layer carrying a pour. Each one
/// there cuts the plane; kept small, so a net with nowhere else to go
/// still gets through.
const POUR_LAYER_COST_FACTOR: f64 = 2.0;

/// How much dearer a poured net's traces are off its pour's layer, while
/// it goes first: that copper is the plane's backbone.
const POUR_NET_OFF_LAYER_FACTOR: f64 = 4.0;

/// How far a strip between two pads stops short of each, µm. A keep-out
/// meeting a pad edge to edge leaves FreeRouting's rooms beside the pad
/// degenerate, and the pad's own net cannot get out; a foreign trace fits
/// no better through the sliver left, which lies within the pad's
/// clearance.
const STRIP_INSET_UM: i64 = 5;

/// A pad's copper about its centre, in board units, as a DSN padstack
/// writes it and FreeRouting keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PadForm {
    /// `(rect ...)`, kept as an `IntBox`: every pin, as the rectangle the
    /// routing gates measure clearance to, round or rounded corners and
    /// all.
    Box { half_w: i64, half_h: i64 },
    /// `(circle ...)`, kept as a `Circle`: the via.
    Circle { radius: i64 },
}

impl PadForm {
    /// The shape FreeRouting keeps, about the origin.
    fn shape(&self) -> PadShape {
        match *self {
            PadForm::Box { half_w, half_h } => PadShape::Box(IntBox::new(-half_w, -half_h, half_w, half_h)),
            PadForm::Circle { radius } => PadShape::Circle(Circle::new(IntPoint::new(0, 0), radius)),
        }
    }

    /// Part of its padstack's name.
    fn name(&self) -> String {
        match *self {
            PadForm::Box { half_w, half_h } => format!("Rect_{}x{}", 2 * half_w, 2 * half_h),
            PadForm::Circle { radius } => format!("Round_{}", 2 * radius),
        }
    }

    /// Its DSN `shape` on `layer`.
    fn dsn(&self, layer: &str) -> String {
        match *self {
            PadForm::Box { half_w, half_h } => format!("(rect {layer} {} {} {} {})", um(-half_w), um(-half_h), um(half_w), um(half_h)),
            PadForm::Circle { radius } => format!("(circle {layer} {})", um(2 * radius)),
        }
    }
}

/// `Shape.max_width`, per form.
fn max_width(shape: &PadShape) -> f64 {
    match shape {
        PadShape::Circle(c) => (2 * c.radius) as f64,
        PadShape::Box(b) => TileShape::Box(*b).max_width(),
        PadShape::Octagon(o) => TileShape::Octagon(*o).max_width(),
        PadShape::Polygon(s) => TileShape::Simplex(s.clone()).max_width(),
    }
}

/// Board units as DSN micrometres.
fn um(units: i64) -> String {
    let sign = if units < 0 { "-" } else { "" };
    let (whole, tenths) = (units.abs() / UNITS_PER_UM, units.abs() % UNITS_PER_UM);
    if tenths == 0 {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{tenths}")
    }
}

/// A DSN name, quoted.
fn quoted(s: &str) -> String {
    format!("\"{s}\"")
}

/// A pin of a placed footprint.
struct PlanPin {
    /// Its pad number.
    number: String,
    /// Its name in its image: the pad number, made unique.
    name: String,
    /// Centre, board units.
    center: IntPoint,
    /// Index into [`Plan::padstacks`].
    padstack: usize,
    /// Index into [`Plan::nets`].
    net: Option<usize>,
    /// Its copper as the model has it, the gates' measure.
    geom: PlacedPad,
}

struct PlanComponent {
    reference: String,
    pins: Vec<PlanPin>,
}

struct PlanPadstack {
    name: String,
    form: PadForm,
    from_layer: usize,
    to_layer: usize,
}

struct PlanNet {
    name: String,
    /// Index into [`Plan::classes`].
    class: usize,
    /// Poured on a layer: FreeRouting's plane.
    plane: bool,
}

/// A copper pour: its net flooded over the whole board on one layer.
struct PlanPlane {
    /// Index into [`Plan::nets`].
    net: usize,
    layer: usize,
}

struct PlanClass {
    name: String,
    /// Board units.
    half_width: i64,
    /// Board units, the rounding margin in (same scale as `Plan::clearance`).
    /// This class's own clearance -- to itself and to every other class's
    /// copper alike. Equal to `Plan::clearance` for a class with no
    /// override, which is the overwhelming majority: only an "escape"
    /// class asking for less room than the board default gives this a
    /// different value.
    clearance: i64,
}

/// A keep-out for traces and vias on one layer: a refdes label, or the
/// strip between two SMD pads of one footprint. µm, `(x0, y0, x1, y1)`.
struct PlanKeepout {
    layer: usize,
    rect: (i64, i64, i64, i64),
}

/// The strips between the facing edges of adjacent SMD pads of one
/// footprint, µm: under the part's body, a
/// track threading between its pads is what the gates send back. Pairs
/// with a third pad between them, or further apart than `max_gap`, have
/// none; through-hole rows are left open. Each stops [`STRIP_INSET_UM`]
/// short of the pads.
fn pad_gap_strips(pads: &[PlacedPad], max_gap: i64) -> Vec<(i64, i64, i64, i64)> {
    let rects: Vec<(i64, i64, i64, i64)> =
        pads.iter().filter(|p| !p.through_hole).map(|p| (p.center.x - p.size.0 / 2, p.center.y - p.size.1 / 2, p.center.x + p.size.0 / 2, p.center.y + p.size.1 / 2)).collect();
    let mut strips = Vec::new();
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            let (ra, rb) = (rects[i], rects[j]);
            let (ox0, ox1) = (ra.0.max(rb.0), ra.2.min(rb.2));
            let (oy0, oy1) = (ra.1.max(rb.1), ra.3.min(rb.3));
            // Between the facing edges, as wide as the pads share.
            let d = STRIP_INSET_UM;
            let (strip, inset) = if oy1 > oy0 && (ra.2 <= rb.0 || rb.2 <= ra.0) {
                let (gx0, gx1) = if ra.2 <= rb.0 { (ra.2, rb.0) } else { (rb.2, ra.0) };
                if gx1 - gx0 > max_gap {
                    continue;
                }
                ((gx0, oy0, gx1, oy1), (gx0 + d, oy0, gx1 - d, oy1))
            } else if ox1 > ox0 && (ra.3 <= rb.1 || rb.3 <= ra.1) {
                let (gy0, gy1) = if ra.3 <= rb.1 { (ra.3, rb.1) } else { (rb.3, ra.1) };
                if gy1 - gy0 > max_gap {
                    continue;
                }
                ((ox0, gy0, ox1, gy1), (ox0, gy0 + d, ox1, gy1 - d))
            } else {
                continue;
            };
            let crosses_other = rects.iter().enumerate().any(|(k, o)| k != i && k != j && o.0 < strip.2 && o.2 > strip.0 && o.1 < strip.3 && o.3 > strip.1);
            // A gap no wider than the insets leaves nothing to keep out.
            if !crosses_other && inset.0 < inset.2 && inset.1 < inset.3 {
                strips.push(inset);
            }
        }
    }
    strips
}

/// The board of a design as the DSN writes it, in the order FreeRouting
/// reads it: shared by [`to_dsn`] and [`board_from_design`], so the two
/// cannot drift apart.
struct Plan {
    layers: Vec<String>,
    /// µm, closed.
    outline: Vec<IrPoint>,
    /// In placement order.
    components: Vec<PlanComponent>,
    /// The pins' stacks by name, then the via's.
    padstacks: Vec<PlanPadstack>,
    /// The nets with a placed pin, in the model's order.
    nets: Vec<PlanNet>,
    /// The classes with a net: the default class, then the rules' own.
    classes: Vec<PlanClass>,
    /// In the order the DSN lists them: by footprint, its pad strips then
    /// its label.
    keepouts: Vec<PlanKeepout>,
    /// The pours whose net has a placed pin, in the rules' order.
    planes: Vec<PlanPlane>,
    /// Board units.
    track_half_width: i64,
    /// Between copper, from copper to the outline's, and from copper to a
    /// keep-out; board units, the rounding margin in.
    clearance: i64,
    edge_clearance: i64,
    keepout_clearance: i64,
}

impl Plan {
    fn of(design: &Design, model: &ConstraintModel, rules: &BoardRules) -> Result<Plan, String> {
        let placement = design.placement.as_ref().ok_or("the design has no placement")?;
        let layer_count = rules.layers.len();
        if layer_count == 0 {
            return Err("the board has no copper layers".into());
        }
        for pour in &rules.pours {
            if !rules.layers.contains(&pour.layer) {
                return Err(format!("the {} pour names layer {}, which is not in the stackup {:?}", pour.net, pour.layer, rules.layers));
            }
        }
        if rules.track_width <= 0 || rules.clearance < 0 || rules.via_diameter <= 0 {
            return Err(format!("unusable rules: track {} clearance {} via {} um", rules.track_width, rules.clearance, rules.via_diameter));
        }
        let mut outline = placement.outline.clone();
        if outline.len() < 3 {
            return Err("the board outline has fewer than 3 corners".into());
        }
        if outline.first() != outline.last() {
            outline.push(outline[0]);
        }
        let mut pin_nets: BTreeMap<&str, &str> = BTreeMap::new();
        for net in &model.nets {
            for pin in &net.pins {
                pin_nets.insert(pin.as_str(), net.name.as_str());
            }
        }
        let net_index: BTreeMap<&str, usize> = model.nets.iter().enumerate().map(|(i, n)| (n.name.as_str(), i)).collect();
        // The pads, with their stacks by key for now.
        let mut stacks: BTreeMap<String, (PadForm, usize, usize)> = BTreeMap::new();
        // Each pin with its stack's key; its stack's number and its net's
        // are settled once all are known.
        let mut placed: Vec<(String, Vec<(PlanPin, String)>)> = Vec::new();
        let mut max_coor = outline.iter().map(|p| p.x.abs().max(p.y.abs())).max().unwrap_or(0);
        let (bx0, by0) = (outline.iter().map(|p| p.x).min().unwrap(), outline.iter().map(|p| p.y).min().unwrap());
        let (bx1, by1) = (outline.iter().map(|p| p.x).max().unwrap(), outline.iter().map(|p| p.y).max().unwrap());
        let mut keepouts = Vec::new();
        let mut keep_out = |layer: usize, (x0, y0, x1, y1): (i64, i64, i64, i64)| {
            // Clipped to the board: the gates look at nothing beyond it.
            let rect = (x0.max(bx0), y0.max(by0), x1.min(bx1), y1.min(by1));
            if rect.0 < rect.2 && rect.1 < rect.3 {
                keepouts.push(PlanKeepout { layer, rect });
            }
        };
        for fp in &placement.footprints {
            let part = model.part(&fp.id).ok_or_else(|| format!("placed footprint {} has no part in the model", fp.id))?;
            let pads = placed_pads(model, part, fp).ok_or_else(|| format!("no footprint geometry for {}", fp.id))?;
            let side_layer = if fp.side == Side::Top { 0 } else { layer_count - 1 };
            for strip in pad_gap_strips(&pads, rules.tuning.between_pads_max_gap_um) {
                keep_out(side_layer, strip);
            }
            if let Some(label) = placed_refdes_box(model, &placement.outline, part, fp) {
                keep_out(side_layer, label);
            }
            let mut seen: BTreeMap<String, usize> = BTreeMap::new();
            let mut pins = Vec::new();
            for pad in pads {
                // Pad numbers repeat (a split thermal pad) or are empty
                // (a mounting hole): KiCad's `@n` makes them unique.
                let count = seen.entry(pad.number.clone()).or_insert(0);
                let name = if *count == 0 && !pad.number.is_empty() { pad.number.clone() } else { format!("{}@{count}", pad.number) };
                *count += 1;
                let (from, to) = if pad.through_hole { (0, layer_count - 1) } else { (side_layer, side_layer) };
                let form = PadForm::Box { half_w: pad.size.0 * UNITS_PER_UM / 2, half_h: pad.size.1 * UNITS_PER_UM / 2 };
                let span = if from == to { rules.layers[from].clone() } else { format!("{}-{}", rules.layers[from], rules.layers[to]) };
                let key = format!("{}[{span}]", form.name());
                stacks.insert(key.clone(), (form, from, to));
                let net = pin_nets.get(format!("{}.{}", fp.id, pad.number).as_str()).and_then(|n| net_index.get(n).copied());
                max_coor = max_coor.max(pad.center.x.abs()).max(pad.center.y.abs());
                let center = IntPoint::new(pad.center.x * UNITS_PER_UM, pad.center.y * UNITS_PER_UM);
                pins.push((PlanPin { number: pad.number.clone(), name, center, padstack: 0, net, geom: pad.clone() }, key));
            }
            placed.push((fp.id.clone(), pins));
        }
        if 5 * max_coor * UNITS_PER_UM >= CRIT_INT {
            return Err(format!("the board reaches {max_coor} um, past what FreeRouting holds unscaled"));
        }
        let mut padstacks: Vec<PlanPadstack> = stacks.iter().map(|(name, &(form, from_layer, to_layer))| PlanPadstack { name: name.clone(), form, from_layer, to_layer }).collect();
        let stack_no: BTreeMap<&str, usize> = stacks.keys().enumerate().map(|(i, k)| (k.as_str(), i)).collect();
        padstacks.push(PlanPadstack {
            name: format!("Via[0-{}]_{}:{}_um", layer_count - 1, rules.via_diameter, rules.via_drill),
            form: PadForm::Circle { radius: rules.via_diameter * UNITS_PER_UM / 2 },
            from_layer: 0,
            to_layer: layer_count - 1,
        });
        // The nets with a placed pin, and the classes with a net: the
        // default one first, as KiCad writes it.
        let mut used = vec![false; model.nets.len()];
        for (_, pins) in &placed {
            for (pin, _) in pins {
                if let Some(n) = pin.net {
                    used[n] = true;
                }
            }
        }
        let class_of = |name: &str| rules.net_classes.iter().position(|c| c.matches(name)).map_or(0, |i| i + 1);
        let mut class_used = vec![false; rules.net_classes.len() + 1];
        for (i, net) in model.nets.iter().enumerate() {
            if used[i] {
                class_used[class_of(&net.name)] = true;
            }
        }
        let mut class_no = vec![usize::MAX; class_used.len()];
        let mut classes = Vec::new();
        for (c, &u) in class_used.iter().enumerate() {
            if !u {
                continue;
            }
            class_no[c] = classes.len();
            let (name, width, clearance_um) = match c {
                0 => ("kicad_default".to_string(), rules.track_width, rules.clearance),
                _ => {
                    let nc = &rules.net_classes[c - 1];
                    (format!("class_{c}"), nc.track_width.unwrap_or(rules.track_width), nc.clearance.unwrap_or(rules.clearance))
                }
            };
            if width <= 0 {
                return Err(format!("net class {name} has track width {width} um"));
            }
            if clearance_um < 0 {
                return Err(format!("net class {name} has clearance {clearance_um} um"));
            }
            classes.push(PlanClass { name, half_width: width * UNITS_PER_UM / 2, clearance: clearance_um * UNITS_PER_UM + ROUNDING_MARGIN_UM * UNITS_PER_UM });
        }
        // Nets by number: FreeRouting makes a plane's net as it reads the
        // plane, before the netlist, so poured nets come first.
        let poured = |name: &str| rules.pours.iter().any(|p| p.net == name);
        let mut order: Vec<usize> = Vec::new();
        for pour in &rules.pours {
            if let Some(i) = model.nets.iter().position(|n| n.name == pour.net) {
                if used[i] && !order.contains(&i) {
                    order.push(i);
                }
            }
        }
        let rest: Vec<usize> = (0..model.nets.len()).filter(|&i| used[i] && !order.contains(&i)).collect();
        order.extend(rest);
        let mut net_no = vec![usize::MAX; model.nets.len()];
        let mut nets = Vec::new();
        for i in order {
            let net = &model.nets[i];
            net_no[i] = nets.len();
            nets.push(PlanNet { name: net.name.clone(), class: class_no[class_of(&net.name)], plane: poured(&net.name) });
        }
        let mut planes = Vec::new();
        for pour in &rules.pours {
            // A pour on a net with nothing to reach is a claim about a
            // board that does not exist.
            let Some(net) = nets.iter().position(|n| n.name == pour.net) else {
                return Err(format!("the {} pour is on a net with no placed pad", pour.net));
            };
            let layer = rules.layers.iter().position(|l| *l == pour.layer).expect("checked above");
            planes.push(PlanPlane { net, layer });
        }
        let components = placed
            .into_iter()
            .map(|(reference, pins)| PlanComponent {
                reference,
                pins: pins.into_iter().map(|(pin, key)| PlanPin { padstack: stack_no[key.as_str()], net: pin.net.map(|n| net_no[n]), ..pin }).collect(),
            })
            .collect();
        let margin = ROUNDING_MARGIN_UM * UNITS_PER_UM;
        Ok(Plan {
            layers: rules.layers.clone(),
            outline,
            components,
            padstacks,
            nets,
            classes,
            keepouts,
            planes,
            track_half_width: rules.track_width * UNITS_PER_UM / 2,
            clearance: rules.clearance * UNITS_PER_UM + margin,
            // The outline is copper `OUTLINE_HALF_WIDTH` wide either side
            // of the edge.
            edge_clearance: rules.tuning.edge_clearance_um.max(EDGE_CLEARANCE_UM) * UNITS_PER_UM - OUTLINE_HALF_WIDTH + margin,
            keepout_clearance: margin,
        })
    }

    /// The board's bounding box, board units: the outline's, grown by
    /// 1000 as FreeRouting grows it.
    fn bounds(&self) -> IntBox {
        let u = UNITS_PER_UM;
        let (xs, ys) = (self.outline.iter().map(|p| p.x * u), self.outline.iter().map(|p| p.y * u));
        IntBox::new(xs.clone().min().unwrap(), ys.clone().min().unwrap(), xs.max().unwrap(), ys.max().unwrap()).offset(1000)
    }

    fn via_padstack(&self) -> &PlanPadstack {
        self.padstacks.last().expect("the via's stack is always there")
    }
}

/// The design as the Specctra DSN KiCad would write for it: µm at
/// resolution 10, one image per placed footprint, the default class and
/// the rules' own classes. For asking FreeRouting what board it makes of
/// the design; [`board_from_design`] builds the same board directly.
pub fn to_dsn(design: &Design, model: &ConstraintModel, rules: &BoardRules) -> Result<String, String> {
    let plan = Plan::of(design, model, rules)?;
    let layer = |l: usize| quoted(&plan.layers[l]);
    let mut out = String::new();
    let _ = writeln!(out, "(pcb design.dsn");
    let _ = writeln!(out, "  (parser\n    (string_quote \")\n    (space_in_quoted_tokens on)\n    (host_cad \"eda-freeroute\")\n    (host_version \"0.1\")\n  )");
    let _ = writeln!(out, "  (resolution um 10)\n  (unit um)\n  (structure");
    for l in 0..plan.layers.len() {
        let _ = writeln!(out, "    (layer {}\n      (type signal)\n      (property\n        (index {l})\n      )\n    )", layer(l));
    }
    // The settings, all of them: given any, FreeRouting takes none of its
    // own. Before the keep-outs and planes, or it skips them.
    let _ = writeln!(out, "    (autoroute_settings\n      (fanout off)\n      (autoroute on)\n      (postroute on)\n      (vias on)\n      (via_costs 50)\n      (plane_via_costs 5)\n      (start_ripup_costs 100)\n      (start_pass_no 1)");
    for (l, r) in layer_rules(&plan, &plan.bounds()).iter().enumerate() {
        let _ = writeln!(
            out,
            "      (layer_rule {}\n        (active {})\n        (preferred_direction {})\n        (preferred_direction_trace_costs {:?})\n        (against_preferred_direction_trace_costs {:?})\n      )",
            layer(l),
            if r.active { "on" } else { "off" },
            if r.horizontal { "horizontal" } else { "vertical" },
            r.preferred,
            r.against
        );
    }
    let _ = writeln!(out, "    )");
    let _ = write!(out, "    (boundary\n      (path pcb 0");
    for p in &plan.outline {
        let _ = write!(out, "  {} {}", p.x, p.y);
    }
    let _ = writeln!(out, ")\n      (clearance_class edge)\n    )");
    let via = plan.via_padstack();
    let _ = writeln!(out, "    (via {})", quoted(&via.name));
    // The clearance, then a class of its own for the outline and one for
    // the keep-outs: `NAME_default` spaces it from everything else.
    let _ = writeln!(
        out,
        "    (rule\n      (width {})\n      (clearance {})\n      (clearance {} (type edge_default))\n      (clearance {} (type keepout_default))\n    )",
        um(2 * plan.track_half_width),
        um(plan.clearance),
        um(plan.edge_clearance),
        um(plan.keepout_clearance)
    );
    for k in &plan.keepouts {
        let (x0, y0, x1, y1) = k.rect;
        let _ = writeln!(out, "    (keepout \"\" (rect {} {x0} {y0} {x1} {y1}) (clearance_class keepout))", layer(k.layer));
    }
    for p in &plan.planes {
        let _ = write!(out, "    (plane {} (polygon {} 0", quoted(&plan.nets[p.net].name), layer(p.layer));
        for c in &plan.outline {
            let _ = write!(out, " {} {}", c.x, c.y);
        }
        let _ = writeln!(out, "))");
    }
    let _ = writeln!(out, "  )");
    let _ = writeln!(out, "  (placement");
    for c in &plan.components {
        let _ = writeln!(out, "    (component {}\n      (place {} 0 0 front 0)\n    )", quoted(&format!("IMG_{}", c.reference)), quoted(&c.reference));
    }
    let _ = writeln!(out, "  )\n  (library");
    for c in &plan.components {
        let _ = writeln!(out, "    (image {}", quoted(&format!("IMG_{}", c.reference)));
        for pin in &c.pins {
            let _ = writeln!(out, "      (pin {} {} {} {})", quoted(&plan.padstacks[pin.padstack].name), quoted(&pin.name), um(pin.center.x), um(pin.center.y));
        }
        let _ = writeln!(out, "    )");
    }
    for ps in &plan.padstacks {
        let _ = writeln!(out, "    (padstack {}", quoted(&ps.name));
        for l in ps.from_layer..=ps.to_layer {
            let _ = writeln!(out, "      (shape {})", ps.form.dsn(&layer(l)));
        }
        let _ = writeln!(out, "      (attach off)\n    )");
    }
    let _ = writeln!(out, "  )\n  (network");
    for (n, net) in plan.nets.iter().enumerate() {
        let mut pins = Vec::new();
        for c in &plan.components {
            for pin in c.pins.iter().filter(|p| p.net == Some(n)) {
                // `"REF"-"PIN"`: the reader splits at the hyphen, quoted or not.
                pins.push(format!("{}-{}", quoted(&c.reference), quoted(&pin.name)));
            }
        }
        let _ = writeln!(out, "    (net {}\n      (pins {})\n    )", quoted(&net.name), pins.join(" "));
    }
    for (k, class) in plan.classes.iter().enumerate() {
        let nets: Vec<String> = plan.nets.iter().filter(|n| n.class == k).map(|n| quoted(&n.name)).collect();
        // No clearance of its own: every class keeps the board's, and so
        // the default clearance class and via.
        let _ = writeln!(
            out,
            "    (class {} {}\n      (circuit\n        (use_via {})\n      )\n      (rule\n        (width {})\n      )\n    )",
            quoted(&class.name),
            nets.join(" "),
            quoted(&via.name),
            um(2 * class.half_width)
        );
    }
    let _ = writeln!(out, "  )\n  (wiring\n  )\n)");
    Ok(out)
}

/// FreeRouting's `ClearanceMatrix` as its DSN reader fills it: values
/// kept even, rounded up, and per class the largest value ever set in its
/// row, odd or not.
struct Matrix {
    layers: usize,
    /// `[j][i][layer]`, the Java's `row[j].column[i]`.
    value: Vec<Vec<Vec<i64>>>,
    row_max: Vec<Vec<i64>>,
}

impl Matrix {
    /// `get_default_instance`: the null class and the default one.
    fn new(layers: usize) -> Matrix {
        let mut m = Matrix { layers, value: vec![vec![vec![0; layers]; 2]; 2], row_max: vec![vec![0; layers]; 2] };
        m.set_default(0);
        m
    }

    fn classes(&self) -> usize {
        self.value.len()
    }

    /// `get_value(i, j, layer)`.
    fn get(&self, i: usize, j: usize, layer: usize) -> i64 {
        self.value[j][i][layer]
    }

    /// `set_value(i, j, layer, value)`.
    fn set(&mut self, i: usize, j: usize, layer: usize, value: i64) {
        let v = value.max(0);
        self.value[j][i][layer] = v + v % 2;
        self.row_max[j][layer] = self.row_max[j][layer].max(value);
    }

    /// `set_default_value`: every class but the null one, on every layer.
    fn set_default(&mut self, value: i64) {
        for l in 0..self.layers {
            for i in 1..self.classes() {
                for j in 1..self.classes() {
                    self.set(i, j, l, value);
                }
            }
        }
    }

    /// `append_class`: a class spaced from the others as the default one
    /// is. Its number.
    fn append_class(&mut self) -> usize {
        let old = self.classes();
        for row in &mut self.value {
            row.push(vec![0; self.layers]);
        }
        self.value.push(vec![vec![0; self.layers]; old + 1]);
        self.row_max.push(vec![0; self.layers]);
        for i in 0..old {
            for l in 0..self.layers {
                let d = self.get(1, i, l);
                self.set(old, i, l, d);
                self.set(i, old, l, d);
            }
        }
        for l in 0..self.layers {
            let d = self.get(1, 1, l);
            self.set(old, old, l, d);
        }
        old
    }

    /// `Structure.set_clearance_rule` for a pair `NAME_default` naming a
    /// class not there yet: the class, spaced `clearance` from the
    /// default one. Its number.
    fn add_default_pair(&mut self, clearance: i64) -> usize {
        let k = self.append_class();
        for l in 0..self.layers {
            self.set(k, 1, l, clearance);
        }
        for l in 0..self.layers {
            self.set(1, k, l, clearance);
        }
        k
    }

    /// A class that keeps `clearance` from *itself and from the default
    /// class alike* -- an "escape" net class, asking for less room than
    /// the board default wherever its copper turns up, not only from its
    /// own nets. Unlike [`Matrix::add_default_pair`] (which only spaces
    /// the new class from the default one, leaving its self-clearance at
    /// whatever the default's happened to be), this also sets the class's
    /// clearance to itself -- both directions matter here, since a trace
    /// squeezing past a fine-pitch neighbour needs the *smaller* number on
    /// both sides of that gap. Its relation to any class appended before
    /// it other than the default (edge, keepout) is left at whatever
    /// [`Matrix::append_class`] copied from the default row, which is the
    /// conservative board-wide value -- board-edge and keep-out clearance
    /// are not part of what an escape class relaxes.
    fn add_own_clearance_class(&mut self, clearance: i64) -> usize {
        let k = self.append_class();
        for l in 0..self.layers {
            self.set(k, k, l, clearance);
            self.set(k, 1, l, clearance);
            self.set(1, k, l, clearance);
        }
        k
    }
}

/// `Math.round` of a positive double.
fn java_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// What the autorouter is told about one layer: whether to route on it,
/// the direction it prefers there, and what a trace costs along that
/// direction and against it.
#[derive(Debug, Clone, Copy)]
struct LayerRule {
    active: bool,
    horizontal: bool,
    preferred: f64,
    against: f64,
}

/// FreeRouting's defaults, `AutorouteSettings(RoutingBoard)`: preferred
/// directions alternating from the one across the board's longer side, the
/// cost against it by the board's aspect, the outer layers dearer on a
/// board of more than two.
fn default_layer_rules(bounds: &IntBox, layer_count: usize) -> Vec<LayerRule> {
    let (w, h) = ((bounds.ur.x - bounds.ll.x) as f64, (bounds.ur.y - bounds.ll.y) as f64);
    let horizontal_add = 0.1 * java_round(10.0 * w / h);
    let vertical_add = 0.1 * java_round(10.0 * h / w);
    let mut horizontal = w < h;
    let mut rules = Vec::with_capacity(layer_count);
    for _ in 0..layer_count {
        horizontal = !horizontal;
        rules.push(LayerRule { active: true, horizontal, preferred: 1.0, against: 1.0 + if horizontal { horizontal_add } else { vertical_add } });
    }
    if layer_count > 2 {
        let outer = 0.2 * layer_count as f64;
        for l in [0, layer_count - 1] {
            rules[l].preferred += outer;
            rules[l].against += outer;
        }
    }
    rules
}

/// The layer rules a plan routes with. FreeRouting's defaults, with an
/// inner layer carrying a plane taken out of routing and the preferred
/// directions of the others alternating anew, as FreeRouting adjusts a
/// board read with planes (`DsnFile.adjust_plane_autoroute_settings`);
/// and an outer layer carrying one made [`POUR_LAYER_COST_FACTOR`] times as
/// dear, since every trace there cuts the plane.
fn layer_rules(plan: &Plan, bounds: &IntBox) -> Vec<LayerRule> {
    let layer_count = plan.layers.len();
    let mut rules = default_layer_rules(bounds, layer_count);
    let outer = |l: usize| l == 0 || l == layer_count - 1;
    if layer_count > 2 && plan.planes.iter().any(|p| !outer(p.layer)) {
        let mut horizontal = rules[0].horizontal;
        for (l, r) in rules.iter_mut().enumerate() {
            if plan.planes.iter().any(|p| p.layer == l && !outer(l)) {
                r.active = false;
            } else if r.active {
                r.horizontal = horizontal;
                horizontal = !horizontal;
            }
        }
    }
    for p in &plan.planes {
        if outer(p.layer) {
            rules[p.layer].preferred *= POUR_LAYER_COST_FACTOR;
            rules[p.layer].against *= POUR_LAYER_COST_FACTOR;
        }
    }
    rules
}

/// The autoroute settings a plan routes with: FreeRouting's, with
/// [`layer_rules`].
fn autoroute_settings(rules: &[LayerRule]) -> AutorouteSettings {
    AutorouteSettings {
        vias_allowed: true,
        via_costs: 50,
        plane_via_costs: 5,
        start_ripup_costs: 100,
        with_fanout: false,
        automatic_neckdown: true,
        layer_active: rules.iter().map(|r| r.active).collect(),
        trace_costs: rules.iter().map(|r| if r.horizontal { (r.preferred, r.against) } else { (r.against, r.preferred) }).collect(),
        preferred_direction_costs: rules.iter().map(|r| r.preferred).collect(),
    }
}

/// The directions a trace may leave a pad of shape `shape` in, and how far
/// it runs inside: none for a round pad; along the longer side of a long
/// one, and every way for a squarish one -- squarer still counting for
/// parts of three pins or fewer. `Pin.get_trace_exit_restrictions` for an
/// unturned pad about its centre.
fn exit_restrictions(shape: &PadShape, pin_count: usize) -> Vec<ExitRestriction> {
    if !matches!(shape, PadShape::Box(_) | PadShape::Octagon(_)) {
        return Vec::new();
    }
    let b = shape.bounding_box();
    let (w, h) = (b.ur.x - b.ll.x, b.ur.y - b.ll.y);
    let factor = if pin_count <= 3 { 3.0 } else { 1.5 };
    let all_dirs = (w.max(h) as f64) < factor * w.min(h) as f64;
    let mut result = Vec::new();
    if all_dirs || w >= h {
        result.push(ExitRestriction { direction: Direction::RIGHT, min_length: b.ur.x as f64 });
        result.push(ExitRestriction { direction: Direction::LEFT, min_length: -b.ll.x as f64 });
    }
    if all_dirs || w <= h {
        result.push(ExitRestriction { direction: Direction::UP, min_length: b.ur.y as f64 });
        result.push(ExitRestriction { direction: Direction::DOWN, min_length: -b.ll.y as f64 });
    }
    result
}

/// The board FreeRouting reads from the DSN [`to_dsn`] writes for the
/// design, built directly.
pub fn board_from_design(design: &Design, model: &ConstraintModel, rules: &BoardRules) -> Result<Board, String> {
    let plan = Plan::of(design, model, rules)?;
    let layer_count = plan.layers.len();
    let layers = plan.layers.iter().map(|name| Layer { name: name.clone(), is_signal: true }).collect();
    let corners: Vec<IntPoint> = plan.outline.iter().map(|p| IntPoint::new(p.x * UNITS_PER_UM, p.y * UNITS_PER_UM)).collect();
    let bounds = plan.bounds();

    // The structure's rule: the clearance, then the outline's class and
    // the keep-outs'.
    let mut matrix = Matrix::new(layer_count);
    matrix.set_default(plan.clearance);
    let edge_class = matrix.add_default_pair(plan.edge_clearance) as i32;
    let keepout_class = matrix.add_default_pair(plan.keepout_clearance) as i32;
    // One matrix class per distinct clearance value among `plan.classes`
    // that differs from the board default -- see `add_own_clearance_class`.
    // Classes that do not override clearance are left sharing class 1 (the
    // default), exactly as before this existed.
    let mut own_clearance_class: BTreeMap<i64, i32> = BTreeMap::new();
    for class in &plan.classes {
        if class.clearance != plan.clearance {
            own_clearance_class.entry(class.clearance).or_insert_with(|| matrix.add_own_clearance_class(class.clearance) as i32);
        }
    }
    let padstacks: Vec<Padstack> = plan
        .padstacks
        .iter()
        .enumerate()
        .map(|(i, ps)| {
            let shape = ps.form.shape();
            let on = |l: usize| (ps.from_layer..=ps.to_layer).contains(&l);
            Padstack {
                no: i + 1,
                from_layer: ps.from_layer as i32,
                to_layer: ps.to_layer as i32,
                max_width: (0..layer_count).map(|l| on(l).then(|| max_width(&shape))).collect(),
                shapes: (0..layer_count).map(|l| on(l).then(|| shape.clone())).collect(),
            }
        })
        .collect();
    let via_no = padstacks.len();
    let default_class = NetClass {
        trace_clearance_class: 1,
        via_rule: Some(0),
        active_layers: vec![true; layer_count],
        trace_half_width: vec![plan.track_half_width; layer_count],
        shove_fixed: false,
        pull_tight: true,
        ignore_cycles_with_areas: false,
        ignored_by_autorouter: false,
    };
    // One via; every class a via rule of its own naming it.
    let via_infos = vec![ViaInfo { padstack: via_no, clearance_class: 1, attach_smd_allowed: false }];
    let mut via_rules = vec![vec![0]];
    let mut net_classes = vec![default_class.clone()];
    for class in &plan.classes {
        via_rules.push(vec![0]);
        let trace_clearance_class = own_clearance_class.get(&class.clearance).copied().unwrap_or(1);
        net_classes.push(NetClass { via_rule: Some(via_rules.len() - 1), trace_half_width: vec![class.half_width; layer_count], trace_clearance_class, ..default_class.clone() });
    }
    let mut clearance = ClearanceMatrix::new(matrix.classes(), layer_count);
    for i in 0..matrix.classes() {
        for j in 0..matrix.classes() {
            for l in 0..layer_count {
                let v = matrix.get(i, j, l);
                if v != 0 {
                    clearance.set(i, j, l, v);
                }
            }
        }
    }
    let nets = plan.nets.iter().enumerate().map(|(n, net)| (n as i32 + 1, Net { no: n as i32 + 1, class: net.class + 1, contains_plane: net.plane })).collect();
    let rules_out = Rules {
        clearance,
        max_clearance: matrix.row_max.clone(),
        padstacks,
        via_infos,
        via_rules,
        net_classes,
        nets,
        // `BasicBoard`'s, with no trace on it yet.
        min_trace_half_width: 10_000,
        default_via_diameter: max_width(&plan.via_padstack().form.shape()),
        pin_edge_to_turn_dist: plan.track_half_width.min(100_000) as f64,
        board_max_trace_half_width: 1000,
        max_trace_half_width: plan.track_half_width.max(100),
        pull_tight_accuracy: 500,
    };

    // The outline first, the keep-outs, the planes, then the pins by
    // component; the board lists them last in first.
    let outline = PolygonShape::new(&corners);
    let n = outline.corners.len();
    let lines = (0..n).map(|i| Line::new(outline.corners[i], outline.corners[(i + 1) % n])).collect();
    let mut items = vec![Item {
        id: 1,
        kind: ItemKind::Outline { half_width: OUTLINE_HALF_WIDTH, shapes: vec![lines], keepout_outside: false },
        first_layer: 0,
        last_layer: layer_count as i32 - 1,
        clearance_class: edge_class,
        fixed: FixedState::SystemFixed,
        component: 0,
        nets: Vec::new(),
    }];
    for k in &plan.keepouts {
        let (x0, y0, x1, y1) = k.rect;
        let u = UNITS_PER_UM;
        items.push(Item {
            id: items.len() as u32 + 1,
            kind: ItemKind::Area { kind: AreaKind::Keepout, layer: k.layer as i32, shape: AreaShape::Tile(TileShape::Box(IntBox::new(x0 * u, y0 * u, x1 * u, y1 * u))) },
            first_layer: k.layer as i32,
            last_layer: k.layer as i32,
            clearance_class: keepout_class,
            fixed: FixedState::SystemFixed,
            component: 0,
            nets: Vec::new(),
        });
    }
    // A plane is copper of its net over the whole board, which the traces
    // of other nets may cross: the pour flows round them.
    for p in &plan.planes {
        let layer = p.layer as i32;
        items.push(Item {
            id: items.len() as u32 + 1,
            kind: ItemKind::Area { kind: AreaKind::Conduction { is_obstacle: false }, layer, shape: AreaShape::Polygon(PolygonShape::new(&corners)) },
            first_layer: layer,
            last_layer: layer,
            clearance_class: 1,
            fixed: FixedState::SystemFixed,
            component: 0,
            nets: vec![p.net as i32 + 1],
        });
    }
    for (c, component) in plan.components.iter().enumerate() {
        for pin in &component.pins {
            let ps = &plan.padstacks[pin.padstack];
            let shape = ps.form.shape();
            let b = shape.bounding_box();
            let (w, h) = (b.ur.x - b.ll.x, b.ur.y - b.ll.y);
            let span = ps.to_layer - ps.from_layer + 1;
            let exits = exit_restrictions(&shape, component.pins.len());
            items.push(Item {
                id: items.len() as u32 + 1,
                kind: ItemKind::Pin {
                    center: pin.center,
                    pads: vec![Some(shape.translate(pin.center.x, pin.center.y)); span],
                    neckdown: vec![(0.5 * w.min(h) as f64 - 1.0).max(1.0) as i64; span],
                    max_width: vec![w.max(h) as f64; span],
                    exits: vec![exits; span],
                },
                first_layer: ps.from_layer as i32,
                last_layer: ps.to_layer as i32,
                // Its net class's, as the default one's: no class has a
                // clearance of its own.
                clearance_class: 1,
                fixed: FixedState::Unfixed,
                component: c as i32 + 1,
                nets: pin.net.map(|n| vec![n as i32 + 1]).unwrap_or_default(),
            });
        }
    }
    let id_max = items.len() as u32;
    items.reverse();
    // Traces keep their class's width into narrow pins: the routing gates
    // hold every track to it.
    let settings = AutorouteSettings { automatic_neckdown: false, ..autoroute_settings(&layer_rules(&plan, &bounds)) };
    // The DSN's micrometres at resolution 10.
    Ok(Board { bounds, layers, rules: rules_out, settings, items, host_cad: true, area_section: 50_000.0, id_max, user_unit: (0.1, 1.0) })
}

/// Whether `p` lies on the pad, border included.
fn pad_contains(shape: &PadShape, p: FloatPoint) -> bool {
    match shape {
        PadShape::Circle(c) => {
            let (dx, dy) = (p.x - c.center.x as f64, p.y - c.center.y as f64);
            dx * dx + dy * dy <= (c.radius * c.radius) as f64
        }
        PadShape::Octagon(o) => {
            let in_box = (o.left_x as f64..=o.right_x as f64).contains(&p.x) && (o.bottom_y as f64..=o.top_y as f64).contains(&p.y);
            in_box && (o.upper_left_diag_x as f64..=o.lower_right_diag_x as f64).contains(&(p.x - p.y)) && (o.lower_left_diag_x as f64..=o.upper_right_diag_x as f64).contains(&(p.x + p.y))
        }
        PadShape::Box(_) | PadShape::Polygon(_) => {
            let b = shape.bounding_box();
            (b.ll.x as f64..=b.ur.x as f64).contains(&p.x) && (b.ll.y as f64..=b.ur.y as f64).contains(&p.y)
        }
    }
}

/// Board units as the nearest µm.
fn to_um(p: FloatPoint) -> IrPoint {
    let unit = UNITS_PER_UM as f64;
    IrPoint { x: (p.x / unit).round() as i64, y: (p.y / unit).round() as i64 }
}

/// The routes on `rb`, a board [`board_from_design`] built for the design,
/// as the design's routing section: traces as tracks and vias, in µm, with
/// corners off the µm grid rounded onto it and necked-down widths rounded
/// down. A track lists the pads its ends lie on. The design's zones stay.
pub fn routing_section(design: &Design, model: &ConstraintModel, rules: &BoardRules, rb: &RoutingBoard) -> Result<RoutingSection, String> {
    let plan = Plan::of(design, model, rules)?;
    // "REF.PIN" of every pin, by item number as `board_from_design` gives
    // them: after the outline, the keep-outs and the planes.
    let first_pin = (plan.keepouts.len() + plan.planes.len()) as u32 + 2;
    let mut pin_names: BTreeMap<u32, String> = BTreeMap::new();
    for c in &plan.components {
        for pin in &c.pins {
            pin_names.insert(first_pin + pin_names.len() as u32, format!("{}.{}", c.reference, pin.number));
        }
    }
    let pins: Vec<&Item> = rb.board.items.iter().filter(|i| matches!(i.kind, ItemKind::Pin { .. })).collect();
    let net_name = |item: &Item| item.nets.first().and_then(|&n| plan.nets.get((n - 1) as usize)).map(|n| n.name.clone());
    let (mut tracks, mut vias) = (Vec::new(), Vec::new());
    for i in 0..rb.board.items.len() {
        if !rb.is_on_board(i) {
            continue;
        }
        let item = rb.item(i);
        let Some(net) = net_name(item) else { continue };
        match &item.kind {
            ItemKind::Trace { layer, half_width, polyline } => {
                let mut pts: Vec<IrPoint> = Vec::new();
                for k in 0..polyline.corner_count() {
                    let p = to_um(polyline.corner_float(k));
                    if pts.last() != Some(&p) {
                        pts.push(p);
                    }
                }
                if pts.len() < 2 {
                    continue;
                }
                let mut on_pins = Vec::new();
                for end in [polyline.corner_float(0), polyline.corner_float(polyline.corner_count() - 1)] {
                    for pin in &pins {
                        let ItemKind::Pin { pads, .. } = &pin.kind else { continue };
                        if !pin.nets.contains(&item.nets[0]) || *layer < pin.first_layer || *layer > pin.last_layer {
                            continue;
                        }
                        let on = pads[(*layer - pin.first_layer) as usize].as_ref().is_some_and(|pad| pad_contains(pad, end));
                        let name = &pin_names[&pin.id];
                        if on && !on_pins.contains(name) {
                            on_pins.push(name.clone());
                        }
                    }
                }
                tracks.push(Track { id: String::new(), net, pins: on_pins, layer: plan.layers[*layer as usize].clone(), width: 2 * half_width / UNITS_PER_UM, pts });
            }
            ItemKind::Via { center, .. } => vias.push(IrVia {
                id: String::new(),
                net,
                at: to_um(FloatPoint::new(center.x as f64, center.y as f64)),
                drill: rules.via_drill,
                diameter: rules.via_diameter,
                from_layer: plan.layers[item.first_layer as usize].clone(),
                to_layer: plan.layers[item.last_layer as usize].clone(),
            }),
            _ => {}
        }
    }
    let pads: Vec<GatePad> = plan
        .components
        .iter()
        .flat_map(|c| c.pins.iter())
        .filter_map(|pin| {
            let ps = &plan.padstacks[pin.padstack];
            Some(GatePad { geom: &pin.geom, net: &plan.nets[pin.net?].name, layers: plan.layers[ps.from_layer..=ps.to_layer].iter().map(String::as_str).collect() })
        })
        .collect();
    let mut tracks = drop_pad_loops(split_at_own_pads(tracks, &pads), &vias, &pads);
    tracks.sort_by(|a: &Track, b: &Track| (&a.net, &a.layer, a.pts.first()).cmp(&(&b.net, &b.layer, b.pts.first())));
    vias.sort_by(|a: &IrVia, b: &IrVia| (&a.net, a.at).cmp(&(&b.net, b.at)));
    // The pours as zones, the board's outline filled; the design's own
    // zones stay.
    let mut zones = design.routing.as_ref().map(|r| r.zones.clone()).unwrap_or_default();
    for p in &plan.planes {
        let (net, layer) = (&plan.nets[p.net].name, &plan.layers[p.layer]);
        if !zones.iter().any(|z| &z.net == net && &z.layer == layer) {
            zones.push(eda_model::ir::Zone { id: String::new(), net: net.clone(), layer: layer.clone(), outline: design.placement.as_ref().map(|pl| pl.outline.clone()).unwrap_or_default() });
        }
    }
    let mut rt = RoutingSection { tracks, vias, zones };
    // Assigned here, at the router's own output, so every track/via/zone
    // is addressable the moment a route finishes -- deterministically:
    // the same design routed twice gets the same ids both times.
    rt.assign_missing_ids();
    Ok(rt)
}

/// A design routed by [`route_design`].
#[derive(Debug, Clone)]
pub struct RoutedDesign {
    pub routing: RoutingSection,
    /// What each pass did.
    pub passes: Vec<PassSummary>,
    /// What the optimizer did after them.
    pub optimized: OptSummary,
    /// The nets whose pins the passes left apart, in the order a next pass
    /// would take them.
    pub unrouted: Vec<String>,
}

/// A placed design routed as FreeRouting's batch autorouter routes it: up
/// to `max_passes` passes over the board [`board_from_design`] builds,
/// stopping once one finds nothing left to route or the board repeats
/// itself; then, for anything left, rescue passes whose search keeps a
/// little room to spare; then FreeRouting's post-route optimizer, up to
/// `optimizer_passes` passes. A net poured on an outer layer goes first:
/// see [`route_pour_nets`]. The routes back as the design's routing section.
pub fn route_design(design: &Design, model: &ConstraintModel, rules: &BoardRules, max_passes: i32, optimizer_passes: i32) -> Result<RoutedDesign, String> {
    let plan = Plan::of(design, model, rules)?;
    let board = board_from_design(design, model, rules)?;
    let (mut rb, mut passes) = route_board(&plan, &board, max_passes);
    let left = pass_items(&rb).len();
    if left > 0 {
        let next = passes.last().map_or(1, |p| p.pass_no + 1);
        let rescue = autoroute_passes_with(&mut rb, next, RESCUE_PASSES, RESCUE_MARGIN);
        if pass_items(&rb).len() <= left {
            passes.extend(rescue);
        } else {
            // The rescue ripped up more than it laid: the plain result,
            // again, for it is deterministic.
            (rb, passes) = route_board(&plan, &board, max_passes);
        }
    }
    // The optimizer counts vias first, and a poured net's are what joins
    // the plane: it would route the net off the pour's layer and leave the
    // plane floating. Routed, that copper stays as it is.
    for net in outer_pour_nets(&plan) {
        for i in 0..rb.board.items.len() {
            let item = &rb.board.items[i];
            if rb.is_on_board(i) && item.nets.contains(&net) && matches!(item.kind, ItemKind::Trace { .. } | ItemKind::Via { .. }) {
                rb.board.items[i].fixed = FixedState::UserFixed;
            }
        }
    }
    let optimized = optimize_board(&mut rb, optimizer_passes);
    let mut unrouted: Vec<String> = Vec::new();
    for i in pass_items(&rb) {
        for &net in &rb.board.items[i].nets {
            let name = &plan.nets[(net - 1) as usize].name;
            if rb.connected_set(i, net).len() < rb.connectable_item_count(net) && !unrouted.contains(name) {
                unrouted.push(name.clone());
            }
        }
    }
    Ok(RoutedDesign { routing: routing_section(design, model, rules, &rb)?, passes, optimized, unrouted })
}

/// The poured nets whose every pour lies on an outer layer, by number.
fn outer_pour_nets(plan: &Plan) -> Vec<i32> {
    let last = plan.layers.len() - 1;
    let mut nets: Vec<i32> = Vec::new();
    for p in &plan.planes {
        let net = p.net as i32 + 1;
        let all_outer = plan.planes.iter().filter(|q| q.net == p.net).all(|q| q.layer == 0 || q.layer == last);
        if all_outer && !nets.contains(&net) {
            nets.push(net);
        }
    }
    nets
}

/// The board routed: the outer pours' nets first, then FreeRouting's
/// passes over the rest.
fn route_board(plan: &Plan, board: &Board, max_passes: i32) -> (RoutingBoard, Vec<PassSummary>) {
    let first = outer_pour_nets(plan);
    let mut board = board.clone();
    if !first.is_empty() {
        // Those planes stay out of the routing; the zones stand for them.
        board.items.retain(|i| !(matches!(i.kind, ItemKind::Area { kind: AreaKind::Conduction { .. }, .. }) && i.nets.iter().any(|n| first.contains(n))));
        for net in board.rules.nets.values_mut() {
            if first.contains(&net.no) {
                net.contains_plane = false;
            }
        }
    }
    let mut rb = RoutingBoard::new(board);
    let mut passes = route_pour_nets(&mut rb, plan, &first, max_passes);
    let next = passes.last().map_or(1, |p| p.pass_no + 1);
    passes.extend(autoroute_passes(&mut rb, next, max_passes));
    (rb, passes)
}

/// Route each net poured on an outer layer before anything else, as copper
/// of its own and on the pour's layer wherever it can: a backbone joining
/// every pad, which the plane then only has to touch. FreeRouting lets the
/// signals cross a plane anywhere, and on an outer layer they cut it into
/// islands; a pad of the net then still reaches the rest by the net's own
/// copper. The signals may push the backbone aside, or rip it up and
/// route it again: fixed, it walled them out of too much of the layer. An
/// inner plane is FreeRouting's own: its layer carries nothing else.
fn route_pour_nets(rb: &mut RoutingBoard, plan: &Plan, nets: &[i32], max_passes: i32) -> Vec<PassSummary> {
    let mut summaries = Vec::new();
    let mut pass_no = 1;
    for &net in nets {
        let saved = rb.board.settings.clone();
        let pour_layers: Vec<usize> = plan.planes.iter().filter(|p| p.net as i32 + 1 == net).map(|p| p.layer).collect();
        let s = &mut rb.board.settings;
        for l in 0..plan.layers.len() {
            // The pour's layer as cheap as a plain layer, the others dear.
            let f = if pour_layers.contains(&l) { 1.0 / POUR_LAYER_COST_FACTOR } else { POUR_NET_OFF_LAYER_FACTOR };
            s.trace_costs[l].0 *= f;
            s.trace_costs[l].1 *= f;
            s.preferred_direction_costs[l] *= f;
        }
        let mut seen = std::collections::HashSet::new();
        for _ in 0..max_passes {
            let items: Vec<usize> = pass_items(rb).into_iter().filter(|&i| rb.item(i).nets.contains(&net)).collect();
            if items.is_empty() || !seen.insert(rb.routing_hash()) {
                break;
            }
            let mut summary = PassSummary { pass_no, items: items.len(), routed: 0, not_routed: 0, insert_errors: 0 };
            for item in items {
                match autoroute_item(rb, item, net, pass_no).result {
                    RouteResult::Routed | RouteResult::AlreadyConnected => summary.routed += 1,
                    RouteResult::NotRouted => summary.not_routed += 1,
                    RouteResult::InsertError => {
                        summary.not_routed += 1;
                        summary.insert_errors += 1;
                    }
                }
            }
            remove_pass_tails(rb);
            summaries.push(summary);
            pass_no += 1;
        }
        rb.board.settings = saved;
    }
    summaries
}

/// A pad as the routing gates see it.
struct GatePad<'a> {
    geom: &'a PlacedPad,
    net: &'a str,
    layers: Vec<&'a str>,
}

impl GatePad<'_> {
    /// The rectangle a rounded rectangle is rounded about, and the radius:
    /// its copper is the points within that of it. As the gates round it.
    fn core(&self) -> ((f64, f64, f64, f64), f64) {
        let g = self.geom;
        let r = g.corner_radius();
        let (cx, cy, hw, hh) = (g.center.x as f64, g.center.y as f64, g.size.0 as f64 / 2.0, g.size.1 as f64 / 2.0);
        (((cx - hw + r).round(), (cy - hh + r).round(), (cx + hw - r).round(), (cy + hh - r).round()), r)
    }

    /// Where on the segment the pad's copper lies deepest, if the segment's
    /// centreline touches it: nearest its centre for a round pad; for a
    /// rectangle, midway along the run through its core, or nearest the
    /// core where the segment misses it.
    fn deepest_on(&self, a: IrPoint, b: IrPoint) -> Option<(f64, f64)> {
        let g = self.geom;
        let (ax, ay, dx, dy) = (a.x as f64, a.y as f64, (b.x - a.x) as f64, (b.y - a.y) as f64);
        let at = |t: f64| (ax + t * dx, ay + t * dy);
        if g.is_round() {
            let (cx, cy) = (g.center.x as f64, g.center.y as f64);
            let len2 = dx * dx + dy * dy;
            let t = if len2 == 0.0 { 0.0 } else { (((cx - ax) * dx + (cy - ay) * dy) / len2).clamp(0.0, 1.0) };
            let p = at(t);
            let reach = (g.size.0.min(g.size.1) / 2) as f64;
            return (((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt() <= reach).then_some(p);
        }
        let (core, r) = self.core();
        // The run through the core, clipped (Liang-Barsky).
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let mut inside = true;
        for (p, q) in [(-dx, ax - core.0), (dx, core.2 - ax), (-dy, ay - core.1), (dy, core.3 - ay)] {
            if p == 0.0 {
                inside &= q >= 0.0;
            } else if p < 0.0 {
                t0 = t0.max(q / p);
            } else {
                t1 = t1.min(q / p);
            }
        }
        if inside && t0 <= t1 {
            return Some(at((t0 + t1) / 2.0));
        }
        // Missing the core: nearest it, the distance being convex along the
        // segment.
        let dist = |(x, y): (f64, f64)| {
            let (ox, oy) = ((core.0 - x).max(x - core.2).max(0.0), (core.1 - y).max(y - core.3).max(0.0));
            (ox * ox + oy * oy).sqrt()
        };
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..100 {
            let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if dist(at(m1)) <= dist(at(m2)) {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        let p = at((lo + hi) / 2.0);
        (dist(p) <= r).then_some(p)
    }
}

/// Every track that runs through a pad of its own net without ending in
/// it cut in two inside the pad: the same copper, meeting where the pad
/// joins it anyway, as the gates would have it -- a pad is where a track
/// ends, not a stepping stone. A track that only grazes a pad, with no
/// point of its copper strictly inside, is left whole.
fn split_at_own_pads(tracks: Vec<Track>, pads: &[GatePad]) -> Vec<Track> {
    let mut done = Vec::new();
    let mut todo = tracks;
    'next: while let Some(t) = todo.pop() {
        let (first, last) = (t.pts[0], t.pts[t.pts.len() - 1]);
        for pad in pads.iter().filter(|p| p.net == t.net && p.layers.contains(&t.layer.as_str())) {
            if pad.geom.contains(first) || pad.geom.contains(last) {
                continue;
            }
            for k in 0..t.pts.len() - 1 {
                let Some((x, y)) = pad.deepest_on(t.pts[k], t.pts[k + 1]) else { continue };
                let cut = IrPoint { x: x.round() as i64, y: y.round() as i64 };
                if !pad.geom.contains(cut) {
                    continue;
                }
                let mut head: Vec<IrPoint> = t.pts[..=k].to_vec();
                let mut tail: Vec<IrPoint> = vec![cut];
                if head.last() != Some(&cut) {
                    head.push(cut);
                }
                tail.extend(t.pts[k + 1..].iter().copied().filter(|&p| p != cut));
                for pts in [head, tail] {
                    if pts.len() >= 2 {
                        todo.push(Track { pts, ..t.clone() });
                    }
                }
                continue 'next;
            }
        }
        done.push(t);
    }
    done
}

/// The runs of track that leave a pad only to come back into it, with
/// nothing else on the way, dropped: FreeRouting's exit stub turning back
/// over its own pin, once [`split_at_own_pads`] has cut it at the pad.
/// Nothing they join is joined otherwise, for the pad joins it.
fn drop_pad_loops(mut tracks: Vec<Track>, vias: &[IrVia], pads: &[GatePad]) -> Vec<Track> {
    loop {
        // Where a run may not pass through: a pad or via of its net, or a
        // point more than two track ends meet at.
        let pad_at = |t: &Track, p: IrPoint| pads.iter().position(|pad| pad.net == t.net && pad.layers.contains(&t.layer.as_str()) && pad.geom.contains(p));
        let via_at = |t: &Track, p: IrPoint| vias.iter().any(|v| v.net == t.net && v.at == p);
        let ends_at = |t: &Track, p: IrPoint| tracks.iter().filter(|o| o.net == t.net && o.layer == t.layer && (o.pts[0] == p || o.pts[o.pts.len() - 1] == p)).count();
        let mut drop: Option<Vec<usize>> = None;
        let starts = tracks.iter().enumerate().flat_map(|(i, t)| [(i, t.pts[0], t.pts[t.pts.len() - 1]), (i, t.pts[t.pts.len() - 1], t.pts[0])]);
        'search: for (i, from, to) in starts {
            let t = &tracks[i];
            let Some(pad) = pad_at(t, from) else { continue };
            // Follow the run from the pad on through plain joints.
            let (mut run, mut at, mut cur) = (vec![i], to, i);
            loop {
                if pad_at(&tracks[cur], at).is_some() || via_at(&tracks[cur], at) || ends_at(&tracks[cur], at) != 2 {
                    break;
                }
                let next = tracks.iter().enumerate().find(|(j, o)| !run.contains(j) && o.net == t.net && o.layer == t.layer && (o.pts[0] == at || o.pts[o.pts.len() - 1] == at));
                let Some((j, o)) = next else { break };
                at = if o.pts[0] == at { o.pts[o.pts.len() - 1] } else { o.pts[0] };
                run.push(j);
                cur = j;
            }
            if pad_at(&tracks[cur], at) == Some(pad) {
                drop = Some(run);
                break 'search;
            }
        }
        let Some(run) = drop else { return tracks };
        let mut i = 0;
        tracks.retain(|_| {
            i += 1;
            !run.contains(&(i - 1))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::footprint::PadShape as ModelPadShape;

    fn pad(x: i64, y: i64, shape: ModelPadShape) -> PlacedPad {
        PlacedPad { number: "1".into(), center: IrPoint { x, y }, size: (1000, 1000), through_hole: false, shape, roundrect_ratio: None }
    }

    fn track(pts: &[(i64, i64)]) -> Track {
        Track { id: String::new(), net: "A".into(), pins: Vec::new(), layer: "F.Cu".into(), width: 200, pts: pts.iter().map(|&(x, y)| IrPoint { x, y }).collect() }
    }

    fn gate_pads(pads: &[PlacedPad]) -> Vec<GatePad<'_>> {
        pads.iter().map(|g| GatePad { geom: g, net: "A", layers: vec!["F.Cu"] }).collect()
    }

    #[test]
    fn a_track_through_a_pad_of_its_net_ends_in_it() {
        for shape in [ModelPadShape::Rect, ModelPadShape::RoundRect, ModelPadShape::Circle] {
            let pads = [pad(0, 0, shape)];
            let split = split_at_own_pads(vec![track(&[(-3000, 100), (3000, 100)])], &gate_pads(&pads));
            assert_eq!(split.len(), 2, "{shape:?}");
            for t in &split {
                assert!(pads[0].contains(t.pts[0]) || pads[0].contains(t.pts[t.pts.len() - 1]), "{shape:?}: {:?}", t.pts);
            }
        }
        // Grazing a rounded corner is not running through it.
        let pads = [pad(0, 0, ModelPadShape::Circle)];
        assert_eq!(split_at_own_pads(vec![track(&[(-3000, 600), (3000, 600)])], &gate_pads(&pads)).len(), 1);
    }

    #[test]
    fn a_run_back_into_its_own_pad_goes() {
        let pads = [pad(0, 0, ModelPadShape::Rect), pad(5000, 0, ModelPadShape::Rect)];
        // Out of the first pad and back into it, and on to the second.
        let tracks = vec![track(&[(0, 0), (0, 2000)]), track(&[(0, 2000), (300, 2000), (300, 300)]), track(&[(300, 300), (5000, 0)])];
        let kept = drop_pad_loops(tracks, &[], &gate_pads(&pads));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].pts[0], IrPoint { x: 300, y: 300 });
        // A via on the way keeps the run: it may lead elsewhere.
        let tracks = vec![track(&[(0, 0), (0, 2000)]), track(&[(0, 2000), (300, 300)])];
        let via = IrVia { id: String::new(), net: "A".into(), at: IrPoint { x: 0, y: 2000 }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() };
        assert_eq!(drop_pad_loops(tracks, &[via], &gate_pads(&pads)).len(), 2);
    }

    /// A net class with its own `clearance` -- an "escape" class narrower
    /// than the board default -- must reach the router's clearance matrix
    /// as its own class, both to itself and to the default class, not
    /// silently collapse onto the board-wide value the way a class with
    /// only a custom `track_width` already did before `clearance` existed
    /// on `NetClass`.
    #[test]
    fn a_net_classs_own_clearance_reaches_the_router_matrix() {
        use eda_model::ir::{FootprintInstance, PlacementSection, Provenance};
        use eda_model::{Footprint, Net, NetClass, Pad, PadKind, Part, Pin, PinKind};

        let pad1 = Footprint {
            name: "PAD1".into(),
            pads: vec![Pad { number: "1".into(), at: (0, 0), size: (1000, 1000), shape: ModelPadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None }],
            courtyard: None,
            model: None,
        };
        let part = |r: &str| Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some("PAD1".into()),
            footprint: Some("PAD1".into()),
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Signal }],
            body_um: None,
            edge: None,
        };
        let model = ConstraintModel {
            parts: vec![part("P1"), part("P2"), part("P3")],
            nets: vec![
                Net { name: "A".into(), pins: vec!["P1.1".into()] },
                Net { name: "CC1".into(), pins: vec!["P2.1".into()] },
                Net { name: "CC2".into(), pins: vec!["P3.1".into()] },
            ],
            footprints: vec![pad1],
            board: BoardRules {
                clearance: 200,
                net_classes: vec![NetClass { name: "cc_escape".into(), nets: vec!["CC1".into(), "CC2".into()], track_width: Some(150), clearance: Some(150), priority: 0 }],
                ..BoardRules::default()
            },
            ..Default::default()
        };
        let placement = PlacementSection {
            outline: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 10_000, y: 0 }, IrPoint { x: 10_000, y: 10_000 }, IrPoint { x: 0, y: 10_000 }],
            footprints: vec![
                FootprintInstance { id: "P1".into(), at: IrPoint { x: 1_000, y: 1_000 }, rot: 0, side: Side::Top, label: Default::default() },
                FootprintInstance { id: "P2".into(), at: IrPoint { x: 3_000, y: 1_000 }, rot: 0, side: Side::Top, label: Default::default() },
                FootprintInstance { id: "P3".into(), at: IrPoint { x: 5_000, y: 1_000 }, rot: 0, side: Side::Top, label: Default::default() },
            ],
            modules: Vec::new(),
        };
        let design = Design {
            schema: 1,
            provenance: Provenance { engine_version: "test".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            placement: Some(placement),
            routing: None,
            drawings: None,
        };
        let plan = Plan::of(&design, &model, &model.board).expect("plan builds");
        let board = board_from_design(&design, &model, &model.board).expect("board builds");

        // `net_classes[0]` is the router's own structural default, added
        // regardless of the rules; `net_classes[1..]` mirror `plan.classes`
        // in order (see `board_from_design`), so a `PlanNet.class` index
        // shifts by one to land on the matching `NetClass`.
        let class_for = |net: &str| {
            let plan_net = plan.nets.iter().find(|n| n.name == net).expect("net placed");
            board.rules.net_classes[plan_net.class + 1].trace_clearance_class
        };
        let default_trace_class = class_for("A");
        let escape_trace_class = class_for("CC1");
        assert_eq!(escape_trace_class, class_for("CC2"), "CC1 and CC2 share the escape class");
        assert_ne!(escape_trace_class, default_trace_class, "the escape class must not collapse onto the default's clearance row");

        let layer = 0;
        // Both raw values carry `Plan`'s own rounding margin (see
        // `ROUNDING_MARGIN_UM`), the same way `plan.clearance` itself does.
        let escape_um = |raw: i64| raw / UNITS_PER_UM - ROUNDING_MARGIN_UM;
        assert_eq!(escape_um(board.rules.clearance.get(escape_trace_class, escape_trace_class, layer)), 150, "the escape class's clearance to its own copper");
        assert_eq!(escape_um(board.rules.clearance.get(escape_trace_class, default_trace_class, layer)), 150, "and to the default class's copper -- this is what lets it actually escape past a neighbour on the default class");
        assert_eq!(escape_um(board.rules.clearance.get(default_trace_class, default_trace_class, layer)), 200, "the default class keeps the board's own clearance, untouched by the escape class existing");
    }
}

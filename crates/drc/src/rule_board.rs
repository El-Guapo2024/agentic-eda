//! The board as the rule engine sees it: [`RuleItem`]s for the pads, tracks, vias, zones, footprints, graphics and texts of a design, the environment the
//! expression language reads (the layers by name, the nets, the net classes, the footprints and the areas), and the [`RuleEngine`] over them with the board's
//! implicit rules and its `.kicad_dru`. It is what `BOARD_INSPECTION_TOOL::makeDRCEngine` and `BOARD::GetItemByIdCache` give the Clearance and Constraints
//! Resolution commands in KiCad.
//!
//! The items are built from the flattened [`DrcBoard`] (the geometry every other provider here works from) and the design's own facts (net classes, the pad
//! and footprint overrides, the zones' settings, the footprints' net ties). Lengths are nanometres, KiCad's unit and the studio's.
//!
//! Only the properties the studio models are on the items ([`crate::known_props`] lists every name KiCad has; a rule that names another compiles and reads
//! it as undefined). Component classes, sheet paths and the fields of a footprint beyond Reference, Value and Footprint are not in the IR.

use crate::board::{DrcBoard, DrcPad};
use crate::constraints::{class_views, disallow, human_readable_class_name, KeepoutRule, RuleEngine, Settings, Units, ZoneConnection};
use crate::dru::ParseError;
use crate::kimath::Shape;
use crate::pcbexpr::Env;
use crate::rule_item::{AreaFacts, Class, FootprintFacts, Prop, RuleItem};
use eda_model::ir::{Design, Point, Side, Nm};
use eda_model::layers::{pcb_layer_id, TECH_LAYERS};
use eda_model::{BoardRules, ConstraintModel, PadKind, PadShape};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

/// Every layer a board file can name, in `PCB_LAYER_ID` order.
pub fn all_layer_names() -> Vec<String> {
    let mut v: Vec<String> = vec!["F.Cu".into()];
    v.extend((1..=30).map(|n| format!("In{n}.Cu")));
    v.push("B.Cu".into());
    v.extend(TECH_LAYERS.iter().map(|s| s.to_string()));
    v.extend((1..=45).map(|n| format!("User.{n}")));
    v
}

/// `m_frame->StringFromValue`-less `MessageTextFromValue( value )` of a units provider: the frame's units, `%.4f` mm, `%.2f` mils, `%.4f` in.
pub fn message_text(value_nm: f64, units: Units) -> String {
    match units {
        Units::Mm => crate::constraints::message_text_from_value(value_nm),
        Units::Mils => format!("{:.2} mils", value_nm / 25_400.0),
        Units::Inch => format!("{:.4} in", value_nm / 25_400_000.0),
    }
}

/// `BOARD_ITEM::LayerMaskDescribe`.
fn layer_mask_describe(layers: &[String], rules: &BoardRules) -> String {
    let enabled = |l: &str| rules.layers.iter().any(|c| c == l) || rules.layer_setup.enabled.iter().any(|e| e == l);
    let mut in_board: Vec<&String> = layers.iter().filter(|l| enabled(l)).collect();
    in_board.sort_by_key(|l| pcb_layer_id(l).unwrap_or(u32::MAX));
    let copper: Vec<&String> = in_board.iter().copied().filter(|l| l.ends_with(".Cu")).collect();
    let tech: Vec<&String> = in_board.iter().copied().filter(|l| !l.ends_with(".Cu")).collect();
    if copper.len() == rules.layers.len() {
        return "all copper layers".to_string();
    }
    for set in [&copper, &tech, &in_board] {
        if let Some(first) = set.first() {
            let mut name = rules.layer_setup.display_name(first);
            if set.len() > 1 {
                name.push_str(" and others");
            }
            return name;
        }
    }
    "no layers".to_string()
}

fn net_msg(net: &str) -> String {
    if net.is_empty() {
        "[<no net>]".to_string()
    } else {
        format!("[{net}]")
    }
}

/// How a rules file and a design come together for the inspection commands.
pub struct RuleBoard<'a> {
    design: &'a Design,
    model: &'a ConstraintModel,
    board: &'a DrcBoard,
    rules: &'a BoardRules,
    pub units: Units,
    pub engine: RuleEngine,
    /// The error `makeDRCEngine` flags as `compileError`: the rules file that did not parse or compile.
    pub compile_error: Option<ParseError>,
    footprints: HashMap<String, Rc<RuleItem>>,
    /// The pad ids as the studio names them (`REF.NUMBER`, `REF.NUMBER#2` for the second pad with a number) with the index into `board.pads`.
    pad_ids: Vec<(String, usize)>,
    /// Some footprint has a courtyard that does not make a closed outline (`MALFORMED_COURTYARDS`): never set here, the courtyards are what the board file has.
    pub courtyard_error: bool,
}

/// The items of the board an inspection can name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    Footprint,
    Pad,
    Track,
    Via,
    Zone,
    Shape,
    Text,
    Group,
}

impl<'a> RuleBoard<'a> {
    pub fn new(design: &'a Design, model: &'a ConstraintModel, board: &'a DrcBoard, units: Units, dru: Option<&str>) -> RuleBoard<'a> {
        let rules = &model.board;
        // The pad ids first: the footprints' net ties and the engine's environment both need them.
        let mut seen: HashMap<(String, String), usize> = HashMap::new();
        let pad_ids: Vec<(String, usize)> = board
            .pads
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let n = seen.entry((p.footprint_ref.clone(), p.number.clone())).or_insert(0);
                *n += 1;
                (if *n == 1 { p.id.clone() } else { format!("{}#{}", p.id, n) }, i)
            })
            .collect();

        let mut rb = RuleBoard {
            design,
            model,
            board,
            rules,
            units,
            engine: RuleEngine::new(Settings::from_rules(rules), Rc::new(Env::simple(&[])), &[], &[]),
            compile_error: None,
            footprints: HashMap::new(),
            pad_ids,
            courtyard_error: false,
        };
        rb.footprints = rb.build_footprints();

        // The environment: layers by name, nets, classes, footprints, areas.
        let mut entries: Vec<(String, String)> = Vec::new();
        let mut standard: HashMap<String, String> = HashMap::new();
        for l in all_layer_names() {
            entries.push((l.clone(), l.clone()));
            let shown = rules.layer_setup.display_name(&l);
            entries.push((shown.clone(), l.clone()));
            standard.insert(l.clone(), shown);
        }
        let r = rules.clone();
        let mut env = Env::new(
            entries,
            standard,
            Box::new(move |net: &str| {
                let e = r.effective_class(net);
                (e.name, e.constituents)
            }),
        );
        env.nets = model.nets.iter().map(|n| n.name.clone()).collect();
        env.footprints = rb.footprints.values().cloned().collect();
        env.footprints.sort_by(|a, b| a.id.cmp(&b.id));
        env.areas = rb.areas();
        let env = Rc::new(env);

        let nets: Vec<String> = model.nets.iter().map(|n| n.name.clone()).collect();
        let classes = class_views(rules, &nets);
        let keepouts = rb.keepout_rules();
        let (engine, error) = RuleEngine::with_rules_file(Settings::from_rules(rules), env, &classes, &keepouts, dru);
        rb.engine = engine;
        rb.compile_error = error;
        rb
    }

    // ------------------------------------------------------------------------------------------------------------------------------ environment

    fn areas(&self) -> Vec<Rc<AreaFacts>> {
        let mut out = Vec::new();
        if let Some(rt) = &self.design.routing {
            for z in &rt.zones {
                out.push(Rc::new(AreaFacts { id: z.id.clone(), name: z.name.clone(), layers: vec![z.layer.clone()], outline: z.outline.clone(), parent_footprint: z.parent_footprint.clone() }));
            }
        }
        out
    }

    /// `addKeepoutZoneRule` for each rule area with a keepout flag set (`isKeepoutZone( zone, true )`).
    fn keepout_rules(&self) -> Vec<KeepoutRule> {
        let Some(rt) = &self.design.routing else { return Vec::new() };
        rt.zones
            .iter()
            .filter(|z| z.is_rule_area && z.outline.len() >= 3)
            .filter_map(|z| {
                let mut flags = 0;
                if z.keepout_tracks {
                    flags |= disallow::TRACKS;
                }
                if z.keepout_vias {
                    // (`DRC_DISALLOW_VIAS` is the through vias alone.)
                    flags |= disallow::THROUGH_VIAS;
                }
                if z.keepout_pads {
                    flags |= disallow::PADS;
                }
                if z.keepout_copper_pour {
                    flags |= disallow::ZONES;
                }
                if z.keepout_footprints {
                    flags |= disallow::FOOTPRINTS;
                }
                (flags != 0).then(|| KeepoutRule { id: z.id.clone(), name: z.name.clone(), parent_ref: z.parent_footprint.clone(), layers: [z.layer.clone()].into_iter().collect(), flags })
            })
            .collect()
    }

    // ----------------------------------------------------------------------------------------------------------------------------- footprints

    fn copper_layers(&self) -> Vec<String> {
        self.rules.layers.clone()
    }

    fn build_footprints(&self) -> HashMap<String, Rc<RuleItem>> {
        let mut out = HashMap::new();
        let Some(pl) = &self.design.placement else { return out };
        let extras: HashMap<&str, &eda_model::ir::FootprintExtra> = self.design.drawings.as_ref().map(|d| d.footprint_extras.iter().map(|e| (e.id.as_str(), e)).collect()).unwrap_or_default();
        for fp in &pl.footprints {
            let part = self.model.part(&fp.id);
            let fpid = part.and_then(|p| p.footprint.clone().or_else(|| p.package.clone())).unwrap_or_default();
            let value = part.and_then(|p| p.value.clone()).unwrap_or_default();
            let side_layer = if fp.side == Side::Bottom { "B.Cu" } else { "F.Cu" };
            let outlines: Vec<Vec<Point>> = self
                .board
                .footprints
                .iter()
                .find(|f| f.id == fp.id)
                .map(|f| {
                    if f.outlines.is_empty() {
                        let (x0, y0, x1, y1) = f.courtyard;
                        vec![vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }]]
                    } else {
                        f.outlines.clone()
                    }
                })
                .unwrap_or_default();
            let flipped = fp.side == Side::Bottom;
            let facts = FootprintFacts {
                reference: fp.id.clone(),
                fpid: fpid.clone(),
                fields: vec![("Reference".into(), fp.id.clone()), ("Value".into(), value.clone()), ("Footprint".into(), fpid.clone())],
                flipped,
                courtyard_front: if flipped { Vec::new() } else { outlines.clone() },
                courtyard_back: if flipped { outlines.clone() } else { Vec::new() },
                sheet: String::new(),
            };
            let mut it = RuleItem::new(fp.id.clone(), Class::Footprint);
            it.layers = vec![side_layer.to_string()];
            it.layer = Some(side_layer.to_string());
            it.position = fp.at;
            it.description = format!("Footprint {}", if fp.id.is_empty() { "<no reference designator>" } else { &fp.id });
            it.shapes = outlines.iter().filter(|o| o.len() >= 3).map(|o| Shape::Polygon { pts: o.clone() }).collect();
            it.set("Reference", Prop::Str(fp.id.clone()));
            it.set("Value", Prop::Str(value));
            it.set("Library Link", Prop::Str(fpid));
            it.set("Position X", Prop::Int(fp.at.x));
            it.set("Position Y", Prop::Int(fp.at.y));
            it.set("Orientation", Prop::Double(fp.rot as f64 / 1000.0));
            it.set("Layer", Prop::Layer(side_layer.to_string()));
            it.set("Locked", Prop::Bool(false));
            if let Some(e) = extras.get(fp.id.as_str()) {
                if let Some(c) = e.clearance {
                    it.overrides.clearance = Some((c, format!("footprint {}", fp.id)));
                    it.local_clearance = Some((c, format!("footprint {}", fp.id)));
                }
                if let Some(z) = e.zone_connection {
                    it.overrides.zone_connection = Some((ZoneConnection::from_pad_connection(z), format!("footprint {}", fp.id)));
                }
                it.overrides.solder_mask_margin = e.solder_mask_margin;
            }
            it.footprint = Some(Rc::new(facts));
            out.insert(fp.id.clone(), Rc::new(it));
        }
        out
    }

    /// The nets (by name) a pad of a net-tie footprint may touch: every pad of its group, its own included (`FOOTPRINT::BuildNetTieCache`).
    fn net_tie_nets(&self, p: &DrcPad) -> Vec<String> {
        let Some(extra) = self.design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == p.footprint_ref)) else { return Vec::new() };
        for group in &extra.net_tie_pad_groups {
            let numbers: Vec<&str> = group.split(',').map(str::trim).collect();
            if !numbers.contains(&p.number.as_str()) {
                continue;
            }
            let others: Vec<&DrcPad> = self.board.pads.iter().filter(|q| q.footprint_ref == p.footprint_ref && numbers.contains(&q.number.as_str())).collect();
            if others.len() < 2 {
                return Vec::new();
            }
            let mut nets: BTreeSet<String> = BTreeSet::new();
            for q in others {
                nets.insert(q.net.clone().unwrap_or_default());
            }
            return nets.into_iter().collect();
        }
        Vec::new()
    }

    // ------------------------------------------------------------------------------------------------------------------------------------ pads

    fn pad_layers(&self, idx: usize, p: &DrcPad) -> Vec<String> {
        if let Some(m) = self.board.mask.pads.get(idx).filter(|m| !m.layers.is_empty()) {
            return m.layers.clone();
        }
        let back = p.side == Side::Bottom;
        match p.kind {
            PadKind::Smd => {
                let (cu, paste, mask) = if back { ("B.Cu", "B.Paste", "B.Mask") } else { ("F.Cu", "F.Paste", "F.Mask") };
                vec![cu.into(), paste.into(), mask.into()]
            }
            PadKind::ThroughHole => {
                let mut l = self.copper_layers();
                l.extend(["F.Mask".into(), "B.Mask".into()]);
                l
            }
            PadKind::NonPlatedHole => {
                let mut l = self.copper_layers();
                l.extend(["F.Mask".into(), "B.Mask".into()]);
                l
            }
        }
    }

    fn pad_item(&self, id: &str, idx: usize) -> RuleItem {
        let p = &self.board.pads[idx];
        let net = p.net.clone().unwrap_or_default();
        let attribute = match p.kind {
            PadKind::Smd => "SMD",
            PadKind::ThroughHole => "PTH",
            PadKind::NonPlatedHole => "NPTH",
        };
        let parent = self.footprints.get(&p.footprint_ref).cloned();
        let layers = self.pad_layers(idx, p);
        let mut it = RuleItem::new(id, Class::Pad);
        it.net = net.clone();
        it.layer = layers.first().cloned();
        it.layers = layers.clone();
        it.pad_attribute = attribute;
        it.shapes = vec![p.copper.clone()];
        it.hole = p.hole.clone();
        it.has_hole = p.hole.is_some();
        it.has_drilled_hole = p.drill_round.is_some() || (p.hole.is_some() && p.drill_slot.is_none());
        it.plated = p.kind == PadKind::ThroughHole;
        it.position = p.center;
        it.flashed = p.layers.clone();
        it.drill = p.drill_round.or_else(|| p.drill_slot.map(|(w, _)| w)).unwrap_or(0);
        it.parent = parent.clone();
        it.footprint = parent.as_ref().and_then(|f| f.footprint.clone());
        it.net_tie = self.net_tie_nets(p);
        if let Some(m) = self.board.mask.pads.get(idx) {
            it.tented = [m.tent_front.unwrap_or(false), m.tent_back.unwrap_or(false)];
            it.overrides.solder_mask_margin = m.margin;
        }
        // `PAD::GetClearanceOverrides`: its own clearance, else its footprint's. `GetLocalClearance` is the pad's alone.
        if let Some(c) = p.zone.pad_clearance {
            it.overrides.clearance = Some((c, "pad".to_string()));
            it.local_clearance = Some((c, "pad".to_string()));
        } else if let Some(c) = p.zone.footprint_clearance {
            it.overrides.clearance = Some((c, format!("footprint {}", p.footprint_ref)));
        }
        if let Some(z) = p.zone.connection {
            it.overrides.zone_connection = Some((ZoneConnection::from_pad_connection(z), "pad".to_string()));
        }
        it.overrides.thermal_gap = p.zone.thermal_gap.map(|g| (g, "pad".to_string()));
        it.overrides.spoke_width = p.zone.thermal_spoke_width.map(|g| (g, "pad".to_string()));
        // properties
        let shape_label = match p.shape {
            PadShape::Circle => "Circle",
            PadShape::Rect => "Rectangle",
            PadShape::Oval => "Oval",
            PadShape::Trapezoid => "Trapezoid",
            PadShape::RoundRect => "Rounded rectangle",
            PadShape::ChamferedRect => "Chamfered rectangle",
            PadShape::Custom => "Custom",
        };
        let type_label = match p.kind {
            PadKind::Smd => "SMD",
            PadKind::ThroughHole => "Through-hole",
            PadKind::NonPlatedHole => "NPTH, mechanical",
        };
        it.set("Pad Number", Prop::Str(p.number.clone()));
        it.set("Pad Type", Prop::Enum(type_label.to_string()));
        it.set("Pad Shape", Prop::Enum(shape_label.to_string()));
        it.set("Size X", Prop::Int(p.size.0));
        it.set("Size Y", Prop::Int(p.size.1));
        it.set("Position X", Prop::Int(p.center.x));
        it.set("Position Y", Prop::Int(p.center.y));
        it.set("Orientation", Prop::Double(-(p.orientation_mdeg as f64) / 1000.0));
        it.set("Net", Prop::Enum(net.clone()));
        it.set("Locked", Prop::Bool(false));
        it.set("Layer", Prop::Layer(layers.first().cloned().unwrap_or_default()));
        if let Some(d) = p.drill_round {
            it.set("Hole Size X", Prop::Int(d));
            it.set("Hole Size Y", Prop::Int(d));
        } else if let Some((w, h)) = p.drill_slot {
            it.set("Hole Size X", Prop::Int(w));
            it.set("Hole Size Y", Prop::Int(h));
        }
        // PAD::GetItemDescription
        let fp_ref = &p.footprint_ref;
        let where_ = layer_mask_describe(&layers, self.rules);
        let nmsg = net_msg(&net);
        it.description = if attribute == "NPTH" {
            format!("NPTH pad of {fp_ref}")
        } else if p.number.is_empty() {
            if attribute == "SMD" {
                format!("Pad {nmsg} of {fp_ref} on {where_}")
            } else {
                format!("PTH pad {nmsg} of {fp_ref}")
            }
        } else if attribute == "SMD" {
            format!("Pad {} {nmsg} of {fp_ref} on {where_}", p.number)
        } else {
            format!("PTH pad {} {nmsg} of {fp_ref}", p.number)
        };
        it
    }

    // ---------------------------------------------------------------------------------------------------------------------------------- tracks

    fn track_item(&self, seg_idx: usize) -> RuleItem {
        let s = &self.board.tracks[seg_idx];
        let net = s.net.clone().unwrap_or_default();
        let class = if s.is_arc() { Class::Arc } else { Class::Track };
        let mut it = RuleItem::new(s.id.clone(), class);
        it.net = net.clone();
        it.layers = vec![s.layer.clone()];
        it.layer = Some(s.layer.clone());
        it.shapes = vec![s.shape()];
        it.position = s.a;
        it.set("Width", Prop::Int(s.width));
        it.set("Layer", Prop::Layer(s.layer.clone()));
        it.set("Net", Prop::Enum(net.clone()));
        it.set("Start X", Prop::Int(s.a.x));
        it.set("Start Y", Prop::Int(s.a.y));
        it.set("End X", Prop::Int(s.b.x));
        it.set("End Y", Prop::Int(s.b.y));
        it.set("Locked", Prop::Bool(false));
        let length = match s.arc_mid {
            Some(mid) => {
                let pts = crate::board::arc_polyline(s.a, mid, s.b, crate::board::ARC_HIGH_DEF);
                pts.windows(2).map(|w| crate::kimath::dist(w[0], w[1]) as f64).sum::<f64>()
            }
            None => crate::kimath::dist(s.a, s.b) as f64,
        };
        let layer_name = self.rules.layer_setup.display_name(&s.layer);
        it.description = format!("{} {} on {}, length {}", if s.is_arc() { "Track (arc)" } else { "Track" }, net_msg(&net), layer_name, message_text(length, self.units));
        it
    }

    // ------------------------------------------------------------------------------------------------------------------------------------ vias

    fn via_item(&self, idx: usize) -> RuleItem {
        let v = &self.board.vias[idx];
        let net = v.net.clone().unwrap_or_default();
        let copper = self.copper_layers();
        let (i0, i1) = (copper.iter().position(|l| *l == v.from_layer), copper.iter().position(|l| *l == v.to_layer));
        let (lo, hi) = match (i0, i1) {
            (Some(a), Some(b)) => (a.min(b), a.max(b)),
            _ => (0, copper.len().saturating_sub(1)),
        };
        let layers: Vec<String> = copper[lo..=hi].to_vec();
        let outer = |l: &str| copper.first().is_some_and(|f| f == l) || copper.last().is_some_and(|f| f == l);
        let kind = {
            let (s_out, e_out) = (outer(&v.from_layer), outer(&v.to_layer));
            if s_out && e_out {
                "Through"
            } else if s_out ^ e_out {
                "Blind"
            } else {
                "Buried"
            }
        };
        let mut it = RuleItem::new(v.id.clone(), Class::Via);
        it.net = net.clone();
        it.layer = layers.first().cloned();
        it.layers = layers.clone();
        it.shapes = vec![v.shape()];
        it.hole = Some(v.hole());
        it.has_hole = true;
        it.has_drilled_hole = true;
        it.plated = true;
        it.position = v.at;
        it.via_type = Some(kind);
        it.drill = v.drill;
        it.flashed = layers;
        if let Some(t) = self.board.mask.vias.get(idx) {
            it.tented = [t.tent_front.unwrap_or(self.rules.solder_mask.tent_vias_front), t.tent_back.unwrap_or(self.rules.solder_mask.tent_vias_back)];
        }
        it.set("Via Type", Prop::Enum(kind.to_string()));
        it.set("Diameter", Prop::Int(v.diameter));
        it.set("Hole", Prop::Int(v.drill));
        it.set("Position X", Prop::Int(v.at.x));
        it.set("Position Y", Prop::Int(v.at.y));
        it.set("Layer Top", Prop::Layer(v.from_layer.clone()));
        it.set("Layer Bottom", Prop::Layer(v.to_layer.clone()));
        it.set("Net", Prop::Enum(net.clone()));
        it.set("Locked", Prop::Bool(false));
        let fmt = match kind {
            "Blind" => "Blind via",
            "Buried" => "Buried via",
            _ => "Via",
        };
        it.description = format!("{fmt} {} on {}", net_msg(&net), layer_mask_describe(&it.layers, self.rules));
        it
    }

    // ------------------------------------------------------------------------------------------------------------------------------------ zones

    fn zone_item(&self, z: &eda_model::ir::Zone) -> RuleItem {
        let mut it = RuleItem::new(z.id.clone(), Class::Zone);
        let layer_name = self.rules.layer_setup.display_name(&z.layer);
        it.layers = vec![z.layer.clone()];
        it.layer = Some(z.layer.clone());
        it.outline = z.outline.clone();
        it.shapes = vec![Shape::Polygon { pts: z.outline.clone() }];
        it.rule_area = z.is_rule_area;
        it.keepout = z.is_rule_area && (z.keepout_tracks || z.keepout_vias || z.keepout_pads || z.keepout_copper_pour || z.keepout_footprints);
        it.teardrop = z.teardrop;
        it.position = z.outline.first().copied().unwrap_or(Point { x: 0, y: 0 });
        it.set("Name", Prop::Str(z.name.clone()));
        it.set("Layer", Prop::Layer(z.layer.clone()));
        it.set("Priority", Prop::Int(z.priority as i64));
        if z.is_rule_area {
            it.description = if z.name.is_empty() { format!("Rule Area on {layer_name}") } else { format!("Rule area '{}' on {layer_name}", z.name) };
            return it;
        }
        let net = z.net.clone();
        it.net = net.clone();
        it.set("Net", Prop::Enum(net.clone()));
        it.set("Clearance", Prop::Int(z.clearance));
        it.set("Minimum Width", Prop::Int(z.min_thickness));
        let pc = ZoneConnection::from_pad_connection(z.pad_connection);
        it.set(
            "Pad Connections",
            Prop::Enum(
                match pc {
                    ZoneConnection::None => "None",
                    ZoneConnection::Thermal => "Thermal reliefs",
                    ZoneConnection::Full => "Solid",
                    ZoneConnection::ThtThermal => "Thermal reliefs for PTH",
                    ZoneConnection::Inherited => "Inherited",
                }
                .to_string(),
            ),
        );
        it.zone_pad_connection = Some(pc);
        it.zone_thermal_gap = z.thermal_gap;
        it.zone_spoke_width = z.thermal_spoke_width;
        it.zone_min_thickness = z.min_thickness;
        it.local_clearance = Some((z.clearance, "zone".to_string()));
        it.description = if z.teardrop {
            format!("Teardrop {} on {layer_name}", net_msg(&net))
        } else if z.name.is_empty() {
            format!("Zone {} on {layer_name}, priority {}", net_msg(&net), z.priority)
        } else {
            format!("Zone '{}' {} on {layer_name}, priority {}", z.name, net_msg(&net), z.priority)
        };
        it
    }

    // ----------------------------------------------------------------------------------------------------------------------- shapes and texts

    fn shape_item(&self, s: &eda_model::ir::Shape) -> RuleItem {
        let mut it = RuleItem::new(s.id().to_string(), Class::Shape);
        let layer = s.layer().to_string();
        it.layers = vec![layer.clone()];
        it.layer = Some(layer.clone());
        it.shapes = crate::board::effective_shapes(s);
        it.position = s.points().first().copied().unwrap_or(Point { x: 0, y: 0 });
        it.set("Layer", Prop::Layer(layer.clone()));
        it.set("Line Width", Prop::Int(s.stroke_width()));
        let name = match s {
            eda_model::ir::Shape::Segment { .. } => "Line",
            eda_model::ir::Shape::Rect { .. } => "Rectangle",
            eda_model::ir::Shape::Circle { .. } => "Circle",
            eda_model::ir::Shape::Arc { .. } => "Arc",
            eda_model::ir::Shape::Polygon { .. } => "Polygon",
            eda_model::ir::Shape::Bezier { .. } => "Curve",
        };
        let on = self.rules.layer_setup.display_name(&layer);
        it.description = if layer.ends_with(".Cu") { format!("{name} {} on {on}", net_msg("")) } else { format!("{name} on {on}") };
        it
    }

    fn text_item(&self, t: &eda_model::ir::Text) -> RuleItem {
        let mut it = RuleItem::new(t.id.clone(), Class::Text);
        it.layers = vec![t.layer.clone()];
        it.layer = Some(t.layer.clone());
        it.position = t.at;
        it.text_height = t.size_nm;
        it.text_thickness = t.stroke_width;
        it.set("Text", Prop::Str(t.content.clone()));
        it.set("Layer", Prop::Layer(t.layer.clone()));
        it.set("Text Height", Prop::Int(t.size_nm));
        it.set("Text Thickness", Prop::Int(t.stroke_width));
        it.set("Position X", Prop::Int(t.at.x));
        it.set("Position Y", Prop::Int(t.at.y));
        it.description = format!("PCB text '{}' on {}", t.content, self.rules.layer_setup.display_name(&t.layer));
        it
    }

    // --------------------------------------------------------------------------------------------------------------------------------- lookup

    /// What `id` names, by the ids the studio selects things by (a footprint's reference, a pad's `REF.NUMBER`, a track, via, zone, shape, text or group id).
    pub fn kind_of(&self, id: &str) -> Option<ItemKind> {
        if self.footprints.contains_key(id) {
            return Some(ItemKind::Footprint);
        }
        if self.design.drawings.as_ref().is_some_and(|d| d.groups.iter().any(|g| g.id == id)) {
            return Some(ItemKind::Group);
        }
        if let Some(rt) = &self.design.routing {
            if rt.tracks.iter().any(|t| t.id == id) {
                return Some(ItemKind::Track);
            }
            if rt.vias.iter().any(|v| v.id == id) {
                return Some(ItemKind::Via);
            }
            if rt.zones.iter().any(|z| z.id == id) {
                return Some(ItemKind::Zone);
            }
        }
        if let Some(d) = &self.design.drawings {
            if d.shapes.iter().any(|s| s.id() == id) {
                return Some(ItemKind::Shape);
            }
            if d.texts.iter().any(|t| t.id == id) {
                return Some(ItemKind::Text);
            }
        }
        if self.pad_ids.iter().any(|(pid, _)| pid == id) {
            return Some(ItemKind::Pad);
        }
        None
    }

    /// The item `id` names. A track is a polyline in the studio and one item per segment in KiCad: the segment nearest `at` (nanometres), else the first. A group is its
    /// first member (`reportClearance`: `*ag->GetItems().begin()`).
    pub fn item(&self, id: &str, at: Option<Point>) -> Result<Rc<RuleItem>, String> {
        match self.kind_of(id) {
            None => Err(format!("no such item: {id}")),
            Some(ItemKind::Footprint) => Ok(self.footprints[id].clone()),
            Some(ItemKind::Pad) => {
                let (pid, idx) = self.pad_ids.iter().find(|(p, _)| p == id).cloned().ok_or("no such pad")?;
                Ok(Rc::new(self.pad_item(&pid, idx)))
            }
            Some(ItemKind::Track) => {
                let segs: Vec<usize> = self.board.tracks.iter().enumerate().filter(|(_, s)| s.id.rsplit_once('#').is_some_and(|(base, _)| base == id)).map(|(i, _)| i).collect();
                let pick = match at {
                    Some(p) => segs.iter().copied().min_by_key(|&i| {
                        let s = &self.board.tracks[i];
                        crate::kimath::Shape::Stadium { a: s.a, b: s.b, r: 0 }.gap_to(&Shape::Circle { c: p, r: 0 }) as i64
                    }),
                    None => segs.first().copied(),
                };
                pick.map(|i| Rc::new(self.track_item(i))).ok_or_else(|| "track has no segments".to_string())
            }
            Some(ItemKind::Via) => {
                let idx = self.board.vias.iter().position(|v| v.id == id).ok_or("no such via")?;
                Ok(Rc::new(self.via_item(idx)))
            }
            Some(ItemKind::Zone) => {
                let z = self.design.routing.as_ref().and_then(|rt| rt.zones.iter().find(|z| z.id == id)).ok_or("no such zone")?;
                Ok(Rc::new(self.zone_item(z)))
            }
            Some(ItemKind::Shape) => {
                let s = self.design.drawings.as_ref().and_then(|d| d.shapes.iter().find(|s| s.id() == id)).ok_or("no such graphic")?;
                Ok(Rc::new(self.shape_item(s)))
            }
            Some(ItemKind::Text) => {
                let t = self.design.drawings.as_ref().and_then(|d| d.texts.iter().find(|t| t.id == id)).ok_or("no such text")?;
                Ok(Rc::new(self.text_item(t)))
            }
            Some(ItemKind::Group) => {
                let g = self.design.drawings.as_ref().and_then(|d| d.groups.iter().find(|g| g.id == id)).ok_or("no such group")?;
                let first = g.member_ids.first().ok_or("Cannot generate clearance report on empty group.")?;
                self.item(first, at)
            }
        }
    }

    /// `checkFootprint` of `reportClearance`: a footprint whose pads are all one logical pad stands for that pad, a footprint with several is itself, a footprint with none too.
    /// The rule item of the `idx`th pad of the board (`DrcBoard::pads`).
    pub fn pad_item_at(&self, idx: usize) -> Option<Rc<RuleItem>> {
        let (pid, i) = self.pad_ids.iter().find(|(_, i)| *i == idx).cloned()?;
        Some(Rc::new(self.pad_item(&pid, i)))
    }

    /// The rule item of the `idx`th track segment of the board (`DrcBoard::tracks`).
    pub fn segment_item_at(&self, idx: usize) -> Rc<RuleItem> {
        Rc::new(self.track_item(idx))
    }

    /// The rule item of the `idx`th via of the board (`DrcBoard::vias`).
    pub fn via_item_at(&self, idx: usize) -> Rc<RuleItem> {
        Rc::new(self.via_item(idx))
    }

    /// A via that is not on the board, as the rules see it -- `PCB_VIA_STITCH::buildPlacementCells`' probe, "it must carry the stitch net because clearance rules can key off
    /// the net class": the via item of a board with it on.
    pub fn probe_via(&self, net: &str, at: Point, diameter: Nm, drill: Nm, from_layer: &str, to_layer: &str) -> Rc<RuleItem> {
        let v = crate::board::DrcVia { id: "via_probe".into(), net: (!net.is_empty()).then(|| net.to_string()), at, drill, diameter, from_layer: from_layer.to_string(), to_layer: to_layer.to_string() };
        let copper = self.copper_layers();
        let (i0, i1) = (copper.iter().position(|l| *l == v.from_layer), copper.iter().position(|l| *l == v.to_layer));
        let (lo, hi) = match (i0, i1) {
            (Some(a), Some(b)) => (a.min(b), a.max(b)),
            _ => (0, copper.len().saturating_sub(1)),
        };
        let layers: Vec<String> = copper[lo..=hi].to_vec();
        let mut it = RuleItem::new(v.id.clone(), Class::Via);
        it.net = net.to_string();
        it.layer = layers.first().cloned();
        it.layers = layers.clone();
        it.shapes = vec![v.shape()];
        it.hole = Some(v.hole());
        it.has_hole = true;
        it.has_drilled_hole = true;
        it.plated = true;
        it.position = at;
        it.via_type = Some(if lo == 0 && hi + 1 == copper.len() { "Through" } else if lo == 0 || hi + 1 == copper.len() { "Blind" } else { "Buried" });
        it.drill = drill;
        it.flashed = layers;
        it.set("Diameter", Prop::Int(diameter));
        it.set("Hole", Prop::Int(drill));
        it.set("Net", Prop::Enum(net.to_string()));
        Rc::new(it)
    }

    /// The rule item of a zone of the board.
    pub fn zone_item_of(&self, z: &eda_model::ir::Zone) -> Rc<RuleItem> {
        Rc::new(self.zone_item(z))
    }

    /// The rule item of a graphic of the board.
    pub fn shape_item_of(&self, s: &eda_model::ir::Shape) -> Rc<RuleItem> {
        Rc::new(self.shape_item(s))
    }

    /// The rule item of a text on a copper layer that is not a board `Text` of the IR (an imported board text, a footprint's text): a `PCB_TEXT` of no net with its text,
    /// layer, size and pen.
    pub fn text_item_of(&self, id: &str, t: &eda_model::ir::FootprintText) -> Rc<RuleItem> {
        let mut it = RuleItem::new(id.to_string(), Class::Text);
        it.layers = vec![t.layer.clone()];
        it.layer = Some(t.layer.clone());
        it.position = t.at;
        it.text_height = t.size.1;
        it.text_thickness = t.thickness;
        it.set("Text", Prop::Str(t.text.clone()));
        it.set("Layer", Prop::Layer(t.layer.clone()));
        it.set("Text Height", Prop::Int(t.size.1));
        it.set("Text Thickness", Prop::Int(t.thickness));
        it.set("Position X", Prop::Int(t.at.x));
        it.set("Position Y", Prop::Int(t.at.y));
        it.description = format!("PCB text '{}' on {}", t.text, self.rules.layer_setup.display_name(&t.layer));
        Rc::new(it)
    }

    /// The rule item of a board text (`Text`) of the IR.
    pub fn board_text_item_of(&self, t: &eda_model::ir::Text) -> Rc<RuleItem> {
        Rc::new(self.text_item(t))
    }

    /// The engine the items are judged by (`DRC_ENGINE::EvalRules`).
    pub fn engine(&self) -> &RuleEngine {
        &self.engine
    }

    /// The board's copper layers, front to back.
    pub fn copper_layer_names(&self) -> Vec<String> {
        self.copper_layers()
    }

    pub fn footprint_stand_in(&self, fp: &Rc<RuleItem>) -> Rc<RuleItem> {
        let pads: Vec<&(String, usize)> = self.pad_ids.iter().filter(|(_, i)| self.board.pads[*i].footprint_ref == fp.id).collect();
        let Some((first_id, first_idx)) = pads.first().map(|p| (*p).clone()) else { return fp.clone() };
        let first = &self.board.pads[first_idx];
        // `PAD::SameLogicalPadAs`: the same number (a footprint's several pads with one number are one pin).
        if pads.iter().all(|(_, i)| self.board.pads[*i].number == first.number) {
            return Rc::new(self.pad_item(&first_id, first_idx));
        }
        fp.clone()
    }

    pub fn footprint_item(&self, reference: &str) -> Option<Rc<RuleItem>> {
        self.footprints.get(reference).cloned()
    }

    /// The enabled copper layers, front first.
    pub fn board_copper_layers(&self) -> Vec<String> {
        self.copper_layers()
    }

    /// `GetEffectiveNetClass()->GetHumanReadableName()` of a net.
    pub fn class_human_name(&self, net: &str) -> String {
        let e = self.rules.effective_class(net);
        human_readable_class_name(&e.constituents)
    }

    /// The name the board shows a layer by (`BOARD::GetLayerName`).
    pub fn layer_name(&self, layer: &str) -> String {
        self.rules.layer_setup.display_name(layer)
    }

    pub fn rules(&self) -> &BoardRules {
        self.rules
    }
}

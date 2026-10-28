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
use crate::pcbexpr::{Condition, Env};
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
        // The classes the project's assignment rules give the footprints, worked out over the footprints as they are (`GetDynamicComponentClassesForFootprint`).
        let dynamic = rb.dynamic_component_classes(&Rc::new(rb.env()));
        if !dynamic.is_empty() {
            for (reference, classes) in dynamic {
                let Some(item) = rb.footprints.get_mut(&reference) else { continue };
                let mut it = (**item).clone();
                if let Some(facts) = it.footprint.as_mut() {
                    let mut f = (**facts).clone();
                    f.classes = sorted_classes(f.classes.into_iter().chain(classes));
                    *facts = Rc::new(f);
                }
                *item = Rc::new(it);
            }
        }
        let env = Rc::new(rb.env());

        let nets: Vec<String> = model.nets.iter().map(|n| n.name.clone()).collect();
        let classes = class_views(rules, &nets);
        let keepouts = rb.keepout_rules();
        let (engine, error) = RuleEngine::with_rules_file(Settings::from_rules(rules), env, &classes, &keepouts, dru);
        rb.engine = engine;
        rb.compile_error = error;
        rb
    }

    /// The environment: layers by name, nets, classes, footprints, areas.
    fn env(&self) -> Env {
        let (rules, model) = (self.rules, self.model);
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
        env.footprints = self.footprints.values().cloned().collect();
        env.footprints.sort_by(|a, b| a.id.cmp(&b.id));
        env.areas = self.areas();
        env
    }

    /// `COMPONENT_CLASS_MANAGER::SyncDynamicComponentClassAssignments` and `GetDynamicComponentClassesForFootprint`: each assignment rule of the project
    /// (`BoardRules::component_classes`), and with "sheet component classes" on one rule per sheet a footprint is on, is compiled from its DRC-language text
    /// (`COMPONENT_CLASS_ASSIGNMENT_DATA::GetAssignmentInDRCLanguage`) and evaluated on each footprint, on the footprint's side (`COMPONENT_CLASS_ASSIGNMENT_RULE::Matches`:
    /// `EvaluateFor( aFootprint, nullptr, 0, aFootprint->GetSide(), nullptr )`). A rule that does not compile leaves the board with no assignment rule at all (`if( success )
    /// m_assignmentRules = std::move( rules )`). The reference of each footprint a rule matched, with the classes it is given.
    fn dynamic_component_classes(&self, env: &Rc<Env>) -> Vec<(String, Vec<String>)> {
        let settings = &self.rules.component_classes;
        // `CompileAssignmentRule` gives no rule for an empty class (`if( ruleSource.empty() ) return nullptr;`) and for a text the rules parser cannot read -- a `"`
        // in the class or a condition ends the `"%s"` it is written into (`(assign_component_class "%s" (condition "%s"))`) -- and one rule that does not
        // compile leaves the board with none (`success = false`, R6-D8)
        if settings.assignments.iter().any(|a| a.class.is_empty() || a.class.contains('"') || assignment_condition(a).contains('"')) {
            return Vec::new();
        }
        let mut rules: Vec<(String, String)> = settings.assignments.iter().map(|a| (a.class.clone(), assignment_condition(a))).collect();
        if settings.sheet_classes {
            let mut sheets: BTreeSet<String> = BTreeSet::new();
            for fp in self.footprints.values() {
                if let Some(f) = fp.footprint.as_ref() {
                    sheets.insert(f.sheet.clone());
                }
            }
            for sheet in sheets {
                // "Don't generate a class for empty sheets (e.g. manually placed footprints) or the root sheet"
                if sheet.is_empty() || sheet == "/" {
                    continue;
                }
                let name = sheet.replace(['"', '\''], "");
                rules.push((name.clone(), format!("( A.memberOfSheet('{}') )", name.trim())));
            }
        }
        if rules.is_empty() {
            return Vec::new();
        }
        let mut compiled = Vec::new();
        for (class, text) in rules {
            let mut condition = Condition::new(&text);
            if !text.is_empty() && !condition.compile(env) {
                return Vec::new();
            }
            compiled.push((class, condition));
        }
        let mut refs: Vec<&Rc<RuleItem>> = self.footprints.values().collect();
        refs.sort_by(|a, b| a.id.cmp(&b.id));
        refs.into_iter()
            .filter_map(|fp| {
                let side = fp.layer.as_deref();
                let classes: Vec<String> = compiled.iter().filter(|(_, c)| c.evaluate_for(env, Some(fp), None, 0, side, None)).map(|(class, _)| class.clone()).collect();
                (!classes.is_empty()).then(|| (fp.id.clone(), classes))
            })
            .collect()
    }

    // ------------------------------------------------------------------------------------------------------------------------------ environment

    fn areas(&self) -> Vec<Rc<AreaFacts>> {
        let mut out = Vec::new();
        if let Some(rt) = &self.design.routing {
            for z in &rt.zones {
                out.push(Rc::new(AreaFacts { id: z.id.clone(), name: z.name.clone(), layers: vec![z.layer.clone()], outline: z.outline.clone(), holes: z.holes.clone(), parent_footprint: z.parent_footprint.clone() }));
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
            // `A.Library_Link`: the link the derived board writes (`footprint_lib_id`: a package's installed KiCad footprint), the one Multichannel reads too (R6-D2),
            // without the importer's `#2` slot, which the writer takes off too (R7-D3)
            let fpid = part.map(|p| eda_model::footprint::without_import_slot(&eda_model::footprint::footprint_lib_id(p), &self.model.footprints)).unwrap_or_default();
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
            let extra = extras.get(fp.id.as_str()).copied();
            let link = self.model.symbol_links.get(&fp.id);
            // `GetSheetname`: the sheet the file named, else the one Update PCB from Schematic gives it (as `eda_kicad`'s writer writes `(sheetname ..)`).
            let sheet = extra.and_then(|e| e.sheetname.clone()).filter(|n| !n.is_empty()).or_else(|| link.map(|l| l.sheetname.clone())).unwrap_or_default();
            let facts = FootprintFacts {
                reference: fp.id.clone(),
                fpid: fpid.clone(),
                fields: footprint_fields(&fp.id, &value, self.design.footprint_edit(&fp.id), extra, link),
                flipped,
                courtyard_front: if flipped { Vec::new() } else { outlines.clone() },
                courtyard_back: if flipped { outlines.clone() } else { Vec::new() },
                sheet,
                classes: extra.map(|e| sorted_classes(e.component_classes.iter().cloned())).unwrap_or_default(),
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
            it.set("Position X", Prop::Int(fp.at.x.get()));
            it.set("Position Y", Prop::Int(fp.at.y.get()));
        // `FOOTPRINT::GetOrientationDegrees`: KiCad's angle, counter-clockwise in (-180, 180], not the model's clockwise `rot` (R6-D1).
        it.set("Orientation", Prop::Double(crate::board::kicad_footprint_orientation(self.design, fp) as f64 / 1000.0));
            it.set("Layer", Prop::Layer(side_layer.to_string()));
            it.set("Locked", Prop::Bool(self.own_lock(&fp.id)));
            if let Some(e) = extra {
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
        it.drill = p.drill_round.or_else(|| p.drill_slot.map(|(w, _)| w)).unwrap_or(Nm::ZERO);
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
        it.set("Size X", Prop::Int(p.size.0.get()));
        it.set("Size Y", Prop::Int(p.size.1.get()));
        it.set("Position X", Prop::Int(p.center.x.get()));
        it.set("Position Y", Prop::Int(p.center.y.get()));
        // `PAD::GetOrientationDegrees`: `orientation_mdeg` is already KiCad's, in [0, 360) (R6-D1).
        it.set("Orientation", Prop::Double(p.orientation_mdeg as f64 / 1000.0));
        it.set("Net", Prop::Enum(net.clone()));
        // (no "Locked": `pad.cpp` masks it, `propMgr.Mask( TYPE_HASH( PAD ), TYPE_HASH( BOARD_ITEM ), _HKI( "Locked" ) )`, R6-D7)
        it.set("Layer", Prop::Layer(layers.first().cloned().unwrap_or_default()));
        if let Some(d) = p.drill_round {
            it.set("Hole Size X", Prop::Int(d.get()));
            it.set("Hole Size Y", Prop::Int(d.get()));
        } else if let Some((w, h)) = p.drill_slot {
            it.set("Hole Size X", Prop::Int(w.get()));
            it.set("Hole Size Y", Prop::Int(h.get()));
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

    /// The item's own lock (`m_isLocked`): its id is in the design's locked ids. `FOOTPRINT::IsLocked` is this alone (`footprint.h`).
    fn own_lock(&self, id: &str) -> bool {
        self.design.drawings.as_ref().is_some_and(|d| d.locked_ids.iter().any(|l| l == id))
    }

    /// `BOARD_ITEM::IsLocked`, the "Locked" property of a track, via, zone, shape or text (R6-D7, R7-D1): its own lock, or its group's --
    ///
    /// ```text
    /// if( EDA_GROUP* group = GetParentGroup() ) { if( group->AsEdaItem()->IsLocked() ) return true; }
    /// .. return m_isLocked;
    /// ```
    ///
    /// and a group's `IsLocked` is the same, so a lock anywhere up the nesting counts.
    fn is_locked(&self, id: &str) -> bool {
        let groups = self.design.drawings.as_ref().map(|d| d.groups.as_slice()).unwrap_or_default();
        let mut at = id.to_string();
        // the nesting never loops (`Group::member_ids`), but a damaged file is not trusted: at most one step per group
        for _ in 0..=groups.len() {
            if self.own_lock(&at) {
                return true;
            }
            match groups.iter().find(|g| g.member_ids.iter().any(|m| *m == at)) {
                Some(g) => at = g.id.clone(),
                None => return false,
            }
        }
        false
    }

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
        it.set("Width", Prop::Int(s.width.get()));
        it.set("Layer", Prop::Layer(s.layer.clone()));
        it.set("Net", Prop::Enum(net.clone()));
        it.set("Start X", Prop::Int(s.a.x.get()));
        it.set("Start Y", Prop::Int(s.a.y.get()));
        it.set("End X", Prop::Int(s.b.x.get()));
        it.set("End Y", Prop::Int(s.b.y.get()));
        it.set("Locked", Prop::Bool(self.is_locked(s.id.rsplit_once('#').map_or(s.id.as_str(), |(base, _)| base))));
        let length = match s.arc_mid {
            Some(mid) => {
                let pts = crate::board::arc_polyline(s.a, mid, s.b, crate::board::ARC_HIGH_DEF);
                pts.windows(2).map(|w| (crate::kimath::dist(w[0], w[1])).as_f64()).sum::<f64>()
            }
            None => (crate::kimath::dist(s.a, s.b)).as_f64(),
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
        // `PCB_VIA::GetViaType` (R7-D2): a microvia is `VIATYPE::MICROVIA` (the file's `micro`, the router's Place Microvia: `ViaExtra::kind`), "Micro"
        // in `ENUM_MAP<VIATYPE>` -- `isMicroVia()` is true and `isBlindVia()` false for it (`IsBlindVia`: `BLIND || BURIED` only); the others by their span
        let micro = self.design.drawings.as_ref().is_some_and(|d| d.via_extras.iter().any(|e| e.id == v.id && e.kind == eda_model::via_extra::ViaKind::Micro));
        let kind = {
            let (s_out, e_out) = (outer(&v.from_layer), outer(&v.to_layer));
            if micro {
                "Micro"
            } else if s_out && e_out {
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
        it.set("Diameter", Prop::Int(v.diameter.get()));
        it.set("Hole", Prop::Int(v.drill.get()));
        it.set("Position X", Prop::Int(v.at.x.get()));
        it.set("Position Y", Prop::Int(v.at.y.get()));
        it.set("Layer Top", Prop::Layer(v.from_layer.clone()));
        it.set("Layer Bottom", Prop::Layer(v.to_layer.clone()));
        it.set("Net", Prop::Enum(net.clone()));
        it.set("Locked", Prop::Bool(self.is_locked(&v.id)));
        let fmt = match kind {
            "Blind" => "Blind via",
            "Buried" => "Buried via",
            "Micro" => "Micro via",
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
        it.holes = z.holes.clone();
        it.shapes = vec![Shape::Polygon { pts: z.outline.clone() }];
        it.rule_area = z.is_rule_area;
        it.keepout = z.is_rule_area && (z.keepout_tracks || z.keepout_vias || z.keepout_pads || z.keepout_copper_pour || z.keepout_footprints);
        it.teardrop = z.teardrop;
        it.position = z.outline.first().copied().unwrap_or(Point { x: Nm::ZERO, y: Nm::ZERO });
        it.set("Name", Prop::Str(z.name.clone()));
        it.set("Layer", Prop::Layer(z.layer.clone()));
        it.set("Priority", Prop::Int(z.priority as i64));
        it.set("Locked", Prop::Bool(self.is_locked(&z.id)));
        if z.is_rule_area {
            it.description = if z.name.is_empty() { format!("Rule Area on {layer_name}") } else { format!("Rule area '{}' on {layer_name}", z.name) };
            return it;
        }
        let net = z.net.clone();
        it.net = net.clone();
        it.set("Net", Prop::Enum(net.clone()));
        it.set("Clearance", Prop::Int(z.clearance.get()));
        it.set("Minimum Width", Prop::Int(z.min_thickness.get()));
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
        it.position = s.points().first().copied().unwrap_or(Point { x: Nm::ZERO, y: Nm::ZERO });
        it.set("Layer", Prop::Layer(layer.clone()));
        it.set("Line Width", Prop::Int(s.stroke_width().get()));
        it.set("Locked", Prop::Bool(self.is_locked(s.id())));
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
        it.set("Text Height", Prop::Int(t.size_nm.get()));
        it.set("Text Thickness", Prop::Int(t.stroke_width.get()));
        it.set("Position X", Prop::Int(t.at.x.get()));
        it.set("Position Y", Prop::Int(t.at.y.get()));
        it.set("Locked", Prop::Bool(self.is_locked(&t.id)));
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
                        crate::kimath::Shape::Stadium { a: s.a, b: s.b, r: Nm::ZERO }.gap_to(&Shape::Circle { c: p, r: Nm::ZERO }) as i64
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
        it.set("Diameter", Prop::Int(diameter.get()));
        it.set("Hole", Prop::Int(drill.get()));
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
        it.set("Text Height", Prop::Int(t.size.1.get()));
        it.set("Text Thickness", Prop::Int(t.thickness.get()));
        it.set("Position X", Prop::Int(t.at.x.get()));
        it.set("Position Y", Prop::Int(t.at.y.get()));
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

    /// `BOARD_INSPECTION_TOOL::reportClearance`'s `checkFootprint`: a footprint whose pads are all one logical pad stands for that pad -- the **last** of
    /// them (`if( !foundPad || pad->SameLogicalPadAs( foundPad ) ) foundPad = pad; else return footprint;`) -- and any other footprint for itself.
    /// `PAD::SameLogicalPadAs` needs a number (`!m_number.IsEmpty() && m_number == aOther->m_number`): two unnumbered pads (a pair of mounting holes)
    /// keep the footprint; a single pad stands in whatever its number.
    pub fn footprint_stand_in(&self, fp: &Rc<RuleItem>) -> Rc<RuleItem> {
        let mut found: Option<&(String, usize)> = None;
        for p in self.pad_ids.iter().filter(|(_, i)| self.board.pads[*i].footprint_ref == fp.id) {
            let same = |f: &(String, usize)| {
                let (a, b) = (&self.board.pads[p.1].number, &self.board.pads[f.1].number);
                !a.is_empty() && a == b
            };
            match found {
                Some(f) if !same(f) => return fp.clone(),
                _ => found = Some(p),
            }
        }
        match found {
            Some((id, idx)) => Rc::new(self.pad_item(id, *idx)),
            None => fp.clone(),
        }
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

/// `COMPONENT_CLASS_MANAGER::sortClassNames`: each name once, in order (`wxString::Cmp`).
fn sorted_classes(names: impl Iterator<Item = String>) -> Vec<String> {
    let set: BTreeSet<String> = names.collect();
    set.into_iter().collect()
}

/// The condition of an assignment rule as `COMPONENT_CLASS_ASSIGNMENT_DATA::GetAssignmentInDRCLanguage` writes it: each condition that says something, joined by
/// `&&` (all) or `||` (any); empty for a rule that always applies.
fn assignment_condition(a: &eda_model::component_classes::ClassAssignment) -> String {
    use eda_model::component_classes::ConditionKind;
    let mut exprs: Vec<String> = Vec::new();
    for c in &a.conditions {
        let primary = c.primary.trim();
        let expr = match c.kind {
            ConditionKind::Reference => {
                let refs: Vec<String> = c.primary.trim().split(',').map(|r| format!("A.Reference == '{r}'")).collect();
                if primary.is_empty() && refs.len() == 1 {
                    // (`wxSplit` of an empty text gives no element)
                    String::new()
                } else {
                    format!("( {} )", refs.join(" || "))
                }
            }
            ConditionKind::Footprint if !primary.is_empty() => format!("( A.Library_Link == '{primary}' )"),
            ConditionKind::Side if c.primary != "Any" => format!("( A.Layer == '{}' )", if c.primary == "Front" { "F.Cu" } else { "B.Cu" }),
            ConditionKind::Rotation if !primary.is_empty() && primary != "Any" && primary.parse::<i32>().is_ok() => format!("( A.Orientation == {primary} deg )"),
            ConditionKind::FootprintField if !primary.is_empty() && !c.secondary.trim().is_empty() => format!("( A.getField('{primary}') == '{}' )", c.secondary.trim()),
            ConditionKind::Custom if !primary.is_empty() => format!("( {primary} )"),
            ConditionKind::SheetName if !primary.is_empty() => format!("( A.memberOfSheet('{primary}') )"),
            _ => String::new(),
        };
        if !expr.is_empty() {
            exprs.push(expr);
        }
    }
    exprs.join(if a.all { " && " } else { " || " })
}

/// `FOOTPRINT::GetField( name )`'s fields, in `m_fields` order: the Reference, the Value, then the Datasheet, the Description and the user fields. A footprint read
/// from a file has the ones the file gave it (its `Datasheet` and `Description` properties kept among its tokens, its user fields as Footprint Properties' fields); one
/// made here has its own user fields and the symbol's other fields, as Update PCB from Schematic gives them (`updateFootprintParameters`; `eda_kicad`'s writer writes
/// them so). There is no "Footprint" field: the library link is `A.Library_Link`.
fn footprint_fields(
    reference: &str,
    value: &str,
    edit: Option<&eda_model::fp_edit::FootprintEdit>,
    extra: Option<&eda_model::ir::FootprintExtra>,
    link: Option<&eda_model::SymbolLink>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![("Reference".into(), reference.to_string()), ("Value".into(), value.to_string())];
    if let Some(e) = extra {
        out.extend(e.tokens.iter().filter_map(|t| property_token(t)));
    }
    let own: &[eda_model::fp_edit::UserField] = edit.map(|e| e.fields.as_slice()).unwrap_or_default();
    if extra.is_none() {
        if let Some(link) = link {
            out.extend(link.fields.iter().filter(|(name, _)| !own.iter().any(|f| f.name == *name)).cloned());
        }
    }
    out.extend(own.iter().map(|f| (f.name.clone(), f.text.clone())));
    out
}

/// The name and text of a `(property "Name" "Text" ..)` token as a file wrote it; `None` for any other token.
fn property_token(token: &str) -> Option<(String, String)> {
    let rest = token.trim_start().strip_prefix("(property")?;
    let (name, rest) = quoted(rest)?;
    let (text, _) = quoted(rest)?;
    Some((name, text))
}

/// The first quoted string of `s` (KiCad's escapes taken back) and what follows it.
fn quoted(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, &s[i + 1..])),
            '\\' => match chars.next() {
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                Some((_, other)) => out.push(other),
                None => return None,
            },
            _ => out.push(c),
        }
    }
    None
}

#[cfg(test)]
mod footprint_facts_tests {
    use super::*;
    use crate::pcbexpr::Condition;
    use eda_model::component_classes::{ClassAssignment, ClassCondition, ConditionKind};
    use eda_model::ir::{FootprintInstance, PlacementSection, Provenance};
    use eda_model::units::mm;
    use eda_model::{Footprint, Net, Pad, Part, Pin, PinKind, SymbolLink};

    /// R5 on sheet `/HV/` with a user field `Voltage=400V` (the symbol's, as Update PCB from Schematic gives it), and R6 on the root sheet; one pad each on net HV.
    fn hv_board(edit: impl FnOnce(&mut Design, &mut ConstraintModel)) -> (Design, ConstraintModel) {
        let pad = Pad { number: "1".into(), at: (Nm::ZERO, Nm::ZERO), size: (mm(1.0), mm(1.0)), shape: eda_model::PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() };
        let part = |r: &str| Part { reference: r.into(), mpn: None, lcsc: None, value: Some("10k".into()), package: Some("P1".into()), footprint: Some("P1".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }], body_nm: None, symbol: None, datasheet: None, edge: None };
        let mut model = ConstraintModel {
            parts: vec![part("R5"), part("R6")],
            nets: vec![Net { name: "HV".into(), pins: vec!["R5.1".into(), "R6.1".into()] }],
            footprints: vec![Footprint { name: "P1".into(), pads: vec![pad], courtyard: Some((mm(2.0), mm(2.0))), ..Default::default() }],
            board: BoardRules { layers: vec!["F.Cu".into(), "B.Cu".into()], ..Default::default() },
            ..Default::default()
        };
        model.symbol_links.insert("R5".into(), SymbolLink { path: "/a/b".into(), sheetname: "/HV/".into(), fields: vec![("Datasheet".into(), "~".into()), ("Voltage".into(), "400V".into())], ..Default::default() });
        model.symbol_links.insert("R6".into(), SymbolLink { path: "/c".into(), sheetname: "/".into(), ..Default::default() });
        let fp = |id: &str, x: f64| FootprintInstance { id: id.into(), at: Point { x: mm(x), y: mm(10.0) }, rot: 0, side: Side::Top, label: Default::default() };
        let mut design = Design {
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![fp("R5", 10.0), fp("R6", 20.0)], modules: Vec::new() }),
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        };
        edit(&mut design, &mut model);
        (design, model)
    }

    fn holds(rb: &RuleBoard, expr: &str, item: &Rc<RuleItem>) -> bool {
        let env = rb.engine().env.clone();
        let mut c = Condition::new(expr);
        assert!(c.compile(&env), "{expr}: {:?}", c.compile_errors());
        c.evaluate_for(&env, Some(item), None, 0, Some("F.Cu"), None)
    }

    /// R5-D1: the footprint's sheet (`GetSheetname`), its fields (`GetField`: no "Footprint" field) and its sheet in `memberOfSheet` for a pad of it.
    #[test]
    fn rule_board_footprint_has_its_sheet_and_fields() {
        let (design, model) = hv_board(|_, _| {});
        let board = crate::board::build(&design, &model);
        let rb = RuleBoard::new(&design, &model, &board, Units::Mm, None);
        let r5 = rb.footprint_item("R5").unwrap();
        assert_eq!(r5.footprint.as_ref().unwrap().sheet, "/HV/");
        assert!(holds(&rb, "A.getField('Voltage') == '400V'", &r5));
        assert!(holds(&rb, "A.getField('Footprint') == ''", &r5), "KiCad's footprints have no Footprint field");
        assert!(holds(&rb, "A.getField('Value') == '10k'", &r5));
        let pads: Vec<Rc<RuleItem>> = (0..board.pads.len()).filter_map(|i| rb.pad_item_at(i)).collect();
        let pad_of = |r: &str| pads.iter().find(|p| p.footprint.as_ref().is_some_and(|f| f.reference == r)).unwrap().clone();
        assert!(holds(&rb, "A.memberOfSheet('/HV/')", &pad_of("R5")));
        assert!(!holds(&rb, "A.memberOfSheet('/HV/')", &pad_of("R6")));
        assert!(holds(&rb, "A.memberOfSheet('/')", &pad_of("R6")));
    }

    /// R6-D1: `A.Orientation` is KiCad's (`FOOTPRINT::GetOrientationDegrees`, in (-180, 180]; `PAD::GetOrientationDegrees`, in [0, 360)), not the model's
    /// clockwise angle: R5 at the model's 270 is KiCad's `(at x y 90)`, and its pad (at 0 in the footprint) is at 90 too. R6 on the back, made here, is
    /// written half a turn round (`FOOTPRINT::Flip`), at 180, and so is its pad. A component class with a Rotation condition goes to the footprint KiCad has
    /// at that angle (`getRotationExpr`: `( A.Orientation == 90 deg )`).
    #[test]
    fn rule_board_orientation_is_kicads() {
        let (design, model) = hv_board(|d, m| {
            let fps = &mut d.placement.as_mut().unwrap().footprints;
            fps[0].rot = 270_000;
            fps[1].side = Side::Bottom;
            m.board.component_classes.assignments.push(ClassAssignment { class: "TURNED".into(), all: true, conditions: vec![ClassCondition { kind: ConditionKind::Rotation, primary: "90".into(), secondary: String::new() }] });
        });
        let board = crate::board::build(&design, &model);
        let rb = RuleBoard::new(&design, &model, &board, Units::Mm, None);
        let (r5, r6) = (rb.footprint_item("R5").unwrap(), rb.footprint_item("R6").unwrap());
        assert!(holds(&rb, "A.Orientation == 90 deg", &r5));
        assert!(holds(&rb, "A.Orientation == 180 deg", &r6));
        assert_eq!(r5.footprint.as_ref().unwrap().classes, ["TURNED"]);
        assert!(r6.footprint.as_ref().unwrap().classes.is_empty());
        let pads: Vec<Rc<RuleItem>> = (0..board.pads.len()).filter_map(|i| rb.pad_item_at(i)).collect();
        let pad_of = |r: &str| pads.iter().find(|p| p.footprint.as_ref().is_some_and(|f| f.reference == r)).unwrap().clone();
        assert!(holds(&rb, "A.Orientation == 90 deg", &pad_of("R5")));
        assert!(holds(&rb, "A.Orientation == 180 deg", &pad_of("R6")));
    }

    /// R6-D7: `A.Locked` is `IsLocked()` -- true for a locked footprint (the design's locked ids), false for another; a pad has no "Locked" (`pad.cpp`
    /// masks it), so a rule on it never takes a pad.
    #[test]
    fn a_locked_footprint_is_locked_on_the_rule_board() {
        let (design, model) = hv_board(|d, _| d.drawings = Some(eda_model::ir::DrawingsSection { locked_ids: vec!["R5".into()], ..Default::default() }));
        let board = crate::board::build(&design, &model);
        let rb = RuleBoard::new(&design, &model, &board, Units::Mm, None);
        assert!(holds(&rb, "A.Locked", &rb.footprint_item("R5").unwrap()));
        assert!(!holds(&rb, "A.Locked", &rb.footprint_item("R6").unwrap()));
        let pad = (0..board.pads.len()).filter_map(|i| rb.pad_item_at(i)).find(|p| p.footprint.as_ref().is_some_and(|f| f.reference == "R5")).unwrap();
        assert!(!holds(&rb, "A.Locked", &pad), "a pad has no Locked property");
    }

    /// R7-D1 (`BOARD_ITEM::IsLocked`: `if( EDA_GROUP* group = GetParentGroup() ) { if( group->AsEdaItem()->IsLocked() ) return true; }`, and the "Locked"
    /// property registered on `BOARD_ITEM`): a track in a locked group (nested one deep) reads as locked, and a locked zone, shape and text do too; a
    /// footprint in a locked group does not (`FOOTPRINT::IsLocked` is its own flag).
    #[test]
    fn a_member_of_a_locked_group_and_a_locked_zone_are_locked() {
        let (design, model) = hv_board(|d, _| {
            let track = |id: &str, y: f64| eda_model::ir::Track { id: id.into(), net: "HV".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: mm(1.0), y: mm(y) }, Point { x: mm(5.0), y: mm(y) }], arc_mid_offset: None };
            let zone: eda_model::ir::Zone = serde_json::from_str(r#"{ "id": "z1", "net": "HV", "layer": "F.Cu", "outline": [{ "x": 0, "y": 0 }, { "x": 3000000, "y": 0 }, { "x": 3000000, "y": 3000000 }] }"#).unwrap();
            d.routing = Some(eda_model::ir::RoutingSection { tracks: vec![track("t1", 1.0), track("t2", 2.0)], vias: vec![], zones: vec![zone], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: vec![], teardrop_settings: Default::default() });
            let group = |id: &str, members: &[&str]| eda_model::ir::Group { id: id.into(), name: String::new(), member_ids: members.iter().map(|m| m.to_string()).collect(), design_block: String::new() };
            d.drawings = Some(eda_model::ir::DrawingsSection { groups: vec![group("G", &["inner", "R6"]), group("inner", &["t1"])], locked_ids: vec!["G".into(), "z1".into()], ..Default::default() });
        });
        let board = crate::board::build(&design, &model);
        let rb = RuleBoard::new(&design, &model, &board, Units::Mm, None);
        let seg = |id: &str| (0..board.tracks.len()).map(|i| rb.segment_item_at(i)).find(|t| t.id.split('#').next() == Some(id)).unwrap();
        assert!(holds(&rb, "A.Locked", &seg("t1")), "a member of a group inside a locked group");
        assert!(!holds(&rb, "A.Locked", &seg("t2")));
        let zones = &design.routing.as_ref().unwrap().zones;
        assert!(holds(&rb, "A.Locked", &rb.zone_item_of(&zones[0])), "a locked zone");
        assert!(!holds(&rb, "A.Locked", &rb.footprint_item("R6").unwrap()), "FOOTPRINT::IsLocked is its own flag");
    }

    /// R7-D2 (`PCB_VIA::IsMicroVia`: `m_viaType == VIATYPE::MICROVIA`; `IsBlindVia`: `BLIND || BURIED` only; `ENUM_MAP<VIATYPE>`'s "Micro"): a via F.Cu - In1.Cu
    /// whose `ViaExtra::kind` is `Micro` is a microvia on the rule board, not a blind via; the same via with no extra is blind.
    #[test]
    fn a_micro_via_is_micro_on_the_rule_board() {
        let (design, model) = hv_board(|d, m| {
            m.board.layers = vec!["F.Cu".into(), "In1.Cu".into(), "In2.Cu".into(), "B.Cu".into()];
            let via = |id: &str| eda_model::ir::Via { id: id.into(), net: "HV".into(), at: Point { x: mm(1.0), y: mm(1.0) }, drill: mm(0.1), diameter: mm(0.3), from_layer: "F.Cu".into(), to_layer: "In1.Cu".into() };
            d.routing = Some(eda_model::ir::RoutingSection { tracks: vec![], vias: vec![via("uv"), via("bv")], zones: vec![], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: vec![], teardrop_settings: Default::default() });
            d.drawings = Some(eda_model::ir::DrawingsSection { via_extras: vec![eda_model::via_extra::ViaExtra { id: "uv".into(), kind: eda_model::via_extra::ViaKind::Micro, tokens: vec![] }], ..Default::default() });
        });
        let board = crate::board::build(&design, &model);
        let rb = RuleBoard::new(&design, &model, &board, Units::Mm, None);
        let via = |id: &str| (0..board.vias.len()).map(|i| rb.via_item_at(i)).find(|v| v.id == id).unwrap();
        let uv = via("uv");
        assert!(holds(&rb, "A.isMicroVia()", &uv));
        assert!(!holds(&rb, "A.isBlindVia()", &uv));
        assert!(holds(&rb, "A.Via_Type == 'Micro'", &uv));
        assert!(uv.description.starts_with("Micro via"), "{}", uv.description);
        let bv = via("bv");
        assert!(holds(&rb, "A.isBlindVia()", &bv) && !holds(&rb, "A.isMicroVia()", &bv));
    }

    /// R7-D3 (`FOOTPRINT::GetFPIDAsString`, `testFootprintSelector`): a part whose footprint is the importer's `Resistor_SMD:R_0603#2` (a second variant of one
    /// library footprint) has the library link the board writes, `Resistor_SMD:R_0603`, in `A.Library_Link` and in `memberOfFootprint('lib:name')`.
    #[test]
    fn library_link_has_no_import_slot() {
        let (design, model) = hv_board(|_, m| {
            let mut variant = m.footprints[0].clone();
            m.footprints[0].name = "Resistor_SMD:R_0603".into();
            variant.name = "Resistor_SMD:R_0603#2".into();
            m.footprints.push(variant);
            m.parts[0].footprint = Some("Resistor_SMD:R_0603#2".into());
            m.parts[0].package = None;
        });
        let board = crate::board::build(&design, &model);
        let rb = RuleBoard::new(&design, &model, &board, Units::Mm, None);
        let fp = rb.footprint_item("R5").unwrap();
        assert!(holds(&rb, "A.Library_Link == 'Resistor_SMD:R_0603'", &fp));
        let pad = (0..board.pads.len()).filter_map(|i| rb.pad_item_at(i)).find(|p| p.footprint.as_ref().is_some_and(|f| f.reference == "R5")).unwrap();
        assert!(holds(&rb, "A.memberOfFootprint('Resistor_SMD:R_0603')", &pad));
    }

    /// R6-D8 (`SyncDynamicComponentClassAssignments`: `if( success ) m_assignmentRules = std::move( rules );`): one row with no class, or a `"` in a value, gives
    /// `CompileAssignmentRule` no rule, and the board has no dynamic class at all -- not even the rows that are good.
    #[test]
    fn a_class_rule_kicad_cannot_compile_leaves_the_board_with_none() {
        for bad in [ClassAssignment { class: String::new(), all: true, conditions: vec![] }, ClassAssignment { class: "Q".into(), all: true, conditions: vec![ClassCondition { kind: ConditionKind::FootprintField, primary: "Voltage".into(), secondary: "4\"00V".into() }] }] {
            let (design, model) = hv_board(|_, m| {
                m.board.component_classes.assignments.push(bad.clone());
                m.board.component_classes.assignments.push(ClassAssignment { class: "PWR".into(), all: true, conditions: vec![] });
            });
            let board = crate::board::build(&design, &model);
            let rb = RuleBoard::new(&design, &model, &board, Units::Mm, None);
            assert!(rb.footprint_item("R5").unwrap().footprint.as_ref().unwrap().classes.is_empty(), "{bad:?}");
        }
    }

    /// R5-D1: the classes a file gives a footprint, the project's assignment rules (`GetAssignmentInDRCLanguage`, evaluated on the footprint) and the sheet classes,
    /// as `hasComponentClass` and `A.ComponentClass` read them, for a footprint and for a pad of it.
    #[test]
    fn component_classes_come_from_the_file_the_rules_and_the_sheets() {
        let (design, model) = hv_board(|d, m| {
            d.drawings = Some(eda_model::ir::DrawingsSection { footprint_extras: vec![eda_model::ir::FootprintExtra { id: "R6".into(), component_classes: vec!["STATIC".into()], ..Default::default() }], ..Default::default() });
            m.board.component_classes.assignments.push(ClassAssignment { class: "PWR".into(), all: true, conditions: vec![ClassCondition { kind: ConditionKind::FootprintField, primary: "Voltage".into(), secondary: "400V".into() }] });
            m.board.component_classes.assignments.push(ClassAssignment { class: "SIXES".into(), all: false, conditions: vec![ClassCondition { kind: ConditionKind::Reference, primary: "R6,R7".into(), secondary: String::new() }] });
            m.board.component_classes.sheet_classes = true;
        });
        let board = crate::board::build(&design, &model);
        let rb = RuleBoard::new(&design, &model, &board, Units::Mm, None);
        let (r5, r6) = (rb.footprint_item("R5").unwrap(), rb.footprint_item("R6").unwrap());
        assert_eq!(r5.footprint.as_ref().unwrap().classes, ["/HV/", "PWR"]);
        assert_eq!(r6.footprint.as_ref().unwrap().classes, ["SIXES", "STATIC"]);
        assert!(holds(&rb, "A.hasComponentClass('PWR')", &r5));
        assert!(!holds(&rb, "A.hasComponentClass('STATIC')", &r5));
        assert!(holds(&rb, "A.ComponentClass == 'pwr'", &r5), "a constituent, in any case");
        assert!(holds(&rb, "A.ComponentClass == '/HV/,PWR'", &r5), "the full name");
        assert!(holds(&rb, "A.ComponentClass != 'STATIC'", &r5));
        let pad = (0..board.pads.len()).filter_map(|i| rb.pad_item_at(i)).find(|p| p.footprint.as_ref().is_some_and(|f| f.reference == "R6")).unwrap();
        assert!(holds(&rb, "A.hasComponentClass('STATIC')", &pad), "a pad answers for its footprint");
    }

    /// R5-D5 (`checkFootprint`, `PAD::SameLogicalPadAs`): two unnumbered pads keep the footprint; pads that share a number stand in by the last of them.
    #[test]
    fn the_stand_in_of_a_footprint_is_its_last_logical_pad_and_unnumbered_pads_are_not_one() {
        let pads = |numbers: &[&str]| -> (Design, ConstraintModel) {
            let numbers: Vec<String> = numbers.iter().map(|n| n.to_string()).collect();
            hv_board(move |_, m| {
                let template = m.footprints[0].pads[0].clone();
                m.footprints[0].pads = numbers.iter().enumerate().map(|(k, n)| Pad { number: n.clone(), at: (mm(2.0 * k as f64), Nm::ZERO), kind: if n.is_empty() { PadKind::NonPlatedHole } else { PadKind::Smd }, drill: n.is_empty().then(|| mm(1.0)), ..template.clone() }).collect();
            })
        };
        let (design, model) = pads(&["", ""]);
        let board = crate::board::build(&design, &model);
        let rb = RuleBoard::new(&design, &model, &board, Units::Mm, None);
        let fp = rb.footprint_item("R5").unwrap();
        assert_eq!(rb.footprint_stand_in(&fp).class, Class::Footprint, "two mounting holes are two pads");
        let (design, model) = pads(&["1", "1"]);
        let board = crate::board::build(&design, &model);
        let rb = RuleBoard::new(&design, &model, &board, Units::Mm, None);
        let fp = rb.footprint_item("R5").unwrap();
        let stand_in = rb.footprint_stand_in(&fp);
        assert_eq!(stand_in.class, Class::Pad);
        assert_eq!(stand_in.position.x, mm(12.0), "the last pad, at x = 10 + 2 mm");
    }
}

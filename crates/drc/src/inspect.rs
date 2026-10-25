//! The Clearance and Constraints Resolution reports: `BOARD_INSPECTION_TOOL::reportClearance` and `InspectConstraints` (`pcbnew/tools/board_inspection_tool.cpp`) at
//! KiCad 8303b2ad, over the [`RuleEngine`] and the items of a [`RuleBoard`]. Each report is a book of pages (`DIALOG_BOOK_REPORTER`), each page the lines of a
//! `WX_HTML_REPORT_BOX` in KiCad's own wording; the resolved constraint of every page comes with them, as numbers, for whoever wants the value rather than the text.
//!
//! This inspects the rules; it checks nothing. kicad-cli stays the only DRC engine, and a rule it enforces is a rule these reports show the winner of.

use crate::constraints::{escape_html, string_from_value, CType, Considered, Report, Resolved, RuleEngine, Units, ZoneConnection};
use crate::rule_board::RuleBoard;
use crate::rule_item::{Class, RuleItem};
use eda_model::layers::pcb_layer_id;
use serde_json::{json, Value};
use std::rc::Rc;

/// What a page settled on: the constraint, in nanometres.
#[derive(Debug, Clone)]
pub struct Resolution {
    pub constraint: CType,
    pub layer: Option<String>,
    pub min: Option<i64>,
    pub opt: Option<i64>,
    pub max: Option<i64>,
    /// `GetName()` of the winner: `rule 'name'`, an implicit rule's name, `net tie`, `board minimum` ...; empty when no rule gave a constraint.
    pub rule: String,
    pub implicit: bool,
    /// `constraint.IsNull()`.
    pub null: bool,
    pub zone_connection: Option<&'static str>,
    pub disallowed: bool,
    pub considered: Vec<Considered>,
}

/// A page of the report: its title and its lines, or one set of lines per copper layer (the "Clearance" page of items that share several layers).
#[derive(Debug, Clone, Default)]
pub struct Page {
    pub title: String,
    pub lines: Vec<String>,
    /// `(layer in the board file, the name the board shows it by, lines)`.
    pub per_layer: Vec<(String, String, Vec<String>)>,
    pub resolutions: Vec<Resolution>,
}

#[derive(Debug, Clone, Default)]
pub struct Inspection {
    pub title: String,
    pub pages: Vec<Page>,
    /// The rules file did not compile: the report is built on the implicit rules alone (`reportCompileError`).
    pub compile_error: Option<String>,
    /// A message for the info bar, when no report is made (`ShowInfoBarError`).
    pub error: Option<String>,
}

impl Inspection {
    pub fn to_json(&self) -> Value {
        let res = |r: &Resolution| {
            json!({
                "constraint": r.constraint.token(),
                "layer": r.layer,
                "min": r.min, "opt": r.opt, "max": r.max,
                "rule": r.rule, "implicit": r.implicit, "null": r.null,
                "zone_connection": r.zone_connection, "disallowed": r.disallowed,
                "considered": r.considered.iter().map(|c| json!({
                    "rule": c.rule, "implicit": c.implicit, "constraint": c.constraint.token(), "condition": c.condition, "applied": c.applied, "note": c.note,
                })).collect::<Vec<_>>(),
            })
        };
        json!({
            "ok": self.error.is_none(),
            "error": self.error,
            "title": self.title,
            "compile_error": self.compile_error,
            "pages": self.pages.iter().map(|p| json!({
                "title": p.title,
                "lines": p.lines,
                "per_layer": p.per_layer.iter().map(|(l, n, lines)| json!({"layer": l, "name": n, "lines": lines})).collect::<Vec<_>>(),
                "resolutions": p.resolutions.iter().map(res).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

fn layer_key(l: &str) -> u32 {
    pcb_layer_id(l).unwrap_or(u32::MAX)
}

fn sorted_layers(mut v: Vec<String>) -> Vec<String> {
    v.sort_by_key(|l| layer_key(l));
    v.dedup();
    v
}

fn is_copper(l: &str) -> bool {
    l.ends_with(".Cu")
}

/// The lines of one page and what it resolved, written the way the C++ writes to its `REPORTER`.
struct Rep {
    lines: Vec<String>,
}

impl Rep {
    fn new() -> Rep {
        Rep { lines: Vec::new() }
    }

    fn line(&mut self, s: impl Into<String>) {
        self.lines.push(s.into());
    }
}

struct Ctx<'a, 'b> {
    rb: &'b RuleBoard<'a>,
    units: Units,
    active: String,
    /// The `compileError` of `makeDRCEngine`.
    compile_error: bool,
}

impl<'a, 'b> Ctx<'a, 'b> {
    fn engine(&self) -> &RuleEngine {
        &self.rb.engine
    }

    fn sfv(&self, nm: i64) -> String {
        string_from_value(nm as f64, self.units, true)
    }

    /// `getItemDescription`: the description, and the net class of a connected item that is not an NPTH pad.
    fn describe(&self, item: &RuleItem) -> String {
        let mut msg = item.description.clone();
        if item.is_connected() && !(item.class == Class::Pad && item.pad_attribute == "NPTH") {
            msg += &format!(" [netclass {}]", self.rb.class_human_name(&item.net));
        }
        msg
    }

    fn header1(&self, title: &str, a: &RuleItem, r: &mut Rep) {
        r.line(format!("<h7>{}</h7>", escape_html(title)));
        r.line(format!("<ul><li>{}</li></ul>", escape_html(&self.describe(a))));
    }

    fn header2(&self, title: &str, a: &RuleItem, b: &RuleItem, r: &mut Rep) {
        r.line(format!("<h7>{}</h7>", escape_html(title)));
        r.line(format!("<ul><li>{}</li><li>{}</li></ul>", escape_html(&self.describe(a)), escape_html(&self.describe(b))));
    }

    fn header3(&self, title: &str, a: &RuleItem, b: &RuleItem, layer: &str, r: &mut Rep) {
        let layer_str = format!("Layer {}", self.rb.layer_name(layer));
        r.line(format!("<h7>{}</h7>", escape_html(title)));
        r.line(format!("<ul><li>{}</li><li>{}</li><li>{}</li></ul>", escape_html(&layer_str), escape_html(&self.describe(a)), escape_html(&self.describe(b))));
    }

    /// `reportCompileError`.
    fn compile_notice(&self, r: &mut Rep) {
        r.line("");
        r.line("Report incomplete: could not compile custom design rules.&nbsp;&nbsp;<a href='$CUSTOM_RULES'>Show design rules.</a>");
    }

    /// One `EvalRules` with its lines appended to the page.
    fn eval(&self, t: CType, a: Option<&Rc<RuleItem>>, b: Option<&Rc<RuleItem>>, layer: Option<&str>, r: &mut Rep) -> (Resolved, Report) {
        let mut report = Report::default();
        let c = self.engine().eval_rules(t, a, b, layer, Some(&mut report));
        r.lines.extend(report.lines.iter().cloned());
        (c, report)
    }

    fn resolution(&self, c: &Resolved, report: &Report, layer: Option<&str>) -> Resolution {
        Resolution {
            constraint: c.ctype,
            layer: layer.map(String::from),
            min: c.value.min,
            opt: c.value.opt,
            max: c.value.max,
            rule: self.engine().constraint_name(c),
            implicit: c.parent.is_some_and(|i| self.engine().rules[i].is_implicit()),
            null: c.is_null(),
            zone_connection: (c.ctype == CType::ZoneConnection).then(|| c.zone_connection.print()),
            disallowed: c.disallow != 0,
            considered: report.considered.clone(),
        }
    }
}

fn report_min(c: &Resolved, units: Units) -> String {
    match c.value.min {
        Some(v) => string_from_value(v as f64, units, true),
        None => "<i>undefined</i>".to_string(),
    }
}

fn report_opt(c: &Resolved, units: Units) -> String {
    match c.value.opt {
        Some(v) => string_from_value(v as f64, units, true),
        None => "<i>undefined</i>".to_string(),
    }
}

fn report_max(c: &Resolved, units: Units) -> String {
    match c.value.max {
        Some(v) => string_from_value(v as f64, units, true),
        None => "<i>undefined</i>".to_string(),
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------ clearance

/// `BOARD_INSPECTION_TOOL::reportClearance`.
pub fn inspect_clearance(rb: &RuleBoard, item_a: Rc<RuleItem>, item_b: Rc<RuleItem>, active_layer: Option<&str>, units: Units) -> Inspection {
    let mut out = Inspection { title: "Clearance Report".to_string(), compile_error: rb.compile_error.as_ref().map(|e| e.to_string()), ..Default::default() };
    let cx = Ctx { rb, units, active: active_layer.unwrap_or("F.Cu").to_string(), compile_error: rb.compile_error.is_some() };
    let (mut a, mut b) = (item_a, item_b);
    if a.class == Class::Footprint {
        a = rb.footprint_stand_in(&a);
    }
    if b.class == Class::Footprint {
        b = rb.footprint_stand_in(&b);
    }
    if a.class != Class::Zone && b.class == Class::Zone {
        std::mem::swap(&mut a, &mut b);
    } else if !a.is_connected() && b.is_connected() {
        std::mem::swap(&mut a, &mut b);
    }

    let active = cx.active.clone();
    let layer_intersection: Vec<String> = sorted_layers(a.layers.iter().filter(|l| b.layers.contains(l)).cloned().collect());
    let copper_intersection: Vec<String> = layer_intersection.iter().filter(|l| is_copper(l)).cloned().collect();
    let (a_fp, b_fp) = (a.class == Class::Footprint, b.class == Class::Footprint);
    let ac = a.is_connected();
    let bc = b.is_connected();

    // ---- a zone and a pad of its net
    if !copper_intersection.is_empty() && a.class == Class::Zone && b.class == Class::Pad && a.net == b.net {
        let layer = if a.layers.contains(&active) { active.clone() } else { sorted_layers(a.layers.clone()).first().cloned().unwrap_or_default() };
        let mut page = Page { title: "Zone".to_string(), ..Default::default() };
        let mut r = Rep::new();
        cx.header3("Zone connection resolution for:", &a, &b, &layer, &mut r);
        let mut report = Report::default();
        let constraint = rb.engine.eval_zone_connection(&b, &a, Some(&layer), Some(&mut report));
        r.lines.extend(report.lines.iter().cloned());
        page.resolutions.push(cx.resolution(&constraint, &report, Some(&layer)));
        match constraint.zone_connection {
            ZoneConnection::Thermal => {
                r.line("");
                r.line("");
                cx.header3("Thermal-relief gap resolution for:", &a, &b, &layer, &mut r);
                let (c, rep) = cx.eval(CType::ThermalReliefGap, Some(&b), Some(&a), Some(&layer), &mut r);
                let gap = c.value.min();
                page.resolutions.push(cx.resolution(&c, &rep, Some(&layer)));
                if cx.compile_error {
                    cx.compile_notice(&mut r);
                }
                r.line("");
                r.line(format!("Resolved thermal relief gap: {}.", cx.sfv(gap)));

                r.line("");
                r.line("");
                cx.header3("Thermal-relief spoke width resolution for:", &a, &b, &layer, &mut r);
                let (c, rep) = cx.eval(CType::ThermalSpokeWidth, Some(&b), Some(&a), Some(&layer), &mut r);
                let width = c.value.opt();
                page.resolutions.push(cx.resolution(&c, &rep, Some(&layer)));
                if cx.compile_error {
                    cx.compile_notice(&mut r);
                }
                r.line("");
                r.line(format!("Resolved spoke width: {}.", cx.sfv(width)));

                r.line("");
                r.line("");
                cx.header3("Thermal-relief min spoke count resolution for:", &a, &b, &layer, &mut r);
                let (c, rep) = cx.eval(CType::MinResolvedSpokes, Some(&b), Some(&a), Some(&layer), &mut r);
                let spokes = c.value.min();
                page.resolutions.push(cx.resolution(&c, &rep, Some(&layer)));
                if cx.compile_error {
                    cx.compile_notice(&mut r);
                }
                r.line("");
                r.line(format!("Resolved min spoke count: {spokes}."));
            }
            ZoneConnection::None => {
                r.line("");
                r.line("");
                cx.header3("Zone clearance resolution for:", &a, &b, &layer, &mut r);
                let mut clearance = a.local_clearance.as_ref().map(|c| c.0.get()).unwrap_or(0);
                r.line("");
                r.line(format!("Zone clearance: {}.", cx.sfv(clearance)));
                let (c, rep) = cx.eval(CType::PhysicalClearance, Some(&b), Some(&a), Some(&layer), &mut r);
                page.resolutions.push(cx.resolution(&c, &rep, Some(&layer)));
                if c.value.min() > clearance {
                    clearance = c.value.min();
                    r.line("");
                    r.line(format!("Overridden by larger physical clearance from {};clearance: {}.", escape_html(&rb.engine.constraint_name(&c)), cx.sfv(clearance)));
                }
                if !b.flashed.contains(&layer) {
                    let (c, rep) = cx.eval(CType::PhysicalHoleClearance, Some(&b), Some(&a), Some(&layer), &mut r);
                    page.resolutions.push(cx.resolution(&c, &rep, Some(&layer)));
                    if c.value.min() > clearance {
                        clearance = c.value.min();
                        r.line("");
                        r.line(format!("Overridden by larger physical hole clearance from {}; clearance: {}.", escape_html(&rb.engine.constraint_name(&c)), cx.sfv(clearance)));
                    }
                }
                if cx.compile_error {
                    cx.compile_notice(&mut r);
                }
                r.line("");
                r.line(format!("Resolved min clearance: {}.", cx.sfv(clearance)));
            }
            _ => {
                r.line("");
                r.line("");
                cx.header3("Zone clearance resolution for:", &a, &b, &layer, &mut r);
                if cx.compile_error {
                    cx.compile_notice(&mut r);
                }
                r.line("");
                r.line(format!("Resolved min clearance: {}.", cx.sfv(0)));
            }
        }
        page.lines = r.lines;
        out.pages.push(page);
    } else if !copper_intersection.is_empty() && !a_fp && !b_fp {
        let same_net = ac && bc && !a.net.is_empty() && a.net == b.net;
        let mut layers: Vec<String> = Vec::new();
        if copper_intersection.contains(&active) {
            layers.push(active.clone());
        }
        for l in &copper_intersection {
            if *l != active {
                layers.push(l.clone());
            }
        }
        let fill = |layer: &str| -> (Vec<String>, Resolution) {
            let mut rep = Rep::new();
            cx.header3("Clearance resolution for:", &a, &b, layer, &mut rep);
            if same_net {
                rep.line("Items belong to the same net. Min clearance is 0.");
                let res = Resolution { constraint: CType::Clearance, layer: Some(layer.to_string()), min: Some(0), opt: None, max: None, rule: "same net".to_string(), implicit: false, null: false, zone_connection: None, disallowed: false, considered: Vec::new() };
                return (rep.lines, res);
            }
            let (constraint, report) = cx.eval(CType::Clearance, Some(&a), Some(&b), Some(layer), &mut rep);
            let clearance = constraint.value.min();
            if cx.compile_error {
                cx.compile_notice(&mut rep);
            }
            rep.line("");
            if constraint.is_null() {
                rep.line("Min clearance is 0.");
            } else if clearance < 0 {
                rep.line(format!("Resolved clearance: {}; clearance will not be tested.", cx.sfv(clearance)));
            } else {
                rep.line(format!("Resolved min clearance: {}.", cx.sfv(clearance)));
            }
            let res = cx.resolution(&constraint, &report, Some(layer));
            (rep.lines, res)
        };
        if layers.len() == 1 {
            let (lines, res) = fill(&layers[0]);
            out.pages.push(Page { title: rb.layer_name(&layers[0]), lines, per_layer: Vec::new(), resolutions: vec![res] });
        } else {
            let mut page = Page { title: "Clearance".to_string(), ..Default::default() };
            for l in &layers {
                let (lines, res) = fill(l);
                page.per_layer.push((l.clone(), rb.layer_name(l), lines));
                page.resolutions.push(res);
            }
            out.pages.push(page);
        }
    }

    // ---- the diff pair
    if ac && bc {
        let (polarity, coupled, _) = crate::pcbexpr::match_dp_suffix(&a.net);
        if polarity != 0 && b.net == coupled {
            let dp_inter: Vec<String> = sorted_layers(a.layers.iter().filter(|l| b.layers.contains(l) && is_copper(l)).cloned().collect());
            let mut dp_layer = active.clone();
            if !dp_inter.contains(&dp_layer) && !dp_inter.is_empty() {
                dp_layer = dp_inter[0].clone();
            }
            let mut page = Page { title: "Diff Pair".to_string(), ..Default::default() };
            let mut r = Rep::new();
            cx.header3("Diff-pair gap resolution for:", &a, &b, &dp_layer, &mut r);
            let (c, rep) = cx.eval(CType::DiffPairGap, Some(&a), Some(&b), Some(&dp_layer), &mut r);
            page.resolutions.push(cx.resolution(&c, &rep, Some(&dp_layer)));
            r.line("");
            r.line(format!("Resolved gap constraints: min {}; opt {}; max {}.", report_min(&c, units), report_opt(&c, units), report_max(&c, units)));
            r.line("");
            r.line("");
            r.line("");
            cx.header3("Diff-pair max uncoupled length resolution for:", &a, &b, &dp_layer, &mut r);
            if !rb.engine.has_rules_for(CType::MaxUncoupled) {
                r.line("");
                r.line("No 'diff_pair_uncoupled' constraints defined.");
            } else {
                let (c, rep) = cx.eval(CType::MaxUncoupled, Some(&a), Some(&b), Some(&dp_layer), &mut r);
                page.resolutions.push(cx.resolution(&c, &rep, Some(&dp_layer)));
                r.line("");
                r.line(format!("Resolved max uncoupled length: {}.", report_max(&c, units)));
            }
            page.lines = r.lines;
            out.pages.push(page);
        }
    }

    // ---- silkscreen
    for layer in ["F.SilkS", "B.SilkS"] {
        let mut warning = String::new();
        let corresponding = |item: &RuleItem, warning: &mut String| -> bool {
            if item.layers.iter().any(|l| l == layer) {
                return true;
            }
            let front = layer.starts_with("F.");
            let (mask, copper) = if front { ("F.Mask", "F.Cu") } else { ("B.Mask", "B.Cu") };
            if item.layers.iter().any(|l| l == mask) {
                return true;
            }
            let tented = item.tented[if front { 0 } else { 1 }];
            if tented && item.layers.iter().any(|l| l == copper) {
                *warning = format!("Note: {} is tented; clearance will only be applied to holes.", cx.describe(item));
                return true;
            }
            false
        };
        let hit = (a.layers.iter().any(|l| l == layer) && corresponding(&b, &mut warning)) || (b.layers.iter().any(|l| l == layer) && corresponding(&a, &mut warning));
        if hit {
            let mut page = Page { title: rb.layer_name(layer), ..Default::default() };
            let mut r = Rep::new();
            cx.header3("Silkscreen clearance resolution for:", &a, &b, layer, &mut r);
            let (c, rep) = cx.eval(CType::SilkClearance, Some(&a), Some(&b), Some(layer), &mut r);
            if cx.compile_error {
                cx.compile_notice(&mut r);
            }
            r.line("");
            if !warning.is_empty() {
                r.line(warning.clone());
            }
            r.line(format!("Resolved min clearance: {}.", cx.sfv(c.value.min())));
            page.resolutions.push(cx.resolution(&c, &rep, Some(layer)));
            page.lines = r.lines;
            out.pages.push(page);
        }
    }

    // ---- courtyards
    for layer in ["F.CrtYd", "B.CrtYd"] {
        let has = |fp: bool, item: &RuleItem| -> bool {
            fp && item.footprint.as_ref().is_some_and(|f| if layer == "F.CrtYd" { !f.courtyard_front.is_empty() } else { !f.courtyard_back.is_empty() })
        };
        if has(a_fp, &a) && has(b_fp, &b) {
            let mut page = Page { title: rb.layer_name(layer), ..Default::default() };
            let mut r = Rep::new();
            cx.header3("Courtyard clearance resolution for:", &a, &b, layer, &mut r);
            let (c, rep) = cx.eval(CType::CourtyardClearance, Some(&a), Some(&b), Some(layer), &mut r);
            if cx.compile_error {
                cx.compile_notice(&mut r);
            }
            r.line("");
            r.line(format!("Resolved min clearance: {}.", cx.sfv(c.value.min())));
            page.resolutions.push(cx.resolution(&c, &rep, Some(layer)));
            page.lines = r.lines;
            out.pages.push(page);
        }
    }

    // ---- holes
    if a.has_hole || b.has_hole {
        let mut layer: Option<String> = None;
        let on_active = |i: &RuleItem| i.layers.contains(&active);
        if a.has_hole && on_active(&b) && is_copper(&active) {
            layer = Some(active.clone());
        } else if b.has_hole && on_active(&a) && is_copper(&active) {
            layer = Some(active.clone());
        } else if a.has_hole && b.on_copper() {
            layer = b.layer.clone();
        } else if b.has_hole && a.on_copper() {
            layer = a.layer.clone();
        }
        let mut page = Page { title: "Hole".to_string(), ..Default::default() };
        let mut page_added = false;
        let mut r = Rep::new();
        if let Some(l) = layer.filter(|l| layer_key(l) != u32::MAX) {
            page_added = true;
            cx.header3("Hole clearance resolution for:", &a, &b, &l, &mut r);
            let (c, rep) = cx.eval(CType::HoleClearance, Some(&a), Some(&b), Some(&l), &mut r);
            if cx.compile_error {
                cx.compile_notice(&mut r);
            }
            r.line("");
            r.line(format!("Resolved min clearance: {}.", cx.sfv(c.value.min())));
            page.resolutions.push(cx.resolution(&c, &rep, Some(&l)));
        }
        if a.has_drilled_hole || b.has_drilled_hole {
            if page_added {
                r.line("");
                r.line("");
                r.line("");
            }
            page_added = true;
            cx.header2("Hole-to-hole clearance resolution for:", &a, &b, &mut r);
            let (c, rep) = cx.eval(CType::HoleToHole, Some(&a), Some(&b), None, &mut r);
            if cx.compile_error {
                cx.compile_notice(&mut r);
            }
            r.line("");
            r.line(format!("Resolved min clearance: {}.", cx.sfv(c.value.min())));
            page.resolutions.push(cx.resolution(&c, &rep, None));
        }
        if page_added {
            page.lines = r.lines;
            out.pages.push(page);
        }
    }

    // ---- the board edge
    for edge in ["Edge.Cuts", "Margin"] {
        let mut layer: Option<String> = None;
        let pick = |from: &RuleItem, other: &RuleItem| -> Option<String> {
            if from.layers.iter().any(|l| l == edge) && other.class != Class::Footprint {
                if other.layers.contains(&active) && is_copper(&active) {
                    return Some(active.clone());
                } else if other.layer.as_deref().is_some_and(is_copper) {
                    return other.layer.clone();
                }
            }
            None
        };
        if a.layers.iter().any(|l| l == edge) && b.class != Class::Footprint {
            layer = pick(&a, &b);
        } else if b.layers.iter().any(|l| l == edge) && a.class != Class::Footprint {
            layer = pick(&b, &a);
        }
        if let Some(l) = layer {
            let mut page = Page { title: format!("{} Clearance", rb.layer_name(edge)), ..Default::default() };
            let mut r = Rep::new();
            cx.header3("Edge clearance resolution for:", &a, &b, &l, &mut r);
            let (c, rep) = cx.eval(CType::EdgeClearance, Some(&a), Some(&b), Some(&l), &mut r);
            if cx.compile_error {
                cx.compile_notice(&mut r);
            }
            r.line("");
            r.line(format!("Resolved min clearance: {}.", cx.sfv(c.value.min())));
            page.resolutions.push(cx.resolution(&c, &rep, Some(&l)));
            page.lines = r.lines;
            out.pages.push(page);
        }
    }

    // ---- physical clearances
    let mut page = Page { title: "Physical Clearances".to_string(), ..Default::default() };
    let mut r = Rep::new();
    if cx.compile_error {
        cx.compile_notice(&mut r);
    } else if !rb.engine.has_rules_for(CType::PhysicalClearance) {
        r.line("");
        r.line("No 'physical_clearance' constraints defined.");
    } else {
        let mut report_layers: Vec<String> = layer_intersection.clone();
        let physical = |i: &RuleItem| -> Vec<String> { i.layers.iter().filter(|l| is_copper(l) || ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask"].contains(&l.as_str())).cloned().collect() };
        if a.layers.iter().any(|l| l == "Edge.Cuts") {
            if b_fp {
                report_layers.extend(["F.CrtYd".to_string(), "B.CrtYd".to_string()]);
            } else {
                report_layers.extend(physical(&b));
            }
        }
        if b.layers.iter().any(|l| l == "Edge.Cuts") {
            if a_fp {
                report_layers.extend(["F.CrtYd".to_string(), "B.CrtYd".to_string()]);
            } else {
                report_layers.extend(physical(&a));
            }
        }
        let report_layers = sorted_layers(report_layers);
        let mut reported = false;
        for layer in &report_layers {
            reported = true;
            cx.header3("Physical clearance resolution for:", &a, &b, layer, &mut r);
            let (c, rep) = cx.eval(CType::PhysicalClearance, Some(&a), Some(&b), Some(layer), &mut r);
            if c.is_null() {
                r.line("");
                r.line(format!("No 'physical_clearance' constraints in effect on {}.", rb.layer_name(layer)));
            } else {
                r.line("");
                r.line(format!("Resolved min clearance: {}.", cx.sfv(c.value.min())));
            }
            page.resolutions.push(cx.resolution(&c, &rep, Some(layer)));
            r.line("");
            r.line("");
            r.line("");
        }
        if !reported {
            cx.header2("Physical clearance resolution for:", &a, &b, &mut r);
            r.line("");
            r.line("Items share no relevant layers.  No 'physical_clearance' constraints will be applied.");
        }
    }
    if a.has_hole || b.has_hole {
        let layer = if a.has_hole && b.layers.contains(&active) {
            active.clone()
        } else if b.has_hole && a.layers.contains(&active) {
            active.clone()
        } else if a.has_hole {
            b.layer.clone().unwrap_or_default()
        } else {
            a.layer.clone().unwrap_or_default()
        };
        cx.header3("Physical hole clearance resolution for:", &a, &b, &layer, &mut r);
        let (c, rep) = cx.eval(CType::PhysicalHoleClearance, Some(&a), Some(&b), Some(&layer), &mut r);
        page.resolutions.push(cx.resolution(&c, &rep, Some(&layer)));
        if cx.compile_error {
            cx.compile_notice(&mut r);
        } else if !rb.engine.has_rules_for(CType::PhysicalHoleClearance) {
            r.line("");
            r.line("No 'physical_hole_clearance' constraints defined.");
        } else {
            r.line("");
            r.line(format!("Resolved min clearance: {}.", cx.sfv(c.value.min())));
        }
    }
    page.lines = r.lines;
    out.pages.push(page);
    out
}

// ---------------------------------------------------------------------------------------------------------------------------------- constraints

/// `BOARD_INSPECTION_TOOL::InspectConstraints` on one item.
pub fn inspect_constraints(rb: &RuleBoard, item: Rc<RuleItem>, units: Units) -> Inspection {
    let mut out = Inspection { title: "Constraints Report".to_string(), compile_error: rb.compile_error.as_ref().map(|e| e.to_string()), ..Default::default() };
    let cx = Ctx { rb, units, active: "F.Cu".to_string(), compile_error: rb.compile_error.is_some() };
    let layer = item.layer.clone();
    let mut push_page = |title: &str, build: &mut dyn FnMut(&mut Rep) -> Vec<Resolution>| {
        let mut r = Rep::new();
        let resolutions = build(&mut r);
        out.pages.push(Page { title: title.to_string(), lines: r.lines, per_layer: Vec::new(), resolutions });
    };
    let min_opt_max = |t: &str, c: &Resolved| format!("Resolved {t} constraints: min {}; opt {}; max {}.", report_min(c, units), report_opt(c, units), report_max(c, units));

    if item.class == Class::Track {
        push_page("Track Width", &mut |r| {
            cx.header1("Track width resolution for:", &item, r);
            let (c, rep) = cx.eval(CType::TrackWidth, Some(&item), None, layer.as_deref(), r);
            if cx.compile_error {
                cx.compile_notice(r);
            }
            r.line("");
            r.line(min_opt_max("width", &c));
            vec![cx.resolution(&c, &rep, layer.as_deref())]
        });
    }

    if item.class == Class::Via {
        push_page("Via Diameter", &mut |r| {
            cx.header1("Via diameter resolution for:", &item, r);
            let (c, rep) = cx.eval(CType::ViaDiameter, Some(&item), None, None, r);
            if cx.compile_error {
                cx.compile_notice(r);
            }
            r.line("");
            r.line(min_opt_max("diameter", &c));
            vec![cx.resolution(&c, &rep, None)]
        });
        push_page("Via Annular Width", &mut |r| {
            cx.header1("Via annular width resolution for:", &item, r);
            let (c, rep) = cx.eval(CType::AnnularWidth, Some(&item), None, None, r);
            if cx.compile_error {
                cx.compile_notice(r);
            }
            r.line("");
            r.line(min_opt_max("annular width", &c));
            vec![cx.resolution(&c, &rep, None)]
        });
    }

    if (item.class == Class::Pad && item.drill > eda_model::units::Nm::ZERO) || item.class == Class::Via {
        push_page("Hole Size", &mut |r| {
            cx.header1("Hole size resolution for:", &item, r);
            let (c, rep) = cx.eval(CType::HoleSize, Some(&item), None, None, r);
            if cx.compile_error {
                cx.compile_notice(r);
            }
            r.line("");
            r.line(min_opt_max("hole size", &c));
            vec![cx.resolution(&c, &rep, None)]
        });
    }

    if matches!(item.class, Class::Pad | Class::Shape | Class::Track | Class::Arc | Class::Via) {
        push_page("Solder Mask", &mut |r| {
            cx.header1("Solder mask expansion resolution for:", &item, r);
            let (c, rep) = cx.eval(CType::SolderMaskExpansion, Some(&item), None, None, r);
            if cx.compile_error {
                cx.compile_notice(r);
            }
            r.line("");
            r.line(format!("Resolved solder mask expansion: {}.", report_opt(&c, units)));
            vec![cx.resolution(&c, &rep, None)]
        });
    }

    if item.class == Class::Pad {
        push_page("Solder Paste", &mut |r| {
            cx.header1("Solder paste absolute clearance resolution for:", &item, r);
            let (c1, rep1) = cx.eval(CType::SolderPasteAbsMargin, Some(&item), None, None, r);
            if cx.compile_error {
                cx.compile_notice(r);
            }
            r.line("");
            r.line(format!("Resolved solder paste absolute clearance: {}.", report_opt(&c1, units)));
            cx.header1("Solder paste relative clearance resolution for:", &item, r);
            let (c2, rep2) = cx.eval(CType::SolderPasteRelMargin, Some(&item), None, None, r);
            if cx.compile_error {
                cx.compile_notice(r);
            }
            r.line("");
            r.line("");
            r.line("");
            r.line(format!("Resolved solder paste relative clearance: {}.", report_opt(&c2, units)));
            vec![cx.resolution(&c1, &rep1, None), cx.resolution(&c2, &rep2, None)]
        });
    }

    if matches!(item.class, Class::Field | Class::Text) {
        push_page("Text Size", &mut |r| {
            cx.header1("Text height resolution for:", &item, r);
            let (c, rep) = cx.eval(CType::TextHeight, Some(&item), None, None, r);
            if cx.compile_error {
                cx.compile_notice(r);
            }
            r.line("");
            r.line(min_opt_max("height", &c));
            r.line("");
            r.line("");
            r.line("");
            cx.header1("Text thickness resolution for:", &item, r);
            let (c2, rep2) = cx.eval(CType::TextThickness, Some(&item), None, None, r);
            if cx.compile_error {
                cx.compile_notice(r);
            }
            r.line("");
            r.line(min_opt_max("thickness", &c2));
            vec![cx.resolution(&c, &rep, None), cx.resolution(&c2, &rep2, None)]
        });
    }

    push_page("Keepouts", &mut |r| {
        cx.header1("Keepout resolution for:", &item, r);
        let (c, rep) = cx.eval(CType::Disallow, Some(&item), None, layer.as_deref(), r);
        if cx.compile_error {
            cx.compile_notice(r);
        }
        r.line("");
        if c.disallow != 0 {
            r.line("Item <b>disallowed</b> at current location.");
        } else {
            r.line("Item allowed at current location.");
        }
        vec![cx.resolution(&c, &rep, layer.as_deref())]
    });

    push_page("Assertions", &mut |r| {
        cx.header1("Assertions for:", &item, r);
        if cx.compile_error {
            cx.compile_notice(r);
        }
        let mut report = Report::default();
        let mut failed = Vec::new();
        rb.engine.process_assertions(&item, Some(&mut report), |n| failed.push(n.to_string()));
        r.lines.extend(report.lines);
        Vec::new()
    });
    out
}

/// The inspection of nothing: a message for the info bar.
pub fn refusal(title: &str, message: &str) -> Inspection {
    Inspection { title: title.to_string(), error: Some(message.to_string()), ..Default::default() }
}

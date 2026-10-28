//! Length tuning on a board: the work behind `/api/tune/*` (`tune_api.rs`), without the HTTP.
//!
//! KiCad's tuning tool is `DRAWING_TOOL::PlaceTuningPattern` and the generator `PCB_TUNING_PATTERN` (`pcbnew/generators/pcb_tuning_pattern.cpp`):
//!
//! * hovering a track shows its length against the target (`updateHoverStatus`: a pattern made on the spot from `CreateNew`, `EditStart`, `Update`);
//! * the first click puts the pattern's start on the track, moving the cursor sets its end and which side it starts on (`UpdateSideFromEnd`) and the
//!   router's placer fits the meander (`EditStart`, `Update`), the second click commits it (`EditFinish`): the tracks of the meander become the members
//!   of the generator, one undo step;
//! * a pattern that exists is tuned again by putting its baseline back and running the placer over it again (`resetToBaseline`, then `Update`), and
//!   removed by putting the baseline back (`Remove`, `removeToBaseline`).
//!
//! The placers are `eda_pns::{meander_placer, dp_meander_placer}`; what they leave on the line becomes tracks through `eda_pns::tuning_commit`, and the
//! verb is `Cmd::CommitTuning`. A [`Scene`] is a board and the router's world built from it; a [`Tuner`] is a pattern being made on one.

use eda_drc::constraints::CType;
use eda_drc::pcbexpr::{Condition, Env};
use eda_drc::rule_item::{Class, RuleItem};
use eda_model::ir::{tessellate_arc, Design, Point, Track, Nm};
use eda_model::setup_pages::{CornerStyle, MeanderSettings as PageSettings};
use eda_model::tuning::{PatternSide, TargetRange, TuningMode, TuningPattern, LENGTH_UNCONSTRAINED_NM};
use eda_model::units::um;
use eda_model::{BoardRules, ConstraintModel};
use eda_ops::{Board, Cmd};
use eda_pns::chain::ShapeLineChain;
use eda_pns::dp_meander_placer::{assemble_diff_pair, skew_setup, DpMeanderPlacer, NO_PAIR, NO_PAIR_FOR_SKEW};
use eda_pns::from_ir::build_node_for_tuning as build_node;
use eda_pns::item::{Item, ItemId};
use eda_pns::kigeom::Seg;
use eda_pns::layer::LayerMap;
use eda_pns::line::Line;
use eda_pns::meander::{MeanderSettings, MeanderSide, MeanderStyle, MinOptMax, LENGTH_UNCONSTRAINED};
use eda_pns::meander_placer::{chain_of, MeanderPlacer, Tuned, TuningStatus};
use eda_pns::node::Node;
use eda_pns::tuning_commit::{line_edit, LineEdit, PieceShape};
use eda_pns::tuning_path::{assemble_tuning_path, LengthEnv};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::rc::Rc;

/// A board and the router's world built from it.
pub struct Scene {
    pub design: Design,
    pub model: ConstraintModel,
    pub world: Node,
    pub layers: LayerMap,
    pub env: LengthEnv,
}

impl Scene {
    pub fn of(design: Design, model: ConstraintModel) -> Scene {
        let (world, layers) = build_node(&design, &model);
        let env = LengthEnv::of(&model);
        Scene { design, model, world, layers, env }
    }

    fn net_name(net: &eda_pns::item::Net) -> String {
        net.as_deref().unwrap_or("").to_string()
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------- picking

/// A track segment under the cursor (`PlaceTuningPattern`'s hover: the track whose centre is nearest, among the ones the cursor is on).
#[derive(Debug, Clone)]
pub struct Pick {
    pub item: ItemId,
    pub track_id: String,
    pub net: String,
    pub layer: String,
    pub width: Nm,
    /// The point of the segment nearest the cursor: where a pattern started here would start (`m_pickerItem->GetFocusPosition()`).
    pub at: Point,
}

/// The track segment or arc at `(x, y)` within `tolerance` of its edge, preferring `layer`. On an arc the pattern starts at the end of it the cursor is
/// nearer to (`getSnappedStartPoint`).
pub fn pick(scene: &Scene, x: Nm, y: Nm, tolerance: Nm, layer: Option<&str>) -> Option<Pick> {
    let cursor = Point { x, y };
    let mut best: Option<(bool, i128, Pick)> = None;
    for (id, item) in scene.world.iter() {
        // (the IR track, net, layer, width, distance from the centre line to the cursor, the middle of the item, the pattern's start)
        let (track, s_net, s_layer, width, dist, centre, at) = match item {
            Item::Segment(s) => {
                let Some((track, _)) = &s.source_track else { continue };
                let seg = Seg::new(s.a, s.b);
                (track.clone(), &s.net, s.layer, s.width, (seg.squared_distance(cursor) as f64).sqrt(), Point { x: (s.a.x + s.b.x) / 2, y: (s.a.y + s.b.y) / 2 }, seg.nearest_point(cursor))
            }
            Item::Arc(a) => {
                let Some(track) = &a.source_track else { continue };
                let at = if eda_pns::seg_ops::length(cursor, a.start()) <= eda_pns::seg_ops::length(cursor, a.end()) { a.start() } else { a.end() };
                (track.clone(), &a.net, a.layer, a.width, a.arc.distance_to(cursor), a.arc.mid, at)
            }
            _ => continue,
        };
        if s_net.is_none() {
            continue;
        }
        let reach = ((width / 2 + tolerance).max(Nm::new(1))).as_f64();
        if dist > reach {
            continue;
        }
        let layer_name = scene.layers.name_of(s_layer).to_string();
        let on_layer = layer.is_some_and(|l| l == layer_name);
        // "the track whose centre is nearest the cursor", the active layer first
        let (dx, dy) = (centre.x - cursor.x, centre.y - cursor.y);
        let d = dx.area(dx) + dy.area(dy);
        let key = (!on_layer, d);
        if best.as_ref().is_none_or(|(o, bd, _)| key < (*o, *bd)) {
            best = Some((key.0, key.1, Pick { item: id, track_id: track, net: Scene::net_name(s_net), layer: layer_name, width, at }));
        }
    }
    best.map(|(_, _, p)| p)
}

// ------------------------------------------------------------------------------------------------------------------------------------ settings

/// The Board Setup > Length-tuning Patterns group a pattern of `kind` starts from.
fn page_settings(model: &ConstraintModel, kind: TuningMode) -> PageSettings {
    let page = &model.board.tuning_patterns;
    match kind {
        TuningMode::Single => page.single_track.clone(),
        TuningMode::DiffPair => page.diff_pair.clone(),
        TuningMode::DiffPairSkew => page.diff_pair_skew.clone(),
    }
}

/// `PCB_TUNING_PATTERN::CreateNew`'s settings: the board's meander defaults for the kind of tuning, no length target, a skew target of zero.
pub fn default_pattern(model: &ConstraintModel, kind: TuningMode, layer: &str) -> TuningPattern {
    let s = page_settings(model, kind);
    TuningPattern {
        id: String::new(),
        mode: kind,
        layer: layer.to_string(),
        origin: Point { x: Nm::ZERO, y: Nm::ZERO },
        end: Point { x: Nm::ZERO, y: Nm::ZERO },
        initial_side: PatternSide::Left,
        min_amplitude: s.min_amplitude_nm,
        max_amplitude: s.max_amplitude_nm,
        spacing: s.spacing_nm,
        corner_radius_percent: s.corner_radius_percentage,
        rounded: s.corner_style == CornerStyle::Round,
        single_sided: s.single_sided,
        override_custom_rules: false,
        target_length: TargetRange::unconstrained(),
        target_skew: TargetRange { min: -100_000, opt: 0, max: 100_000 },
        is_time_domain: false,
        target_delay: TargetRange::delay_unconstrained(),
        target_skew_delay: TargetRange { min: -100_000, opt: 0, max: 100_000 },
        base_line: Vec::new(),
        base_line_coupled: Vec::new(),
        last_track_width: Nm::ZERO,
        last_diff_pair_gap: Nm::ZERO,
        last_tuning_length: Nm::ZERO,
        last_tuning_delay: 0,
        last_netname: String::new(),
        last_status: "tuned".into(),
        member_ids: Vec::new(),
    }
}

fn int(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_f64).filter(|f| f.is_finite()).map(|f| f.round() as i64)
}

/// A target as the dialog sends it: a number (the target, with the default tolerance round it), an object `{min?, opt?, max?}`, or null (no target).
fn range_of(v: &Value, skew: bool) -> Option<TargetRange> {
    if v.is_null() {
        return Some(if skew { TargetRange { min: -100_000, opt: 0, max: 100_000 } } else { TargetRange::unconstrained() });
    }
    if let Some(n) = v.as_f64() {
        return Some(TargetRange::around(Nm::kiround(n)));
    }
    let obj = v.as_object()?;
    let get = |k: &str| obj.get(k).and_then(Value::as_f64).map(|f| f.round() as i64);
    let opt = get("opt").or_else(|| get("min")).unwrap_or(0);
    if !skew && opt >= LENGTH_UNCONSTRAINED_NM.get() {
        return Some(TargetRange::unconstrained());
    }
    let base = TargetRange::around(Nm::new(opt));
    Some(TargetRange { min: get("min").unwrap_or(base.min), opt, max: get("max").unwrap_or(base.max) })
}

/// A delay target as the dialog sends it, in picoseconds: a number (the target, with the default tolerance of 0.1 ps round it), an object `{min?, opt?, max?}`, or null
/// (no target). Attoseconds.
fn delay_range_of(v: &Value, skew: bool) -> Option<TargetRange> {
    let atto = |ps: f64| (ps * 1_000_000.0).round() as i64;
    if v.is_null() {
        return Some(if skew { TargetRange::delay_around(0) } else { TargetRange::delay_unconstrained() });
    }
    if let Some(n) = v.as_f64().filter(|f| f.is_finite()) {
        return Some(TargetRange::delay_around(atto(n)));
    }
    let obj = v.as_object()?;
    let get = |k: &str| obj.get(k).and_then(Value::as_f64).filter(|f| f.is_finite()).map(atto);
    let opt = get("opt").or_else(|| get("min")).unwrap_or(0);
    if !skew && opt >= eda_model::tuning::DELAY_UNCONSTRAINED_AS {
        return Some(TargetRange::delay_unconstrained());
    }
    let base = TargetRange::delay_around(opt);
    Some(TargetRange { min: get("min").unwrap_or(base.min), opt, max: get("max").unwrap_or(base.max) })
}

/// The settings of a request over a pattern: whatever the object carries (micrometres) replaces what the pattern had. The time domain's are in picoseconds:
/// `time_domain` and `target_delay`, `target_skew_delay`.
pub fn apply_settings(p: &mut TuningPattern, v: &Value) {
    if let Some(n) = int(v, "min_amplitude") {
        p.min_amplitude = Nm::new(n);
    }
    if let Some(n) = int(v, "max_amplitude") {
        p.max_amplitude = Nm::new(n);
    }
    if let Some(n) = int(v, "spacing") {
        p.spacing = Nm::new(n);
    }
    if let Some(n) = int(v, "corner_radius_percent") {
        p.corner_radius_percent = n.clamp(0, 100) as u32;
    }
    if let Some(b) = v.get("rounded").and_then(Value::as_bool) {
        p.rounded = b;
    }
    if let Some(b) = v.get("single_sided").and_then(Value::as_bool) {
        p.single_sided = b;
    }
    if let Some(b) = v.get("override_custom_rules").and_then(Value::as_bool) {
        p.override_custom_rules = b;
    }
    if let Some(side) = v.get("initial_side").and_then(Value::as_str).and_then(PatternSide::from_token) {
        p.initial_side = side;
    }
    if let Some(t) = v.get("target_length").and_then(|t| range_of(t, false)) {
        p.target_length = t;
    }
    if let Some(t) = v.get("target_skew").and_then(|t| range_of(t, true)) {
        p.target_skew = t;
    }
    if let Some(b) = v.get("time_domain").and_then(Value::as_bool) {
        p.is_time_domain = b;
    }
    if let Some(t) = v.get("target_delay").and_then(|t| delay_range_of(t, false)) {
        p.target_delay = t;
    }
    if let Some(t) = v.get("target_skew_delay").and_then(|t| delay_range_of(t, true)) {
        p.target_skew_delay = t;
    }
}

/// `PNS::MEANDER_SETTINGS` (nanometres) for a pattern.
pub fn meander_settings(p: &TuningPattern) -> MeanderSettings {
    let mut s = MeanderSettings::default();
    s.min_amplitude = p.min_amplitude;
    s.max_amplitude = p.max_amplitude;
    s.spacing = p.spacing;
    s.corner_radius_percentage = p.corner_radius_percent as i64;
    s.corner_style = if p.rounded { MeanderStyle::Round } else { MeanderStyle::Chamfer };
    s.single_sided = p.single_sided;
    s.override_custom_rules = p.override_custom_rules;
    s.initial_side = match p.initial_side {
        PatternSide::Left => MeanderSide::Left,
        PatternSide::Default => MeanderSide::Default,
        PatternSide::Right => MeanderSide::Right,
    };
    // `m_keepEndpoints = true; // Required for re-grouping`
    s.keep_endpoints = true;
    let range = |r: &TargetRange, unconstrained: bool| {
        let mut m = MinOptMax::default();
        if unconstrained {
            m.set_opt(LENGTH_UNCONSTRAINED);
            m.set_min(0);
            m.set_max(LENGTH_UNCONSTRAINED);
        } else {
            m.set_opt(r.opt);
            m.set_min(r.min);
            m.set_max(r.max);
        }
        m
    };
    s.target_length = range(&p.target_length, p.target_length.is_unconstrained());
    s.target_skew = range(&p.target_skew, false);
    // the time domain: the targets are delays, attoseconds as they are
    s.is_time_domain = p.is_time_domain;
    let delay = |r: &TargetRange| {
        let mut m = MinOptMax::default();
        m.set_opt(r.opt);
        m.set_min(r.min);
        m.set_max(r.max);
        m
    };
    s.target_length_delay = delay(&p.target_delay);
    s.target_skew_delay = delay(&p.target_skew_delay);
    s
}

/// What the placer changed of a pattern's settings (the amplitude and spacing keys, a flipped initial side), back into the pattern.
fn sync_back(p: &mut TuningPattern, s: &MeanderSettings) {
    p.max_amplitude = s.max_amplitude;
    p.spacing = s.spacing;
    p.initial_side = match s.initial_side {
        MeanderSide::Left => PatternSide::Left,
        MeanderSide::Default => PatternSide::Default,
        MeanderSide::Right => PatternSide::Right,
    };
}

// --------------------------------------------------------------------------------------------------------------------------------- constraints

/// A design rule's length (or skew) constraint for a net.
#[derive(Debug, Clone, PartialEq)]
pub struct Constraint {
    pub name: String,
    pub range: TargetRange,
}

/// `PCB_TUNING_PATTERN::CreateNew` / `ShowPropertiesDialog` / `EditStart`'s rule lookup (R7-D4):
///
/// ```text
/// constraint = bds.m_DRCEngine->EvalRules( LENGTH_CONSTRAINT, aStartItem, nullptr, layer );   // SKEW_CONSTRAINT for a skew pattern
/// if( !constraint.IsNull() ) .. m_settings.SetTargetLength( constraint.GetValue() );
/// ```
///
/// through the main rule engine (`eda_drc::rule_board::RuleBoard`, the compiled `.kicad_dru` the router, Inspect and the DRC's own checks use): A is the real
/// start track `track_id` (its width, layer, net class, groups, sheet .. all there), B is none, and a rules file that does not compile gives no rule. The value
/// is what `MEANDER_SETTINGS::SetTargetLength( const MINOPTMAX& )` makes of it: `SetTargetLength( Opt() )` (the optimum, else the minimum, with the default
/// tolerance round it), then the rule's own minimum and maximum when it has them. `None` when no rule applies, or the track is not on the board.
pub fn rule_constraint(design: &Design, model: &ConstraintModel, kind: TuningMode, track_id: &str, layer: &str) -> Option<Constraint> {
    use eda_drc::rule_board::RuleBoard;
    let board = eda_drc::board::build(design, model);
    let idx = board.tracks.iter().position(|s| s.id.rsplit_once('#').map_or(s.id.as_str(), |(base, _)| base) == track_id && s.layer == layer).or_else(|| board.tracks.iter().position(|s| s.id.rsplit_once('#').map_or(s.id.as_str(), |(base, _)| base) == track_id))?;
    let rb = RuleBoard::new(design, model, &board, eda_drc::constraints::Units::Mm, model.board.custom_rules_text.as_deref());
    let item = rb.segment_item_at(idx);
    let ctype = if kind == TuningMode::DiffPairSkew { CType::Skew } else { CType::Length };
    let c = rb.engine().eval_rules(ctype, Some(&item), None, Some(layer), None);
    if c.is_null() || c.parent.is_none() {
        return None;
    }
    let rule = c.parent.and_then(|i| rb.engine().rules.get(i));
    // `constraint.GetOption( DRC_CONSTRAINT::OPTIONS::TIME_DOMAIN )`: KiCad then sets the target delay (`SetTargetLengthDelay`) and no length. **Differs:** a time
    // domain rule gives the tuner no target here yet (its values are times, not lengths)
    if rule.and_then(|r| r.find_constraint(ctype)).is_some_and(|k| k.options & eda_drc::dru::OPT_TIME_DOMAIN != 0) {
        return None;
    }
    let name = rule.map(|r| r.name.clone()).unwrap_or(c.name.clone());
    let opt = c.value.opt();
    let tolerance = TargetRange::around(Nm::new(opt));
    Some(Constraint { name, range: TargetRange { min: c.value.min.unwrap_or(tolerance.min), opt, max: c.value.max.unwrap_or(tolerance.max) } })
}

/// **INACTIVE** (R7-D4; kept, not deleted, per the user's rule for replaced code): the tuner's old rule lookup, a second evaluator over `BoardRules::custom_rules`
/// (`eda_kicad::parse_custom_rules`, a second `.kicad_dru` reader beside the rule engine's `crates/drc/src/dru.rs`) on two stand-in tracks with a net and a layer
/// only, and B the partner. Nothing calls it: [`rule_constraint`] is the lookup. Delete it with the reader once nothing reads `custom_rules`.
///
/// `DRC_ENGINE::EvalRules( LENGTH_CONSTRAINT / SKEW_CONSTRAINT, track, partner, layer )`: the last custom rule of the constraint type that applies, in file order. Its
/// value is what `MEANDER_SETTINGS::SetTargetLength( constraint.GetValue() )` makes of it: the optimum is the rule's, else its minimum, with the default tolerance round it
/// unless the rule has a minimum or a maximum of its own.
#[allow(dead_code)]
pub fn constraint_for(rules: &BoardRules, kind: TuningMode, net: &str, partner: Option<&str>, layer: &str) -> Option<Constraint> {
    let (ty, ctype) = if kind == TuningMode::DiffPairSkew { ("skew", CType::Skew) } else { ("length", CType::Length) };
    // The conditions run through the PCBEXPR port (`DRC_RULE_CONDITION::EvaluateFor`, crates/drc/src/pcbexpr.rs) on two track stand-ins: a net and its class are
    // what a length rule's condition can ask of a track the tuner has not drawn yet.
    let classes: HashMap<String, String> = [Some(net), partner]
        .into_iter()
        .flatten()
        .map(|n| (n.to_string(), rules.class_of(n).map(|c| c.name.clone()).unwrap_or_else(|| "Default".to_string())))
        .collect();
    let mut env = Env::simple(&[layer]);
    env.nets = classes.keys().cloned().collect();
    env.classes = Box::new(move |n: &str| {
        let class = classes.get(n).cloned().unwrap_or_else(|| "Default".to_string());
        (class.clone(), vec![class])
    });
    let env = Rc::new(env);
    let track = |id: &str, n: &str| {
        let mut item = RuleItem::new(id, Class::Track);
        item.net = n.to_string();
        item.layers = vec![layer.to_string()];
        Rc::new(item)
    };
    let (a, b) = (track("a", net), track("b", partner.unwrap_or(net)));
    let mut won: Option<Constraint> = None;
    for r in rules.custom_rules.iter().filter(|r| r.constraint_type == ty) {
        if r.min.is_none() && r.max.is_none() && r.opt.is_none() {
            continue;
        }
        // `(layer outer)` / `(layer inner)` are the outer and the inner copper layers (`DRC_RULES_PARSER::parseLayer`), any other a wildcard name
        if !eda_kicad::rule_layer_matches(r.layer.as_deref(), layer) {
            continue;
        }
        if let Some(text) = r.condition.as_deref() {
            let mut condition = Condition::new(text);
            if !condition.compile(&env) {
                continue; // a condition this evaluator cannot read never applies
            }
            if !condition.evaluate_for(&env, Some(&a), Some(&b), ctype as u32, Some(layer), None) {
                continue;
            }
        }
        let opt = r.opt.or(r.min).unwrap_or(Nm::ZERO);
        let tolerance = TargetRange::around(opt);
        won = Some(Constraint { name: r.name.clone(), range: TargetRange { min: r.min.map_or(tolerance.min, Nm::get), opt: opt.get(), max: r.max.map_or(tolerance.max, Nm::get) } });
    }
    won
}

// ------------------------------------------------------------------------------------------------------------------------------------ readout

/// What the status box beside the cursor shows (`TUNING_STATUS_VIEW_ITEM`): the length (or the skew), the range it should be in, and whether it is.
#[derive(Debug, Clone)]
pub struct Readout {
    pub current: Nm,
    /// `current length` or `current skew`.
    pub label: &'static str,
    /// The range (`min`, `opt`, `max`) the status is judged against; `None` for a pattern with no target (the box shows `---`).
    pub target: Option<(Nm, Nm, Nm)>,
    pub status: TuningStatus,
    /// The custom rule the target comes from.
    pub constraint: Option<String>,
    pub partner: Option<String>,
    /// The length of each net of a pair (P, N).
    pub nets: Option<(Nm, Nm)>,
    /// A time domain pattern's delay and its target (`current delay`), attoseconds; the status is still the length's, as KiCad judges it.
    pub delay: Option<DelayReadout>,
}

/// What the status box shows of a time domain pattern: the delay (or the skew as a delay) now and the range it should be in.
#[derive(Debug, Clone)]
pub struct DelayReadout {
    /// Attoseconds.
    pub current: i64,
    /// `current delay` or `current skew`.
    pub label: &'static str,
    /// The range (`min`, `opt`, `max`) of the target delay, attoseconds; `None` when the pattern has none.
    pub target: Option<(i64, i64, i64)>,
}

impl Readout {
    pub fn json(&self) -> Value {
        let mm = |nm: Nm| nm.mm();
        let ps = |a: i64| a as f64 / 1e6;
        json!({
            "time_domain": self.delay.is_some(),
            "delay": self.delay.as_ref().map(|d| json!({
                "label": d.label,
                "current": ps(d.current),
                "min": d.target.map(|t| ps(t.0)),
                "opt": d.target.map(|t| ps(t.1)),
                "max": d.target.map(|t| ps(t.2)),
                "delta": d.target.map(|t| ps(d.current - t.1)),
            })),
            "label": self.label,
            "current": mm(self.current),
            "min": self.target.map(|t| mm(t.0)),
            "opt": self.target.map(|t| mm(t.1)),
            "max": self.target.map(|t| mm(t.2)),
            "delta": self.target.map(|t| mm(self.current - t.1)),
            "status": self.status.name(),
            "constraint": self.constraint,
            "partner": self.partner,
            "nets": self.nets.map(|(p, n)| json!([mm(p), mm(n)])),
        })
    }
}

/// A `MINOPTMAX` of lengths (a count of nanometres each, [`MinOptMax`]) as lengths.
fn nm_range(m: &MinOptMax) -> (Nm, Nm, Nm) {
    (Nm::new(m.min()), Nm::new(m.opt()), Nm::new(m.max()))
}

fn judge(len: Nm, t: Option<(Nm, Nm, Nm)>) -> TuningStatus {
    match t {
        Some((min, _, max)) if len > max => {
            let _ = min;
            TuningStatus::TooLong
        }
        Some((min, _, _)) if len < min => TuningStatus::TooShort,
        _ => TuningStatus::Tuned,
    }
}

/// The range of a pattern's target for the kind of tuning, nanometres; `None` when it has none.
fn target_of(p: &TuningPattern) -> Option<TargetRange> {
    match p.mode {
        TuningMode::DiffPairSkew => Some(p.target_skew),
        _ => (!p.target_length.is_unconstrained()).then_some(p.target_length),
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------- hover

/// `PlaceTuningPattern`'s `updateHoverStatus` for the track `pick`: the pattern it would make there, and the length it starts from, without placing anything.
pub struct Hover {
    pub pattern: TuningPattern,
    pub readout: Readout,
    /// The line the track is part of (micrometres), highlighted while it is the one under the cursor.
    pub path: Vec<Point>,
}

pub fn hover(scene: &Scene, kind: TuningMode, pk: &Pick, settings: &Value) -> Result<Hover, String> {
    let mut pattern = default_pattern(&scene.model, kind, &pk.layer);
    pattern.is_time_domain = profile_time_domain(scene, kind, &pk.net);
    apply_settings(&mut pattern, settings);
    let (partner, constraint) = constrain(scene, &mut pattern, pk);
    let params = scene.env.delay_params(&pk.net);
    let path_delay = |id: ItemId| match (&params, assemble_tuning_path(&scene.world, id)) {
        (Some(p), Some(path)) => path.delay(&scene.world, &scene.env, p),
        _ => 0,
    };
    let line = scene.world.assemble_line_with(pk.item, eda_pns::tuning_path::assemble_opts()).ok_or("Please select a track whose length you want to tune.")?;
    let (current, nets, label, delay) = match kind {
        TuningMode::Single => {
            let path = assemble_tuning_path(&scene.world, pk.item).ok_or("Please select a track whose length you want to tune.")?;
            (path.length(&scene.world, &scene.env), None, "current length", path_delay(pk.item))
        }
        TuningMode::DiffPair => {
            let pair = assemble_diff_pair(&scene.world, pk.item).ok_or(NO_PAIR)?;
            let len = |l: &Line| l.segment_ids.first().and_then(|id| assemble_tuning_path(&scene.world, *id)).map(|p| p.length(&scene.world, &scene.env)).unwrap_or(Nm::ZERO);
            let (p, n) = (len(&pair.p), len(&pair.n));
            let delay = pair.p.segment_ids.first().map_or(0, |id| path_delay(*id)).max(pair.n.segment_ids.first().map_or(0, |id| path_delay(*id)));
            (p.max(n), Some((p, n)), "current length", delay)
        }
        TuningMode::DiffPairSkew => {
            let setup = skew_setup(&scene.world, &scene.env, pk.item).map_err(|e| if e.is_empty() { NO_PAIR_FOR_SKEW.to_string() } else { e })?;
            (setup.own_length - setup.coupled_length, Some((setup.own_length, setup.coupled_length)), "current skew", setup.own_delay - setup.coupled_delay)
        }
    };
    let ms = meander_settings(&pattern);
    let target = match kind {
        TuningMode::DiffPairSkew => Some(nm_range(&ms.target_skew)),
        _ => target_of(&pattern).map(|_| nm_range(&ms.target_length)),
    };
    let delay = pattern.is_time_domain.then(|| DelayReadout {
        current: delay,
        label: if kind == TuningMode::DiffPairSkew { "current skew" } else { "current delay" },
        target: match kind {
            TuningMode::DiffPairSkew => Some((ms.target_skew_delay.min(), ms.target_skew_delay.opt(), ms.target_skew_delay.max())),
            _ => (!pattern.target_delay.is_delay_unconstrained()).then(|| (ms.target_length_delay.min(), ms.target_length_delay.opt(), ms.target_length_delay.max())),
        },
    });
    // in the time domain the judge is the length the delay target comes to, as KiCad's (the placer makes it from the path's delay)
    let target = if pattern.is_time_domain { None } else { target };
    let readout = Readout { current, label, target, status: judge(current, target), constraint: constraint.map(|c| c.name), partner, nets, delay };
    Ok(Hover { pattern, readout, path: line.pts.clone() })
}

/// `PCB_TUNING_PATTERN::CreateNew`: a new length or pair pattern on a net whose class names a tuning profile that has the time domain on is a time domain pattern
/// (when a design rule has a length for the net, it is the rule's units that decide, and [`constrain`] sets them afterwards). A skew pattern starts in length.
fn profile_time_domain(scene: &Scene, kind: TuningMode, net: &str) -> bool {
    kind != TuningMode::DiffPairSkew && scene.model.board.tuning_profile_of(net).is_some_and(|p| p.enable_time_domain_tuning)
}

/// `PCB_TUNING_PATTERN::EditStart`'s rule lookup: unless the pattern overrides the custom rules, a length (skew) rule that applies to the net sets its target.
fn constrain(scene: &Scene, pattern: &mut TuningPattern, pk: &Pick) -> (Option<String>, Option<Constraint>) {
    let partner = (pattern.mode != TuningMode::Single).then(|| eda_pns::diff_pair::dp_coupled_net_name(&pk.net)).flatten();
    if pattern.override_custom_rules {
        return (partner, None);
    }
    // `EvalRules( .., aStartItem, nullptr, layer )`: the track the pattern starts on (R7-D4)
    let c = rule_constraint(&scene.design, &scene.model, pattern.mode, &pk.track_id, &pk.layer);
    if let Some(c) = &c {
        if pattern.mode == TuningMode::DiffPairSkew {
            pattern.target_skew = c.range;
        } else {
            pattern.target_length = c.range;
        }
    }
    (partner, c)
}

// ------------------------------------------------------------------------------------------------------------------------------------ tuner

enum Placer {
    Single(MeanderPlacer),
    Pair(DpMeanderPlacer),
}

/// What one move of the cursor made.
pub struct Moved {
    /// The tuned lines (one for a single track, two for a pair) with what each becomes.
    pub edits: Vec<(Line, LineEdit)>,
    pub readout: Readout,
    /// Whether the placer moved at all (the cursor was not on the start).
    pub moved: bool,
}

/// A tuning pattern being made on a board: `PlaceTuningPattern` after the first click.
pub struct Tuner {
    pub scene: Rc<Scene>,
    pub pattern: TuningPattern,
    placer: Placer,
    pub constraint: Option<Constraint>,
    pub partner: Option<String>,
    pub net: String,
    /// The pair's line the clicked track is on is P (for `UpdateSideFromEnd`'s centreline).
    own_is_p: bool,
    skew_coupled: Option<Nm>,
    /// The last cursor position the placer was moved to, and what it made of it.
    last: Option<(Point, Moved)>,
}

impl Tuner {
    /// The first click, on `pk` (`PlaceTuningPattern` creating the pattern and `EditStart` starting the router's placer): the pattern starts on the track at the point nearest `at`.
    /// `settings` are the dialog's, over the board's meander defaults for the kind of tuning.
    pub fn start(scene: Rc<Scene>, kind: TuningMode, pk: &Pick, at: Point, settings: &Value) -> Result<Tuner, String> {
        let mut pattern = default_pattern(&scene.model, kind, &pk.layer);
        pattern.is_time_domain = profile_time_domain(&scene, kind, &pk.net);
        apply_settings(&mut pattern, settings);
        let (partner, constraint) = constrain(&scene, &mut pattern, pk);
        let ms = meander_settings(&pattern);
        let mut skew_coupled = None;
        let mut own_is_p = true;
        let placer = match kind {
            TuningMode::Single => Placer::Single(MeanderPlacer::start(&scene.world, &scene.env, pk.item, at, ms)?),
            TuningMode::DiffPair => {
                let p = DpMeanderPlacer::start(&scene.world, &scene.env, pk.item, at, ms)?;
                own_is_p = p.pair().net_p.as_deref() == Some(pk.net.as_str());
                Placer::Pair(p)
            }
            TuningMode::DiffPairSkew => {
                let setup = skew_setup(&scene.world, &scene.env, pk.item)?;
                skew_coupled = Some(setup.coupled_length);
                own_is_p = setup.own_is_p;
                Placer::Single(MeanderPlacer::start_skew(&scene.world, &scene.env, pk.item, at, ms, setup.coupled_length, setup.coupled_delay)?)
            }
        };
        let origin = match &placer {
            Placer::Single(p) => p.current_start(),
            Placer::Pair(p) => p.current_start(),
        };
        pattern.origin = origin;
        pattern.end = origin;
        pattern.last_netname = pk.net.clone();
        pattern.last_track_width = pk.width;
        Ok(Tuner { scene, pattern, placer, constraint, partner, net: pk.net.clone(), own_is_p, skew_coupled, last: None })
    }

    pub fn settings(&self) -> &MeanderSettings {
        match &self.placer {
            Placer::Single(p) => p.settings(),
            Placer::Pair(p) => p.settings(),
        }
    }

    fn update_settings(&mut self, s: MeanderSettings) {
        match &mut self.placer {
            Placer::Single(p) => p.update_settings(s),
            Placer::Pair(p) => p.update_settings(s),
        }
    }

    /// The dialog's settings, over the pattern (`applyCommonSettings`).
    pub fn set_settings(&mut self, v: &Value) {
        apply_settings(&mut self.pattern, v);
        let (_, constraint) = {
            let pk = Pick { item: self.initial_item(), track_id: String::new(), net: self.net.clone(), layer: self.pattern.layer.clone(), width: self.pattern.last_track_width, at: self.pattern.origin };
            constrain(&self.scene, &mut self.pattern, &pk)
        };
        self.constraint = constraint;
        let s = meander_settings(&self.pattern);
        self.update_settings(s);
    }

    fn initial_item(&self) -> ItemId {
        match &self.placer {
            Placer::Single(p) => p.initial_segment(),
            Placer::Pair(p) => p.initial_segment(),
        }
    }

    /// The `1`-`4` keys: `MEANDER_PLACER_BASE::SpacingStep` / `AmplitudeStep`.
    pub fn step(&mut self, what: &str) -> bool {
        let (amplitude, spacing) = match what {
            "amplitude_up" => (1, 0),
            "amplitude_down" => (-1, 0),
            "spacing_up" => (0, 1),
            "spacing_down" => (0, -1),
            _ => return false,
        };
        match &mut self.placer {
            Placer::Single(p) => {
                if amplitude != 0 {
                    p.amplitude_step(amplitude);
                } else {
                    p.spacing_step(spacing);
                }
            }
            Placer::Pair(p) => {
                if amplitude != 0 {
                    p.amplitude_step(amplitude);
                } else {
                    p.spacing_step(spacing);
                }
            }
        }
        let s = self.settings().clone();
        sync_back(&mut self.pattern, &s);
        true
    }

    /// `UpdateSideFromEnd`: the first meander goes on the side of the baseline's last segment the cursor is on.
    fn side_from_end(&self, raw_primary: &ShapeLineChain, raw_coupled: Option<&ShapeLineChain>, end: Point) -> Option<PatternSide> {
        if raw_primary.segment_count() == 0 {
            return None;
        }
        let mut base = raw_primary.c_segment(raw_primary.segment_count() - 1);
        if let (TuningMode::DiffPair, Some(c)) = (self.pattern.mode, raw_coupled) {
            if c.point_count() > 0 {
                let off = Point { x: Nm::kiround((c.c_last_point().x - raw_primary.c_last_point().x).as_f64() / 2.0), y: Nm::kiround((c.c_last_point().y - raw_primary.c_last_point().y).as_f64() / 2.0) };
                base = Seg::new(Point { x: base.a.x + off.x, y: base.a.y + off.y }, Point { x: base.b.x + off.x, y: base.b.y + off.y });
            }
        }
        if base.a == base.b {
            return None;
        }
        Some(if base.side(end) < 0 { PatternSide::Left } else { PatternSide::Right })
    }

    /// The cursor is at `to`: the placer fits the meander from the start of the pattern to there (`EditStart`, `Update`). `follow_cursor` says whether the side the first
    /// meander is on follows the cursor (`UpdateSideFromEnd`, while placing) or stays what the pattern has (a pattern tuned again).
    pub fn mv(&mut self, to: Point, follow_cursor: bool) -> Result<&Moved, String> {
        // The side goes by a first cut of the line to the cursor, as `EditStart` takes it from the baseline `initBaseLines` has just made.
        if follow_cursor && self.pattern.initial_side != PatternSide::Default {
            let side = match &self.placer {
                Placer::Single(p) => {
                    let chain = chain_of(p.origin_line());
                    let (_, raw, _) = chain.split3(p.current_start(), to);
                    self.side_from_end(&raw, None, to)
                }
                Placer::Pair(p) => {
                    let (cp, cn) = (chain_of(&p.pair().p), chain_of(&p.pair().n));
                    let start = p.current_start();
                    let (_, raw_p, _) = cp.split3(start, to);
                    let (_, raw_n, _) = cn.split3(start, to);
                    if self.own_is_p {
                        self.side_from_end(&raw_p, Some(&raw_n), to)
                    } else {
                        self.side_from_end(&raw_n, Some(&raw_p), to)
                    }
                }
            };
            if let Some(side) = side {
                self.pattern.initial_side = side;
            }
        }
        let ms = meander_settings(&self.pattern);
        self.update_settings(ms);
        let scene = self.scene.clone();
        let (moved, edits, length, status, target, nets, cut, delay_now) = match &mut self.placer {
            Placer::Single(p) => {
                let moved = p.do_move(to);
                let edits = if moved { line_edit(&scene.world, p.origin_line(), &p.tuned(), true).map(|e| vec![(p.origin_line().clone(), e)]).unwrap_or_default() } else { Vec::new() };
                // the skew tuner's status box shows the skew against the skew's own range (`GetPreviewItems`: `statusItem->SetMinMax(
                // m_settings.m_targetSkew.Min(), m_settings.m_targetSkew.Max() )`), not the partner's length plus it (findings p5 F4-3)
                let t = if self.skew_coupled.is_some() {
                    nm_range(&p.settings().target_skew)
                } else {
                    p.target()
                };
                let cut = p.tuned().raw;
                (moved, edits, p.tuning_length_result(), p.tuning_status(), Some(t), None, Some((cut, None)), p.tuning_delay_result())
            }
            Placer::Pair(p) => {
                let moved = p.do_move(to);
                let t = p.tuned();
                let mut edits = Vec::new();
                if moved {
                    for (line, tuned) in [(p.pair().p.clone(), t.p()), (p.pair().n.clone(), t.n())] {
                        if let Some(e) = line_edit(&scene.world, &line, &tuned, true) {
                            edits.push((line, e));
                        }
                    }
                    if edits.len() != 2 {
                        edits.clear();
                    }
                }
                let s = p.settings();
                let (a, b) = p.net_lengths();
                let cut = if self.own_is_p { (t.raw_p.clone(), Some(t.raw_n.clone())) } else { (t.raw_n.clone(), Some(t.raw_p.clone())) };
                (moved, edits, p.tuning_length_result(), p.tuning_status(), Some(nm_range(&s.target_length)), Some((a, b)), Some(cut), p.tuning_delay_result())
            }
        };
        let s = self.settings().clone();
        sync_back(&mut self.pattern, &s);
        let time_domain = self.pattern.is_time_domain;
        let unconstrained = match self.pattern.mode {
            TuningMode::DiffPairSkew => false,
            _ if time_domain => self.pattern.target_delay.is_delay_unconstrained(),
            _ => self.pattern.target_length.is_unconstrained(),
        };
        let target = if unconstrained { None } else { target };
        let label = if self.skew_coupled.is_some() { "current skew" } else { "current length" };
        let delay = time_domain.then(|| DelayReadout {
            current: delay_now,
            label: if self.skew_coupled.is_some() { "current skew" } else { "current delay" },
            target: match self.pattern.mode {
                TuningMode::DiffPairSkew => Some((s.target_skew_delay.min(), s.target_skew_delay.opt(), s.target_skew_delay.max())),
                _ => (!self.pattern.target_delay.is_delay_unconstrained()).then(|| (s.target_length_delay.min(), s.target_length_delay.opt(), s.target_length_delay.max())),
            },
        });
        let readout = Readout { current: length, label, target, status: if moved { status } else { judge(length, target) }, constraint: self.constraint.as_ref().map(|c| c.name.clone()), partner: self.partner.clone(), nets, delay };
        if moved && !edits.is_empty() {
            if let Some((primary, coupled)) = cut {
                self.record_cut(&primary, coupled.as_ref(), length, status);
                self.pattern.last_tuning_delay = if time_domain { delay_now } else { 0 };
            }
        }
        self.last = Some((to, Moved { edits, readout, moved }));
        Ok(&self.last.as_ref().expect("just set").1)
    }

    /// The pattern's record of the cut it was made on (`m_origin`, `m_end`, `m_baseLine`, `m_baseLineCoupled`) and of the result (`m_tuningLength`, `m_tuningStatus`).
    fn record_cut(&mut self, primary: &ShapeLineChain, coupled: Option<&ShapeLineChain>, length: Nm, status: TuningStatus) {
        let pts = |c: &ShapeLineChain| c.points().to_vec();
        if primary.point_count() >= 2 {
            self.pattern.base_line = pts(primary);
            self.pattern.origin = primary.c_point(0);
            self.pattern.end = primary.c_last_point();
        }
        self.pattern.base_line_coupled = match (self.pattern.mode, coupled) {
            (TuningMode::DiffPair | TuningMode::DiffPairSkew, Some(c)) if c.point_count() >= 2 => pts(c),
            _ => Vec::new(),
        };
        if let Placer::Pair(p) = &self.placer {
            self.pattern.last_track_width = p.pair().width;
            self.pattern.last_diff_pair_gap = p.pair().gap.max(Nm::ZERO);
        }
        self.pattern.last_tuning_length = length;
        self.pattern.last_status = status.name().to_string();
    }

    /// What the last move left, if it made anything.
    pub fn last(&self) -> Option<&Moved> {
        self.last.as_ref().map(|(_, m)| m)
    }

    pub fn last_end(&self) -> Option<Point> {
        self.last.as_ref().map(|(p, _)| *p)
    }

    /// `EditFinish`: the command that puts the pattern and its tracks on the board, from what the last move made.
    pub fn command(&self) -> Result<Cmd, String> {
        let moved = self.last().filter(|m| m.moved && !m.edits.is_empty()).ok_or("Move the cursor along the track to where the pattern ends first.")?;
        let layers = &self.scene.layers;
        let mut remove: Vec<String> = Vec::new();
        let mut tracks: Vec<Track> = Vec::new();
        let mut members: Vec<usize> = Vec::new();
        for (line, edit) in &moved.edits {
            let (net, layer, width) = (Scene::net_name(&line.net), layers.name_of(line.layer).to_string(), line.width);
            for id in &edit.remove_track_ids {
                if !remove.contains(id) {
                    remove.push(id.clone());
                }
            }
            for kept in &edit.keep {
                tracks.push(Track { id: String::new(), net: net.clone(), pins: Vec::new(), layer: layer.clone(), width, pts: kept.pts.clone(), arc_mid_offset: None });
            }
            for piece in &edit.pieces {
                if piece.member {
                    members.push(tracks.len());
                }
                tracks.push(track_of(&net, &layer, width, &piece.shape));
            }
        }
        Ok(Cmd::CommitTuning { pattern: Some(self.pattern.clone()), remove_pattern: None, remove_track_ids: remove, tracks, member_indices: members })
    }
}

fn track_of(net: &str, layer: &str, width: Nm, shape: &PieceShape) -> Track {
    match shape {
        PieceShape::Poly(pts) => Track { id: String::new(), net: net.to_string(), pins: Vec::new(), layer: layer.to_string(), width, pts: pts.clone(), arc_mid_offset: None },
        PieceShape::Arc { start, mid, end } => Track::new_arc(net.to_string(), layer.to_string(), width, *start, *mid, *end),
    }
}

/// The preview of a move, as the studio draws it: every track the edit makes (the line up to the pattern, the meander, the line after it, what stays of the tracks it
/// replaces), the ids of the tracks it replaces (hidden while it is shown), the pattern's outline.
pub fn preview_json(scene: &Scene, moved: &Moved) -> Value {
    let mut pieces: Vec<Value> = Vec::new();
    let mut hide: Vec<String> = Vec::new();
    for (line, edit) in &moved.edits {
        let (net, layer, width) = (Scene::net_name(&line.net), scene.layers.name_of(line.layer).to_string(), line.width);
        for id in &edit.remove_track_ids {
            if !hide.contains(id) {
                hide.push(id.clone());
            }
        }
        let pts_json = |pts: &[Point]| Value::Array(pts.iter().map(|p| json!([p.x, p.y])).collect());
        for kept in &edit.keep {
            pieces.push(json!({ "pts": pts_json(&kept.pts), "member": false, "net": net, "layer": layer, "width": width }));
        }
        for piece in &edit.pieces {
            let pts: Vec<Point> = match &piece.shape {
                PieceShape::Poly(pts) => pts.clone(),
                PieceShape::Arc { start, mid, end } => tessellate_arc(*start, *mid, *end, 16),
            };
            pieces.push(json!({ "pts": pts_json(&pts), "member": piece.member, "net": net, "layer": layer, "width": width }));
        }
    }
    json!({ "pieces": pieces, "hide": hide })
}

// ----------------------------------------------------------------------------------------------------------------------------- existing patterns

fn apply_cmd(scene: &Scene, cmd: &Cmd) -> Result<Design, String> {
    let mut board = Board::new(scene.design.clone(), &scene.model, Nm::new(1), Nm::ZERO);
    board.apply(cmd).map_err(|e| crate::board::reasons(&e))?;
    Ok(board.design().clone())
}

/// `pickSegment` over a baseline: the world's segment on `layer` nearest `at`, within the clearance and half the width of it, preferring the one whose middle is
/// nearest the baseline.
pub fn segment_near(scene: &Scene, at: Point, layer: &str, baseline: &[Point]) -> Option<ItemId> {
    let layer_idx = scene.layers.index_of(layer)?;
    let mut best: Option<((i128, i128), ItemId)> = None;
    for (id, item) in scene.world.iter() {
        let Item::Segment(s) = item else { continue };
        if s.layer != layer_idx || s.net.is_none() {
            continue;
        }
        let seg = Seg::new(s.a, s.b);
        let slop = Node::clearance(&scene.env.rules, &s.net, &None) + s.width / 2 + Nm::new(1);
        let d = seg.squared_distance(at);
        if d > slop.area(slop) {
            continue;
        }
        let centre = Point { x: (s.a.x + s.b.x) / 2, y: (s.a.y + s.b.y) / 2 };
        let dc = baseline.windows(2).map(|w| Seg::new(w[0], w[1]).squared_distance(centre)).min().unwrap_or(0);
        let key = (d, dc);
        if best.as_ref().is_none_or(|(k, _)| key < *k) {
            best = Some((key, id));
        }
    }
    best.map(|(_, id)| id)
}

/// `getPNSLine`: the line of the world that both ends of `baseline` are on.
fn line_of_baseline(scene: &Scene, baseline: &[Point], layer: &str) -> Option<Line> {
    let (first, last) = (*baseline.first()?, *baseline.last()?);
    for at in [first, last] {
        let Some(item) = segment_near(scene, at, layer, baseline) else { continue };
        let Some(line) = scene.world.assemble_line_with(item, eda_pns::tuning_path::assemble_opts()) else { continue };
        let chain = chain_of(&line);
        if chain.point_on_edge(first, um(1.0)) && chain.point_on_edge(last, um(1.0)) {
            return Some(line);
        }
    }
    None
}

/// What `resetToBaseline` / `removeToBaseline` make of one line: the stretch the pattern is on put back as the baseline it was made on.
fn baseline_edit(scene: &Scene, pattern: &TuningPattern, baseline: &[Point]) -> Result<(Line, LineEdit), String> {
    let line = line_of_baseline(scene, baseline, &pattern.layer).ok_or("Unable to find the tracks this tuning pattern was made on: they have been moved or edited since.")?;
    let chain = chain_of(&line);
    let (first, last) = (baseline[0], (*baseline.last().unwrap()));
    let (pre, raw, post) = chain.split3(first, last);
    if raw.point_count() < 2 {
        return Err("The tuning pattern has nothing between its start and its end.".into());
    }
    let mut straight = ShapeLineChain::from_points(baseline);
    // the baseline starts and ends where the cut did: the points of the line nearest its two ends
    straight.set_point(0, raw.c_point(0));
    let n = straight.point_count() - 1;
    straight.set_point(n, raw.c_last_point());
    let tuned = Tuned { pre, raw, tuned: straight, post };
    let edit = line_edit(&scene.world, &line, &tuned, false).ok_or("The tuning pattern's stretch of track could not be put back.")?;
    Ok((line, edit))
}

/// `PCB_TUNING_PATTERN::Remove` (and the first half of `Update`): the pattern's tracks are replaced by the baseline, the pattern is gone.
pub fn reset_cmd(scene: &Scene, pattern: &TuningPattern) -> Result<Cmd, String> {
    let mut remove: Vec<String> = Vec::new();
    let mut tracks: Vec<Track> = Vec::new();
    let mut baselines = vec![pattern.base_line.as_slice()];
    // a pair's pattern puts both lines back (`if( m_tuningMode == DIFF_PAIR )`); a skew pattern only ever touched its own
    if pattern.mode == TuningMode::DiffPair {
        baselines.push(pattern.base_line_coupled.as_slice());
    }
    for baseline in baselines {
        if baseline.len() < 2 {
            return Err("The tuning pattern has no baseline to put back.".into());
        }
        let (line, edit) = baseline_edit(scene, pattern, baseline)?;
        let (net, layer, width) = (Scene::net_name(&line.net), scene.layers.name_of(line.layer).to_string(), line.width);
        for id in edit.remove_track_ids {
            if !remove.contains(&id) {
                remove.push(id);
            }
        }
        for kept in &edit.keep {
            tracks.push(Track { id: String::new(), net: net.clone(), pins: Vec::new(), layer: layer.clone(), width, pts: kept.pts.clone(), arc_mid_offset: None });
        }
        for piece in &edit.pieces {
            tracks.push(track_of(&net, &layer, width, &piece.shape));
        }
    }
    // the members go too, wherever they are (a pattern edited by hand since): they are the pattern's
    for m in &pattern.member_ids {
        if !remove.contains(m) {
            remove.push(m.clone());
        }
    }
    Ok(Cmd::CommitTuning { pattern: None, remove_pattern: Some(pattern.id.clone()), remove_track_ids: remove, tracks, member_indices: Vec::new() })
}

/// What a pattern being tuned again keeps beside its tuner: the command that put its tracks back to the baseline, the board that makes and the pattern as it was.
pub struct EditCtx {
    pub reset: Cmd,
    pub after: Rc<Scene>,
    /// Where the pattern ends (the point the tuner is moved to).
    pub end: Point,
    pub pattern: TuningPattern,
    pub settings: Value,
}

/// A pattern being tuned again.
pub struct Retune {
    pub ctx: EditCtx,
    pub tuner: Tuner,
}

fn point_setting(v: &Value, key: &str) -> Option<Point> {
    let p = v.get(key)?;
    Some(Point { x: Nm::kiround(p.get("x")?.as_f64()?), y: Nm::kiround(p.get("y")?.as_f64()?) })
}

/// The settings of a pattern as the request keys name them.
const SETTING_KEYS: [&str; 10] = ["min_amplitude", "max_amplitude", "spacing", "corner_radius_percent", "rounded", "single_sided", "override_custom_rules", "initial_side", "target_length", "target_skew"];

/// A tuner on `after` for `pattern`, starting at `origin`: the pattern's own settings, the request's over them (the length target of a rule is looked up again
/// unless the pattern overrides the rules).
fn tuner_for(after: Rc<Scene>, pattern: &TuningPattern, settings: &Value, origin: Point) -> Result<Tuner, String> {
    let start = segment_near(&after, origin, &pattern.layer, &pattern.base_line).ok_or("Unable to find the track the tuning pattern starts on.")?;
    let (net, width) = match after.world.get(start) {
        Some(Item::Segment(s)) => (Scene::net_name(&s.net), s.width),
        _ => return Err("Unable to find the track the tuning pattern starts on.".into()),
    };
    let pk = Pick { item: start, track_id: String::new(), net, layer: pattern.layer.clone(), width, at: origin };
    let mut carried = Map::new();
    let me = serde_json::to_value(pattern).map_err(|e| e.to_string())?;
    for key in SETTING_KEYS {
        if let Some(v) = me.get(key) {
            carried.insert(key.into(), v.clone());
        }
    }
    if let Some(over) = settings.as_object() {
        for (k, v) in over {
            carried.insert(k.clone(), v.clone());
        }
    }
    let mut tuner = Tuner::start(after, pattern.mode, &pk, origin, &Value::Object(carried))?;
    tuner.pattern.id = pattern.id.clone();
    Ok(tuner)
}

/// `PCB_TUNING_PATTERN::EditStart` over a pattern that exists: its tracks put back to the baseline (`resetToBaseline`) and the placer started on the result.
/// `settings` (JSON) may carry new `origin` / `end` points and any of the dialog's settings.
pub fn begin_retune(scene: &Scene, pattern: &TuningPattern, settings: &Value) -> Result<Retune, String> {
    let reset = reset_cmd(scene, pattern)?;
    let after = Rc::new(Scene::of(apply_cmd(scene, &reset)?, scene.model.clone()));
    let origin = point_setting(settings, "origin").unwrap_or(pattern.origin);
    let end = point_setting(settings, "end").unwrap_or(pattern.end);
    let tuner = tuner_for(after.clone(), pattern, settings, origin)?;
    Ok(Retune { ctx: EditCtx { reset, after, end, pattern: pattern.clone(), settings: settings.clone() }, tuner })
}

impl EditCtx {
    /// The start of the pattern moved (its first edit point): the placer is started again at the new origin, on the same board, with the settings the tuner has now.
    pub fn tuner_at(&self, origin: Point, now: &TuningPattern) -> Result<Tuner, String> {
        let mut settings = if self.settings.is_object() { self.settings.clone() } else { json!({}) };
        let current = serde_json::to_value(now).map_err(|e| e.to_string())?;
        if let Some(o) = settings.as_object_mut() {
            for key in SETTING_KEYS {
                if let Some(v) = current.get(key) {
                    o.insert(key.into(), v.clone());
                }
            }
        }
        tuner_for(self.after.clone(), &self.pattern, &settings, origin)
    }
}

impl Retune {
    /// `Update`: the placer run from the start of the pattern to its end.
    pub fn fit(&mut self) -> Result<&Moved, String> {
        let end = self.ctx.end;
        fit(&mut self.tuner, end)
    }

    /// `EditFinish`: the commands that make the change, in order (one undo step).
    pub fn commands(&self) -> Result<Vec<Cmd>, String> {
        Ok(vec![self.ctx.reset.clone(), self.tuner.command()?])
    }
}

/// `Update` for a tuner of a pattern that exists: moved to `end`, the first meander on the side the pattern says.
pub fn fit(tuner: &mut Tuner, end: Point) -> Result<&Moved, String> {
    let moved = tuner.mv(end, false)?;
    if !moved.moved || moved.edits.is_empty() {
        return Err("The tuning pattern could not be fitted again: the tracks between its start and its end are gone.".into());
    }
    Ok(tuner.last().expect("just moved"))
}

/// `PCB_TUNING_PATTERN::EditStart`, `Update`, `EditFinish` over a pattern that exists: its tracks put back to the baseline and the placer run over it again with
/// `settings` (JSON, over what the pattern has). The commands, in order, are one undo step (`Cmd::Batch`).
pub fn retune_cmds(scene: &Scene, pattern: &TuningPattern, settings: &Value) -> Result<Vec<Cmd>, String> {
    let mut r = begin_retune(scene, pattern, settings)?;
    r.fit()?;
    r.commands()
}

// ------------------------------------------------------------------------------------------------------------------------------------- edit points

fn half(a: Nm) -> Nm {
    Nm::kiround(a.as_f64() / 2.0)
}

/// The offsets of a pair's centreline from the baseline of the clicked line, at the start and at the end (`MakeEditPoints`).
fn centerline_offsets(p: &TuningPattern) -> (Point, Point) {
    if p.mode == TuningMode::DiffPair && p.base_line_coupled.len() >= 2 {
        let (first, last) = (p.base_line_coupled[0], *p.base_line_coupled.last().unwrap());
        (Point { x: half(first.x - p.origin.x), y: half(first.y - p.origin.y) }, Point { x: half(last.x - p.end.x), y: half(last.y - p.end.y) })
    } else {
        (Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::ZERO, y: Nm::ZERO })
    }
}

/// The first segment of the pattern's baseline, moved to the centreline of a pair.
fn base_segment(p: &TuningPattern) -> (Point, Point) {
    let (off, _) = centerline_offsets(p);
    let (a, b) = if p.base_line.len() >= 2 { (p.base_line[0], p.base_line[1]) } else { (p.origin, p.end) };
    (Point { x: a.x + off.x, y: a.y + off.y }, Point { x: b.x + off.x, y: b.y + off.y })
}

fn resized(v: (f64, f64), len: f64) -> (f64, f64) {
    let n = v.0.hypot(v.1);
    if n == 0.0 {
        (0.0, 0.0)
    } else {
        (v.0 * len / n, v.1 * len / n)
    }
}

/// `PCB_TUNING_PATTERN::MakeEditPoints`: the pattern's four handles (micrometres) -- where it starts, where it ends, the width of its meanders (on the side the first
/// one is) and their spacing.
pub fn handle_positions(p: &TuningPattern) -> [Point; 4] {
    let (off, off_end) = centerline_offsets(p);
    let (a, b) = base_segment(p);
    let mut amplitude = p.max_amplitude + half(p.last_track_width);
    if p.mode == TuningMode::DiffPair {
        amplitude += p.last_track_width + p.last_diff_pair_gap;
    }
    if p.initial_side == PatternSide::Left {
        amplitude = -amplitude;
    }
    let dir = ((b.x - a.x).as_f64(), (b.y - a.y).as_f64());
    let perp = (-dir.1, dir.0);
    let w = resized(perp, amplitude.as_f64());
    let along = resized(dir, (p.spacing.as_f64() * 1.5).round());
    let width_handle = Point { x: a.x + Nm::kiround(w.0), y: a.y + Nm::kiround(w.1) };
    let spacing_handle = Point { x: a.x + Nm::kiround(w.0 + along.0), y: a.y + Nm::kiround(w.1 + along.1) };
    [Point { x: p.origin.x + off.x, y: p.origin.y + off.y }, Point { x: p.end.x + off_end.x, y: p.end.y + off_end.y }, width_handle, spacing_handle]
}

/// The distance from `q` to the infinite line through `a` and `b`.
fn line_distance(a: Point, b: Point, q: Point) -> f64 {
    let (dx, dy) = ((b.x - a.x).as_f64(), (b.y - a.y).as_f64());
    let n = dx.hypot(dy);
    if n == 0.0 {
        return ((q.x - a.x).as_f64()).hypot((q.y - a.y).as_f64());
    }
    (dx * (q.y - a.y).as_f64() - dy * (q.x - a.x).as_f64()).abs() / n
}

/// `PCB_TUNING_PATTERN::UpdateFromEditPoints` for one handle moved to `pos`: the start or the end of the pattern, the amplitude and the side of its meanders,
/// the spacing between them. Amplitude and spacing keep to 10 micrometres.
pub fn apply_handle(p: &mut TuningPattern, handle: usize, pos: Point) {
    let (off, off_end) = centerline_offsets(p);
    let (a, b) = base_segment(p);
    let ten = |v: f64| Nm::new(10_000) * (v / 10_000.0).round() as i64;
    match handle {
        0 => p.origin = Point { x: pos.x - off.x, y: pos.y - off.y },
        1 => p.end = Point { x: pos.x - off_end.x, y: pos.y - off_end.y },
        2 => {
            let mut value = line_distance(a, b, pos) - (p.last_track_width.as_f64() / 2.0).round();
            if p.mode == TuningMode::DiffPair {
                value -= (p.last_track_width + p.last_diff_pair_gap).as_f64();
            }
            p.max_amplitude = ten(value).max(Nm::new(1));
            // `base.Side( wHandle ) < 0` is the left
            let side = Seg::new(a, b).side(pos);
            p.initial_side = if side < 0 { PatternSide::Left } else { PatternSide::Right };
        }
        3 => {
            let w = handle_positions(p)[2];
            let value = (line_distance(a, w, pos) / 1.5).round();
            p.spacing = ten(value).max(Nm::new(1));
        }
        _ => {}
    }
}

/// Applies `cmds` in order to a copy of the scene's board, for the next pattern of a batch to be tuned on.
pub fn apply_all(scene: &Scene, cmds: &[Cmd]) -> Result<Scene, String> {
    let mut board = Board::new(scene.design.clone(), &scene.model, Nm::new(1), Nm::ZERO);
    for c in cmds {
        board.apply(c).map_err(|e| crate::board::reasons(&e))?;
    }
    Ok(Scene::of(board.design().clone(), scene.model.clone()))
}

/// Does the design's `design.json` carry this pattern? The patterns, ordered as the file keeps them.
pub fn patterns_of(design: &Design) -> Vec<TuningPattern> {
    design.drawings.as_ref().map(|d| d.tuning_patterns.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::units::mm;

    fn rule(name: &str, ty: &str, min: Option<Nm>, max: Option<Nm>, opt: Option<Nm>, cond: Option<&str>) -> eda_model::CustomRule {
        eda_model::CustomRule { name: name.into(), constraint_type: ty.into(), min, max, opt, layer: None, severity: None, condition: cond.map(String::from) }
    }

    #[test]
    fn a_length_rule_sets_the_target_the_way_set_target_length_does() {
        let mut rules = BoardRules::default();
        rules.custom_rules = vec![rule("all", "length", Some(mm(99.9)), Some(mm(100.1)), None, None)];
        let c = constraint_for(&rules, TuningMode::Single, "SIG", None, "F.Cu").expect("a rule");
        // Opt() with no opt is the minimum; the rule's own min and max win over the tolerance round it
        assert_eq!(c.range, TargetRange { min: 99_900_000, opt: 99_900_000, max: 100_100_000 });
        assert_eq!(c.name, "all");
        // a rule with an optimum only: the default tolerance either side
        rules.custom_rules = vec![rule("opt", "length", None, None, Some(mm(50.0)), None)];
        assert_eq!(constraint_for(&rules, TuningMode::DiffPair, "SIG", None, "F.Cu").unwrap().range, TargetRange { min: 49_900_000, opt: 50_000_000, max: 50_100_000 });
        // a skew rule is for the skew tuner only
        rules.custom_rules = vec![rule("skew", "skew", None, Some(mm(0.1)), None, None)];
        assert!(constraint_for(&rules, TuningMode::Single, "SIG", None, "F.Cu").is_none());
        assert_eq!(constraint_for(&rules, TuningMode::DiffPairSkew, "USB_P", Some("USB_N"), "F.Cu").unwrap().range, TargetRange { min: -100_000, opt: 0, max: 100_000 });
    }

    #[test]
    fn the_last_rule_that_applies_wins_and_a_condition_picks_the_nets() {
        let mut rules = BoardRules::default();
        rules.custom_rules = vec![
            rule("first", "length", Some(mm(10.0)), Some(mm(10.2)), None, None),
            rule("clk", "length", Some(mm(20.0)), Some(mm(20.2)), None, Some("A.NetName == 'CLK'")),
            rule("never", "length", Some(mm(30.0)), Some(mm(30.2)), None, Some("A.NetName == 'OTHER'")),
        ];
        assert_eq!(constraint_for(&rules, TuningMode::Single, "CLK", None, "F.Cu").unwrap().name, "clk");
        assert_eq!(constraint_for(&rules, TuningMode::Single, "SIG", None, "F.Cu").unwrap().name, "first");
        rules.custom_rules[0].layer = Some("B.Cu".into());
        assert!(constraint_for(&rules, TuningMode::Single, "SIG", None, "F.Cu").is_none(), "a rule for another layer");
    }

    /// r6-board-io F4: a rule of two constraints gives the length tuner its length whichever comes first, and `(layer outer)` takes in F.Cu.
    #[test]
    fn a_rule_on_the_outer_layers_targets_a_track_on_f_cu() {
        let mut rules = BoardRules::default();
        rules.custom_rules = eda_kicad::parse_custom_rules("(version 1)\n(rule L (constraint skew (max 0.1mm)) (constraint length (min 40mm) (max 41mm)) (layer outer) (condition \"A.NetName == 'CLK'\"))\n");
        let c = constraint_for(&rules, TuningMode::Single, "CLK", None, "F.Cu").expect("the length of rule L");
        assert_eq!((c.name.as_str(), c.range.min, c.range.max), ("L", 40_000_000, 41_000_000));
        assert!(constraint_for(&rules, TuningMode::Single, "CLK", None, "In1.Cu").is_none(), "an inner layer is not outer");
        assert_eq!(constraint_for(&rules, TuningMode::DiffPairSkew, "CLK", Some("CLK_N"), "B.Cu").expect("the skew of rule L").range.max, 100_000);
    }

    fn pattern_for_handles(mode: TuningMode) -> TuningPattern {
        let mut p = default_pattern(&ConstraintModel::default(), mode, "F.Cu");
        p.origin = Point { x: mm(10.0), y: mm(5.0) };
        p.end = Point { x: mm(30.0), y: mm(5.0) };
        p.base_line = vec![p.origin, p.end];
        p.last_track_width = mm(0.25);
        p.max_amplitude = mm(1.0);
        p.spacing = mm(0.6);
        p.initial_side = PatternSide::Right;
        if mode == TuningMode::DiffPair {
            p.base_line_coupled = vec![Point { x: mm(10.0), y: mm(5.7) }, Point { x: mm(30.0), y: mm(5.7) }];
            p.last_diff_pair_gap = mm(0.45);
        }
        p
    }

    #[test]
    fn the_edit_points_of_a_pattern_are_where_make_edit_points_puts_them() {
        let p = pattern_for_handles(TuningMode::Single);
        let h = handle_positions(&p);
        assert_eq!(h[0], Point { x: mm(10.0), y: mm(5.0) });
        assert_eq!(h[1], Point { x: mm(30.0), y: mm(5.0) });
        // the width handle is the amplitude and half the track away from the start of the baseline, on the side of the first meander: perpendicular is (-dy, dx), so +y for a baseline along +x
        assert_eq!(h[2], Point { x: mm(10.0), y: mm(5.0) + mm(1.0) + Nm::new(125_000) });
        // the spacing handle is one and a half spacings along from it
        assert_eq!(h[3], Point { x: mm(10.0) + mm(0.9), y: mm(5.0) + Nm::new(1_125_000) });
        let mut left = p.clone();
        left.initial_side = PatternSide::Left;
        assert_eq!(handle_positions(&left)[2], Point { x: mm(10.0), y: mm(5.0) - Nm::new(1_125_000) });
        // a pair's handles are on the centreline: half way to the partner's baseline, and the amplitude is counted from there
        let d = handle_positions(&pattern_for_handles(TuningMode::DiffPair));
        assert_eq!(d[0], Point { x: mm(10.0), y: mm(5.35) });
        assert_eq!(d[2], Point { x: mm(10.0), y: mm(5.35) + mm(1.0) + Nm::new(125_000) + mm(0.25) + mm(0.45) });
    }

    #[test]
    fn dragging_a_handle_to_where_it_is_changes_nothing_and_elsewhere_changes_what_it_stands_for() {
        for mode in [TuningMode::Single, TuningMode::DiffPair] {
            let p = pattern_for_handles(mode);
            let h = handle_positions(&p);
            for (i, at) in h.iter().enumerate() {
                let mut q = p.clone();
                apply_handle(&mut q, i, *at);
                assert_eq!(q, p, "{mode:?} handle {i}");
            }
        }
        let mut p = pattern_for_handles(TuningMode::Single);
        // the width handle dragged to the other side and further out: a wider pattern that starts on the left
        apply_handle(&mut p, 2, Point { x: mm(10.0), y: mm(5.0) - Nm::new(1_525_000) });
        assert_eq!((p.max_amplitude, p.initial_side), (mm(1.4), PatternSide::Left));
        // the spacing handle dragged two spacings along from the width handle's line
        let w = handle_positions(&p)[2];
        apply_handle(&mut p, 3, Point { x: w.x + mm(1.5), y: w.y });
        assert_eq!(p.spacing, mm(1.0));
        apply_handle(&mut p, 1, Point { x: mm(25.0), y: mm(5.0) });
        assert_eq!(p.end, Point { x: mm(25.0), y: mm(5.0) });
    }

    #[test]
    fn settings_come_from_the_page_then_the_request() {
        let model = ConstraintModel::default();
        let mut p = default_pattern(&model, TuningMode::DiffPair, "F.Cu");
        assert_eq!((p.min_amplitude, p.max_amplitude, p.spacing, p.corner_radius_percent, p.rounded), (mm(0.2), mm(1.0), mm(1.0), 80, true), "the pair's page: a spacing of 1 mm");
        apply_settings(&mut p, &json!({ "max_amplitude": 1_500_000, "spacing": 800_000.0, "rounded": false, "initial_side": "right", "target_length": 52_000_000, "target_skew": { "max": 50_000 } }));
        assert_eq!((p.max_amplitude, p.spacing, p.rounded, p.initial_side), (mm(1.5), mm(0.8), false, PatternSide::Right));
        assert_eq!(p.target_length, TargetRange { min: 51_900_000, opt: 52_000_000, max: 52_100_000 });
        assert_eq!(p.target_skew, TargetRange { min: -100_000, opt: 0, max: 50_000 });
        apply_settings(&mut p, &json!({ "target_length": null }));
        assert!(p.target_length.is_unconstrained());
        let ms = meander_settings(&p);
        assert_eq!((ms.max_amplitude, ms.spacing, ms.corner_style, ms.keep_endpoints), (mm(1.5), mm(0.8), MeanderStyle::Chamfer, true));
        assert_eq!(ms.target_length.opt(), LENGTH_UNCONSTRAINED);
    }

    /// KiCad's own board with tuning patterns (`qa/data/pcbnew/issue22536.kicad_pcb`: four patterns made by KiCad on three copper layers, each tuned to 3000 mil): every one is
    /// tuned again from the baseline it was made on and comes out within the range it was made to -- the placer reproduces what KiCad's did, over KiCad's own arcs. Runs
    /// when the KiCad source is at `KICAD_SRC_DIR`.
    #[test]
    fn the_tuning_patterns_of_a_board_kicad_made_are_tuned_again_to_the_range_they_were_made_to() {
        let Ok(dir) = std::env::var("KICAD_SRC_DIR") else { return };
        let Ok(text) = std::fs::read_to_string(std::path::Path::new(&dir).join("qa/data/pcbnew/issue22536.kicad_pcb")) else { return };
        let (design, model, _) = eda_kicad::import_kicad_pcb(&text).unwrap();
        let scene = Scene::of(design, model);
        let patterns = patterns_of(&scene.design);
        assert_eq!(patterns.len(), 4);
        let mut tuned_again = 0;
        for p in &patterns {
            let (lo, hi) = (p.target_length.min as f64 / 1e6, p.target_length.max as f64 / 1e6);
            let cmds = match retune_cmds(&scene, p, &Value::Null) {
                Ok(c) => c,
                Err(e) => panic!("pattern {} ({}, {} members): {e}", p.id, p.layer, p.member_ids.len()),
            };
            let after = apply_all(&scene, &cmds).expect("the commands apply");
            let q = patterns_of(&after.design).into_iter().find(|q| q.id == p.id).expect("the pattern is still there");
            let mm = q.last_tuning_length.mm();
            eprintln!("pattern {} on {}: {} members -> {} after, {mm:.4} mm, made to {lo:.4}..{hi:.4} ({})", p.id, p.layer, p.member_ids.len(), q.member_ids.len(), q.last_status);
            assert!(mm >= lo && mm <= hi, "pattern {} on {}: {mm:.4} mm, made to {lo:.4}..{hi:.4}", p.id, p.layer);
            // and the board's own measure of the net agrees with the placer's
            let seg = after.world.iter().find_map(|(id, it)| match it {
                Item::Segment(s) if s.net.as_deref() == Some(q.last_netname.as_str()) && after.layers.name_of(s.layer) == q.layer => Some(id),
                _ => None,
            });
            if let Some(seg) = seg {
                let len = assemble_tuning_path(&after.world, seg).map(|t| t.length(&after.world, &after.env)).unwrap_or(Nm::ZERO).mm();
                assert!((len - mm).abs() < 0.06, "pattern {}: the net is {len:.4} mm on the board, the placer said {mm:.4}", p.id);
            }
            tuned_again += 1;
        }
        assert_eq!(tuned_again, 4);
    }

    /// kicad-cli's own lengths for the nets of `qa/data/pcbnew/length_calculations.kicad_pcb` (a `.kicad_dru` rule `(constraint length (max 0.001mm))` that nothing can meet
    /// makes `kicad-cli pcb drc` report each net's actual length; these are what it said): the tuner's measure of the same nets is the same, through pads, vias and the stackup.
    /// The 1.3 um of `Net-(R10-Pad1)` is its arc, which the world has as 32 chords.
    #[test]
    fn the_length_of_a_net_is_what_kicad_cli_measures_on_its_own_length_calculation_board() {
        let Ok(dir) = std::env::var("KICAD_SRC_DIR") else { return };
        let Ok(text) = std::fs::read_to_string(std::path::Path::new(&dir).join("qa/data/pcbnew/length_calculations.kicad_pcb")) else { return };
        let (design, model, _) = eda_kicad::import_kicad_pcb(&text).unwrap();
        let scene = Scene::of(design, model);
        for (net, kicad) in [("Net-(R1-Pad2)", 13.3350), ("Net-(R10-Pad1)", 13.5475), ("Net-(R11-Pad2)", 13.3350), ("Net-(R13-Pad2)", 33.5100), ("Net-(R15-Pad2)", 28.4750), ("Net-(R3-Pad2)", 28.4750), ("Net-(R5-Pad2)", 28.4750), ("Net-(R7-Pad2)", 43.6150)] {
            let seg = scene.world.iter().find_map(|(id, it)| match it {
                Item::Segment(s) if s.net.as_deref() == Some(net) => Some(id),
                _ => None,
            });
            let Some(seg) = seg else { panic!("no track on {net}") };
            let path = assemble_tuning_path(&scene.world, seg).unwrap();
            let ours = path.length(&scene.world, &scene.env).mm();
            assert!((ours - kicad).abs() < 0.003, "{net}: kicad-cli says {kicad:.4} mm, the tuner measures {ours:.4} ({} items)", path.items.len());
        }
    }

    /// The tuner on real boards: KiCad's own QA boards, a few tracks of each picked at random, every kind of tuning asked to lengthen them by 3 mm to a cursor along the track.
    /// Nothing may panic, what the tuner makes must be accepted by `Cmd::CommitTuning`, and a single track that comes out `tuned` must be as long on the board as the placer said.
    /// Slow tier (`EDA_SLOW_TESTS`, and `KICAD_SRC_DIR`).
    #[test]
    fn the_tuner_on_the_boards_of_kicads_own_qa_never_makes_what_the_board_would_refuse() {
        let (Ok(dir), true) = (std::env::var("KICAD_SRC_DIR"), std::env::var_os("EDA_SLOW_TESTS").is_some()) else { return };
        let mut rng = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move |n: u64| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng % n.max(1)
        };
        let (mut tried, mut started, mut committed, mut measured, mut in_pad) = (0, 0, 0, 0, 0);
        for name in ["issue8909", "issue3812", "issue6284", "issue5093", "issue14559", "issue11814", "stonehenge", "issue7325", "issue5102", "issue6039", "length_calculations"] {
            let Ok(text) = std::fs::read_to_string(std::path::Path::new(&dir).join(format!("qa/data/pcbnew/{name}.kicad_pcb"))) else { continue };
            let Ok((design, model, _)) = eda_kicad::import_kicad_pcb(&text) else { continue };
            let scene = Rc::new(Scene::of(design, model));
            // the straight segments of the nets worth tuning: long enough to have a stretch to put a meander on
            let mut candidates: Vec<(ItemId, Point, Point, String)> = scene
                .world
                .iter()
                .filter_map(|(id, it)| match it {
                    Item::Segment(s) if s.net.is_some() && s.source_track.is_some() && ((s.b.x - s.a.x).abs() + (s.b.y - s.a.y).abs()) > mm(3.0) => Some((id, s.a, s.b, scene.layers.name_of(s.layer).to_string())),
                    _ => None,
                })
                .collect();
            candidates.sort_by_key(|c| c.0);
            for _ in 0..12 {
                if candidates.is_empty() {
                    break;
                }
                let (_, a, b, layer) = candidates[next(candidates.len() as u64) as usize].clone();
                let mid = Point { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
                let Some(pk) = pick(&scene, mid.x, mid.y, mm(0.06), Some(&layer)) else { continue };
                for kind in [TuningMode::Single, TuningMode::DiffPair, TuningMode::DiffPairSkew] {
                    tried += 1;
                    // its length (or skew) now, and the target 3 mm longer
                    let Ok(h) = hover(&scene, kind, &pk, &Value::Null) else { continue };
                    let target_nm = h.readout.current + mm(3.0);
                    let settings = if kind == TuningMode::DiffPairSkew { json!({}) } else { json!({ "target_length": target_nm }) };
                    let Ok(mut t) = Tuner::start(scene.clone(), kind, &pk, pk.at, &settings) else { continue };
                    started += 1;
                    // the end of the stretch: an end of the picked segment, and the cursor a little off it
                    let to = if next(2000) == 0 { b } else { a };
                    let cursor = Point { x: to.x + Nm::new(next(400_000) as i64) - mm(0.2), y: to.y + Nm::new(next(400_000) as i64) - mm(0.2) };
                    let Ok(moved) = t.mv(cursor, true) else { continue };
                    let (was_moved, status, current) = (moved.moved && !moved.edits.is_empty(), moved.readout.status, moved.readout.current.mm());
                    let Ok(cmd) = t.command() else { continue };
                    let after = apply_cmd(&scene, &cmd).unwrap_or_else(|e| panic!("{name}: the board refuses what the {kind:?} tuner made on {}: {e}", pk.net));
                    committed += 1;
                    if kind == TuningMode::Single && was_moved && status == TuningStatus::Tuned {
                        let scene2 = Scene::of(after, scene.model.clone());
                        // a straight member of the pattern (its first may be an arc of a rounded meander)
                        let members = scene2.design.drawings.as_ref().and_then(|d| d.tuning_patterns.last()).map(|p| p.member_ids.clone()).filter(|m| !m.is_empty()).expect("the new pattern has members");
                        let seg = scene2.world.iter().find_map(|(id, it)| match it {
                            Item::Segment(s) if s.source_track.as_ref().is_some_and(|(tid, _)| members.contains(tid)) => Some(id),
                            _ => None,
                        });
                        if let Some(seg) = seg {
                            let path = assemble_tuning_path(&scene2.world, seg).expect("a path");
                            let len = path.length(&scene2.world, &scene2.env).mm();
                            // a meander that ran into a pad or a via of its own net is measured as KiCad's length calculation does (`clipLineToPad`: what is in the pad is the straight way out), which
                            // the placer, like KiCad's, does not take into account: those are counted and left out
                            let (mut raw, mut clipped) = (0.0f64, 0.0f64);
                            for it in &path.items {
                                if let eda_pns::tuning_path::PathItem::Line { line, clipped: c } = it {
                                    raw += line.pts.windows(2).map(|w| ((w[1].x - w[0].x).as_f64()).hypot((w[1].y - w[0].y).as_f64())).sum::<f64>() / 1e6;
                                    clipped += c.length().mm();
                                }
                            }
                            if raw - clipped > 0.005 {
                                in_pad += 1;
                            } else {
                                assert!((len - current).abs() < 0.08, "{name}: net {} is {len:.4} mm on the board, the placer said {current:.4}", pk.net);
                                measured += 1;
                                // the pattern put back (`Remove`) leaves the net as long as it was, and tuned again (`Regenerate`) as long as it was tuned
                                let pat = scene2.design.drawings.as_ref().and_then(|d| d.tuning_patterns.last()).cloned().expect("the pattern");
                                let removed = apply_cmd(&scene2, &reset_cmd(&scene2, &pat).unwrap_or_else(|e| panic!("{name}: {} cannot be removed: {e}", pk.net))).expect("the removal applies");
                                let scene3 = Scene::of(removed, scene.model.clone());
                                let back = scene3.world.iter().find_map(|(id, it)| match it {
                                    Item::Segment(s3) if s3.net.as_deref() == Some(pk.net.as_str()) && s3.source_track.is_some() && Seg::new(s3.a, s3.b).squared_distance(pat.origin) <= 4000 => Some(id),
                                    _ => None,
                                });
                                if let Some(back) = back {
                                    let blen = assemble_tuning_path(&scene3.world, back).map(|p| p.length(&scene3.world, &scene3.env)).unwrap_or(Nm::ZERO).mm();
                                    let was = h.readout.current.mm();
                                    assert!((blen - was).abs() < 0.01, "{name}: net {} is {blen:.4} mm once its pattern is removed, it was {was:.4}", pk.net);
                                }
                                let again = retune_cmds(&scene2, &pat, &Value::Null).unwrap_or_else(|e| panic!("{name}: {} cannot be tuned again: {e}", pk.net));
                                let scene4 = apply_all(&scene2, &again).expect("the retune applies");
                                let q = scene4.design.drawings.as_ref().and_then(|d| d.tuning_patterns.iter().find(|q| q.id == pat.id)).expect("the pattern is still there");
                                assert!((q.last_tuning_length.mm() - current).abs() < 0.08, "{name}: net {} tuned again to {:.4}, it was {current:.4}", pk.net, q.last_tuning_length.mm());
                            }
                        }
                    }
                }
            }
        }
        eprintln!("tried {tried}, started {started}, committed {committed}, tuned and measured {measured} (left out, a meander in a pad: {in_pad})");
        assert!(started > 20 && committed > 10 && measured > 10, "the test is not testing much: {started} started, {committed} committed, {measured} measured");
    }
}

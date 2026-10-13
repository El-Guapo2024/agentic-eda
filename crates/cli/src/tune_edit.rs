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

use eda_drc::pcbexpr::{compile, matches_compiled, Facts};
use eda_model::ir::{tessellate_arc, Design, Point, Track, Um};
use eda_model::setup_pages::{CornerStyle, MeanderSettings as PageSettings};
use eda_model::tuning::{PatternSide, TargetRange, TuningMode, TuningPattern, LENGTH_UNCONSTRAINED_UM};
use eda_model::{BoardRules, ConstraintModel};
use eda_ops::{Board, Cmd};
use eda_pns::chain::ShapeLineChain;
use eda_pns::dp_meander_placer::{assemble_diff_pair, skew_setup, DpMeanderPlacer, NO_PAIR, NO_PAIR_FOR_SKEW};
use eda_pns::from_ir::build_node;
use eda_pns::item::{Item, ItemId};
use eda_pns::kigeom::{to_nm, to_um, Seg};
use eda_pns::layer::LayerMap;
use eda_pns::line::Line;
use eda_pns::meander::{MeanderSettings, MeanderSide, MeanderStyle, MinOptMax, LENGTH_UNCONSTRAINED};
use eda_pns::meander_placer::{chain_of, MeanderPlacer, Tuned, TuningStatus};
use eda_pns::node::Node;
use eda_pns::tuning_commit::{line_edit, LineEdit, PieceShape};
use eda_pns::tuning_path::{assemble_tuning_path, LengthEnv};
use serde_json::{json, Map, Value};
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
    pub width: Um,
    /// The point of the segment nearest the cursor: where a pattern started here would start (`m_pickerItem->GetFocusPosition()`).
    pub at: Point,
}

/// The straight track segment at `(x, y)` within `tolerance` of its edge, preferring `layer`. An arc track is not picked (the world has its chords, not the arc).
pub fn pick(scene: &Scene, x: Um, y: Um, tolerance: Um, layer: Option<&str>) -> Option<Pick> {
    let cursor = Point { x, y };
    let arc_tracks: std::collections::HashSet<&str> = scene.design.routing.iter().flat_map(|r| r.tracks.iter()).filter(|t| t.arc().is_some()).map(|t| t.id.as_str()).collect();
    let mut best: Option<(bool, i128, Pick)> = None;
    for (id, item) in scene.world.iter() {
        let Item::Segment(s) = item else { continue };
        let Some((track, _)) = &s.source_track else { continue };
        if arc_tracks.contains(track.as_str()) || s.net.is_none() {
            continue;
        }
        let seg = Seg::new(s.a, s.b);
        let reach = (s.width / 2 + tolerance).max(1) as i128;
        if seg.squared_distance(cursor) > reach * reach {
            continue;
        }
        let layer_name = scene.layers.name_of(s.layer).to_string();
        let on_layer = layer.is_some_and(|l| l == layer_name);
        // "the track whose centre is nearest the cursor", the active layer first
        let centre = Point { x: (s.a.x + s.b.x) / 2, y: (s.a.y + s.b.y) / 2 };
        let d = ((centre.x - cursor.x) as i128).pow(2) + ((centre.y - cursor.y) as i128).pow(2);
        let key = (!on_layer, d);
        if best.as_ref().is_none_or(|(o, bd, _)| key < (*o, *bd)) {
            best = Some((key.0, key.1, Pick { item: id, track_id: track.clone(), net: Scene::net_name(&s.net), layer: layer_name, width: s.width, at: seg.nearest_point(cursor) }));
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
        origin: Point { x: 0, y: 0 },
        end: Point { x: 0, y: 0 },
        initial_side: PatternSide::Left,
        min_amplitude: s.min_amplitude_um,
        max_amplitude: s.max_amplitude_um,
        spacing: s.spacing_um,
        corner_radius_percent: s.corner_radius_percentage,
        rounded: s.corner_style == CornerStyle::Round,
        single_sided: s.single_sided,
        override_custom_rules: false,
        target_length: TargetRange::unconstrained(),
        target_skew: TargetRange { min: -100, opt: 0, max: 100 },
        base_line: Vec::new(),
        base_line_coupled: Vec::new(),
        last_track_width: 0,
        last_diff_pair_gap: 0,
        last_tuning_length: 0,
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
        return Some(if skew { TargetRange { min: -100, opt: 0, max: 100 } } else { TargetRange::unconstrained() });
    }
    if let Some(n) = v.as_f64() {
        return Some(TargetRange::around(n.round() as i64));
    }
    let obj = v.as_object()?;
    let get = |k: &str| obj.get(k).and_then(Value::as_f64).map(|f| f.round() as i64);
    let opt = get("opt").or_else(|| get("min")).unwrap_or(0);
    if !skew && opt >= LENGTH_UNCONSTRAINED_UM {
        return Some(TargetRange::unconstrained());
    }
    let base = TargetRange::around(opt);
    Some(TargetRange { min: get("min").unwrap_or(base.min), opt, max: get("max").unwrap_or(base.max) })
}

/// The settings of a request over a pattern: whatever the object carries (micrometres) replaces what the pattern had.
pub fn apply_settings(p: &mut TuningPattern, v: &Value) {
    if let Some(n) = int(v, "min_amplitude") {
        p.min_amplitude = n;
    }
    if let Some(n) = int(v, "max_amplitude") {
        p.max_amplitude = n;
    }
    if let Some(n) = int(v, "spacing") {
        p.spacing = n;
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
}

/// `PNS::MEANDER_SETTINGS` (nanometres) for a pattern.
pub fn meander_settings(p: &TuningPattern) -> MeanderSettings {
    let mut s = MeanderSettings::default();
    s.min_amplitude = p.min_amplitude * 1000;
    s.max_amplitude = p.max_amplitude * 1000;
    s.spacing = p.spacing * 1000;
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
            m.set_opt(r.opt * 1000);
            m.set_min(r.min * 1000);
            m.set_max(r.max * 1000);
        }
        m
    };
    s.target_length = range(&p.target_length, p.target_length.is_unconstrained());
    s.target_skew = range(&p.target_skew, false);
    s
}

/// What the placer changed of a pattern's settings (the amplitude and spacing keys, a flipped initial side), back into the pattern.
fn sync_back(p: &mut TuningPattern, s: &MeanderSettings) {
    p.max_amplitude = s.max_amplitude / 1000;
    p.spacing = s.spacing / 1000;
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

/// `DRC_ENGINE::EvalRules( LENGTH_CONSTRAINT / SKEW_CONSTRAINT, track, partner, layer )`: the last custom rule of the constraint type that applies, in file order. Its
/// value is what `MEANDER_SETTINGS::SetTargetLength( constraint.GetValue() )` makes of it: the optimum is the rule's, else its minimum, with the default tolerance round it
/// unless the rule has a minimum or a maximum of its own.
pub fn constraint_for(rules: &BoardRules, kind: TuningMode, net: &str, partner: Option<&str>, layer: &str) -> Option<Constraint> {
    let ty = if kind == TuningMode::DiffPairSkew { "skew" } else { "length" };
    let class_of = |n: &str| rules.class_of(n).map(|c| c.name.clone()).unwrap_or_else(|| "Default".to_string());
    let (class_a, class_b) = (class_of(net), partner.map(class_of).unwrap_or_else(|| class_of(net)));
    let a = Facts { item_type: "Track", net_class: &class_a, net_name: net, reference: "", inside_courtyards: &[] };
    let b = Facts { item_type: "Track", net_class: &class_b, net_name: partner.unwrap_or(net), reference: "", inside_courtyards: &[] };
    let mut won: Option<Constraint> = None;
    for r in rules.custom_rules.iter().filter(|r| r.constraint_type == ty) {
        if r.min.is_none() && r.max.is_none() && r.opt.is_none() {
            continue;
        }
        if let Some(pat) = &r.layer {
            if !eda_model::glob_match(pat, layer) {
                continue;
            }
        }
        let condition = r.condition.as_deref().and_then(compile);
        if r.condition.is_some() && condition.is_none() {
            continue; // a condition this evaluator cannot read never applies
        }
        if !matches_compiled(condition.as_ref(), &a, &b) {
            continue;
        }
        let opt = r.opt.or(r.min).unwrap_or(0);
        let tolerance = TargetRange::around(opt);
        won = Some(Constraint { name: r.name.clone(), range: TargetRange { min: r.min.unwrap_or(tolerance.min), opt, max: r.max.unwrap_or(tolerance.max) } });
    }
    won
}

// ------------------------------------------------------------------------------------------------------------------------------------ readout

/// What the status box beside the cursor shows (`TUNING_STATUS_VIEW_ITEM`): the length (or the skew), the range it should be in, and whether it is.
#[derive(Debug, Clone)]
pub struct Readout {
    /// Nanometres.
    pub current: i64,
    /// `current length` or `current skew`.
    pub label: &'static str,
    /// The range (`min`, `opt`, `max`) the status is judged against, nanometres; `None` for a pattern with no target (the box shows `---`).
    pub target: Option<(i64, i64, i64)>,
    pub status: TuningStatus,
    /// The custom rule the target comes from.
    pub constraint: Option<String>,
    pub partner: Option<String>,
    /// The length of each net of a pair (P, N), nanometres.
    pub nets: Option<(i64, i64)>,
}

impl Readout {
    pub fn json(&self) -> Value {
        let mm = |nm: i64| nm as f64 / 1e6;
        json!({
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

fn judge(len: i64, t: Option<(i64, i64, i64)>) -> TuningStatus {
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
    apply_settings(&mut pattern, settings);
    let (partner, constraint) = constrain(scene, &mut pattern, pk);
    let line = scene.world.assemble_line_with(pk.item, eda_pns::tuning_path::assemble_opts()).ok_or("Please select a track whose length you want to tune.")?;
    let (current, nets, label) = match kind {
        TuningMode::Single => {
            let path = assemble_tuning_path(&scene.world, pk.item).ok_or("Please select a track whose length you want to tune.")?;
            (path.length(&scene.world, &scene.env), None, "current length")
        }
        TuningMode::DiffPair => {
            let pair = assemble_diff_pair(&scene.world, pk.item).ok_or(NO_PAIR)?;
            let len = |l: &Line| l.segment_ids.first().and_then(|id| assemble_tuning_path(&scene.world, *id)).map(|p| p.length(&scene.world, &scene.env)).unwrap_or(0);
            let (p, n) = (len(&pair.p), len(&pair.n));
            (p.max(n), Some((p, n)), "current length")
        }
        TuningMode::DiffPairSkew => {
            let setup = skew_setup(&scene.world, &scene.env, pk.item).map_err(|e| if e.is_empty() { NO_PAIR_FOR_SKEW.to_string() } else { e })?;
            (setup.own_length - setup.coupled_length, Some((setup.own_length, setup.coupled_length)), "current skew")
        }
    };
    let ms = meander_settings(&pattern);
    let target = match kind {
        TuningMode::DiffPairSkew => Some((ms.target_skew.min(), ms.target_skew.opt(), ms.target_skew.max())),
        _ => target_of(&pattern).map(|_| (ms.target_length.min(), ms.target_length.opt(), ms.target_length.max())),
    };
    let readout = Readout { current, label, target, status: judge(current, target), constraint: constraint.map(|c| c.name), partner, nets };
    Ok(Hover { pattern, readout, path: line.pts.clone() })
}

/// `PCB_TUNING_PATTERN::EditStart`'s rule lookup: unless the pattern overrides the custom rules, a length (skew) rule that applies to the net sets its target.
fn constrain(scene: &Scene, pattern: &mut TuningPattern, pk: &Pick) -> (Option<String>, Option<Constraint>) {
    let partner = (pattern.mode != TuningMode::Single).then(|| eda_pns::diff_pair::dp_coupled_net_name(&pk.net)).flatten();
    if pattern.override_custom_rules {
        return (partner, None);
    }
    let c = constraint_for(&scene.env.rules, pattern.mode, &pk.net, partner.as_deref(), &pk.layer);
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
    skew_coupled: Option<i64>,
    /// The last cursor position the placer was moved to, and what it made of it.
    last: Option<(Point, Moved)>,
}

impl Tuner {
    /// The first click, on `pk` (`PlaceTuningPattern` creating the pattern and `EditStart` starting the router's placer): the pattern starts on the track at the point nearest `at`.
    /// `settings` are the dialog's, over the board's meander defaults for the kind of tuning.
    pub fn start(scene: Rc<Scene>, kind: TuningMode, pk: &Pick, at: Point, settings: &Value) -> Result<Tuner, String> {
        let mut pattern = default_pattern(&scene.model, kind, &pk.layer);
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
                Placer::Single(MeanderPlacer::start_skew(&scene.world, &scene.env, pk.item, at, ms, setup.coupled_length)?)
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
                let off = Point { x: ((c.c_last_point().x - raw_primary.c_last_point().x) as f64 / 2.0).round() as i64, y: ((c.c_last_point().y - raw_primary.c_last_point().y) as f64 / 2.0).round() as i64 };
                base = Seg::new(Point { x: base.a.x + off.x, y: base.a.y + off.y }, Point { x: base.b.x + off.x, y: base.b.y + off.y });
            }
        }
        if base.a == base.b {
            return None;
        }
        Some(if base.side(to_nm(end)) < 0 { PatternSide::Left } else { PatternSide::Right })
    }

    /// The cursor is at `to`: the placer fits the meander from the start of the pattern to there (`EditStart`, `Update`). `follow_cursor` says whether the side the first
    /// meander is on follows the cursor (`UpdateSideFromEnd`, while placing) or stays what the pattern has (a pattern tuned again).
    pub fn mv(&mut self, to: Point, follow_cursor: bool) -> Result<&Moved, String> {
        // The side goes by a first cut of the line to the cursor, as `EditStart` takes it from the baseline `initBaseLines` has just made.
        if follow_cursor && self.pattern.initial_side != PatternSide::Default {
            let side = match &self.placer {
                Placer::Single(p) => {
                    let chain = chain_of(p.origin_line());
                    let (_, raw, _) = chain.split3(to_nm(p.current_start()), to_nm(to));
                    self.side_from_end(&raw, None, to)
                }
                Placer::Pair(p) => {
                    let (cp, cn) = (chain_of(&p.pair().p), chain_of(&p.pair().n));
                    let start = to_nm(p.current_start());
                    let (_, raw_p, _) = cp.split3(start, to_nm(to));
                    let (_, raw_n, _) = cn.split3(start, to_nm(to));
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
        let (moved, edits, length, status, target, nets, cut) = match &mut self.placer {
            Placer::Single(p) => {
                let moved = p.do_move(to);
                let edits = if moved { line_edit(&scene.world, p.origin_line(), &p.tuned(), true).map(|e| vec![(p.origin_line().clone(), e)]).unwrap_or_default() } else { Vec::new() };
                let t = p.target();
                let cut = p.tuned().raw;
                (moved, edits, p.tuning_length_result(), p.tuning_status(), Some(t), None, Some((cut, None)))
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
                (moved, edits, p.tuning_length_result(), p.tuning_status(), Some((s.target_length.min(), s.target_length.opt(), s.target_length.max())), Some((a, b)), Some(cut))
            }
        };
        let s = self.settings().clone();
        sync_back(&mut self.pattern, &s);
        let unconstrained = match self.pattern.mode {
            TuningMode::DiffPairSkew => false,
            _ => self.pattern.target_length.is_unconstrained(),
        };
        let target = if unconstrained { None } else { target };
        let label = if self.skew_coupled.is_some() { "current skew" } else { "current length" };
        let readout = Readout { current: length, label, target, status: if moved { status } else { judge(length, target) }, constraint: self.constraint.as_ref().map(|c| c.name.clone()), partner: self.partner.clone(), nets };
        if moved && !edits.is_empty() {
            if let Some((primary, coupled)) = cut {
                self.record_cut(&primary, coupled.as_ref(), length, status);
            }
        }
        self.last = Some((to, Moved { edits, readout, moved }));
        Ok(&self.last.as_ref().expect("just set").1)
    }

    /// The pattern's record of the cut it was made on (`m_origin`, `m_end`, `m_baseLine`, `m_baseLineCoupled`) and of the result (`m_tuningLength`, `m_tuningStatus`).
    fn record_cut(&mut self, primary: &ShapeLineChain, coupled: Option<&ShapeLineChain>, length: i64, status: TuningStatus) {
        let pts = |c: &ShapeLineChain| c.points().iter().map(|p| to_um(*p)).collect::<Vec<Point>>();
        if primary.point_count() >= 2 {
            self.pattern.base_line = pts(primary);
            self.pattern.origin = to_um(primary.c_point(0));
            self.pattern.end = to_um(primary.c_last_point());
        }
        self.pattern.base_line_coupled = match (self.pattern.mode, coupled) {
            (TuningMode::DiffPair | TuningMode::DiffPairSkew, Some(c)) if c.point_count() >= 2 => pts(c),
            _ => Vec::new(),
        };
        if let Placer::Pair(p) = &self.placer {
            self.pattern.last_track_width = p.pair().width;
            self.pattern.last_diff_pair_gap = p.pair().gap.max(0) / 1000;
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

fn track_of(net: &str, layer: &str, width: Um, shape: &PieceShape) -> Track {
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
    let mut board = Board::new(scene.design.clone(), &scene.model, 1, 0);
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
        let slop = (Node::clearance(&scene.env.rules, &s.net, &None) + s.width / 2 + 1) as i128;
        let d = seg.squared_distance(at);
        if d > slop * slop {
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
        if chain.point_on_edge(to_nm(first), 1_000) && chain.point_on_edge(to_nm(last), 1_000) {
            return Some(line);
        }
    }
    None
}

/// What `resetToBaseline` / `removeToBaseline` make of one line: the stretch the pattern is on put back as the baseline it was made on.
fn baseline_edit(scene: &Scene, pattern: &TuningPattern, baseline: &[Point]) -> Result<(Line, LineEdit), String> {
    let line = line_of_baseline(scene, baseline, &pattern.layer).ok_or("Unable to find the tracks this tuning pattern was made on: they have been moved or edited since.")?;
    let chain = chain_of(&line);
    let (first, last) = (to_nm(baseline[0]), to_nm(*baseline.last().unwrap()));
    let (pre, raw, post) = chain.split3(first, last);
    if raw.point_count() < 2 {
        return Err("The tuning pattern has nothing between its start and its end.".into());
    }
    let mut straight = ShapeLineChain::from_points(&baseline.iter().map(|p| to_nm(*p)).collect::<Vec<_>>());
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

/// `PCB_TUNING_PATTERN::EditStart`, `Update`, `EditFinish` over a pattern that exists: its tracks put back to the baseline and the placer run over it again with
/// `settings` (JSON, over what the pattern has). The commands, in order, are one undo step (`Cmd::Batch`).
pub fn retune_cmds(scene: &Scene, pattern: &TuningPattern, settings: &Value) -> Result<Vec<Cmd>, String> {
    let reset = reset_cmd(scene, pattern)?;
    let after_reset = Scene::of(apply_cmd(scene, &reset)?, scene.model.clone());
    let start = segment_near(&after_reset, pattern.origin, &pattern.layer, &pattern.base_line).ok_or("Unable to find the track the tuning pattern starts on.")?;
    let (net, width) = match after_reset.world.get(start) {
        Some(Item::Segment(s)) => (Scene::net_name(&s.net), s.width),
        _ => return Err("Unable to find the track the tuning pattern starts on.".into()),
    };
    let pk = Pick { item: start, track_id: String::new(), net, layer: pattern.layer.clone(), width, at: pattern.origin };
    // the pattern's own settings, the request's over them; the length target of a rule is looked up again unless the pattern overrides the rules
    let mut carried = Map::new();
    let me = serde_json::to_value(pattern).map_err(|e| e.to_string())?;
    for key in ["min_amplitude", "max_amplitude", "spacing", "corner_radius_percent", "rounded", "single_sided", "override_custom_rules", "initial_side", "target_length", "target_skew"] {
        if let Some(v) = me.get(key) {
            carried.insert(key.into(), v.clone());
        }
    }
    if let Some(over) = settings.as_object() {
        for (k, v) in over {
            carried.insert(k.clone(), v.clone());
        }
    }
    let mut tuner = Tuner::start(Rc::new(after_reset), pattern.mode, &pk, pattern.origin, &Value::Object(carried))?;
    let moved = tuner.mv(pattern.end, false)?;
    if !moved.moved || moved.edits.is_empty() {
        return Err("The tuning pattern could not be fitted again: the tracks between its start and its end are gone.".into());
    }
    tuner.pattern.id = pattern.id.clone();
    let tune = tuner.command()?;
    Ok(vec![reset, tune])
}

/// Applies `cmds` in order to a copy of the scene's board, for the next pattern of a batch to be tuned on.
pub fn apply_all(scene: &Scene, cmds: &[Cmd]) -> Result<Scene, String> {
    let mut board = Board::new(scene.design.clone(), &scene.model, 1, 0);
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

    fn rule(name: &str, ty: &str, min: Option<Um>, max: Option<Um>, opt: Option<Um>, cond: Option<&str>) -> eda_model::CustomRule {
        eda_model::CustomRule { name: name.into(), constraint_type: ty.into(), min, max, opt, layer: None, severity: None, condition: cond.map(String::from) }
    }

    #[test]
    fn a_length_rule_sets_the_target_the_way_set_target_length_does() {
        let mut rules = BoardRules::default();
        rules.custom_rules = vec![rule("all", "length", Some(99_900), Some(100_100), None, None)];
        let c = constraint_for(&rules, TuningMode::Single, "SIG", None, "F.Cu").expect("a rule");
        // Opt() with no opt is the minimum; the rule's own min and max win over the tolerance round it
        assert_eq!(c.range, TargetRange { min: 99_900, opt: 99_900, max: 100_100 });
        assert_eq!(c.name, "all");
        // a rule with an optimum only: the default tolerance either side
        rules.custom_rules = vec![rule("opt", "length", None, None, Some(50_000), None)];
        assert_eq!(constraint_for(&rules, TuningMode::DiffPair, "SIG", None, "F.Cu").unwrap().range, TargetRange { min: 49_900, opt: 50_000, max: 50_100 });
        // a skew rule is for the skew tuner only
        rules.custom_rules = vec![rule("skew", "skew", None, Some(100), None, None)];
        assert!(constraint_for(&rules, TuningMode::Single, "SIG", None, "F.Cu").is_none());
        assert_eq!(constraint_for(&rules, TuningMode::DiffPairSkew, "USB_P", Some("USB_N"), "F.Cu").unwrap().range, TargetRange { min: -100, opt: 0, max: 100 });
    }

    #[test]
    fn the_last_rule_that_applies_wins_and_a_condition_picks_the_nets() {
        let mut rules = BoardRules::default();
        rules.custom_rules = vec![
            rule("first", "length", Some(10_000), Some(10_200), None, None),
            rule("clk", "length", Some(20_000), Some(20_200), None, Some("A.NetName == 'CLK'")),
            rule("never", "length", Some(30_000), Some(30_200), None, Some("A.NetName == 'OTHER'")),
        ];
        assert_eq!(constraint_for(&rules, TuningMode::Single, "CLK", None, "F.Cu").unwrap().name, "clk");
        assert_eq!(constraint_for(&rules, TuningMode::Single, "SIG", None, "F.Cu").unwrap().name, "first");
        rules.custom_rules[0].layer = Some("B.Cu".into());
        assert!(constraint_for(&rules, TuningMode::Single, "SIG", None, "F.Cu").is_none(), "a rule for another layer");
    }

    #[test]
    fn settings_come_from_the_page_then_the_request() {
        let model = ConstraintModel::default();
        let mut p = default_pattern(&model, TuningMode::DiffPair, "F.Cu");
        assert_eq!((p.min_amplitude, p.max_amplitude, p.spacing, p.corner_radius_percent, p.rounded), (200, 1_000, 1_000, 80, true), "the pair's page: a spacing of 1 mm");
        apply_settings(&mut p, &json!({ "max_amplitude": 1500, "spacing": 800.0, "rounded": false, "initial_side": "right", "target_length": 52_000, "target_skew": { "max": 50 } }));
        assert_eq!((p.max_amplitude, p.spacing, p.rounded, p.initial_side), (1_500, 800, false, PatternSide::Right));
        assert_eq!(p.target_length, TargetRange { min: 51_900, opt: 52_000, max: 52_100 });
        assert_eq!(p.target_skew, TargetRange { min: -100, opt: 0, max: 50 });
        apply_settings(&mut p, &json!({ "target_length": null }));
        assert!(p.target_length.is_unconstrained());
        let ms = meander_settings(&p);
        assert_eq!((ms.max_amplitude, ms.spacing, ms.corner_style, ms.keep_endpoints), (1_500_000, 800_000, MeanderStyle::Chamfer, true));
        assert_eq!(ms.target_length.opt(), LENGTH_UNCONSTRAINED);
    }
}

//! KiCad 10's microvia stack (`PCB_VIA_STACK`, `pcbnew/generators/pcb_via_stack.cpp` of KiCad 25ec2335; the model is [`eda_model::via_stack`]): the part that changes
//! the board. UI-LEFT-3 X3-1.
//!
//! - [`build_members`] is `BuildMembers`: one microvia per pair of adjacent copper layers of the span (`LAYER_RANGE( start, end )`), each of the stack's size and drill
//!   (with "Use netclass values" the net class's microvia sizes, `GetNetclassViaSize`, sized again by the rules, `SetSizeFromRules`; else the stack's own, a 0 being the
//!   board's current via size), filled when the stack is stacked, filled or capped, and capped only where it reaches an outer layer; a staggered stack's hops step from
//!   `m_hops` (extended along its last step by the pitch, or along X from the position when there is none) and each pair is joined by a trace on the layer they share,
//!   of the net class's width as the rules have it on that layer (`EvalRules( TRACK_WIDTH_CONSTRAINT, .. )`), never under the board's minimum.
//! - [`Board::regenerate_via_stack`] is `Regenerate`: the members reshaped in place when every rebuilt one matches one by kind and layers (`reuseMembers`: "A drag
//!   rebuilds on every motion event"), else replaced.
//! - `set_via_stack` is the placement (`DRAWING_TOOL::PlaceMicroviaStack`'s `placeStack`, with its DRC refusal "Microvia stack location violates DRC.") and the
//!   Microvia Stack dialog's OK (`ShowPropertiesDialog`: the settings, the hops re-spaced to the pitch, then `Update`); `delete_via_stack` is `Remove`;
//!   `make_via_stack` is Create Stack from Selection (`GENERATOR_TOOL::MakeViaStackFromSelection`, `CreateFromItems`). Move, Rotate and Flip are the group's
//!   (`PCB_GROUP::Move` ..: the members move with it) and the stack's own (the hops "must follow every transform", Flip flips the span): [`transform_via_stack`].
//!
//! Where it differs: the net a placed stack takes from what its first hop lands on (`FindNetAtPosition`) is worked out by the studio, which has the zone fills (the
//! design does not). The placement's DRC check judges each via as Draw Via's does (`crate::via_place::via_violates_drc`) and each of a staggered stack's traces with
//! [`trace_violates_drc`] (`CheckItemDRCViolation` on a `PCB_TRACK`).

use super::Board;
use eda_model::ir::{Nm, Point, Track, Via};
use eda_model::via_extra::{ViaExtra, ViaKind};
use eda_model::via_stack::{is_external_copper, is_span_valid, layer_range, MicroviaStack, ViaStackStyle};
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// `DRAWING_TOOL::PlaceMicroviaStack`'s info bar message, word for word.
pub const STACK_VIOLATES_DRC: &str = "Microvia stack location violates DRC.";
/// Its message for a span the board does not have.
pub const STACK_LAYERS_MISSING: &str = "The microvia stack layers are not present on this board.";
/// `MakeViaStackFromSelection`'s message when `CreateFromItems` gives no reason.
pub const MAKE_STACK_REFUSED: &str = "A microvia stack needs two or more microvias on the same net, each spanning one layer step, with no gap between them.";
/// `CreateFromItems`' message for vias of different sizes.
pub const MAKE_STACK_MIXED_SIZES: &str = "The selected microvias have different diameters or hole sizes. A microvia stack uses one size for every hop.";

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// `BOARD_DESIGN_SETTINGS::GetCurrentViaSize` and `GetCurrentViaDrill`: the via size the toolbar has chosen, which the studio sends; a 0 is the board's default (the
/// Default class's).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CurrentSizes {
    #[serde(default)]
    pub via_size_nm: Nm,
    #[serde(default)]
    pub via_drill_nm: Nm,
}

/// A drop of the router's Place Microvia Stack at Track End with a stacked preset (`PENDING_STACK_EXPANSION`): where its via went, the layers it spans and the preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StackDrop {
    pub at: Point,
    pub start: String,
    pub end: String,
    pub preset: eda_model::setup_pages::ViaStackPreset,
}

/// One member `BuildMembers` makes.
#[derive(Debug, Clone, PartialEq)]
pub enum Member {
    /// A single-hop microvia from `top` to `bottom` (`SetLayerPair`, sanitised so the upper layer is first).
    Via { at: Point, size: Nm, drill: Nm, top: String, bottom: String, filled: bool, capped: bool },
    /// A staggered stack's connecting trace.
    Trace { a: Point, b: Point, layer: String, width: Nm },
}

impl Member {
    /// `memberKey`: "Identifies the hop a member implements. Two members with the same key are interchangeable."
    fn key(&self) -> (u8, String, String) {
        match self {
            Member::Via { top, bottom, .. } => (0, top.clone(), bottom.clone()),
            Member::Trace { layer, .. } => (1, layer.clone(), String::new()),
        }
    }
}

/// What `BuildMembers` reads of the board: its copper layers front to back, the current sizes, the net's class sizes and the board's minimum track width.
pub struct BuildInputs<'a> {
    pub copper: &'a [String],
    pub current: CurrentSizes,
    pub rules: &'a eda_model::BoardRules,
}

impl BuildInputs<'_> {
    fn current_via(&self) -> (Nm, Nm) {
        let c = self.current;
        (if c.via_size_nm > Nm::ZERO { c.via_size_nm } else { self.rules.via_diameter }, if c.via_drill_nm > Nm::ZERO { c.via_drill_nm } else { self.rules.via_drill })
    }

    /// `GetNetclassViaSize`: the class's microvia diameter and drill, "falling back to the current via size when the net class leaves them unset".
    pub fn netclass_via_size(&self, net: &str) -> (Nm, Nm) {
        let (d, h) = self.rules.microvia_of(net);
        let (cd, ch) = self.current_via();
        (d.unwrap_or(cd), h.unwrap_or(ch))
    }
}

/// `hopPos`: where hop `i` of the stack is.
fn hop_pos(s: &MicroviaStack, i: usize) -> Point {
    if s.style != ViaStackStyle::Staggered {
        return s.origin; // "stacked hops are coaxial"
    }
    if let Some(h) = s.hops.as_ref() {
        if i < h.len() {
            return h[i];
        }
        if h.len() > 1 {
            let last = h.len() - 1;
            let d = (h[last].x.get() as f64 - h[last - 1].x.get() as f64, h[last].y.get() as f64 - h[last - 1].y.get() as f64);
            let len = (d.0 * d.0 + d.1 * d.1).sqrt();
            if len > 0.0 {
                // `VECTOR2I::Resize( m_pitch )`
                let k = s.pitch_nm.get() as f64 / len;
                let step = ((d.0 * k).round() as i64, (d.1 * k).round() as i64);
                if step != (0, 0) {
                    let n = (i - last) as i64;
                    return Point { x: h[last].x + Nm::new(step.0 * n), y: h[last].y + Nm::new(step.1 * n) };
                }
            }
        }
    }
    Point { x: s.origin.x + Nm::new(s.pitch_nm.get() * i as i64), y: s.origin.y }
}

/// `BuildMembers` (see the module doc), before the rules size them: the vias first, then the traces. Empty for a span the board does not have.
pub fn build_members(s: &MicroviaStack, net: &str, inp: &BuildInputs) -> Vec<Member> {
    let mut out = Vec::new();
    if !is_span_valid(inp.copper, &s.start_layer, &s.end_layer) {
        return out;
    }
    let layers = layer_range(inp.copper, &s.start_layer, &s.end_layer);
    if layers.len() < 2 {
        return out;
    }
    let hops = layers.len() - 1;
    let (cur_size, cur_drill) = inp.current_via();
    let (size, drill) = if s.use_netclass {
        inp.netclass_via_size(net)
    } else {
        (if s.via_size_nm > Nm::ZERO { s.via_size_nm } else { cur_size }, if s.via_drill_nm > Nm::ZERO { s.via_drill_nm } else { cur_drill })
    };
    let pos = |l: &str| inp.copper.iter().position(|c| c == l).unwrap_or(0);
    for i in 0..hops {
        // "Only a hop reaching an outer layer has a face to cap."
        let capped = s.capped && (is_external_copper(&layers[i]) || is_external_copper(&layers[i + 1]));
        // "Stacked hops land on filled copper, and capping needs a filled via under it."
        let filled = s.style == ViaStackStyle::Stacked || s.filled || capped;
        let (top, bottom) = if pos(&layers[i]) <= pos(&layers[i + 1]) { (layers[i].clone(), layers[i + 1].clone()) } else { (layers[i + 1].clone(), layers[i].clone()) };
        out.push(Member::Via { at: hop_pos(s, i), size, drill, top, bottom, filled, capped });
    }
    if s.style == ViaStackStyle::Staggered {
        // "The trace takes the net's width, not whatever the toolbar shows." (`nc->HasTrackWidth()`: the effective class always has one, its Default part's at least.)
        let width = inp.rules.width_of(net);
        for i in 0..hops.saturating_sub(1) {
            let (a, b) = (hop_pos(s, i), hop_pos(s, i + 1));
            if a == b {
                continue;
            }
            out.push(Member::Trace { a, b, layer: layers[i + 1].clone(), width });
        }
    }
    out
}

/// A member via's own settings beyond `Via`: a microvia (`SetViaType( VIATYPE::MICROVIA )`) whose drill is filled and capped as the hop says
/// (`Padstack().Drill().is_filled` / `is_capped`), its other tokens kept.
fn member_extra(id: &str, old: Option<&ViaExtra>, filled: bool, capped: bool) -> ViaExtra {
    let mut e = old.cloned().unwrap_or_else(|| ViaExtra { id: id.to_string(), ..Default::default() });
    e.id = id.to_string();
    e.kind = ViaKind::Micro;
    let mut p = e.props();
    p.filling = Some(filled);
    p.capping = Some(capped);
    e.set_props(&p);
    e
}

/// `CheckItemDRCViolation` of a trace that is not on the board yet (a staggered stack's, before `placeStack` commits it): `ItemHasDRCViolation` against every
/// track segment, via and pad of another net on the trace's layer (zones skipped: "the pour is not refilled yet"), at the clearance the rules give the pair on that
/// layer (`EvalRules( CLEARANCE_CONSTRAINT, .. )` less the DRC epsilon), and a rule area that keeps tracks out (`DISALLOW_CONSTRAINT`). Items on a hidden layer are not
/// judged (`view->Query`).
pub fn trace_violates_drc(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, trace: &Track, hidden_layers: &[String]) -> bool {
    use eda_drc::constraints::CType;
    use eda_drc::kimath::Shape;
    if hidden_layers.iter().any(|h| h == &trace.layer) {
        return false;
    }
    const PROBE: &str = "trk_via_stack_probe";
    let mut d = design.clone();
    let rt = d.routing.get_or_insert_with(|| eda_model::ir::RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: vec![], teardrop_settings: Default::default() });
    rt.tracks.push(Track { id: PROBE.into(), ..trace.clone() });
    let board = eda_drc::board::build(&d, model);
    let rb = eda_drc::rule_board::RuleBoard::new(&d, model, &board, eda_drc::constraints::Units::Mm, model.board.custom_rules_text.as_deref());
    let Ok(probe) = rb.item(PROBE, None) else { return false };
    let layer = trace.layer.as_str();
    let mine = Shape::Stadium { a: trace.pts[0], b: *trace.pts.last().unwrap(), r: trace.width / 2 };
    let other_net = |n: &Option<String>| n.as_deref().is_some_and(|n| !n.is_empty() && n != trace.net);
    let hits = |other: &Shape, item: &std::rc::Rc<eda_drc::rule_item::RuleItem>| {
        let c = rb.engine().eval_rules(CType::Clearance, Some(&probe), Some(item), Some(layer), None).value.min.unwrap_or(0);
        c >= 0 && mine.collides(other, Nm::new((c - 500).max(0))).is_some()
    };
    for (i, s) in board.tracks.iter().enumerate() {
        if s.id == PROBE || s.id.starts_with(&format!("{PROBE}#")) || s.layer != layer || !other_net(&s.net) {
            continue;
        }
        if hits(&s.shape(), &rb.segment_item_at(i)) {
            return true;
        }
    }
    let copper: Vec<&String> = model.board.layers.iter().filter(|l| l.ends_with(".Cu")).collect();
    let at = |l: &str| copper.iter().position(|c| c.as_str() == l);
    for (i, v) in board.vias.iter().enumerate() {
        if !other_net(&v.net) {
            continue;
        }
        let (a, b, x) = (at(&v.from_layer), at(&v.to_layer), at(layer));
        let on = matches!((a, b, x), (Some(a), Some(b), Some(x)) if x >= a.min(b) && x <= a.max(b));
        let shape = Shape::Circle { c: v.at, r: if on { v.diameter / 2 } else { v.drill / 2 } };
        if hits(&shape, &rb.via_item_at(i)) {
            return true;
        }
    }
    for (i, pad) in board.pads.iter().enumerate() {
        if !other_net(&pad.net) {
            continue;
        }
        let (Some(shape), Some(item)) = (pad.copper_on(layer), rb.pad_item_at(i)) else { continue };
        if hits(shape, &item) {
            return true;
        }
    }
    // a rule area that keeps tracks out, on the trace's layer
    let segs = [trace.pts[0], *trace.pts.last().unwrap()];
    design.routing.iter().flat_map(|rt| rt.zones.iter()).any(|z| {
        z.is_rule_area && z.keepout_tracks && (z.layer == layer || z.layers.iter().any(|l| l == layer)) && z.outline.len() >= 3 && {
            let poly = Shape::Polygon { pts: z.outline.clone() };
            mine.collides(&poly, Nm::ZERO).is_some() || segs.iter().any(|p| poly.collides(&Shape::Circle { c: *p, r: Nm::ZERO }, Nm::ZERO).is_some())
        }
    })
}

/// `MINOPTMAX::PinnedOpt( aDefault )`: the opt (or the default) held between the min and the max.
fn pinned_opt(v: &eda_drc::constraints::MinOptMax, default: i64) -> i64 {
    let x = v.opt.unwrap_or(default);
    let x = v.max.map_or(x, |m| x.min(m));
    v.min.map_or(x, |m| x.max(m))
}

impl Board<'_> {
    fn copper(&self) -> Vec<String> {
        self.model.board.layers.iter().filter(|l| l.ends_with(".Cu")).cloned().collect()
    }

    /// The net a stack's routing members are on, else its stored one (`GetNetCode`).
    fn stack_net(&self, s: &MicroviaStack) -> String {
        let rt = self.design.routing.as_ref();
        s.net_of(|m| rt.and_then(|rt| rt.vias.iter().find(|v| v.id == m).map(|v| v.net.clone()).or_else(|| rt.tracks.iter().find(|t| t.id == m).map(|t| t.net.clone()))))
    }

    /// `Regenerate` of the stack `id` (see the module doc).
    pub(crate) fn regenerate_via_stack(&mut self, id: &str, current: CurrentSizes) -> Result<(), Vec<CheckResult>> {
        let s = self.design.drawings.as_ref().and_then(|d| d.board_items.via_stacks.iter().find(|v| v.id == id)).cloned().ok_or_else(|| fail("ops_unknown_via_stack", id, "no microvia stack with this id"))?;
        let net = self.stack_net(&s);
        let copper = self.copper();
        let rebuilt = build_members(&s, &net, &BuildInputs { copper: &copper, current, rules: &self.model.board });
        // "if( rebuilt.empty() && !GetItems().empty() ) return;"
        if rebuilt.is_empty() && !s.member_ids.is_empty() {
            return Ok(());
        }
        let members: Vec<String> = s.member_ids.clone();
        let rt = self.routing_mut();
        // `reuseMembers`: every live member is a via or a trace of a key no other has, and the rebuilt ones match them one for one.
        let mut live: BTreeMap<(u8, String, String), String> = BTreeMap::new();
        let mut reusable = rebuilt.len() == members.len() && !rebuilt.is_empty();
        for m in &members {
            let key = if let Some(v) = rt.vias.iter().find(|v| &v.id == m) {
                // `TopLayer()`, `BottomLayer()`: a via's pair is kept upper layer first (`SanitizeLayers`), whichever way round the IR has it (a flipped one)
                let at = |l: &str| copper.iter().position(|c| c == l);
                let (top, bottom) = if at(&v.from_layer) <= at(&v.to_layer) { (v.from_layer.clone(), v.to_layer.clone()) } else { (v.to_layer.clone(), v.from_layer.clone()) };
                Some((0u8, top, bottom))
            } else {
                rt.tracks.iter().find(|t| &t.id == m && t.pts.len() == 2 && t.arc_mid_offset.is_none()).map(|t| (1u8, t.layer.clone(), String::new()))
            };
            match key {
                Some(k) if !live.contains_key(&k) => {
                    live.insert(k, m.clone());
                }
                _ => reusable = false,
            }
        }
        let mut matched: Vec<(String, &Member)> = Vec::new();
        if reusable {
            for fresh in &rebuilt {
                match live.remove(&fresh.key()) {
                    Some(id) => matched.push((id, fresh)),
                    None => {
                        reusable = false;
                        break;
                    }
                }
            }
        }
        let mut new_ids: Vec<String> = Vec::new();
        let mut extras: Vec<(String, bool, bool)> = Vec::new();
        if reusable {
            for (mid, fresh) in &matched {
                match fresh {
                    Member::Via { at, size, drill, filled, capped, .. } => {
                        if let Some(v) = rt.vias.iter_mut().find(|v| &v.id == mid) {
                            v.net = net.clone();
                            v.at = *at;
                            v.diameter = *size;
                            v.drill = *drill;
                        }
                        extras.push((mid.clone(), *filled, *capped));
                    }
                    Member::Trace { a, b, width, .. } => {
                        if let Some(t) = rt.tracks.iter_mut().find(|t| &t.id == mid) {
                            t.net = net.clone();
                            t.pts = vec![*a, *b];
                            t.width = *width;
                        }
                    }
                }
            }
            new_ids = members.clone();
        } else {
            // The members go, the rebuilt ones come in their place.
            rt.vias.retain(|v| !members.contains(&v.id));
            rt.tracks.retain(|t| !members.contains(&t.id));
            let before: BTreeSet<String> = rt.vias.iter().map(|v| v.id.clone()).chain(rt.tracks.iter().map(|t| t.id.clone())).collect();
            let mut want: Vec<(Point, String, String, bool, bool)> = Vec::new();
            for m in &rebuilt {
                match m {
                    Member::Via { at, size, drill, top, bottom, filled, capped } => {
                        rt.vias.push(Via { id: String::new(), net: net.clone(), at: *at, drill: *drill, diameter: *size, from_layer: top.clone(), to_layer: bottom.clone() });
                        want.push((*at, top.clone(), bottom.clone(), *filled, *capped));
                    }
                    Member::Trace { a, b, layer, width } => rt.tracks.push(Track { id: String::new(), net: net.clone(), pins: Vec::new(), layer: layer.clone(), width: *width, pts: vec![*a, *b], arc_mid_offset: None }),
                }
            }
            rt.assign_missing_ids();
            for v in rt.vias.iter().filter(|v| !before.contains(&v.id)) {
                new_ids.push(v.id.clone());
                if let Some(w) = want.iter().find(|w| w.0 == v.at && w.1 == v.from_layer && w.2 == v.to_layer) {
                    extras.push((v.id.clone(), w.3, w.4));
                }
            }
            new_ids.extend(rt.tracks.iter().filter(|t| !before.contains(&t.id)).map(|t| t.id.clone()));
        }
        self.sort_routing();
        let dr = self.drawings_mut();
        if !reusable {
            dr.via_extras.retain(|x| !members.contains(&x.id));
            dr.locked_ids.retain(|x| !members.contains(x));
        }
        for (vid, filled, capped) in &extras {
            let made = member_extra(vid, dr.via_extras.iter().find(|x| &x.id == vid), *filled, *capped);
            dr.via_extras.retain(|x| &x.id != vid);
            dr.via_extras.push(made);
        }
        dr.via_extras.sort_by(|a, b| a.id.cmp(&b.id));
        new_ids.sort();
        if let Some(slot) = dr.board_items.via_stacks.iter_mut().find(|v| v.id == id) {
            slot.member_ids = new_ids.clone();
            slot.net = net;
        }
        self.size_members_by_rules(id, &s, &new_ids);
        Ok(())
    }

    /// The rules' part of `BuildMembers`: a hop of a stack that uses its net class's sizes takes the rules' via diameter and hole (`SetSizeFromRules`: "Net class sizes
    /// follow the via rules, which can depend on each hop's layers and position"), and a connecting trace the rules' width on its layer (`EvalRules(
    /// TRACK_WIDTH_CONSTRAINT, trace, nullptr, layer )`, its pinned opt, never under `m_TrackMinWidth`). Judged on the board with the members on it.
    fn size_members_by_rules(&mut self, _id: &str, s: &MicroviaStack, members: &[String]) {
        let wants_vias = s.use_netclass;
        let wants_traces = s.style == ViaStackStyle::Staggered;
        if !wants_vias && !wants_traces {
            return;
        }
        use eda_drc::constraints::CType;
        let drc_board = eda_drc::board::build(&self.design, self.model);
        let rb = eda_drc::rule_board::RuleBoard::new(&self.design, self.model, &drc_board, eda_drc::constraints::Units::Mm, self.model.board.custom_rules_text.as_deref());
        let eval = |ctype: CType, id: &str, layer: &str| -> Option<eda_drc::constraints::Resolved> {
            let item = rb.item(id, None).ok()?;
            let c = rb.engine().eval_rules(ctype, Some(&item), None, Some(layer), None);
            (!c.is_null()).then_some(c)
        };
        let min_width = self.model.board.track_width_min_nm;
        let mut via_sizes: Vec<(String, Nm, Nm)> = Vec::new();
        let mut widths: Vec<(String, Nm)> = Vec::new();
        if let Some(rt) = &self.design.routing {
            for m in members {
                if let Some(v) = rt.vias.iter().find(|v| &v.id == m).filter(|_| wants_vias) {
                    let d = eval(CType::ViaDiameter, m, &v.from_layer).map_or(v.diameter.get(), |c| pinned_opt(&c.value, v.diameter.get()));
                    let h = eval(CType::HoleSize, m, &v.from_layer).map_or(v.drill.get(), |c| pinned_opt(&c.value, v.drill.get()));
                    via_sizes.push((m.clone(), Nm::new(d), Nm::new(h)));
                } else if let Some(t) = rt.tracks.iter().find(|t| &t.id == m).filter(|_| wants_traces) {
                    // "if( !width.IsNull() && width.GetSeverity() != RPT_SEVERITY_IGNORE ) layerWidth = width.GetValue().PinnedOpt();"
                    let w = eval(CType::TrackWidth, m, &t.layer).filter(|c| c.value.opt.is_some()).map_or(t.width.get(), |c| pinned_opt(&c.value, t.width.get()));
                    widths.push((m.clone(), Nm::new(w).max(min_width)));
                }
            }
        }
        let rt = self.routing_mut();
        for (m, d, h) in via_sizes {
            if let Some(v) = rt.vias.iter_mut().find(|v| v.id == m) {
                v.diameter = d;
                v.drill = h;
            }
        }
        for (m, w) in widths {
            if let Some(t) = rt.tracks.iter_mut().find(|t| t.id == m) {
                t.width = w;
            }
        }
    }

    /// `Cmd::PcbItem(SetViaStack)`: see the module doc and [`crate::pcb_items::PcbItemCmd::SetViaStack`].
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_via_stack(&mut self, id: Option<&str>, mut stack: MicroviaStack, current: CurrentSizes, allow_drc_violations: bool, hidden_layers: &[String], barcode_shapes: &BTreeMap<String, Vec<Vec<Point>>>) -> Result<(), Vec<CheckResult>> {
        let copper = self.copper();
        let what = id.unwrap_or("via_stack");
        if !is_span_valid(&copper, &stack.start_layer, &stack.end_layer) {
            return Err(fail("ops_bad_via_stack", what, STACK_LAYERS_MISSING));
        }
        if let Some(why) = stack.to_preset().problem(&copper) {
            return Err(fail("ops_bad_via_stack", what, why));
        }
        if !stack.net.is_empty() {
            self.known_net(&stack.net)?;
        }
        stack.name = MicroviaStack::DISPLAY_NAME.into();
        // "Stacked stacks land on filled copper, so fill is forced on."
        if stack.style == ViaStackStyle::Stacked {
            stack.filled = true;
            stack.hops = None;
        }
        match id {
            Some(id) => {
                let old = self.design.drawings.as_ref().and_then(|d| d.board_items.via_stacks.iter().find(|v| v.id == id)).cloned().ok_or_else(|| fail("ops_unknown_via_stack", id, "no microvia stack with this id"))?;
                // `ShowPropertiesDialog`: the dialog's settings over the stack (`ApplyPreset( temp.ToPreset() )`); its place, hops, members and lock stay.
                let mut next = old.clone();
                next.apply_preset(&stack.to_preset());
                if next.style == ViaStackStyle::Stacked {
                    next.hops = None;
                }
                // "A staggered stack stores explicit hop positions, so re-space them to the (possibly new) pitch while keeping each hop's direction."
                if next.style == ViaStackStyle::Staggered {
                    if let Some(h) = next.hops.as_ref().filter(|h| h.len() >= 2) {
                        let mut out = vec![h[0]];
                        for i in 1..h.len() {
                            let d = (h[i].x.get() as f64 - h[i - 1].x.get() as f64, h[i].y.get() as f64 - h[i - 1].y.get() as f64);
                            let len = (d.0 * d.0 + d.1 * d.1).sqrt();
                            let step = if len > 0.0 {
                                let k = next.pitch_nm.get() as f64 / len;
                                ((d.0 * k).round() as i64, (d.1 * k).round() as i64)
                            } else {
                                (0, 0)
                            };
                            out.push(Point { x: out[i - 1].x + Nm::new(step.0), y: out[i - 1].y + Nm::new(step.1) });
                        }
                        next.hops = Some(out);
                    }
                }
                // "If the stack has no net, inherit it from the copper it now sits on" (the studio's `FindNetAtPosition`, sent as the stack's net); SetNetCode writes
                // through to the members.
                let net_now = self.stack_net(&old);
                if net_now.is_empty() && !stack.net.is_empty() {
                    next.net = stack.net.clone();
                    if let Some(rt) = self.design.routing.as_mut() {
                        for v in rt.vias.iter_mut().filter(|v| old.member_ids.contains(&v.id)) {
                            v.net = stack.net.clone();
                        }
                        for t in rt.tracks.iter_mut().filter(|t| old.member_ids.contains(&t.id)) {
                            t.net = stack.net.clone();
                        }
                    }
                }
                let dr = self.drawings_mut();
                if let Some(slot) = dr.board_items.via_stacks.iter_mut().find(|v| v.id == id) {
                    *slot = next;
                }
                self.regenerate_via_stack(id, current)
            }
            None => {
                // `placeStack`: "Refuse placement over foreign copper, matching the plain via tool. Build the stack's vias off-board and check each before anything is
                // committed."
                let rebuilt = build_members(&stack, &stack.net, &BuildInputs { copper: &copper, current, rules: &self.model.board });
                let (_, stackup) = self.effective_stackup();
                let check = crate::via_place::ViaCheck { stackup: Some(&stackup), barcode_shapes: Some(barcode_shapes) };
                let violates = rebuilt.iter().any(|m| match m {
                    Member::Via { at, size, drill, top, bottom, .. } => {
                        let v = Via { id: String::new(), net: stack.net.clone(), at: *at, drill: *drill, diameter: *size, from_layer: top.clone(), to_layer: bottom.clone() };
                        crate::via_place::via_violates_drc(&self.design, self.model, &v, hidden_layers, &check)
                    }
                    Member::Trace { a, b, layer, width } => {
                        let t = Track { id: String::new(), net: stack.net.clone(), pins: Vec::new(), layer: layer.clone(), width: *width, pts: vec![*a, *b], arc_mid_offset: None };
                        trace_violates_drc(&self.design, self.model, &t, hidden_layers)
                    }
                });
                if violates {
                    if !allow_drc_violations {
                        return Err(fail("ops_via_drc", what, STACK_VIOLATES_DRC));
                    }
                    crate::notice::note_drc_violation(STACK_VIOLATES_DRC);
                }
                stack.member_ids.clear();
                stack.id = String::new();
                let dr = self.drawings_mut();
                let before: BTreeSet<String> = dr.board_items.via_stacks.iter().map(|v| v.id.clone()).collect();
                dr.board_items.via_stacks.push(stack);
                dr.assign_missing_ids();
                let new_id = dr.board_items.via_stacks.iter().find(|v| !before.contains(&v.id)).map(|v| v.id.clone()).expect("just added");
                self.regenerate_via_stack(&new_id, current)
            }
        }
    }

    /// `PCB_VIA_STACK::Remove`: every member, then the stack.
    pub(crate) fn delete_via_stack(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_via_stack", id, "the board has no drawings yet"))?;
        let pos = dr.board_items.via_stacks.iter().position(|v| v.id == id).ok_or_else(|| fail("ops_unknown_via_stack", id, "no microvia stack with this id"))?;
        let gone = dr.board_items.via_stacks.remove(pos);
        dr.via_extras.retain(|x| !gone.member_ids.contains(&x.id));
        dr.locked_ids.retain(|x| !gone.member_ids.contains(x) && x != id);
        if let Some(rt) = self.design.routing.as_mut() {
            rt.vias.retain(|v| !gone.member_ids.contains(&v.id));
            rt.tracks.retain(|t| !gone.member_ids.contains(&t.id));
        }
        Ok(())
    }

    /// Create Stack from Selection (`MakeViaStackFromSelection` over `CreateFromItems`): the selected microvias -- two or more, of one net and one size, none in a
    /// generator, each one layer step, together tiling a contiguous span -- and the selected traces that join two of their positions on a landing layer, made the members
    /// of a new stack whose settings are read off them. Refused with KiCad's message otherwise.
    pub(crate) fn make_via_stack(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let copper = self.copper();
        let refuse = |msg: &str| fail("ops_bad_via_stack", "make_via_stack", msg);
        let rt = self.design.routing.as_ref().ok_or_else(|| refuse(MAKE_STACK_REFUSED))?;
        let dr = self.design.drawings.as_ref();
        let in_generator = |id: &str| dr.is_some_and(|d| d.board_items.via_stacks.iter().any(|s| s.member_ids.iter().any(|m| m == id)) || d.board_items.via_stitches.iter().any(|s| s.member_ids.iter().any(|m| m == id)) || d.tuning_patterns.iter().any(|p| p.member_ids.iter().any(|m| m == id)));
        let extra = |id: &str| dr.and_then(|d| d.via_extras.iter().find(|x| x.id == id));
        let mut vias: Vec<&Via> = Vec::new();
        let mut traces: Vec<&Track> = Vec::new();
        for id in ids {
            if let Some(v) = rt.vias.iter().find(|v| &v.id == id) {
                if extra(id).is_none_or(|x| x.kind != ViaKind::Micro) || in_generator(id) {
                    return Err(refuse(MAKE_STACK_REFUSED));
                }
                if let Some(first) = vias.first() {
                    if v.net != first.net {
                        return Err(refuse(MAKE_STACK_REFUSED));
                    }
                    if v.diameter != first.diameter || v.drill != first.drill {
                        return Err(refuse(MAKE_STACK_MIXED_SIZES));
                    }
                }
                vias.push(v);
            } else if let Some(t) = rt.tracks.iter().find(|t| &t.id == id && t.arc_mid_offset.is_none()) {
                traces.push(t);
            } else {
                return Err(refuse(MAKE_STACK_REFUSED));
            }
        }
        if vias.len() < 2 {
            return Err(refuse(MAKE_STACK_REFUSED));
        }
        let ord = |l: &str| copper.iter().position(|c| c == l);
        // "Each via must span one hop and together they must tile a contiguous range."
        let mut by_upper: BTreeMap<usize, &Via> = BTreeMap::new();
        for v in &vias {
            let (Some(a), Some(b)) = (ord(&v.from_layer), ord(&v.to_layer)) else { return Err(refuse(MAKE_STACK_REFUSED)) };
            if a.abs_diff(b) != 1 || by_upper.insert(a.min(b), v).is_some() {
                return Err(refuse(MAKE_STACK_REFUSED));
            }
        }
        let (first, last) = (*by_upper.keys().next().unwrap(), *by_upper.keys().next_back().unwrap());
        if by_upper.len() != last - first + 1 {
            return Err(refuse(MAKE_STACK_REFUSED));
        }
        let top = by_upper[&first];
        let coaxial = vias.iter().all(|v| v.at == top.at);
        let mut s = MicroviaStack::new(top.at);
        s.start_layer = copper[first].clone();
        s.end_layer = copper[last + 1].clone();
        s.via_size_nm = top.diameter;
        s.via_drill_nm = top.drill;
        s.net = top.net.clone();
        // `is_filled.value_or( m_FillVias )`: the board's default fill (the setup's `(filling yes)`) when the via says none
        let board_fill = self.design.drawings.as_ref().is_some_and(|d| d.setup_tokens.iter().any(|t| t.starts_with("(filling ") && !t.contains(" no)")));
        s.filled = extra(&top.id).and_then(|x| x.props().filling).unwrap_or(board_fill);
        // "Only a hop reaching an outer layer carries capping, so only those can say the stack is capped."
        s.capped = vias.iter().any(|v| (is_external_copper(&v.from_layer) || is_external_copper(&v.to_layer)) && extra(&v.id).and_then(|x| x.props().capping).unwrap_or(false));
        if coaxial {
            s.style = ViaStackStyle::Stacked;
            s.filled = true;
        } else {
            s.style = ViaStackStyle::Staggered;
            let hops: Vec<Point> = by_upper.values().map(|v| v.at).collect();
            let d = ((hops[1].x - hops[0].x).get() as f64, (hops[1].y - hops[0].y).get() as f64);
            s.pitch_nm = Nm::new((d.0 * d.0 + d.1 * d.1).sqrt().round() as i64);
            s.hops = Some(hops);
        }
        // "Only traces that connect hop positions on a shared landing layer belong to the stack, anything else must stay loose or the next regenerate would delete it."
        let landing: BTreeSet<&str> = ((first + 1)..=last).map(|i| copper[i].as_str()).collect();
        let positions: BTreeSet<Point> = vias.iter().map(|v| v.at).collect();
        let mut members: Vec<String> = vias.iter().map(|v| v.id.clone()).collect();
        for t in &traces {
            let (a, b) = (t.pts[0], *t.pts.last().unwrap());
            if t.pts.len() == 2 && landing.contains(t.layer.as_str()) && a != b && t.net == s.net && positions.contains(&a) && positions.contains(&b) {
                members.push(t.id.clone());
            }
        }
        members.sort();
        s.member_ids = members.clone();
        let dr = self.drawings_mut();
        // `stack->AddItem( item )`: an item is in one group at a time (`EDA_GROUP::AddItem` takes it out of the one it was in)
        for g in dr.groups.iter_mut() {
            g.member_ids.retain(|m| !members.contains(m));
        }
        dr.groups.retain(|g| !g.member_ids.is_empty());
        dr.board_items.via_stacks.push(s);
        dr.assign_missing_ids();
        Ok(())
    }

    /// `ExpandMultiHopMicrovias` with the router's matcher (`MatchPendingStackExpansion`, `finishInteractive`): each microvia of the board at one of `drops`' places,
    /// spanning that drop's start and end layer (either way round), crossing more than one layer step and in no group or generator (`isExpandableMicrovia`), is
    /// replaced by a stack of the drop's preset over its span: stacked, filled, of the via's size and net, at its place, made at once (`Regenerate`). A drop
    /// no via matches does nothing. "Expand Microvia Stacks", appended to the route's undo step by the caller.
    pub(crate) fn expand_microvia_stacks(&mut self, drops: &[StackDrop], current: CurrentSizes) -> Result<(), Vec<CheckResult>> {
        let copper = self.copper();
        let ord = |l: &str| copper.iter().position(|c| c == l);
        let in_group = |id: &str| {
            self.design.drawings.as_ref().is_some_and(|d| {
                d.groups.iter().any(|g| g.member_ids.iter().any(|m| m == id))
                    || d.board_items.via_stacks.iter().any(|s| s.member_ids.iter().any(|m| m == id))
                    || d.board_items.via_stitches.iter().any(|s| s.member_ids.iter().any(|m| m == id))
                    || d.tuning_patterns.iter().any(|p| p.member_ids.iter().any(|m| m == id))
            })
        };
        let micro = |id: &str| self.design.drawings.as_ref().and_then(|d| d.via_extras.iter().find(|x| x.id == id)).is_some_and(|x| x.kind == ViaKind::Micro);
        let mut found: Vec<(Via, &StackDrop)> = Vec::new();
        for v in self.design.routing.iter().flat_map(|rt| rt.vias.iter()) {
            let (Some(a), Some(b)) = (ord(&v.from_layer), ord(&v.to_layer)) else { continue };
            if !micro(&v.id) || in_group(&v.id) || a.abs_diff(b) <= 1 {
                continue;
            }
            let drop = drops.iter().find(|d| d.at == v.at && ((d.start == v.from_layer && d.end == v.to_layer) || (d.start == v.to_layer && d.end == v.from_layer)));
            if let Some(d) = drop {
                found.push((v.clone(), d));
            }
        }
        for (v, d) in found {
            // `via->LayerPair( &top, &bottom )`: the upper layer first
            let (top, bottom) = if ord(&v.from_layer) <= ord(&v.to_layer) { (v.from_layer.clone(), v.to_layer.clone()) } else { (v.to_layer.clone(), v.from_layer.clone()) };
            let mut s = MicroviaStack::new(v.at);
            s.apply_preset(&d.preset);
            s.start_layer = top;
            s.end_layer = bottom;
            s.style = ViaStackStyle::Stacked;
            s.hops = None;
            s.via_size_nm = v.diameter;
            s.via_drill_nm = v.drill;
            s.net = v.net.clone();
            s.filled = true;
            if let Some(rt) = self.design.routing.as_mut() {
                rt.vias.retain(|x| x.id != v.id);
            }
            let dr = self.drawings_mut();
            dr.via_extras.retain(|x| x.id != v.id);
            dr.locked_ids.retain(|x| *x != v.id);
            let before: BTreeSet<String> = dr.board_items.via_stacks.iter().map(|x| x.id.clone()).collect();
            dr.board_items.via_stacks.push(s);
            dr.assign_missing_ids();
            let id = dr.board_items.via_stacks.iter().find(|x| !before.contains(&x.id)).map(|x| x.id.clone()).expect("just added");
            self.regenerate_via_stack(&id, current)?;
        }
        Ok(())
    }

    /// `BOARD_COMMIT::Push`'s `parentGroup->RemoveItem( boardItem )` for a member that is gone: each stack loses its members that are no longer on the board, and a stack
    /// left with none is gone (the writer would not write it: "A microvia stack with no members is the same kind of ghost").
    pub(crate) fn tidy_via_stacks(&mut self) {
        let Some(dr) = self.design.drawings.as_ref() else { return };
        if dr.board_items.via_stacks.is_empty() {
            return;
        }
        let live: BTreeSet<String> = self.design.routing.iter().flat_map(|rt| rt.vias.iter().map(|v| v.id.clone()).chain(rt.tracks.iter().map(|t| t.id.clone()))).collect();
        let dr = self.drawings_mut();
        for s in dr.board_items.via_stacks.iter_mut() {
            s.member_ids.retain(|m| live.contains(m));
        }
        dr.board_items.via_stacks.retain(|s| !s.member_ids.is_empty());
    }
}

/// `PCB_VIA_STACK::Move`, `Rotate`, `Flip` and `Mirror` on the stack itself: the position and the hops follow ("The hop polyline stores absolute positions, so it must
/// follow every transform or a later regenerate snaps the vias back to the old location"); a Flip turns the span over (`GetFlippedLayer` of both ends).
pub(crate) fn transform_via_stack(s: &mut MicroviaStack, x: crate::pcb_transform::Xform, copper: usize) {
    s.origin = x.point(s.origin);
    for p in s.hops.iter_mut().flatten() {
        *p = x.point(*p);
    }
    if x.is_flip() {
        s.start_layer = crate::pcb_transform::flip_layer(&s.start_layer, copper);
        s.end_layer = crate::pcb_transform::flip_layer(&s.end_layer, copper);
    }
}

#[cfg(test)]
#[path = "via_stack_tests.rs"]
mod tests;

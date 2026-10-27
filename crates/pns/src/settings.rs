//! Port of `PNS::ROUTING_SETTINGS`/`PNS::SIZES_SETTINGS`
//! (`pcbnew/router/pns_routing_settings.h`, `pns_sizes_settings.h`).
//!
//! KiCad's `SIZES_SETTINGS` is filled in by `PNS_KICAD_IFACE` from the
//! board's net classes right before a routing operation starts -- the
//! numbers themselves (not the defaults baked into `SIZES_SETTINGS`'s own
//! constructor, which only matter before a board is loaded) come from
//! `BOARD_DESIGN_SETTINGS`. This port's equivalent source is
//! `eda_model::BoardRules`, which already has exactly that per-net-class
//! resolution (`width_of`/`clearance_of`/`via_diameter_of`/`via_drill_of`)
//! -- see [`SizesSettings::for_net`].

use eda_model::ir::Nm;
use eda_model::BoardRules;

/// `PNS_MODE` (`RM_MarkObstacles`/`RM_Shove`/`RM_Walkaround`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Collisions are reported but never resolved -- the line stops (or is
    /// marked violating) at the obstacle. Still routable; just not helpful.
    MarkObstacles,
    /// Push colliding tracks/vias out of the way (`crate::shove`).
    Shove,
    /// Hug around whatever is in the way instead of pushing it
    /// (`crate::walkaround`). KiCad's own default (`ROUTING_SETTINGS`'s
    /// constructor: `m_routingMode = RM_Walkaround`, confirmed by the
    /// router-orchestration research spec against `pns_routing_settings.
    /// cpp` -- not `RM_Shove`, which would be the more obvious guess).
    #[default]
    Walkaround,
}

/// `PNS_OPTIMIZATION_EFFORT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum OptEffort {
    Low,
    #[default]
    Medium,
    Full,
}

/// `PNS::ROUTING_SETTINGS`. Defaults mirror `ROUTING_SETTINGS`'s own
/// constructor (`pns_routing_settings.cpp`) for every field this port has.
///
/// Every field is read by the router: `mode`, `optimizer_effort`,
/// `shove_vias`, `jump_over_obstacles`, `shove_iteration_limit`, `shove_time_limit_ms` and
/// `via_force_prop_iteration_limit` by the shove and the via placement,
/// `walkaround_iteration_limit` and `walkaround_hug_length_threshold` by the
/// walkaround, `smart_pads` and `optimizer_effort` by the optimizer,
/// `remove_loops`, `fix_all_segments`, `free_angle_mode`, `snap_to_tracks` and `can_violate_drc`
/// by the line placer and the dragger, `smooth_dragged_segments` by the dragger. The studio's HTTP API reads all of them
/// from the `settings` object of a start request
/// (`crates/cli/src/route_api.rs`), and Interactive Router Settings sets them.
///
/// `auto_posture` is read by the line placer's mouse trail (`MOUSE_TRAIL_TRACER`), `optimize_entire_dragged_track` by the dragger. KiCad's other
/// settings are not here because nothing here could read them: `m_suggestFinish` (KiCad hides it: "not implemented"),
/// `m_followMouse`, `m_snapToPads` and the walkaround's time limit
/// (`m_walkaroundTimeLimit`: KiCad declares it and nothing reads it).
#[derive(Debug, Clone)]
pub struct RoutingSettings {
    pub mode: Mode,
    pub optimizer_effort: OptEffort,
    /// `ShoveVias()`: when shoving, also push vias out of the way (`true`)
    /// rather than treating them as fixed obstacles to walk around.
    pub shove_vias: bool,
    /// `RemoveLoops()`: retiring a segment that makes the line double back
    /// over itself.
    pub remove_loops: bool,
    /// `SmartPads()`: shorten/angle the line's own first/last segment for a
    /// cleaner entry/exit from a pad (`OPTIMIZER::SMART_PADS`).
    pub smart_pads: bool,
    /// `JumpOverObstacles()`: a track a shove pushed into a solid goes behind
    /// it (rank - 1) instead of being "reflected" back at the pusher.
    pub jump_over_obstacles: bool,
    pub shove_iteration_limit: i32,
    /// `ShoveTimeLimit()` (`shove_time_limit` in the settings file), in milliseconds: `SHOVE::shoveMainLoop` gives up (`SH_INCOMPLETE`)
    /// once a shove has run this long, whatever it has left to push. Checked after each iteration, like the iteration limit; the
    /// clock starts with each loop. Default 1000.
    pub shove_time_limit_ms: i32,
    pub walkaround_iteration_limit: i32,
    /// `ViaForcePropIterationLimit()`: how many steps `VIA::PushoutForce`
    /// takes to get a via off what it sits on.
    pub via_force_prop_iteration_limit: i32,
    /// `GetFixAllSegments()`: a plain click (as opposed to the final,
    /// route-ending click) fixes the *entire* current head, not all but
    /// its last segment, which stays free and follows the cursor. KiCad's
    /// default is `true`.
    pub fix_all_segments: bool,
    /// `WalkaroundHugLengthThreshold()`: a walkaround within this multiple
    /// (twice, for the whole walk) of the direct path's length is taken as it
    /// is; a longer one hugs the obstacle to the point nearest the cursor.
    pub walkaround_hug_length_threshold: f64,
    /// `GetAllowDRCViolationsSetting()` (`CanViolateDRC`, `can_violate_drc` in
    /// the settings file): only with this set does `AllowDRCViolations()` let
    /// `MarkObstacles` mode commit a colliding head. Default `false`.
    pub can_violate_drc: bool,
    /// `GetFreeAngleMode()`: in `MarkObstacles` mode the head is a straight
    /// line from the last fixed point to the cursor, at any angle
    /// (`buildInitialLine`). In the other modes it does nothing, like KiCad's.
    pub free_angle_mode: bool,
    /// `SmoothDraggedSegments()` ("Smooth dragged segments", `smooth_dragged_segments`): a dragged segment or corner snaps to line
    /// up with the segments around it -- within a quarter of the track width in Highlight collisions and Walk around, half of it in
    /// Shove (`DRAGGER::dragMarkObstacles`, `dragWalkaround`, `dragShove`: `SetSnapThreshhold`). Default `true`.
    pub smooth_dragged_segments: bool,
    /// `GetSnapToTracks()` (set from the magnetic settings by `TOOL_BASE::checkSnap`, `snap_to_tracks`): a route ends on the
    /// same-net track under the cursor -- in the middle of a segment too, which is split there -- instead of in empty space beside
    /// it. The default is the magnetic settings' own, "Capture cursor in track tool". Default `true`.
    pub snap_to_tracks: bool,
    /// `GetOptimizeEntireDraggedTrack()` ("Optimize entire track being dragged", `optimize_entire_dragged_track`): after a drag is walked or shoved the
    /// dragged line is run through the optimizer, restricted to the area the drag changed (`DRAGGER::optimizeAndUpdateDraggedLine`). Default `false`.
    pub optimize_entire_dragged_track: bool,
    /// `GetAutoPosture()` ("Use mouse path to set track posture", `auto_posture`): the posture of the head -- whether the 45-degree elbow is a straight
    /// leg first or a diagonal one -- is read from how the cursor has moved since the route (or the last fixed leg) started (`MOUSE_TRAIL_TRACER`);
    /// off, it is the posture the route started with, or the one `/` flipped it to. Default `true`.
    pub auto_posture: bool,
    /// `GetCornerMode()` (`corner_mode`; the Track Corner Mode actions, `SwitchRounding*`): how the head's corner is built -- mitered or rounded (an
    /// arc), 45 or 90 degrees (`DIRECTION_45::BuildInitialTrace`). Also read by the optimizer's merge (its bypasses are built in this mode) and
    /// for smart pads (45-degree modes only). Default `MITERED_45`.
    pub corner_mode: crate::direction45::CornerMode,
}

impl RoutingSettings {
    /// `ROUTING_SETTINGS::AllowDRCViolations()`.
    pub fn allow_drc_violations(&self) -> bool {
        self.mode == Mode::MarkObstacles && self.can_violate_drc
    }

    /// `GetFreeAngleMode() && Mode() == RM_MarkObstacles`: the only place KiCad's router builds a head at any angle.
    pub fn free_angle(&self) -> bool {
        self.free_angle_mode && self.mode == Mode::MarkObstacles
    }
}

impl Default for RoutingSettings {
    fn default() -> Self {
        RoutingSettings {
            mode: Mode::default(),
            optimizer_effort: OptEffort::default(),
            shove_vias: true,
            remove_loops: true,
            smart_pads: true,
            jump_over_obstacles: false,
            shove_iteration_limit: 250,
            shove_time_limit_ms: 1000,
            walkaround_iteration_limit: 40,
            via_force_prop_iteration_limit: 40,
            fix_all_segments: true,
            walkaround_hug_length_threshold: 1.5,
            can_violate_drc: false,
            free_angle_mode: false,
            smooth_dragged_segments: true,
            snap_to_tracks: true,
            optimize_entire_dragged_track: false,
            auto_posture: true,
            corner_mode: crate::direction45::CornerMode::Mitered45,
        }
    }
}

/// `PNS::SIZES_SETTINGS`, resolved for one net.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizesSettings {
    pub clearance: Nm,
    pub track_width: Nm,
    pub via_diameter: Nm,
    pub via_drill: Nm,
}

impl SizesSettings {
    /// `PNS_KICAD_IFACE::ImportSizes` -- this port's version pulls straight
    /// from the board's own resolved net-class rules instead of a wx
    /// dialog's current values.
    pub fn for_net(rules: &BoardRules, net: &str) -> Self {
        SizesSettings { clearance: rules.clearance_of(net), track_width: rules.width_of(net), via_diameter: rules.via_diameter_of(net), via_drill: rules.via_drill_of(net) }
    }
}

/// What `PNS_KICAD_IFACE_BASE::ImportSizes` gives a route, the net class's sizes in use (`UseNetClassTrack()`, `UseNetClassVia()`,
/// `UseNetClassDiffPair()`: the width list on its first entry and no custom size, KiCad's default).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedSizes {
    /// `SetClearance`: the start's `CT_CLEARANCE` when it is at least the board's minimum, else the board's minimum clearance.
    pub clearance: Nm,
    pub track_width: Nm,
    /// `SetWidthSource`: the name of the rule the width is from ("netclass 'Power'", a custom rule's name), "board minimum track width" or
    /// "netclass 'Default'".
    pub width_source: String,
    /// `SetClearanceSource`: the clearance rule's name, or "board minimum clearance" -- what `ROUTER_TOOL::UpdateMessagePanel` shows as "(from %s)"
    /// after "Min Clearance: %s" (r5 R5-8).
    pub clearance_source: String,
    /// `SetDiffPairWidthSource` / `SetDiffPairGapSource`: "board minimum track width" / "board minimum clearance", the rule's name, or the
    /// connected track's ("existing track" is not set for the pair in KiCad: the width source stays the rule's or the board minimum's).
    pub diff_pair_width_source: String,
    pub diff_pair_gap_source: String,
    pub via_diameter: Nm,
    pub via_drill: Nm,
    pub diff_pair_width: Nm,
    pub diff_pair_gap: Nm,
    pub diff_pair_via_gap: Nm,
    /// `SetHoleToHole`: `CT_HOLE_TO_HOLE` of two dummy vias of the net.
    pub hole_to_hole: Nm,
    /// `SetDiffPairHoleToHole`: the larger of that and the one of the via and its coupled one.
    pub diff_pair_hole_to_hole: Nm,
}

/// `PNS_KICAD_IFACE_BASE::ImportSizes( aSizes, aStartItem, aNet, aStartPosition )` with the net class's sizes in use: every size is the rule
/// engine's (`m_ruleResolver->QueryConstraint`, [`crate::rule_resolver::RuleResolver::query_constraint`]) for a dummy track of the start item's net
/// on `layer` (`dummyTrack.SetEnds( aStartItem->Anchor( startAnchor ), .. )`, the anchor of a segment nearer `at`), so the start net's class, a
/// `.kicad_dru` rule (`track_width`, `diff_pair_gap`, `via_diameter`, `hole_size`, `hole_to_hole`) and a pair's class width (`A.inDiffPair('*')`)
/// all count:
/// - with `use_connected_track_width` (`bds.m_UseConnectedTrackWidth && !bds.m_TempOverrideTrackWidth`: the project's `board.auto_track_width`,
///   on by default; `ROUTER_TOOL::prepareInteractive` clears the override before each start) the width of the track joined to the start item
///   ([`inherit_track_width`], source "existing track") comes first, for the route and for the pair ("First try to pick up diff pair width from
///   starting track, if enabled");
/// - else the width is the larger of the board's minimum track width and `CT_WIDTH`'s opt; with no start item (or no rule) the current width,
///   the Default class's (`GetCurrentTrackWidth()` on the list's first entry);
/// - the via the larger of the board's minimum via size / through drill and `CT_VIA_DIAMETER` / `CT_VIA_HOLE` of a dummy via of the net;
/// - the pair's width `CT_WIDTH` of the track and its coupled one (`&dummyTrack, &coupledTrack`, both at `aStartItem->Anchor( 0 )`, not at the
///   start anchor) over the board's minimum width, its gap and via gap `CT_DIFF_PAIR_GAP` over the board's minimum clearance;
/// - hole to hole `CT_HOLE_TO_HOLE` of two dummy vias on no layer (`UNDEFINED_LAYER`), and of the via and its coupled one for a pair -- asked
///   with no start item too (a via of no net then), as KiCad asks it outside `if( aStartItem )`.
///
/// KiCad's `PNS::VIA dummyVia` is default-built, so the dummy via is at (0, 0) here too (a rule that reads its position sees that, r4 R4-5).
///
/// A world with no rule resolver answers from the net classes ([`SizesSettings::for_net`], [`crate::dp_placer::DpSizes::resolve`]).
pub fn import_sizes(world: &crate::node::Node, rules: &BoardRules, start: Option<(&crate::item::Item, eda_model::ir::Point)>, layer: i32, use_connected_track_width: bool) -> ImportedSizes {
    let mut out = import_rule_sizes(world, rules, start, layer);
    // `if( bds.m_UseConnectedTrackWidth && !bds.m_TempOverrideTrackWidth && aStartItem != nullptr ) found = inheritTrackWidth( .. )`
    if let (true, Some((item, at))) = (use_connected_track_width, start) {
        if let Some(w) = inherit_track_width(world, item, at, layer) {
            out.track_width = w;
            out.width_source = "existing track".into();
            out.diff_pair_width = w;
            // the pair's width source is not set by the inheritance, and `!found` skips `CT_WIDTH`: it stays the board minimum's
            out.diff_pair_width_source = "board minimum track width".into();
        }
    }
    out
}

/// [`import_sizes`] without the connected track's width.
fn import_rule_sizes(world: &crate::node::Node, rules: &BoardRules, start: Option<(&crate::item::Item, eda_model::ir::Point)>, layer: i32) -> ImportedSizes {
    use crate::rule_resolver::Side;
    use eda_drc::constraints::CType;
    let net: crate::item::Net = start.and_then(|(it, _)| it.net().clone()).filter(|n| &**n != "\u{1}");
    let name = net.as_deref().unwrap_or("");
    let coupled: crate::item::Net = crate::diff_pair::dp_coupled_net_name(name).and_then(|n| crate::item::net_of(&n));
    let Some(r) = world.rule_resolver() else {
        let s = SizesSettings::for_net(rules, name);
        let dp = crate::dp_placer::DpSizes::resolve(rules, name, coupled.as_deref().unwrap_or(""));
        return ImportedSizes {
            clearance: s.clearance,
            track_width: if start.is_some() { s.track_width.max(rules.track_width_min_nm) } else { rules.track_width.max(rules.track_width_min_nm) },
            width_source: String::new(),
            clearance_source: String::new(),
            diff_pair_width_source: String::new(),
            diff_pair_gap_source: String::new(),
            via_diameter: s.via_diameter,
            via_drill: s.via_drill,
            diff_pair_width: dp.width,
            diff_pair_gap: dp.gap,
            diff_pair_via_gap: dp.via_gap,
            hole_to_hole: dp.hole_to_hole,
            diff_pair_hole_to_hole: dp.hole_to_hole,
        };
    };
    // "Find the start anchor which is closest to the start mouse location" (the single track's dummy), and `Anchor( 0 )` (the pair's)
    let anchors = start.map(|(it, at)| {
        let anchors = it.anchors();
        match (it, anchors.as_slice()) {
            (crate::item::Item::Segment(_), [a0, a1, ..]) => {
                let d = |p: &eda_model::ir::Point| ((p.x - at.x).as_f64()).hypot((p.y - at.y).as_f64());
                (if d(a1) < d(a0) { *a1 } else { *a0 }, *a0)
            }
            (_, [a0, ..]) => (*a0, *a0),
            _ => (at, at),
        }
    });
    let mut out = ImportedSizes {
        clearance: rules.min_clearance_nm,
        track_width: rules.track_width_min_nm,
        width_source: "board minimum track width".into(),
        clearance_source: "board minimum clearance".into(),
        diff_pair_width_source: "board minimum track width".into(),
        diff_pair_gap_source: "board minimum clearance".into(),
        via_diameter: rules.via_diameter_min_nm,
        via_drill: rules.via_drill_min_nm,
        diff_pair_width: rules.track_width_min_nm,
        diff_pair_gap: rules.min_clearance_nm,
        diff_pair_via_gap: rules.min_clearance_nm,
        hole_to_hole: rules.hole_to_hole_min_nm,
        diff_pair_hole_to_hole: rules.hole_to_hole_min_nm,
    };
    let nm = Nm::new;
    let mut found = false;
    let all_layers = crate::layer::LayerRange::new(0, (r.layer_count() as i32 - 1).max(0));
    // `PNS::VIA dummyVia, coupledVia;`: default-built, at (0, 0); their nets only with a start item
    let origin = eda_model::ir::Point { x: Nm::ZERO, y: Nm::ZERO };
    let via = Side::Via { net: &net, layers: all_layers, pos: origin };
    let coupled_via = Side::Via { net: &coupled, layers: all_layers, pos: origin };
    if let Some((p, p0)) = anchors {
        let dummy = Side::Track { net: &net, layer, seg: Some((p, p)) };
        if let Some((c, rule)) = r.query_constraint(CType::Clearance, &dummy, None, Some(layer)) {
            if nm(c.min()) >= rules.min_clearance_nm {
                out.clearance = nm(c.min());
                out.clearance_source = rule;
            }
        }
        if let Some((c, rule)) = r.query_constraint(CType::TrackWidth, &dummy, None, Some(layer)) {
            out.track_width = out.track_width.max(nm(c.opt()));
            found = true;
            if out.track_width == nm(c.opt()) {
                out.width_source = rule;
            }
        }
        let pair_track = Side::Track { net: &net, layer, seg: Some((p0, p0)) };
        let coupled_track = Side::Track { net: &coupled, layer, seg: Some((p0, p0)) };
        if let Some((c, rule)) = r.query_constraint(CType::TrackWidth, &pair_track, Some(&coupled_track), Some(layer)) {
            out.diff_pair_width = out.diff_pair_width.max(nm(c.opt()));
            if out.diff_pair_width == nm(c.opt()) {
                out.diff_pair_width_source = rule;
            }
        }
        if let Some((c, rule)) = r.query_constraint(CType::DiffPairGap, &pair_track, Some(&coupled_track), Some(layer)) {
            out.diff_pair_gap = out.diff_pair_gap.max(nm(c.opt()));
            out.diff_pair_via_gap = out.diff_pair_via_gap.max(nm(c.opt()));
            if out.diff_pair_gap == nm(c.opt()) {
                out.diff_pair_gap_source = rule;
            }
        }
        if let Some((c, _)) = r.query_constraint(CType::ViaDiameter, &via, None, Some(layer)) {
            out.via_diameter = out.via_diameter.max(nm(c.opt()));
        }
        if let Some((c, _)) = r.query_constraint(CType::HoleSize, &via, None, Some(layer)) {
            out.via_drill = out.via_drill.max(nm(c.opt()));
        }
    } else {
        out.via_diameter = rules.via_diameter;
        out.via_drill = rules.via_drill;
    }
    if let Some((c, _)) = r.query_constraint(CType::HoleToHole, &via, Some(&via), None) {
        out.hole_to_hole = nm(c.min());
    }
    let mut h2h = out.hole_to_hole;
    if let Some((c, _)) = r.query_constraint(CType::HoleToHole, &via, Some(&coupled_via), None) {
        h2h = nm(c.min());
    }
    out.diff_pair_hole_to_hole = h2h.max(out.hole_to_hole);
    if !found {
        // `GetCurrentTrackWidth()` on the list's first entry: the Default class's width
        out.track_width = out.track_width.max(rules.track_width);
        out.width_source = "netclass 'Default'".into();
    }
    out
}

/// `PNS_KICAD_IFACE_BASE::inheritTrackWidth( aItem, .., aStartPosition )`: a track's own width (`SEGMENT_T`, `ARC_T`); for a pad or via, of the
/// tracks joined to it (`FindJoint( p, aItem )`, `linkedSegs.ExcludeItem( aItem ).FilterKinds( SEGMENT_T | ARC_T )`) on the start layer the
/// one whose far end is nearest the cursor ("the far-end direction is a proxy for which exit stub the user is pointing at"), else the
/// narrowest on the start layer, else the narrowest of all; none with no track joined, or for anything else.
pub fn inherit_track_width(world: &crate::node::Node, item: &crate::item::Item, at: eda_model::ir::Point, layer: i32) -> Option<Nm> {
    use crate::item::Item;
    if item.is_track() {
        return Some(item.width()).filter(|w| *w > Nm::ZERO);
    }
    let p = match item {
        Item::Via(v) => v.pos,
        Item::Solid(s) => s.pos,
        _ => return None,
    };
    let joint = world.joint_at(p, item.net())?;
    let linked: Vec<&Item> = joint.links.iter().filter_map(|&id| world.get(id)).filter(|i| i.is_track() && i.width() > Nm::ZERO).collect();
    let sq = |a: eda_model::ir::Point, b: eda_model::ir::Point| ((a.x - b.x).get() as i128).pow(2) + ((a.y - b.y).get() as i128).pow(2);
    // `if( aStartPosition != VECTOR2I() )`
    if at != (eda_model::ir::Point { x: Nm::ZERO, y: Nm::ZERO }) {
        let closest = linked.iter().filter(|i| i.layers().start() == layer).filter_map(|i| i.track_ends().map(|(a, b)| (i, if sq(a, p) > sq(b, p) { a } else { b }))).min_by_key(|(_, far)| sq(*far, at));
        if let Some((i, _)) = closest {
            return Some(i.width());
        }
    }
    linked.iter().filter(|i| i.layers().start() == layer).map(|i| i.width()).min().or_else(|| linked.iter().map(|i| i.width()).min())
}

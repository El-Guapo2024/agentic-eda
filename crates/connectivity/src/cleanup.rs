//! `TRACKS_CLEANER` (`pcbnew/tracks_cleaner.cpp`) plus its dialog
//! (`pcbnew/dialogs/dialog_cleanup_tracks_and_vias{,_base}.cpp`) --
//! "Cleanup Tracks & Vias..." (`pcbnew.GlobalEdit.cleanupTracksAndVias`).
//! HTTP glue: `crates/cli/src/cleanup_api.rs`. UI: `CleanupTracksDialog.tsx`.
//!
//! Every checkbox in the base dialog's ctor is left unchecked (no
//! `SetValue(true)` call anywhere in the read snapshot) -- this port's
//! [`CleanupOptions::default()`] matches that literally rather than
//! guessing at a "sensible" default.
//!
//! Every pass leaves alone what is locked and what the dialog's "Filter Items" box skips (`track->IsLocked() || filterItem( track )`, see
//! [`crate::cleanup_filter`]): those tracks and vias are never removed and never merged, but they still count as copper to the passes (a protected track
//! is what a dangling end is connected to, what a merge may not swallow, what a duplicate duplicates).
//!
//! Adaptations forced by this model's shape (documented once here, not
//! repeated at each call site below):
//! - KiCad's `PCB_TRACK` is always a single two-point segment (or arc);
//!   this model's `Track` is a polyline of 2+ points
//!   (`crates/model/src/ir.rs`'s `Track::pts`, and `crates/kicad`'s
//!   im/exporter already treats a multi-point `Track` as N-1 consecutive
//!   `(segment ...)`s). Every pass works on segments, as KiCad does: a polyline the cleanup may change is cut into its segments
//!   first, and a polyline that loses one comes back as the runs of segments that stay (`explode`, `finish`). The runs and a
//!   merged track are new tracks (new ids, no `pins`); KiCad keeps the uuid of what it does not delete, and modifies the first of
//!   two merged segments in place.
//! - "Track in pad" measures the track's width against the pad's copper (`PlacedPad::signed_distance`, see
//!   `track_inside_pad`) rather than KiCad's `SHAPE_POLY_SET` boolean subtract; the same for a plain pad, sampled for a pad
//!   made of pieces, and an oval is its inscribed ellipse.
//! - Collinearity is an exact integer cross-product test over this
//!   model's already-integer nm coordinates, not KiCad's
//!   `ApproxCollinear` angular-tolerance test over floating geometry --
//!   stricter, never looser, than upstream.
//! - The merge pass's "is the shared joint a node" check
//!   ([`is_node`]) does not model `testTrackEndpointIsNode`'s "elide
//!   other collinear tracks" exception for a true 3-way star junction
//!   (two collinear arms plus a third, non-collinear one, all meeting at
//!   exactly one point): this port simply declines to merge there rather
//!   than risk mis-handling a real node. A rare case, and the safer of
//!   the two failure directions (a missed merge, not a wrongly-merged
//!   short).
//! - Zone containment for `is_node` uses each zone's raw outline, not its
//!   computed fill (unlike `crate::items`, which always prefers the real
//!   fill) -- cheaper, and erring toward "there's copper here, don't
//!   merge" is the safe direction for a check that only ever *skips* a
//!   merge.

use std::collections::{BTreeMap, BTreeSet};

use eda_drc::zone_copper::ZoneCopper;
use eda_model::ir::{Design, Point, RoutingSection, Track, Via, Zone};
use eda_model::footprint::PlacedPad;
use eda_model::{ConstraintModel, PadKind};

use crate::algo::build_graph;
use crate::cleanup_filter::{CleanupFilter, Protected};
use crate::dangling::{dangling_tracks_and_vias_with, DanglingKind};
use crate::geom::point_in_polygon;
use crate::items::{ItemRef, ItemShape};
use eda_model::units::Nm;

/// `dialog_cleanup_tracks_and_vias_base.cpp`'s checkboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CleanupOptions {
    /// "Delete tracks connecting different nets" (`m_cleanShortCircuitOpt`).
    pub delete_shorting: bool,
    /// "Delete redundant vias" (`m_cleanViasOpt`).
    pub delete_redundant_vias: bool,
    /// "Delete vias connected on only one layer" (`m_deleteDanglingViasOpt`
    /// -- despite the member's name, this is the dialog's *via* dangling
    /// check, not a second track option; see `via_dangling`'s own doc in
    /// `dangling.rs`).
    pub delete_dangling_vias: bool,
    /// "Merge co-linear tracks" (`m_mergeSegmOpt`) -- also gates null-
    /// segment removal, matching `CleanupBoard`'s own
    /// `removeNullSegments = aMergeSegments || aRemoveMisConnected`.
    pub merge_segments: bool,
    /// "Delete tracks unconnected at one end" (`m_deleteUnconnectedOpt`).
    pub delete_dangling_tracks: bool,
    /// "Delete tracks fully inside pads" (`m_deleteTracksInPadsOpt`).
    pub delete_tracks_in_pads: bool,
}

/// `CLEANUP_ITEM_TYPE` (`pcbnew/cleanup_item.h`), the subset this port
/// produces (no `CLEANUP_INVALID_SHAPE`/graphics kinds -- those belong to
/// `GlobalEdit.cleanupGraphics`, a different dialog, out of scope here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupKind {
    RedundantVia,
    ZeroLengthTrack,
    DuplicateTrack,
    ShortingTrack,
    ShortingVia,
    TrackInPad,
    DanglingTrack,
    DanglingVia,
    MergedTracks,
}

impl CleanupKind {
    pub fn label(&self) -> &'static str {
        match self {
            CleanupKind::RedundantVia => "redundant via",
            CleanupKind::ZeroLengthTrack => "zero-length track",
            CleanupKind::DuplicateTrack => "duplicate track",
            CleanupKind::ShortingTrack => "track shorting two nets",
            CleanupKind::ShortingVia => "via shorting two nets",
            CleanupKind::TrackInPad => "track fully inside a pad",
            CleanupKind::DanglingTrack => "dangling track",
            CleanupKind::DanglingVia => "dangling via (connected on only one layer)",
            CleanupKind::MergedTracks => "merged collinear tracks",
        }
    }
}

/// One proposed change -- `CLEANUP_ITEM`. The caller
/// (`crates/cli/src/cleanup_api.rs`) applies every change in a
/// [`CleanupReport`] as a single atomic `Cmd::CommitRoute`, same undo
/// granularity as KiCad's one `BOARD_COMMIT::Push()`.
#[derive(Debug, Clone)]
pub struct CleanupChange {
    pub kind: CleanupKind,
    pub net: String,
    pub remove_track_ids: Vec<String>,
    pub remove_via_ids: Vec<String>,
    pub add_track: Option<Track>,
}

#[derive(Debug, Clone, Default)]
pub struct CleanupReport {
    pub changes: Vec<CleanupChange>,
    /// The board's tracks the cleanup removes: those removed outright, and each polyline a segment of which goes (its other
    /// segments come back in [`CleanupReport::added`]).
    pub removed_tracks: Vec<String>,
    /// The tracks the cleanup adds: merged tracks, and the runs of a polyline's segments that stay when another segment of it goes.
    pub added: Vec<Track>,
}

impl CleanupReport {
    pub fn remove_track_ids(&self) -> Vec<String> {
        self.removed_tracks.clone()
    }
    pub fn remove_via_ids(&self) -> Vec<String> {
        let mut out: BTreeSet<String> = BTreeSet::new();
        for c in &self.changes {
            out.extend(c.remove_via_ids.iter().cloned());
        }
        out.into_iter().collect()
    }
    pub fn add_tracks(&self) -> Vec<Track> {
        self.added.clone()
    }
}

/// A placed pad's shape plus which copper it reaches: `None` = every
/// layer (a plated through-hole pad), `Some(name)` = just that one named
/// layer (an SMD pad, on whichever side its footprint sits).
struct PadGeom {
    pad: PlacedPad,
    layer: Option<String>,
}

fn pad_covers_layer(p: &PadGeom, layer: &str) -> bool {
    match &p.layer {
        None => true,
        Some(l) => l == layer,
    }
}

/// Every placed pad on the board, with enough to test containment and
/// layer reach -- deliberately not `crate::items::build_items` (which
/// pulls in a full zone-fill computation this step has no use for).
fn pad_geoms(design: &Design, model: &ConstraintModel) -> Vec<PadGeom> {
    let mut out = Vec::new();
    let Some(pl) = &design.placement else { return out };
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        let Some(footprint) = model.footprint_of(part) else { continue };
        let Some(placed) = eda_model::footprint::placed_pads(model, part, fp) else { continue };
        let mut lib_pads: Vec<&eda_model::Pad> = footprint.pads.iter().collect();
        lib_pads.sort_by(|a, b| a.number.cmp(&b.number));
        for (pad, lp) in placed.into_iter().zip(lib_pads.iter()) {
            // an SMD pad on its own side: the footprint's, or the other one for a pad drawn on the footprint's back (`opposite_side`), as `crate::items` places it
            let layer = if lp.kind == PadKind::ThroughHole {
                None
            } else if (fp.side == eda_model::ir::Side::Top) != lp.opposite_side {
                Some("F.Cu".to_string())
            } else {
                Some("B.Cu".to_string())
            };
            out.push(PadGeom { pad, layer });
        }
    }
    out
}

fn working_design(base: &Design, tracks: &[Track], vias: &[Via]) -> Design {
    let mut d = base.clone();
    let (zones, track_width_presets, via_presets, teardrop_settings) = match &d.routing {
        Some(rt) => (rt.zones.clone(), rt.track_width_presets.clone(), rt.via_presets.clone(), rt.teardrop_settings),
        None => (Vec::new(), Vec::new(), Vec::new(), Default::default()),
    };
    d.routing = Some(RoutingSection { tracks: tracks.to_vec(), vias: vias.to_vec(), zones, track_width_presets, via_presets, teardrop_settings });
    d
}

/// The working list's name for segment `k` of polyline `id` (see [`explode`]).
fn piece_id(id: &str, k: usize) -> String {
    format!("{id}~{k}")
}

/// The working list's name for the `n`th merged track (never a board id: board ids have no `~`).
fn merged_id(n: usize) -> String {
    format!("~merged{n}")
}

/// KiCad's `PCB_TRACK` is one segment; this model's `Track` is a polyline. Every pass works on segments, as KiCad does
/// (`deleteDanglingTracks` and `removeShortingTrackSegments` remove the one `PCB_TRACK` at fault, not what it is drawn with), so each
/// polyline of three or more points the cleanup may touch is cut into its segments first ([`piece_id`]); [`finish`] puts back the
/// runs of segments that stay. An arc is one item already, and a locked or filtered polyline is never changed, so neither is cut.
fn explode(tracks: &[Track], prot: &Protected) -> (Vec<Track>, BTreeMap<String, (String, usize)>) {
    let mut out = Vec::with_capacity(tracks.len());
    let mut pieces = BTreeMap::new();
    for t in tracks {
        if t.pts.len() <= 2 || t.arc().is_some() || prot.track(&t.id) {
            out.push(t.clone());
            continue;
        }
        for (k, w) in t.pts.windows(2).enumerate() {
            let id = piece_id(&t.id, k);
            pieces.insert(id.clone(), (t.id.clone(), k));
            out.push(Track { id, net: t.net.clone(), pins: Vec::new(), layer: t.layer.clone(), width: t.width, pts: vec![w[0], w[1]], arc_mid_offset: None });
        }
    }
    (out, pieces)
}

/// What the board loses and gains once every pass has run over the working list `work` (see [`explode`]): a polyline whose
/// segments all stay is left as it is; one that lost a segment is removed, and each run of its consecutive segments that stay
/// comes back as a track (with no `pins`, as a merged track). A merged track still in `work` is added. The changes' own ids
/// are mapped back to the board's: a segment to its polyline; a merged track removed again by a later pass to nothing.
fn finish(original: &[Track], work: &[Track], pieces: &BTreeMap<String, (String, usize)>, changes: &mut [CleanupChange]) -> (Vec<String>, Vec<Track>) {
    let alive: BTreeSet<&str> = work.iter().map(|t| t.id.as_str()).collect();
    let mut removed = BTreeSet::new();
    let mut added = Vec::new();
    for t in original {
        let cut = pieces.contains_key(&piece_id(&t.id, 0));
        if !cut {
            if !alive.contains(t.id.as_str()) {
                removed.insert(t.id.clone());
            }
            continue;
        }
        let kept: Vec<bool> = (0..t.pts.len() - 1).map(|k| alive.contains(piece_id(&t.id, k).as_str())).collect();
        if kept.iter().all(|&k| k) {
            continue;
        }
        removed.insert(t.id.clone());
        let mut k = 0;
        while k < kept.len() {
            if !kept[k] {
                k += 1;
                continue;
            }
            let start = k;
            while k < kept.len() && kept[k] {
                k += 1;
            }
            added.push(Track { id: String::new(), net: t.net.clone(), pins: Vec::new(), layer: t.layer.clone(), width: t.width, pts: t.pts[start..=k].to_vec(), arc_mid_offset: None });
        }
    }
    for t in work {
        if t.id.starts_with('~') {
            let mut t = t.clone();
            t.id.clear();
            added.push(t);
        }
    }
    for c in changes.iter_mut() {
        let mut ids: Vec<String> = Vec::new();
        for id in &c.remove_track_ids {
            let shown = match pieces.get(id) {
                Some((orig, _)) => orig.clone(),
                None if id.starts_with('~') => continue,
                None => id.clone(),
            };
            if !ids.contains(&shown) {
                ids.push(shown);
            }
        }
        c.remove_track_ids = ids;
        if let Some(t) = c.add_track.as_mut() {
            t.id.clear();
        }
    }
    (removed.into_iter().collect(), added)
}

fn is_zero_length(t: &Track) -> bool {
    t.pts.len() == 2 && t.pts[0] == t.pts[1]
}

/// `PCB_TRACK::IsPointOnEnds` both ways: the same two ends, either direction.
fn same_endpoints(a: &Track, b: &Track) -> bool {
    a.pts == b.pts || (a.pts.len() == b.pts.len() && a.pts.iter().eq(b.pts.iter().rev()))
}

/// `PCB_VIA::GetViaType` and `GetLayerSet`, for `cleanup`'s duplicate-via test: the via's type, and its span whichever way it is written.
fn via_key(v: &Via, kinds: &[eda_model::via_extra::ViaExtra]) -> (Point, eda_model::via_extra::ViaKind, (String, String)) {
    let kind = kinds.iter().find(|e| e.id == v.id).map(|e| e.kind).unwrap_or_default();
    let span = if v.from_layer <= v.to_layer { (v.from_layer.clone(), v.to_layer.clone()) } else { (v.to_layer.clone(), v.from_layer.clone()) };
    (v.at, kind, span)
}

/// `TRACKS_CLEANER::cleanup`'s via/null-segment half: redundant vias and zero-length tracks.
///
/// A via is redundant when a via not yet looked at (later in the list; or a locked or filtered one, wherever it is: source never
/// flags those `SKIP_STRUCT`) has its position, type and layer set -- so of two equal vias the earlier one goes -- or when it touches
/// a pad that has every copper layer (`GetConnectedPads( via )`, `( pad->GetLayerSet() & all_cu ) == all_cu`), wherever on the pad.
#[allow(clippy::too_many_arguments)] // the passes' shared state, one argument each
fn remove_redundant(tracks: &mut Vec<Track>, vias: &mut Vec<Via>, base: &Design, model: &ConstraintModel, copper: &ZoneCopper, opts: &CleanupOptions, prot: &Protected, changes: &mut Vec<CleanupChange>) {
    if opts.delete_redundant_vias {
        let kinds: Vec<eda_model::via_extra::ViaExtra> = base.drawings.as_ref().map(|d| d.via_extras.clone()).unwrap_or_default();
        let keys: Vec<_> = vias.iter().map(|v| via_key(v, &kinds)).collect();
        // the vias on a pad with every copper layer: their neighbours in the connectivity graph
        let graph = build_graph(&working_design(base, tracks, vias), model, copper);
        let last_layer = model.board.layers.len().max(1) as i32 - 1;
        let full_stack_pads: BTreeSet<(String, String)> = graph.items.iter().filter_map(|i| match &i.item {
            ItemRef::Pad { reference, number } if i.pad_piece_of.is_some() || (i.layer_lo == 0 && i.layer_hi == last_layer) => Some((reference.clone(), number.clone())),
            _ => None,
        }).collect();
        let on_full_pad: BTreeSet<String> = graph.items.iter().filter_map(|i| match &i.item {
            ItemRef::Via { via_id } if i.connected.iter().any(|&n| matches!(&graph.items[n].item, ItemRef::Pad { reference, number } if full_stack_pads.contains(&(reference.clone(), number.clone())))) => Some(via_id.clone()),
            _ => None,
        }).collect();
        let mut gone = vec![false; vias.len()];
        for i in 0..vias.len() {
            let v = &vias[i];
            if prot.via(&v.id) {
                continue;
            }
            let twin = (0..vias.len()).any(|j| j != i && !gone[j] && (j > i || prot.via(&vias[j].id)) && keys[j] == keys[i]);
            if twin || on_full_pad.contains(&v.id) {
                gone[i] = true;
                changes.push(CleanupChange { kind: CleanupKind::RedundantVia, net: v.net.clone(), remove_track_ids: vec![], remove_via_ids: vec![v.id.clone()], add_track: None });
            }
        }
        let mut k = 0;
        vias.retain(|_| {
            k += 1;
            !gone[k - 1]
        });
    }

    let remove_null = opts.merge_segments || opts.delete_shorting;
    if remove_null {
        let mut kept = Vec::with_capacity(tracks.len());
        for t in std::mem::take(tracks) {
            if is_zero_length(&t) && !prot.track(&t.id) {
                changes.push(CleanupChange { kind: CleanupKind::ZeroLengthTrack, net: t.net.clone(), remove_track_ids: vec![t.id.clone()], remove_via_ids: vec![], add_track: None });
            } else {
                kept.push(t);
            }
        }
        *tracks = kept;
    }

    remove_duplicates(tracks, prot, changes);
}

/// `TRACKS_CLEANER::cleanup`'s `aDeleteDuplicateSegments` pass -- run
/// unconditionally regardless of every checkbox (see `CleanupBoard`'s own
/// doc: it runs `cleanup(..., true, ...)` either as part of the merge-
/// gated first call, or as its own explicit always-on second call when
/// merging is off).
///
/// Straight segments only (`PCB_TRACE_T`, not an arc) that are not null. A segment is compared with those not yet looked at (later in
/// the list, or locked or filtered anywhere: those are never flagged `SKIP_STRUCT`) and not already deleted; on a match (both ends,
/// the width and the layer) it is the segment looked at, the earlier one, that goes.
fn remove_duplicates(tracks: &mut Vec<Track>, prot: &Protected, changes: &mut Vec<CleanupChange>) {
    let straight = |t: &Track| t.arc().is_none() && !is_zero_length(t);
    let mut gone = vec![false; tracks.len()];
    for i in 0..tracks.len() {
        let t = &tracks[i];
        if prot.track(&t.id) || !straight(t) {
            continue;
        }
        let twin = (0..tracks.len()).any(|j| {
            let k = &tracks[j];
            j != i && !gone[j] && (j > i || prot.track(&k.id)) && straight(k) && k.layer == t.layer && k.width == t.width && same_endpoints(k, t)
        });
        if twin {
            gone[i] = true;
            changes.push(CleanupChange { kind: CleanupKind::DuplicateTrack, net: t.net.clone(), remove_track_ids: vec![t.id.clone()], remove_via_ids: vec![], add_track: None });
        }
    }
    let mut k = 0;
    tracks.retain(|_| {
        k += 1;
        !gone[k - 1]
    });
}

/// `deleteTracksInPads`' `track->TransformShapeToPolygon(..)` then `poly.BooleanSubtract( *pad->GetEffectivePolygon(..) )` and
/// `poly.IsEmpty()`: is the whole track, its width included, inside the pad's copper? The pad's copper less half the width (the
/// points whose `signed_distance` is at most minus half the width) is convex for a plain pad, so both ends (or an arc's every
/// tessellation point) in it hold the whole track; a pad made of pieces (a custom or chamfered pad) is not convex, so the
/// segment is also sampled every half width. An oval is measured by its inscribed ellipse, so a track at an oval's very edge
/// may be kept where KiCad deletes it.
fn track_inside_pad(t: &Track, pad: &PlacedPad) -> bool {
    let r = t.width.as_f64() / 2.0;
    let inside = |p: Point| pad.signed_distance(p) <= -r;
    if !t.pts.iter().all(|&p| inside(p)) {
        return false;
    }
    if pad.pieces.is_empty() {
        return true;
    }
    let step = r.max(1000.0) / 2.0;
    t.pts.windows(2).all(|w| {
        let n = (crate::geom::dist(w[0], w[1]) / step).ceil().clamp(1.0, 2000.0) as i64;
        (1..n).all(|i| {
            let f = i as f64 / n as f64;
            let at = |a: Nm, b: Nm| Nm::new((a.as_f64() + (b - a).as_f64() * f).round() as i64);
            inside(Point { x: at(w[0].x, w[1].x), y: at(w[0].y, w[1].y) })
        })
    })
}

/// `TRACKS_CLEANER::deleteTracksInPads`: a track (a segment or an arc, not a via) that lies whole inside the copper of a pad it is
/// connected to (`GetConnectedPads( track )`, `pad->HitTest` of both ends, then [`track_inside_pad`]).
fn remove_tracks_in_pads(tracks: &mut Vec<Track>, vias: &[Via], base: &Design, model: &ConstraintModel, copper: &ZoneCopper, prot: &Protected, changes: &mut Vec<CleanupChange>) {
    let graph = build_graph(&working_design(base, tracks, vias), model, copper);
    let mut bad = BTreeSet::new();
    for it in &graph.items {
        let ItemRef::TrackSeg { track_id, .. } = &it.item else { continue };
        if prot.track(track_id) || bad.contains(track_id) {
            continue;
        }
        let Some(t) = tracks.iter().find(|t| &t.id == track_id) else { continue };
        // a polyline left whole (a locked one) is protected; anything else here is one segment or an arc
        if t.pts.len() != 2 && t.arc().is_none() {
            continue;
        }
        let in_pad = it.connected.iter().any(|&n| match &graph.items[n].shape {
            ItemShape::Pad(pad) => matches!(graph.items[n].item, ItemRef::Pad { .. }) && track_inside_pad(t, pad),
            _ => false,
        });
        if in_pad {
            bad.insert(track_id.clone());
            changes.push(CleanupChange { kind: CleanupKind::TrackInPad, net: t.net.clone(), remove_track_ids: vec![track_id.clone()], remove_via_ids: vec![], add_track: None });
        }
    }
    tracks.retain(|t| !bad.contains(&t.id));
}

/// `TRACKS_CLEANER::removeShortingTrackSegments`: a track segment or a via (`m_brd->Tracks()`) connected to a pad
/// (`GetConnectedPads`) or to a track, arc or via (`GetConnectedTracks`) whose net differs (`GetNetCode() !=`, no net included: a
/// track on a pad with no net shorts, two items with no net do not). Zones are not looked at. Only the segment at fault goes.
fn remove_shorting(tracks: &mut Vec<Track>, vias: &mut Vec<Via>, base: &Design, model: &ConstraintModel, copper: &ZoneCopper, prot: &Protected, changes: &mut Vec<CleanupChange>) {
    let wd = working_design(base, tracks, vias);
    let graph = build_graph(&wd, model, copper);
    let mut bad_tracks = BTreeSet::new();
    let mut bad_vias = BTreeSet::new();

    for it in &graph.items {
        let shorts = || it.connected.iter().any(|&nb| {
            let other = &graph.items[nb];
            matches!(other.item, ItemRef::Pad { .. } | ItemRef::TrackSeg { .. } | ItemRef::Via { .. }) && other.net != it.net
        });
        match &it.item {
            ItemRef::TrackSeg { track_id, .. } if !prot.track(track_id) && shorts() => {
                bad_tracks.insert(track_id.clone());
            }
            ItemRef::Via { via_id } if !prot.via(via_id) && shorts() => {
                bad_vias.insert(via_id.clone());
            }
            _ => {}
        }
    }

    for t in tracks.iter().filter(|t| bad_tracks.contains(&t.id)) {
        changes.push(CleanupChange { kind: CleanupKind::ShortingTrack, net: t.net.clone(), remove_track_ids: vec![t.id.clone()], remove_via_ids: vec![], add_track: None });
    }
    for v in vias.iter().filter(|v| bad_vias.contains(&v.id)) {
        changes.push(CleanupChange { kind: CleanupKind::ShortingVia, net: v.net.clone(), remove_track_ids: vec![], remove_via_ids: vec![v.id.clone()], add_track: None });
    }
    tracks.retain(|t| !bad_tracks.contains(&t.id));
    vias.retain(|v| !bad_vias.contains(&v.id));
}

/// `TRACKS_CLEANER::deleteDanglingTracks`: iterate `dangling_tracks_and_vias_with( .., false )` (`TestTrackEndpointDangling( track,
/// false )`, the cleaner's call: a segment inside one pad dangles) to a fixed point, since removing one dangling segment can dangle
/// whatever it was attached to. Only the dangling segment goes, not the polyline it is drawn with ([`explode`]).
#[allow(clippy::too_many_arguments)] // the passes' shared state, one argument each
fn remove_dangling(tracks: &mut Vec<Track>, vias: &mut Vec<Via>, base: &Design, model: &ConstraintModel, copper: &ZoneCopper, want_tracks: bool, want_vias: bool, prot: &Protected, changes: &mut Vec<CleanupChange>) -> bool {
    let mut deleted_any = false;
    let max_iters = tracks.len() + vias.len() + 2;
    for _ in 0..max_iters {
        let wd = working_design(base, tracks, vias);
        let graph = build_graph(&wd, model, copper);
        let dangling = dangling_tracks_and_vias_with(&graph, false);

        let mut track_ids = BTreeSet::new();
        let mut via_ids = BTreeSet::new();
        for d in &dangling {
            match d.kind {
                DanglingKind::Track if want_tracks => {
                    // `#N` names a segment of a polyline left whole, which only a locked or filtered one is
                    let id = d.id.split('#').next().unwrap_or(&d.id);
                    if !prot.track(id) {
                        track_ids.insert(id.to_string());
                    }
                }
                DanglingKind::Via if want_vias && !prot.via(&d.id) => {
                    via_ids.insert(d.id.clone());
                }
                _ => {}
            }
        }
        if track_ids.is_empty() && via_ids.is_empty() {
            break;
        }
        for t in tracks.iter().filter(|t| track_ids.contains(&t.id)) {
            changes.push(CleanupChange { kind: CleanupKind::DanglingTrack, net: t.net.clone(), remove_track_ids: vec![t.id.clone()], remove_via_ids: vec![], add_track: None });
        }
        for v in vias.iter().filter(|v| via_ids.contains(&v.id)) {
            changes.push(CleanupChange { kind: CleanupKind::DanglingVia, net: v.net.clone(), remove_track_ids: vec![], remove_via_ids: vec![v.id.clone()], add_track: None });
        }
        tracks.retain(|t| !track_ids.contains(&t.id));
        vias.retain(|v| !via_ids.contains(&v.id));
        deleted_any = true;
    }
    deleted_any
}

fn collinear(a: Point, b: Point, c: Point) -> bool {
    let cross = (b.x - a.x).area(c.y - a.y) - (b.y - a.y).area(c.x - a.x);
    cross == 0
}

/// If straight tracks `a`/`b` share exactly one endpoint, `Some((shared,
/// far_a, far_b))`.
fn shared_endpoint(a: &Track, b: &Track) -> Option<(Point, Point, Point)> {
    if a.pts.len() != 2 || b.pts.len() != 2 {
        return None;
    }
    let (a0, a1) = (a.pts[0], a.pts[1]);
    let (b0, b1) = (b.pts[0], b.pts[1]);
    if a1 == b0 && a0 != b1 {
        Some((a1, a0, b1))
    } else if a1 == b1 && a0 != b0 {
        Some((a1, a0, b0))
    } else if a0 == b0 && a1 != b1 {
        Some((a0, a1, b1))
    } else if a0 == b1 && a1 != b0 {
        Some((a0, a1, b0))
    } else {
        None
    }
}

/// `testTrackEndpointIsNode`: does anything besides the two merge
/// candidates (named by `exclude`) touch `p`? See the module doc for the
/// "elide a 3-way collinear junction" gap this simplifies away.
fn is_node(p: Point, layer: &str, exclude: (&str, &str), tracks: &[Track], vias: &[Via], pads: &[PadGeom], zones: &[Zone]) -> bool {
    if tracks.iter().any(|t| t.id != exclude.0 && t.id != exclude.1 && t.layer == layer && t.pts.contains(&p)) {
        return true;
    }
    if vias.iter().any(|v| v.at == p) {
        return true;
    }
    if pads.iter().any(|pg| pad_covers_layer(pg, layer) && pg.pad.signed_distance(p) <= 0.0) {
        return true;
    }
    zones.iter().any(|z| z.is_on_layer(layer) && z.outline.len() >= 3 && point_in_polygon(p, &z.outline) && !z.holes.iter().any(|h| h.len() >= 3 && point_in_polygon(p, h)))
}

/// `TRACKS_CLEANER::cleanup`'s merge pass, run to a fixed point (matches
/// source's own `do { ... } while (mergeSegments(...))`).
fn merge_collinear(tracks: &mut Vec<Track>, vias: &[Via], pads: &[PadGeom], zones: &[Zone], prot: &Protected, merged_count: &mut usize, changes: &mut Vec<CleanupChange>) -> bool {
    let mut changed_any = false;
    loop {
        let mut found: Option<(usize, usize, Point, Point)> = None;
        'outer: for i in 0..tracks.len() {
            // `filterItem( segment )` and `filterItem( candidate )`, and `testMergeCollinearSegments`'s `IsLocked()` for both
            if prot.track(&tracks[i].id) {
                continue;
            }
            for j in (i + 1)..tracks.len() {
                if prot.track(&tracks[j].id) {
                    continue;
                }
                if tracks[i].net != tracks[j].net || tracks[i].layer != tracks[j].layer || tracks[i].width != tracks[j].width {
                    continue;
                }
                let Some((shared, far_i, far_j)) = shared_endpoint(&tracks[i], &tracks[j]) else { continue };
                if !collinear(far_i, shared, far_j) {
                    continue;
                }
                // Reject a fold-back: the shared point must genuinely sit
                // between the two far ends, not coincide with one of
                // them pointing back the way it came.
                let d1 = (shared.x - far_i.x, shared.y - far_i.y);
                let d2 = (far_j.x - shared.x, far_j.y - shared.y);
                if d1.0.get() * d2.0.get() + d1.1.get() * d2.1.get() <= 0 {
                    continue;
                }
                if is_node(shared, &tracks[i].layer, (tracks[i].id.as_str(), tracks[j].id.as_str()), tracks, vias, pads, zones) {
                    continue;
                }
                found = Some((i, j, far_i, far_j));
                break 'outer;
            }
        }
        let Some((i, j, far_i, far_j)) = found else { break };
        let (lo, hi) = (i.min(j), i.max(j));
        let b = tracks.remove(hi);
        let a = tracks.remove(lo);
        *merged_count += 1;
        let merged = Track { id: merged_id(*merged_count), net: a.net.clone(), pins: Vec::new(), layer: a.layer.clone(), width: a.width, pts: vec![far_i, far_j], arc_mid_offset: None };
        changes.push(CleanupChange { kind: CleanupKind::MergedTracks, net: a.net.clone(), remove_track_ids: vec![a.id.clone(), b.id.clone()], remove_via_ids: vec![], add_track: Some(merged.clone()) });
        tracks.push(merged);
        changed_any = true;
    }
    changed_any
}

/// Compute every change "Cleanup Tracks & Vias..." would make, without
/// touching `design` -- the dialog's dry run (`m_firstRun`/"Build
/// Changes"). The caller re-runs this (or trusts its own cached result)
/// before applying; see `cleanup_api.rs`'s own doc for why apply recomputes
/// rather than trusting a stale preview. Matches `CleanupBoard`'s own call
/// order: redundant vias/null/duplicate segments (then merge, if asked),
/// then shorting, then tracks-in-pads, then dangling, then a final merge
/// pass if dangling-removal actually deleted anything.
pub fn compute_cleanup(design: &Design, model: &ConstraintModel, copper: &ZoneCopper, opts: CleanupOptions) -> CleanupReport {
    compute_cleanup_filtered(design, model, copper, opts, &CleanupFilter::default())
}

/// [`compute_cleanup`] with the dialog's "Filter Items" box (`TRACKS_CLEANER::SetFilter`): tracks and vias the filter skips are left alone like the locked
/// ones (which every cleanup leaves alone, `IsLocked()`), though they still count as copper to the passes.
pub fn compute_cleanup_filtered(design: &Design, model: &ConstraintModel, copper: &ZoneCopper, opts: CleanupOptions, filter: &CleanupFilter) -> CleanupReport {
    let prot = filter.protected(design, model);
    let original: Vec<Track> = design.routing.as_ref().map(|r| r.tracks.clone()).unwrap_or_default();
    let (mut tracks, pieces) = explode(&original, &prot);
    let mut vias: Vec<Via> = design.routing.as_ref().map(|r| r.vias.clone()).unwrap_or_default();
    let zones: Vec<Zone> = design.routing.as_ref().map(|r| r.zones.clone()).unwrap_or_default();
    let pads = pad_geoms(design, model);

    let mut changes = Vec::new();
    let mut merged = 0usize;

    remove_redundant(&mut tracks, &mut vias, design, model, copper, &opts, &prot, &mut changes);

    if opts.merge_segments {
        merge_collinear(&mut tracks, &vias, &pads, &zones, &prot, &mut merged, &mut changes);
    }

    if opts.delete_shorting {
        remove_shorting(&mut tracks, &mut vias, design, model, copper, &prot, &mut changes);
    }

    if opts.delete_tracks_in_pads {
        remove_tracks_in_pads(&mut tracks, &vias, design, model, copper, &prot, &mut changes);
    }

    let deleted_any = if opts.delete_dangling_tracks || opts.delete_dangling_vias {
        remove_dangling(&mut tracks, &mut vias, design, model, copper, opts.delete_dangling_tracks, opts.delete_dangling_vias, &prot, &mut changes)
    } else {
        false
    };

    if deleted_any && opts.merge_segments {
        merge_collinear(&mut tracks, &vias, &pads, &zones, &prot, &mut merged, &mut changes);
    }

    let (removed_tracks, added) = finish(&original, &tracks, &pieces, &mut changes);
    CleanupReport { changes, removed_tracks, added }
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use eda_model::units::Nm;
    use crate::tests_support::{pad_center, two_pad_model};
    use eda_model::ir::{FootprintInstance, Side};

    #[test]
    fn a_zero_length_track_is_removed_when_merge_is_on() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, a], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { merge_segments: true, ..Default::default() });
        assert_eq!(report.remove_track_ids(), vec!["t1".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::ZeroLengthTrack));
    }

    #[test]
    fn nothing_changes_with_every_option_off() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, a], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions::default());
        assert!(report.changes.is_empty(), "{:?}", report.changes.iter().map(|c| c.kind).collect::<Vec<_>>());
    }

    #[test]
    fn duplicate_tracks_are_always_removed_regardless_of_other_options() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, b], arc_mid_offset: None });
        rt.tracks.push(Track { id: "t2".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![b, a], arc_mid_offset: None }); // reverse order, still a dup
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions::default());
        // `cleanup` compares a segment with those not yet looked at, so of two equal ones the earlier goes
        assert_eq!(report.remove_track_ids(), vec!["t1".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::DuplicateTrack));
    }

    #[test]
    fn redundant_vias_at_the_same_spot_collapse_to_one() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: a, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.vias.push(Via { id: "v2".into(), net: "N1".into(), at: a, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_redundant_vias: true, ..Default::default() });
        assert_eq!(report.remove_via_ids(), vec!["v1".to_string()], "the earlier of two equal vias goes");
    }

    #[test]
    fn a_via_on_a_through_hole_pad_is_redundant() {
        let (mut design, mut model) = two_pad_model();
        model.footprints.push(eda_model::Footprint {
            name: "THPAD".into(),
            pads: vec![eda_model::Pad { opposite_side: false, ext: Default::default(), number: "1".into(), at: (Nm::ZERO, Nm::ZERO), size: (mm(1.0), mm(1.0)), shape: eda_model::PadShape::Circle, kind: PadKind::ThroughHole, drill: Some(mm(0.5)), drill_slot: None, rot: 0, roundrect_ratio: None }],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
            models3d: vec![],
            art: Default::default(),
        });
        model.parts[0].footprint = Some("THPAD".into());
        model.parts[0].package = Some("THPAD".into());
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().vias.push(Via { id: "v1".into(), net: "N1".into(), at: a, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_redundant_vias: true, ..Default::default() });
        assert_eq!(report.remove_via_ids(), vec!["v1".to_string()]);
    }

    #[test]
    fn a_track_fully_inside_a_pad_is_removable() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        // A tiny stub that never leaves R1's own pad.
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.05), pts: vec![a, Point { x: a.x + Nm::new(5), y: a.y }], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_tracks_in_pads: true, ..Default::default() });
        assert_eq!(report.remove_track_ids(), vec!["t1".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::TrackInPad));
    }

    #[test]
    fn a_track_bridging_two_different_nets_pads_is_shorting() {
        let (mut design, mut model) = two_pad_model();
        // Give R2's pad its own net so R1-R2 is a real short, not a
        // same-net ratsnest connection.
        model.nets[0].pins = vec!["R1.1".into()];
        model.nets.push(eda_model::Net { name: "N2".into(), pins: vec!["R2.1".into()] });
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, b], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_shorting: true, ..Default::default() });
        assert_eq!(report.remove_track_ids(), vec!["t1".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::ShortingTrack));
    }

    #[test]
    fn a_dangling_stub_is_removed_when_asked() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let free_end = Point { x: a.x, y: a.y + mm(10.0) };
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, free_end], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_dangling_tracks: true, ..Default::default() });
        assert_eq!(report.remove_track_ids(), vec!["t1".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::DanglingTrack));
    }

    #[test]
    fn two_collinear_stubs_merge_into_one_track() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let mid = Point { x: (a.x + b.x) / 2, y: a.y };
        assert_eq!(a.y, b.y, "fixture pads must be level for this collinearity test");
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, mid], arc_mid_offset: None });
        rt.tracks.push(Track { id: "t2".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![mid, b], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { merge_segments: true, ..Default::default() });
        let merge = report.changes.iter().find(|c| matches!(c.kind, CleanupKind::MergedTracks)).expect("a merge change");
        assert_eq!(merge.remove_track_ids.len(), 2);
        let merged = report.add_tracks().into_iter().next().expect("a merged track");
        assert_eq!(merged.pts, vec![a, b]);
    }

    #[test]
    fn a_third_track_at_the_joint_blocks_the_merge() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let mid = Point { x: (a.x + b.x) / 2, y: a.y };
        let off = Point { x: mid.x, y: mid.y + mm(5.0) };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, mid], arc_mid_offset: None });
        rt.tracks.push(Track { id: "t2".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![mid, b], arc_mid_offset: None });
        // A third leg stubbing off the same joint -- a real node, must
        // not be silently absorbed by the merge.
        rt.tracks.push(Track { id: "t3".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![mid, off], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { merge_segments: true, ..Default::default() });
        assert!(report.changes.iter().all(|c| !matches!(c.kind, CleanupKind::MergedTracks)), "{:?}", report.changes.iter().map(|c| c.kind).collect::<Vec<_>>());
    }

    #[test]
    fn a_via_drops_a_dangling_via_connected_to_only_one_layer() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x, y: a.y + mm(3.0) };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, via_at], arc_mid_offset: None });
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_dangling_vias: true, ..Default::default() });
        assert_eq!(report.remove_via_ids(), vec!["v1".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::DanglingVia));
    }

    #[test]
    fn no_placement_section_does_not_panic() {
        let (mut design, model) = two_pad_model();
        design.placement = None;
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_tracks_in_pads: true, delete_shorting: true, ..Default::default() });
        assert!(report.changes.is_empty());
    }

    fn trk(id: &str, net: &str, width: Nm, pts: Vec<Point>) -> Track {
        Track { id: id.into(), net: net.into(), pins: vec![], layer: "F.Cu".into(), width, pts, arc_mid_offset: None }
    }

    /// R1 gets a one-pad footprint of the given pad.
    fn one_pad_part(model: &mut ConstraintModel, name: &str, size: Nm, shape: eda_model::PadShape, kind: PadKind) {
        let drill = (kind == PadKind::ThroughHole).then(|| mm(0.8));
        model.footprints.push(eda_model::Footprint {
            name: name.into(),
            pads: vec![eda_model::Pad { opposite_side: false, ext: Default::default(), number: "1".into(), at: (Nm::ZERO, Nm::ZERO), size: (size, size), shape, kind, drill, drill_slot: None, rot: 0, roundrect_ratio: None }],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
            models3d: vec![],
            art: Default::default(),
        });
        model.parts[0].footprint = Some(name.into());
        model.parts[0].package = Some(name.into());
    }

    #[test]
    fn cleanup_dangling_removes_only_the_dangling_segment_of_a_polyline() {
        // `deleteDanglingTracks` removes the one `PCB_TRACK` that dangles: T1's Q-R; R1.1-Q still holds T2 at Q
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let q = Point { x: (a.x + b.x) / 2, y: a.y };
        let r = Point { x: q.x, y: q.y + mm(5.0) };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(trk("T1", "N1", mm(0.2), vec![a, q, r]));
        rt.tracks.push(trk("T2", "N1", mm(0.2), vec![q, b]));
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_dangling_tracks: true, ..Default::default() });
        assert_eq!(report.remove_track_ids(), vec!["T1".to_string()]);
        let added = report.add_tracks();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].pts, vec![a, q], "the segment that leads to the junction stays");
        assert_eq!(report.changes.len(), 1);
        assert_eq!(report.changes[0].remove_track_ids, vec!["T1".to_string()], "the change names the board's track");
    }

    #[test]
    fn a_polyline_whose_segments_all_stay_is_left_alone() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let q = Point { x: a.x, y: a.y - mm(3.0) };
        design.routing.as_mut().unwrap().tracks.push(trk("T1", "N1", mm(0.2), vec![a, q, b]));
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_dangling_tracks: true, delete_shorting: true, delete_tracks_in_pads: true, merge_segments: true, delete_redundant_vias: true, delete_dangling_vias: true });
        assert!(report.changes.is_empty() && report.add_tracks().is_empty() && report.remove_track_ids().is_empty(), "{:?}", report.changes);
    }

    #[test]
    fn a_shorting_last_segment_goes_and_the_first_stays() {
        let (mut design, mut model) = two_pad_model();
        model.nets[0].pins = vec!["R1.1".into()];
        model.nets.push(eda_model::Net { name: "N2".into(), pins: vec!["R2.1".into()] });
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let q = Point { x: a.x, y: a.y - mm(3.0) };
        design.routing.as_mut().unwrap().tracks.push(trk("T1", "N1", mm(0.2), vec![a, q, b]));
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_shorting: true, ..Default::default() });
        assert!(matches!(report.changes[0].kind, CleanupKind::ShortingTrack));
        assert_eq!(report.remove_track_ids(), vec!["T1".to_string()]);
        assert_eq!(report.add_tracks().iter().map(|t| t.pts.clone()).collect::<Vec<_>>(), vec![vec![a, q]]);
    }

    #[test]
    fn shorting_track_to_a_no_net_pad_is_removed() {
        // `segment->GetNetCode() != testedPad->GetNetCode()`, net 0 included
        let (mut design, mut model) = two_pad_model();
        model.nets[0].pins = vec!["R1.1".into()];
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        design.routing.as_mut().unwrap().tracks.push(trk("t1", "N1", mm(0.2), vec![a, b]));
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_shorting: true, ..Default::default() });
        assert_eq!(report.remove_track_ids(), vec!["t1".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::ShortingTrack));
    }

    #[test]
    fn a_track_inside_one_pad_with_nothing_else_dangles_for_the_cleaner() {
        // `TestTrackEndpointDangling( track, false )`: the pad counts once, at the nearer end
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().tracks.push(trk("t1", "N1", mm(0.1), vec![a, Point { x: a.x + mm(0.1), y: a.y }]));
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_dangling_tracks: true, ..Default::default() });
        assert_eq!(report.remove_track_ids(), vec!["t1".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::DanglingTrack));
    }

    #[test]
    fn a_track_in_a_pad_is_measured_with_its_width() {
        let (mut design, mut model) = two_pad_model();
        one_pad_part(&mut model, "SQ1", mm(1.0), eda_model::PadShape::Rect, PadKind::Smd);
        let a = pad_center(&design, &model, "R1", "1");
        let (l, r) = (Point { x: a.x - mm(0.3), y: a.y }, Point { x: a.x + mm(0.3), y: a.y });
        design.routing.as_mut().unwrap().tracks.push(trk("wide", "N1", mm(0.8), vec![l, r]));
        design.routing.as_mut().unwrap().tracks.push(trk("thin", "N1", mm(0.2), vec![l, r]));
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_tracks_in_pads: true, ..Default::default() });
        assert_eq!(report.remove_track_ids(), vec!["thin".to_string()], "the 0.8 mm track overhangs the 1 mm pad");
    }

    #[test]
    fn a_via_off_the_centre_of_a_through_hole_pad_is_redundant() {
        let (mut design, mut model) = two_pad_model();
        one_pad_part(&mut model, "TH17", mm(1.7), eda_model::PadShape::Circle, PadKind::ThroughHole);
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().vias.push(Via { id: "v1".into(), net: "N1".into(), at: Point { x: a.x + mm(0.3), y: a.y }, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_redundant_vias: true, ..Default::default() });
        assert_eq!(report.remove_via_ids(), vec!["v1".to_string()]);
    }

    #[test]
    fn a_via_written_upside_down_is_still_a_duplicate() {
        // `GetLayerSet()` has no direction
        let (mut design, model) = two_pad_model();
        let p = Point { x: mm(10.0), y: mm(15.0) };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: p, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.vias.push(Via { id: "v2".into(), net: "N1".into(), at: p, drill: mm(0.3), diameter: mm(0.6), from_layer: "B.Cu".into(), to_layer: "F.Cu".into() });
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { delete_redundant_vias: true, ..Default::default() });
        assert_eq!(report.remove_via_ids(), vec!["v1".to_string()]);
    }

    #[test]
    fn a_duplicate_segment_inside_a_polyline_goes_and_the_polyline_is_cut() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let q = Point { x: a.x, y: a.y - mm(3.0) };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(trk("T1", "N1", mm(0.2), vec![a, q, b]));
        rt.tracks.push(trk("t2", "N1", mm(0.2), vec![q, b]));
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions::default());
        // T1's second segment is earlier in the list than t2, so it is the one that goes
        assert_eq!(report.remove_track_ids(), vec!["T1".to_string()]);
        assert_eq!(report.add_tracks().iter().map(|t| t.pts.clone()).collect::<Vec<_>>(), vec![vec![a, q]]);
    }

    #[test]
    fn an_smd_pad_on_the_footprints_other_side_is_on_that_sides_copper() {
        let (mut design, mut model) = two_pad_model();
        one_pad_part(&mut model, "BK1", mm(1.0), eda_model::PadShape::Rect, PadKind::Smd);
        model.footprints.last_mut().unwrap().pads[0].opposite_side = true;
        let pads = pad_geoms(&design, &model);
        assert_eq!(pads[0].layer.as_deref(), Some("B.Cu"));
        design.placement.as_mut().unwrap().footprints[0].side = Side::Bottom;
        assert_eq!(pad_geoms(&design, &model)[0].layer.as_deref(), Some("F.Cu"));
    }

    #[test]
    fn unused_import_guard() {
        // Keep FootprintInstance/Side imports honest if a future edit
        // trims the fixtures above; cheap sentinel, not a real assertion.
        let _ = FootprintInstance { id: "x".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, rot: 0, side: Side::Top, label: Default::default() };
    }
}

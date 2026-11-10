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
//!   first, and a polyline that loses or changes one comes back as the runs of segments that stay (`explode`, `finish`): the first
//!   run under the polyline's id, the others new tracks that take its mask opening and group. A merged track is the first of the two
//!   segments modified in place, under its id (`mergeCollinearSegments`: `*aSeg1 = dummy_seg`), as KiCad keeps the uuid of what it
//!   does not delete.
//! - "Track in pad" measures the track's width against the pad's copper (`PlacedPad::signed_distance`, see
//!   `track_inside_pad`) rather than KiCad's `SHAPE_POLY_SET` boolean subtract; the same for a plain pad, sampled for a pad
//!   made of pieces, and an oval is its inscribed ellipse.
//! - Collinearity is `SEG::ApproxCollinear`'s, on the nanometre counts (the shorter segment's ends within 1 nm of the longer one's line).
//! - The merge is `testMergeCollinearSegments` ([`test_merge`]): two touching straight segments of one width, collinear, merge into the span of
//!   their four ends -- end to end, overlapping, folded back or one inside the other -- unless an end that another item meets would not be an end
//!   of the merged segment, or more than two ends are met. The items at the ends are asked as KiCad asks them: a track's or via's end there,
//!   a pad's copper or a zone's outline (`ZONE::HitTest`: a corner or an edge, not its inside, round 4 A-5) within `( width + 1 ) / 2` of it,
//!   a zone only when its fill touches the segment. A segment with a straight track of another width attached is not merged (`mergeSegments`'
//!   necking-down rule). A track the dialog's filter skips is not counted as attached, a locked one is (`!filterItem( candidate )`, R6-Z4).

use std::collections::{BTreeMap, BTreeSet};

use eda_drc::zone_copper::ZoneCopper;
use eda_model::ir::{Design, Point, RoutingSection, Track, Via, Zone};
use eda_model::footprint::PlacedPad;
use eda_model::{ConstraintModel, PadKind};

use crate::algo::build_graph;
use crate::cleanup_filter::{CleanupFilter, Protected};
use crate::dangling::{dangling_tracks_and_vias_with, DanglingKind};
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
    /// The tracks the cleanup adds: a track a merge changed, under its own id (it is in [`CleanupReport::removed_tracks`] too: the board's track
    /// of that id, modified in place), and the runs of a polyline's segments that stay when another segment of it goes or changes (the first
    /// under the polyline's id, the others with none).
    pub added: Vec<Track>,
    /// The new tracks of [`CleanupReport::added`] (indices into it) that stand for a removed polyline, as (index, the polyline's id): each takes its
    /// mask opening and group (`Cmd::CommitRoute`'s `track_sources`).
    pub sources: Vec<(usize, String)>,
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
    /// The indices into [`CleanupReport::add_tracks`] of the tracks that keep their id (`Cmd::CommitRoute`'s `keep_track_ids`).
    pub fn kept_track_indices(&self) -> Vec<usize> {
        self.added.iter().enumerate().filter(|(_, t)| !t.id.is_empty()).map(|(i, _)| i).collect()
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
    let (zones, track_width_presets, via_presets, diff_pair_presets, teardrop_settings) = match &d.routing {
        Some(rt) => (rt.zones.clone(), rt.track_width_presets.clone(), rt.via_presets.clone(), rt.diff_pair_presets.clone(), rt.teardrop_settings),
        None => (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Default::default()),
    };
    d.routing = Some(RoutingSection { tracks: tracks.to_vec(), vias: vias.to_vec(), zones, track_width_presets, via_presets, diff_pair_presets, teardrop_settings });
    d
}

/// The working list's name for segment `k` of polyline `id` (see [`explode`]).
fn piece_id(id: &str, k: usize) -> String {
    format!("{id}~{k}")
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

/// The runs of consecutive segments of a cut polyline that stay, in its order: segment `k + 1` goes on the run when one of its ends is the run's
/// last point (or, for a run of one segment, either of its points: a merge may have turned the segment round, `testMergeCollinearSegments`' dummy
/// runs the way its bounding box does).
fn runs_of(segs: &[Option<&Track>]) -> Vec<Vec<Point>> {
    let mut runs: Vec<Vec<Point>> = Vec::new();
    let mut cur: Vec<Point> = Vec::new();
    for s in segs {
        let Some(s) = s else {
            if cur.len() >= 2 {
                runs.push(std::mem::take(&mut cur));
            }
            cur.clear();
            continue;
        };
        let (a, b) = (s.pts[0], s.pts[1]);
        if cur.len() == 2 && (cur[0] == a || cur[0] == b) && cur[1] != a && cur[1] != b {
            cur.reverse();
        }
        match cur.last() {
            Some(&l) if l == a => cur.push(b),
            Some(&l) if l == b => cur.push(a),
            _ => {
                if cur.len() >= 2 {
                    runs.push(std::mem::take(&mut cur));
                }
                cur = vec![a, b];
            }
        }
    }
    if cur.len() >= 2 {
        runs.push(cur);
    }
    runs
}

/// What the board loses and gains once every pass has run over the working list `work` (see [`explode`]):
/// - a track that is gone is removed;
/// - a track a merge changed (`mergeCollinearSegments` modifies the first segment in place) is removed and added back under its own id, so it
///   keeps its mask opening, group and lock;
/// - a polyline whose segments all stay as they were is left as it is; one that lost or changed a segment is removed, and each run of its
///   consecutive segments that stay comes back as a track: the first under the polyline's id (KiCad keeps the uuid of every segment it does not
///   delete, and this model has one id for the whole line), the others as new tracks taking its mask opening and group (`sources`).
///
/// The changes' own ids are mapped back to the board's: a segment to its polyline.
fn finish(original: &[Track], work: &[Track], pieces: &BTreeMap<String, (String, usize)>, changes: &mut [CleanupChange]) -> (Vec<String>, Vec<Track>, Vec<(usize, String)>) {
    let by_id: BTreeMap<&str, &Track> = work.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut removed = BTreeSet::new();
    let mut added: Vec<Track> = Vec::new();
    let mut sources: Vec<(usize, String)> = Vec::new();
    for t in original {
        let cut = pieces.contains_key(&piece_id(&t.id, 0));
        if !cut {
            match by_id.get(t.id.as_str()) {
                None => {
                    removed.insert(t.id.clone());
                }
                Some(w) if w.pts != t.pts || w.pins != t.pins => {
                    removed.insert(t.id.clone());
                    added.push((*w).clone());
                }
                Some(_) => {}
            }
            continue;
        }
        let segs: Vec<Option<&Track>> = (0..t.pts.len() - 1).map(|k| by_id.get(piece_id(&t.id, k).as_str()).copied()).collect();
        if segs.iter().enumerate().all(|(k, s)| s.is_some_and(|s| s.pts[..] == t.pts[k..=k + 1])) {
            continue;
        }
        removed.insert(t.id.clone());
        for (n, pts) in runs_of(&segs).into_iter().enumerate() {
            if n > 0 {
                sources.push((added.len(), t.id.clone()));
            }
            // the pads the polyline ends on stay with the run that keeps its id (`pins` names no end; the merges' pads are the segments')
            let pins = if n == 0 { t.pins.clone() } else { Vec::new() };
            added.push(Track { id: if n == 0 { t.id.clone() } else { String::new() }, net: t.net.clone(), pins, layer: t.layer.clone(), width: t.width, pts, arc_mid_offset: None });
        }
    }
    for c in changes.iter_mut() {
        let mut ids: Vec<String> = Vec::new();
        for id in &c.remove_track_ids {
            let shown = match pieces.get(id) {
                Some((orig, _)) => orig.clone(),
                None => id.clone(),
            };
            if !ids.contains(&shown) {
                ids.push(shown);
            }
        }
        c.remove_track_ids = ids;
        if let Some(t) = c.add_track.as_mut() {
            if let Some((orig, _)) = pieces.get(&t.id) {
                t.id = orig.clone();
            }
        }
    }
    (removed.into_iter().collect(), added, sources)
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

/// The copper layer range of a via (`from_layer`..`to_layer` in the board's stack order, either way round).
fn via_spans(v: &Via, layer: &str, stack: &[String]) -> bool {
    let at = |l: &str| stack.iter().position(|s| s == l);
    match (at(&v.from_layer), at(&v.to_layer), at(layer)) {
        (Some(a), Some(b), Some(l)) => a.min(b) <= l && l <= a.max(b),
        // a layer the stack does not name: the via's own two
        _ => v.from_layer == layer || v.to_layer == layer,
    }
}

/// Is the straight segment `a`-`b` of half width `hw` among the items connectivity joins to a zone fill fragment `ring` (`checkZoneItemConnection`)?
fn fill_touches_segment(ring: &[Point], a: Point, b: Point, hw: f64) -> bool {
    crate::geom::touches(&ItemShape::Zone { outline: ring.to_vec() }, ring, &ItemShape::Segment { a, b, half_width: hw }, &[a, b])
}

/// `SEG::mutualDistanceSquared` and `SEG::ApproxCollinear( aSeg, 1 )` (`libs/kimath/src/geometry/seg.cpp`), on the nanometre counts: the shorter
/// segment's ends within 1 nm of the line through the longer one, `rescale`'s rounding to nearest kept. A zero-length longer segment is not collinear.
fn approx_collinear(s1: (Point, Point), s2: (Point, Point)) -> bool {
    let sq = |s: (Point, Point)| (s.1.x - s.0.x).area(s.1.x - s.0.x) + (s.1.y - s.0.y).area(s.1.y - s.0.y);
    let (a, b) = if sq(s1) < sq(s2) { (s2, s1) } else { (s1, s2) };
    let p = (a.0.y.get() - a.1.y.get()) as i128;
    let q = (a.1.x.get() - a.0.x.get()) as i128;
    let r = -p * a.0.x.get() as i128 - q * a.0.y.get() as i128;
    let l = p * p + q * q;
    if l == 0 {
        return false;
    }
    let det1 = p * b.0.x.get() as i128 + q * b.0.y.get() as i128 + r;
    let det2 = p * b.1.x.get() as i128 + q * b.1.y.get() as i128 + r;
    // `rescale( det, det, l )`: the product is never negative and `l` is positive, so it rounds up from the half
    let dsq = |d: i128| (d * d + l / 2) / l;
    dsq(det1) <= 1 && dsq(det2) <= 1
}

/// `PCB_TRACK::IsPointOnEnds( p )` with no distance: an end of one of a polyline's segments (a vertex) exactly at `p`.
fn has_end_at(t: &Track, p: Point) -> bool {
    t.pts.contains(&p)
}

/// `TRACKS_CLEANER::testMergeCollinearSegments( aSeg1, aSeg2, aDummySeg )`: the merged segment of the straight tracks `seg1` and `seg2`, or `None`
/// when they may not merge. Locked tracks never merge (the caller skips them: `IsLocked()`). The four ends are flagged where another item
/// connected to the segment meets them (`collectPtsSeg1` / `collectPtsSeg2`, every item but the two):
/// - a track, an arc or a via: `IsPointOnEnds( p )`, an end exactly there -- a via only when it reaches the segments' layer (connected);
/// - a pad: `PAD::HitTest( p, ( width + 1 ) / 2 )`, its copper on the layer within half the width of the end;
/// - a zone: `ZONE::HitTest( p, ( width + 1 ) / 2 )` -- `accuracy = std::max( aAccuracy, mmToIU( 0.1 ) )`, `HitTestForCorner( p, accuracy * 2 ) ||
///   HitTestForEdge( p, accuracy )`, the outline's corners and edges, holes included, not its inside -- of a zone whose **fill** on the layer
///   touches the segment (connectivity joins a zone by its fill, `checkZoneItemConnection`); an unfilled zone joins nothing.
///
/// More than two flags is "a node in the center". The merged segment spans the four ends' bounding box (its diagonal, the way `aSeg1` runs: `min`
/// to `max` when its x and y fall together, else `( min_x, max_y )` to `( max_x, min_y )`), and every flagged end must be one of its ends. So two
/// segments that overlap, or one folded back over the other, or one inside the other, merge into their span as two meeting end to end do.
/// `testTrackEndpointIsNode( aSeg1, .. )` comes last: it counts only an anchor at both of `aSeg1`'s ends at once, which a segment of non-zero
/// length (the null segments go first when merging is on) never has, so it never refuses here.
#[allow(clippy::too_many_arguments)]
fn test_merge(seg1: &Track, seg2: &Track, tracks: &[Track], vias: &[Via], pads: &[PadGeom], zones: &[Zone], copper: &ZoneCopper, stack: &[String]) -> Option<[Point; 2]> {
    let layer = seg1.layer.as_str();
    let ends = [seg1.pts[0], seg1.pts[1], seg2.pts[0], seg2.pts[1]];
    let flagged_by = |seg: &Track, p: Point| -> bool {
        let (a, b) = (seg.pts[0], seg.pts[1]);
        let accuracy = ((seg.width.get() + 1) / 2) as f64;
        if tracks.iter().any(|t| t.id != seg1.id && t.id != seg2.id && t.layer == layer && has_end_at(t, p)) {
            return true;
        }
        if vias.iter().any(|v| v.at == p && via_spans(v, layer, stack)) {
            return true;
        }
        if pads.iter().any(|pg| pad_covers_layer(pg, layer) && pg.pad.signed_distance(p) <= accuracy) {
            return true;
        }
        let zone_accuracy = accuracy.max(100_000.0);
        let hits = |ring: &[Point]| {
            let n = ring.len();
            ring.iter().any(|c| ((c.x - p.x).as_f64()).hypot((c.y - p.y).as_f64()) <= 2.0 * zone_accuracy) || (0..n).any(|i| crate::geom::point_seg_distance(p, ring[i], ring[(i + 1) % n]) <= zone_accuracy)
        };
        let hw = seg.width.as_f64() / 2.0;
        zones.iter().any(|z| {
            if !z.is_on_layer(layer) || z.outline.len() < 3 || !(hits(&z.outline) || z.holes.iter().any(|h| h.len() >= 2 && hits(h))) {
                return false;
            }
            let Some(fill) = copper.get(&z.id, layer) else { return false };
            fill.polys.iter().filter_map(|poly| poly.first()).any(|chain| {
                let ring: Vec<Point> = chain.iter().map(|q| Point { x: Nm::new(q.x), y: Nm::new(q.y) }).collect();
                ring.len() >= 3 && fill_touches_segment(&ring, a, b, hw)
            })
        })
    };
    let flags: Vec<bool> = (0..4).map(|i| flagged_by(if i < 2 { seg1 } else { seg2 }, ends[i])).collect();
    if flags.iter().filter(|&&f| f).count() > 2 {
        return None;
    }
    let min_x = ends.iter().map(|p| p.x).min().unwrap();
    let min_y = ends.iter().map(|p| p.y).min().unwrap();
    let max_x = ends.iter().map(|p| p.x).max().unwrap();
    let max_y = ends.iter().map(|p| p.y).max().unwrap();
    let (s, e) = (seg1.pts[0], seg1.pts[1]);
    let dummy = if (s.x > e.x) == (s.y > e.y) {
        [Point { x: min_x, y: min_y }, Point { x: max_x, y: max_y }]
    } else {
        [Point { x: min_x, y: max_y }, Point { x: max_x, y: min_y }]
    };
    if (0..4).any(|i| flags[i] && !dummy.contains(&ends[i])) {
        return None;
    }
    Some(dummy)
}

/// Do the straight segments `a` and `b` touch (connectivity's `Collide` of their effective shapes, the widths included)?
fn segments_touch(a: &Track, b: &Track) -> bool {
    crate::geom::seg_seg_distance(a.pts[0], a.pts[1], b.pts[0], b.pts[1]) <= (a.width.as_f64() + b.width.as_f64()) / 2.0
}

/// `TRACKS_CLEANER::cleanup`'s `mergeSegments`: "Do not merge an end which has different width tracks attached -- it's a common use-case for
/// necking-down a track between pads." A segment that connectivity joins (anywhere along it) to a straight track (`PCB_TRACE_T`, not an arc) of
/// another width takes part in no merge as the segment the pass starts from. A track the dialog's filter leaves out is no candidate (`!filterItem( candidate )`,
/// R6-Z4); a locked one is.
fn has_different_width_attached(seg: &Track, tracks: &[Track], prot: &Protected) -> bool {
    let (a, b) = (seg.pts[0], seg.pts[1]);
    let hw = seg.width.as_f64() / 2.0;
    tracks.iter().any(|t| {
        t.id != seg.id
            && !prot.filtered_tracks.contains(&t.id)
            && t.layer == seg.layer
            && t.width != seg.width
            && t.arc().is_none()
            && t.pts.windows(2).any(|w| crate::geom::seg_seg_distance(a, b, w[0], w[1]) <= hw + t.width.as_f64() / 2.0)
    })
}

/// `TRACKS_CLEANER::cleanup`'s merge pass (`mergeSegments`), run to a fixed point as source's own `do { ... } while( mergeSegments( .. ) )`. Each pass
/// first finds, for every straight segment in the board's order (KiCad's `candidate < segment` on the items' addresses), the first later straight
/// segment of the same width it touches that is `ApproxCollinear` with it and may merge with it (`testMergeCollinearSegments`); then merges the
/// pairs in that order, each tested again on the board as the earlier merges left it, a pair one of whose segments an earlier merge removed
/// skipped (`IS_DELETED`). A merge is `mergeCollinearSegments`: the first segment is modified in place (`*aSeg1 = dummy_seg`: it keeps its id,
/// so its mask opening, its group and its lock stay; the pads it ends on are both segments'), the second goes.
///
/// The two segments need not be of one net (UI-LEFT-2 PP23): the candidates are whatever connectivity joins to the segment (`citem->ConnectedItems()`), and
/// `CN_VISITOR::operator()` (`pcbnew/connectivity/connectivity_algo.cpp`) joins two items of different nets unless neither can change net
/// (`!aCandidate->CanChangeNet() && !m_item->CanChangeNet()`), which a track always can (`CN_LIST::Add( PCB_TRACK* )`: `new CN_ITEM( track, true )`). So two
/// touching collinear tracks of two nets merge, the merged track keeping the first one's net (`*aSeg1 = dummy_seg`), before "Delete tracks connecting different
/// nets" -- which `CleanupBoard` runs after `cleanup` -- sees the short.
#[allow(clippy::too_many_arguments)]
fn merge_collinear(tracks: &mut Vec<Track>, vias: &[Via], pads: &[PadGeom], zones: &[Zone], copper: &ZoneCopper, stack: &[String], prot: &Protected, changes: &mut Vec<CleanupChange>) -> bool {
    let mut changed_any = false;
    loop {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for i in 0..tracks.len() {
            let seg = &tracks[i];
            // `filterItem( segment )`, and `testMergeCollinearSegments`'s `IsLocked()` ("one can merge only collinear segments, not vias or arcs")
            if prot.track(&seg.id) || seg.pts.len() != 2 || seg.arc().is_some() || has_different_width_attached(seg, tracks, prot) {
                continue;
            }
            for cand in &tracks[i + 1..] {
                if prot.track(&cand.id) || cand.pts.len() != 2 || cand.arc().is_some() || cand.width != seg.width || cand.layer != seg.layer || !segments_touch(seg, cand) {
                    continue;
                }
                if approx_collinear((seg.pts[0], seg.pts[1]), (cand.pts[0], cand.pts[1])) && test_merge(seg, cand, tracks, vias, pads, zones, copper, stack).is_some() {
                    pairs.push((seg.id.clone(), cand.id.clone()));
                    break;
                }
            }
        }
        if pairs.is_empty() {
            break;
        }
        changed_any = true;
        for (id1, id2) in pairs {
            let (Some(i), Some(j)) = (tracks.iter().position(|t| t.id == id1), tracks.iter().position(|t| t.id == id2)) else { continue };
            let Some(dummy) = test_merge(&tracks[i], &tracks[j], tracks, vias, pads, zones, copper, stack) else { continue };
            let b = tracks.remove(j);
            let i = if j < i { i - 1 } else { i };
            let a = &mut tracks[i];
            let before = a.clone();
            a.pts = dummy.to_vec();
            for pin in &b.pins {
                if !a.pins.contains(pin) {
                    a.pins.push(pin.clone());
                }
            }
            changes.push(CleanupChange { kind: CleanupKind::MergedTracks, net: a.net.clone(), remove_track_ids: vec![before.id.clone(), b.id.clone()], remove_via_ids: vec![], add_track: Some(a.clone()) });
        }
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

    remove_redundant(&mut tracks, &mut vias, design, model, copper, &opts, &prot, &mut changes);

    if opts.merge_segments {
        merge_collinear(&mut tracks, &vias, &pads, &zones, copper, &model.board.layers, &prot, &mut changes);
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
        merge_collinear(&mut tracks, &vias, &pads, &zones, copper, &model.board.layers, &prot, &mut changes);
    }

    let (removed_tracks, added, sources) = finish(&original, &tracks, &pieces, &mut changes);
    CleanupReport { changes, removed_tracks, added, sources }
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

    /// Round 4 A-5: `testMergeCollinearSegments` asks a zone `ZONE::HitTest( joint, ( width + 1 ) / 2 )`, its corners and edges: two segments meeting
    /// in the middle of a pour of their net merge; meeting on its edge they do not.
    #[test]
    fn merge_collinear_inside_a_zone() {
        let (mut design, model) = two_pad_model();
        let p = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        let rt = design.routing.as_mut().unwrap();
        rt.zones.push(Zone { id: "z".into(), net: "N1".into(), layer: "F.Cu".into(), outline: vec![p(-50.0, -50.0), p(50.0, -50.0), p(50.0, 50.0), p(-50.0, 50.0)], ..Default::default() });
        rt.tracks.push(Track { id: "a".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(-20.0, -30.0), p(-15.0, -30.0)], arc_mid_offset: None });
        rt.tracks.push(Track { id: "b".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(-15.0, -30.0), p(-10.0, -30.0)], arc_mid_offset: None });
        let opts = CleanupOptions { merge_segments: true, ..Default::default() };
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), opts);
        assert_eq!(report.changes.iter().filter(|c| matches!(c.kind, CleanupKind::MergedTracks)).count(), 1, "{:?}", report.changes.iter().map(|c| c.kind).collect::<Vec<_>>());
        // the joint on the zone's edge: a node, once the zone's fill reaches the segments (R5-Z6: connectivity joins a zone by its fill)
        let rt = design.routing.as_mut().unwrap();
        let n = rt.tracks.len();
        rt.tracks[n - 2].pts = vec![p(-20.0, -50.0), p(-15.0, -50.0)];
        rt.tracks[n - 1].pts = vec![p(-15.0, -50.0), p(-10.0, -50.0)];
        let merges = |copper: &ZoneCopper| compute_cleanup(&design, &model, copper, opts).changes.iter().filter(|c| matches!(c.kind, CleanupKind::MergedTracks)).count();
        assert_eq!(merges(&ZoneCopper::default()), 1, "an unfilled zone is connected to nothing");
        let mut copper = ZoneCopper::default();
        let ring = [(-50.0, -50.0), (50.0, -50.0), (50.0, 50.0), (-50.0, 50.0)].iter().map(|&(x, y)| eda_clipper2::Point64 { x: mm(x).get(), y: mm(y).get(), z: 0 }).collect();
        copper.zones.insert(("z".into(), "F.Cu".into()), eda_shape_poly_set::ShapePolySet::from_outline(ring));
        assert_eq!(merges(&copper), 0);
    }

    /// R5-Z4 (`mergeSegments`: "Do not merge an end which has different width tracks attached"): C1 (0.15 mm), A (0.25 mm), B (0.25 mm), C2 (0.15 mm)
    /// in a collinear chain, A and B meeting at a free point: nothing merges. Without C1 and C2, A and B merge.
    #[test]
    fn merge_skips_a_segment_with_a_different_width_attached() {
        let (mut design, model) = two_pad_model();
        let p = |x: f64| Point { x: mm(x), y: mm(15.0) };
        let t = |id: &str, a: f64, b: f64, w: f64| Track { id: id.into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(w), pts: vec![p(a), p(b)], arc_mid_offset: None };
        design.routing.as_mut().unwrap().tracks = vec![t("c1", 2.0, 4.0, 0.15), t("a", 4.0, 6.0, 0.25), t("b", 6.0, 8.0, 0.25), t("c2", 8.0, 10.0, 0.15)];
        let opts = CleanupOptions { merge_segments: true, ..Default::default() };
        let merges = |d: &Design| compute_cleanup(d, &model, &ZoneCopper::default(), opts).changes.iter().filter(|c| matches!(c.kind, CleanupKind::MergedTracks)).count();
        assert_eq!(merges(&design), 0);
        design.routing.as_mut().unwrap().tracks.retain(|t| t.id == "a" || t.id == "b");
        assert_eq!(merges(&design), 1);
    }

    /// R5-Z6: a pad counts at the joint within half the track's width (`PAD::HitTest( p, ( width + 1 ) / 2 )`), and a via only when it reaches the
    /// segments' layer (connectivity joins it to them).
    #[test]
    fn merge_keeps_a_joint_within_half_width_of_a_pad_and_ignores_a_via_off_the_layer() {
        let (mut design, mut model) = two_pad_model();
        model.board.layers = vec!["F.Cu".into(), "In1.Cu".into(), "In2.Cu".into(), "B.Cu".into()];
        let c = pad_center(&design, &model, "R1", "1");
        let edge = c.x + (0..).map(|k| Nm::new(k * 1_000)).find(|&d| design_pad_distance(&design, &model, Point { x: c.x + d, y: c.y }) > 0.0).unwrap();
        // the joint 0.1 mm outside the pad's edge, the segments 0.25 mm wide running away from it, upwards
        let j = Point { x: edge + mm(0.1), y: c.y };
        let t = |id: &str, a: Point, b: Point| Track { id: id.into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.25), pts: vec![a, b], arc_mid_offset: None };
        design.routing.as_mut().unwrap().tracks = vec![t("a", Point { x: j.x, y: j.y - mm(2.0) }, j), t("b", j, Point { x: j.x, y: j.y + mm(2.0) })];
        let opts = CleanupOptions { merge_segments: true, ..Default::default() };
        let merges = |d: &Design, m: &ConstraintModel| compute_cleanup(d, m, &ZoneCopper::default(), opts).changes.iter().filter(|c| matches!(c.kind, CleanupKind::MergedTracks)).count();
        assert_eq!(merges(&design, &model), 0, "the pad is at the joint");
        // away from the pad, a blind via In1-In2 at the joint does not reach F.Cu: the segments merge; one from F.Cu keeps them apart
        let far = Point { x: j.x + mm(5.0), y: j.y };
        design.routing.as_mut().unwrap().tracks = vec![t("a", Point { x: far.x, y: far.y - mm(2.0) }, far), t("b", far, Point { x: far.x, y: far.y + mm(2.0) })];
        let via = |from: &str, to: &str| Via { id: "v".into(), net: "N1".into(), at: far, drill: mm(0.3), diameter: mm(0.6), from_layer: from.into(), to_layer: to.into() };
        design.routing.as_mut().unwrap().vias = vec![via("In1.Cu", "In2.Cu")];
        assert_eq!(merges(&design, &model), 1);
        design.routing.as_mut().unwrap().vias = vec![via("F.Cu", "In1.Cu")];
        assert_eq!(merges(&design, &model), 0);
    }

    fn design_pad_distance(design: &Design, model: &ConstraintModel, p: Point) -> f64 {
        pad_geoms(design, model).iter().map(|g| g.pad.signed_distance(p)).fold(f64::MAX, f64::min)
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
        // `mergeCollinearSegments`: the first segment is modified in place, the second goes
        assert_eq!((merged.id.as_str(), report.remove_track_ids()), ("t1", vec!["t1".to_string(), "t2".to_string()]));
        assert_eq!(report.kept_track_indices(), vec![0]);
    }

    /// `testMergeCollinearSegments`' span of the four ends: two collinear segments of one width that overlap merge into the span, under the first's
    /// id; one inside the other with nothing at its ends goes, the outer one unchanged; one folded back over the other merges too (the old pass
    /// took only two segments meeting end to end).
    #[test]
    fn overlapping_contained_and_folded_back_collinear_segments_merge_into_their_span() {
        let (design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let at = |f: f64| Point { x: a.x + Nm::new(((b.x - a.x).get() as f64 * f) as i64), y: a.y };
        let opts = CleanupOptions { merge_segments: true, ..Default::default() };
        let run = |tracks: Vec<Track>| {
            let mut d = design.clone();
            d.routing.as_mut().unwrap().tracks = tracks;
            compute_cleanup(&d, &model, &ZoneCopper::default(), opts)
        };
        // overlapping: a..0.6 and 0.4..b
        let r = run(vec![trk("t1", "N1", mm(0.2), vec![a, at(0.6)]), trk("t2", "N1", mm(0.2), vec![at(0.4), b])]);
        assert_eq!(r.remove_track_ids(), vec!["t1".to_string(), "t2".to_string()]);
        assert_eq!(r.add_tracks().iter().map(|t| (t.id.clone(), t.pts.clone())).collect::<Vec<_>>(), vec![("t1".to_string(), vec![a, b])]);
        // the second inside the first: it goes, the first stays as it is (nothing to re-add)
        let r = run(vec![trk("t1", "N1", mm(0.2), vec![a, b]), trk("t2", "N1", mm(0.2), vec![at(0.3), at(0.7)])]);
        assert_eq!((r.remove_track_ids(), r.add_tracks().len()), (vec!["t2".to_string()], 0));
        // folded back at a free end: a..0.6, then back to 0.4
        let r = run(vec![trk("t1", "N1", mm(0.2), vec![a, at(0.6)]), trk("t2", "N1", mm(0.2), vec![at(0.6), at(0.4)])]);
        assert_eq!((r.remove_track_ids(), r.add_tracks().len()), (vec!["t2".to_string()], 0));
        // folded back at the pad of R2: a pad there meets two ends (seg1's and seg2's), three with R1's, "a node in the center"
        let r = run(vec![trk("t1", "N1", mm(0.2), vec![a, b]), trk("t2", "N1", mm(0.2), vec![b, at(0.5)])]);
        assert!(r.changes.iter().all(|c| !matches!(c.kind, CleanupKind::MergedTracks)));
        // a track ending at the inner segment's end is a flag the span would not keep: no merge
        let off = Point { x: at(0.7).x, y: a.y + mm(3.0) };
        let r = run(vec![trk("t1", "N1", mm(0.2), vec![a, b]), trk("t2", "N1", mm(0.2), vec![at(0.3), at(0.7)]), trk("t3", "N1", mm(0.2), vec![at(0.7), off])]);
        assert!(r.changes.iter().all(|c| !matches!(c.kind, CleanupKind::MergedTracks)));
        // 1 nm off the line is `ApproxCollinear`; 3 nm is not
        let nudge = |dy: i64| Point { x: b.x, y: b.y + Nm::new(dy) };
        let r = run(vec![trk("t1", "N1", mm(0.2), vec![a, at(0.5)]), trk("t2", "N1", mm(0.2), vec![at(0.5), nudge(1)])]);
        assert_eq!(r.changes.iter().filter(|c| matches!(c.kind, CleanupKind::MergedTracks)).count(), 1);
        let r = run(vec![trk("t1", "N1", mm(0.2), vec![a, at(0.5)]), trk("t2", "N1", mm(0.2), vec![at(0.5), nudge(3)])]);
        assert_eq!(r.changes.iter().filter(|c| matches!(c.kind, CleanupKind::MergedTracks)).count(), 0);
    }

    /// A polyline whose middle vertex is on the line through its ends: its two segments merge, and the line comes back under its own id.
    #[test]
    fn a_polyline_with_a_collinear_vertex_merges_and_keeps_its_id() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let mid = Point { x: (a.x + b.x) / 2, y: a.y };
        let mut line = trk("p1", "N1", mm(0.2), vec![a, mid, b]);
        line.pins = vec!["R1.1".into(), "R2.1".into()];
        design.routing.as_mut().unwrap().tracks.push(line);
        let r = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { merge_segments: true, ..Default::default() });
        assert_eq!(r.remove_track_ids(), vec!["p1".to_string()]);
        let added = r.add_tracks();
        assert_eq!(added.iter().map(|t| (t.id.clone(), t.pts.clone(), t.pins.clone())).collect::<Vec<_>>(), vec![("p1".to_string(), vec![a, b], vec!["R1.1".to_string(), "R2.1".to_string()])]);
        assert!(r.sources.is_empty());
    }

    /// A dangling middle segment of a polyline goes: the run before it keeps the line's id, the one after is new and takes the line's mask and
    /// group (`sources`).
    #[test]
    fn the_runs_of_a_cut_polyline_keep_its_id_and_its_mask_and_group() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let p = |dx: f64, dy: f64| Point { x: a.x + mm(dx), y: a.y + mm(dy) };
        // a duplicate of the polyline's middle segment, which the duplicate pass always removes (the earlier of the two)
        let line = trk("p1", "N1", mm(0.2), vec![a, p(0.0, 2.0), p(2.0, 2.0), p(2.0, 4.0)]);
        let dup = trk("d1", "N1", mm(0.2), vec![p(0.0, 2.0), p(2.0, 2.0)]);
        design.routing.as_mut().unwrap().tracks = vec![dup, line];
        let r = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions::default());
        // the earlier of the two duplicates goes: `d1`, a whole track
        assert_eq!(r.remove_track_ids(), vec!["d1".to_string()]);
        let mut design2 = design.clone();
        design2.routing.as_mut().unwrap().tracks.reverse();
        let r = compute_cleanup(&design2, &model, &ZoneCopper::default(), CleanupOptions::default());
        assert_eq!(r.remove_track_ids(), vec!["p1".to_string()]);
        let added = r.add_tracks();
        assert_eq!(added.iter().map(|t| (t.id.clone(), t.pts.clone())).collect::<Vec<_>>(), vec![("p1".to_string(), vec![a, p(0.0, 2.0)]), (String::new(), vec![p(2.0, 2.0), p(2.0, 4.0)])]);
        assert_eq!(r.sources, vec![(1, "p1".to_string())]);
        assert_eq!(r.kept_track_indices(), vec![0]);
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

    /// `CN_VISITOR` joins two tracks of different nets (`CN_ITEM( track, true )`), so `mergeSegments` merges them: the first one's net and id stay (UI-LEFT-2 PP23).
    #[test]
    fn collinear_tracks_of_two_nets_merge_into_the_first_ones_net() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let mid = Point { x: (a.x + b.x) / 2, y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(trk("t1", "N1", mm(0.2), vec![a, mid]));
        rt.tracks.push(trk("t2", "N2", mm(0.2), vec![mid, b]));
        let report = compute_cleanup(&design, &model, &ZoneCopper::default(), CleanupOptions { merge_segments: true, ..Default::default() });
        let merged: Vec<_> = report.changes.iter().filter(|c| matches!(c.kind, CleanupKind::MergedTracks)).collect();
        assert_eq!(merged.len(), 1, "{:?}", report.changes);
        let t = merged[0].add_track.as_ref().unwrap();
        assert_eq!((t.id.as_str(), t.net.as_str(), t.pts.clone()), ("t1", "N1", vec![a, b]));
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

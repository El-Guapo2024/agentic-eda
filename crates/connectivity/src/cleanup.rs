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
//! Adaptations forced by this model's shape (documented once here, not
//! repeated at each call site below):
//! - KiCad's `PCB_TRACK` is always a single two-point segment (or arc);
//!   this model's `Track` is a polyline of 2+ points
//!   (`crates/model/src/ir.rs`'s `Track::pts`, and `crates/kicad`'s
//!   im/exporter already treats a multi-point `Track` as N-1 consecutive
//!   `(segment ...)`s -- see that crate's own docs). Duplicate/zero-length
//!   detection below operates at whole-`Track` granularity; the merge
//!   pass only ever considers straight 2-point tracks as candidates (the
//!   same restriction `crates/cli/src/tune_api.rs`'s length tuner already
//!   applies to its own single-track scope), so a `Track` that is already
//!   a merged multi-point polyline is left alone.
//! - "Track in pad" and "redundant via on a THT pad" use
//!   `PlacedPad::signed_distance` (containment of both endpoints) rather
//!   than KiCad's exact `SHAPE_POLY_SET` boolean-subtract-is-empty test --
//!   equivalent for the rect/round/round-rect pad shapes this model has,
//!   approximate only for a custom pad outline (which this model doesn't
//!   support at all yet).
//! - Collinearity is an exact integer cross-product test over this
//!   model's already-integer µm coordinates, not KiCad's
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

use std::collections::BTreeSet;

use eda_model::ir::{Design, Point, RoutingSection, Track, Via, Zone};
use eda_model::footprint::PlacedPad;
use eda_model::{ConstraintModel, PadKind};

use crate::algo::build_graph;
use crate::dangling::{dangling_tracks_and_vias, DanglingKind};
use crate::geom::point_in_polygon;
use crate::items::ItemRef;

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
}

impl CleanupReport {
    pub fn remove_track_ids(&self) -> Vec<String> {
        let mut out: BTreeSet<String> = BTreeSet::new();
        for c in &self.changes {
            out.extend(c.remove_track_ids.iter().cloned());
        }
        out.into_iter().collect()
    }
    pub fn remove_via_ids(&self) -> Vec<String> {
        let mut out: BTreeSet<String> = BTreeSet::new();
        for c in &self.changes {
            out.extend(c.remove_via_ids.iter().cloned());
        }
        out.into_iter().collect()
    }
    pub fn add_tracks(&self) -> Vec<Track> {
        self.changes.iter().filter_map(|c| c.add_track.clone()).collect()
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
            let layer = if lp.kind == PadKind::ThroughHole {
                None
            } else if fp.side == eda_model::ir::Side::Top {
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

fn is_zero_length(t: &Track) -> bool {
    t.pts.len() == 2 && t.pts[0] == t.pts[1]
}

/// Same endpoints, either direction -- a track has no inherent "start
/// means something" the way a signed path might.
fn same_endpoints(a: &Track, b: &Track) -> bool {
    a.pts == b.pts || (a.pts.len() == b.pts.len() && a.pts.iter().eq(b.pts.iter().rev()))
}

/// `TRACKS_CLEANER::cleanup`'s via/null-segment half: redundant vias
/// (same position + layer span, or sharing a position with a through-hole
/// pad spanning every copper layer) and zero-length tracks.
fn remove_redundant(tracks: &mut Vec<Track>, vias: &mut Vec<Via>, full_stack_pad_centers: &[Point], opts: &CleanupOptions, changes: &mut Vec<CleanupChange>) {
    if opts.delete_redundant_vias {
        let mut seen: Vec<(Point, String, String)> = Vec::new();
        let mut kept = Vec::with_capacity(vias.len());
        for v in std::mem::take(vias) {
            let key = (v.at, v.from_layer.clone(), v.to_layer.clone());
            if seen.contains(&key) || full_stack_pad_centers.contains(&v.at) {
                changes.push(CleanupChange { kind: CleanupKind::RedundantVia, net: v.net.clone(), remove_track_ids: vec![], remove_via_ids: vec![v.id.clone()], add_track: None });
            } else {
                seen.push(key);
                kept.push(v);
            }
        }
        *vias = kept;
    }

    let remove_null = opts.merge_segments || opts.delete_shorting;
    if remove_null {
        let mut kept = Vec::with_capacity(tracks.len());
        for t in std::mem::take(tracks) {
            if is_zero_length(&t) {
                changes.push(CleanupChange { kind: CleanupKind::ZeroLengthTrack, net: t.net.clone(), remove_track_ids: vec![t.id.clone()], remove_via_ids: vec![], add_track: None });
            } else {
                kept.push(t);
            }
        }
        *tracks = kept;
    }

    remove_duplicates(tracks, changes);
}

/// `TRACKS_CLEANER::cleanup`'s `aDeleteDuplicateSegments` pass -- run
/// unconditionally regardless of every checkbox (see `CleanupBoard`'s own
/// doc: it runs `cleanup(..., true, ...)` either as part of the merge-
/// gated first call, or as its own explicit always-on second call when
/// merging is off).
fn remove_duplicates(tracks: &mut Vec<Track>, changes: &mut Vec<CleanupChange>) {
    let mut kept: Vec<Track> = Vec::with_capacity(tracks.len());
    'outer: for t in std::mem::take(tracks) {
        for k in &kept {
            if k.layer == t.layer && k.width == t.width && same_endpoints(k, &t) {
                changes.push(CleanupChange { kind: CleanupKind::DuplicateTrack, net: t.net.clone(), remove_track_ids: vec![t.id.clone()], remove_via_ids: vec![], add_track: None });
                continue 'outer;
            }
        }
        kept.push(t);
    }
    *tracks = kept;
}

/// `TRACKS_CLEANER::deleteTracksInPads`: a straight 2-point track whose
/// both endpoints sit inside the same pad's copper (see module docs for
/// the containment-vs-boolean-subtract adaptation).
fn remove_tracks_in_pads(tracks: &mut Vec<Track>, pads: &[PadGeom], changes: &mut Vec<CleanupChange>) {
    let mut kept = Vec::with_capacity(tracks.len());
    'outer: for t in std::mem::take(tracks) {
        if t.pts.len() == 2 {
            for p in pads {
                if !pad_covers_layer(p, &t.layer) {
                    continue;
                }
                if p.pad.signed_distance(t.pts[0]) <= 0.0 && p.pad.signed_distance(t.pts[1]) <= 0.0 {
                    changes.push(CleanupChange { kind: CleanupKind::TrackInPad, net: t.net.clone(), remove_track_ids: vec![t.id.clone()], remove_via_ids: vec![], add_track: None });
                    continue 'outer;
                }
            }
        }
        kept.push(t);
    }
    *tracks = kept;
}

/// `TRACKS_CLEANER::removeShortingTrackSegments`: any track segment or
/// via geometrically touching a pad/track/via/zone of a different,
/// non-empty net. Needs the real connectivity graph (`build_graph`),
/// since "touching" is a geometry question this function has no cheaper
/// shortcut for.
fn remove_shorting(tracks: &mut Vec<Track>, vias: &mut Vec<Via>, base: &Design, model: &ConstraintModel, changes: &mut Vec<CleanupChange>) {
    let wd = working_design(base, tracks, vias);
    let graph = build_graph(&wd, model);
    let mut bad_tracks = BTreeSet::new();
    let mut bad_vias = BTreeSet::new();

    for it in &graph.items {
        if it.net.is_empty() {
            continue;
        }
        for &nb in &it.connected {
            let other = &graph.items[nb];
            if other.net.is_empty() || other.net == it.net {
                continue;
            }
            match &it.item {
                ItemRef::TrackSeg { track_id, .. } => {
                    bad_tracks.insert(track_id.clone());
                }
                ItemRef::Via { via_id } => {
                    bad_vias.insert(via_id.clone());
                }
                _ => {}
            }
            break;
        }
    }

    for id in &bad_tracks {
        if let Some(t) = tracks.iter().find(|t| &t.id == id) {
            changes.push(CleanupChange { kind: CleanupKind::ShortingTrack, net: t.net.clone(), remove_track_ids: vec![id.clone()], remove_via_ids: vec![], add_track: None });
        }
    }
    for id in &bad_vias {
        if let Some(v) = vias.iter().find(|v| &v.id == id) {
            changes.push(CleanupChange { kind: CleanupKind::ShortingVia, net: v.net.clone(), remove_track_ids: vec![], remove_via_ids: vec![id.clone()], add_track: None });
        }
    }
    tracks.retain(|t| !bad_tracks.contains(&t.id));
    vias.retain(|v| !bad_vias.contains(&v.id));
}

/// `TRACKS_CLEANER::deleteDanglingTracks`: iterate `dangling_tracks_and_
/// vias` (already the exact port of `TestTrackEndpointDangling` DRC uses)
/// to a fixed point, since removing one dangling stub can dangle whatever
/// it was attached to.
fn remove_dangling(tracks: &mut Vec<Track>, vias: &mut Vec<Via>, base: &Design, model: &ConstraintModel, want_tracks: bool, want_vias: bool, changes: &mut Vec<CleanupChange>) -> bool {
    let mut deleted_any = false;
    let max_iters = tracks.len() + vias.len() + 2;
    for _ in 0..max_iters {
        let wd = working_design(base, tracks, vias);
        let graph = build_graph(&wd, model);
        let dangling = dangling_tracks_and_vias(&graph);

        let mut track_ids = BTreeSet::new();
        let mut via_ids = BTreeSet::new();
        for d in &dangling {
            match d.kind {
                DanglingKind::Track if want_tracks => {
                    track_ids.insert(d.id.split('#').next().unwrap_or(&d.id).to_string());
                }
                DanglingKind::Via if want_vias => {
                    via_ids.insert(d.id.clone());
                }
                _ => {}
            }
        }
        if track_ids.is_empty() && via_ids.is_empty() {
            break;
        }
        for id in &track_ids {
            if let Some(t) = tracks.iter().find(|t| &t.id == id) {
                changes.push(CleanupChange { kind: CleanupKind::DanglingTrack, net: t.net.clone(), remove_track_ids: vec![id.clone()], remove_via_ids: vec![], add_track: None });
            }
        }
        for id in &via_ids {
            if let Some(v) = vias.iter().find(|v| &v.id == id) {
                changes.push(CleanupChange { kind: CleanupKind::DanglingVia, net: v.net.clone(), remove_track_ids: vec![], remove_via_ids: vec![id.clone()], add_track: None });
            }
        }
        tracks.retain(|t| !track_ids.contains(&t.id));
        vias.retain(|v| !via_ids.contains(&v.id));
        deleted_any = true;
    }
    deleted_any
}

fn collinear(a: Point, b: Point, c: Point) -> bool {
    let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
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
    zones.iter().any(|z| z.layer == layer && z.outline.len() >= 3 && point_in_polygon(p, &z.outline))
}

/// `TRACKS_CLEANER::cleanup`'s merge pass, run to a fixed point (matches
/// source's own `do { ... } while (mergeSegments(...))`).
fn merge_collinear(tracks: &mut Vec<Track>, vias: &[Via], pads: &[PadGeom], zones: &[Zone], changes: &mut Vec<CleanupChange>) -> bool {
    let mut changed_any = false;
    loop {
        let mut found: Option<(usize, usize, Point, Point)> = None;
        'outer: for i in 0..tracks.len() {
            for j in (i + 1)..tracks.len() {
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
                if d1.0 * d2.0 + d1.1 * d2.1 <= 0 {
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
        let merged = Track { id: String::new(), net: a.net.clone(), pins: Vec::new(), layer: a.layer.clone(), width: a.width, pts: vec![far_i, far_j], arc_mid_offset: None };
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
pub fn compute_cleanup(design: &Design, model: &ConstraintModel, opts: CleanupOptions) -> CleanupReport {
    let mut tracks: Vec<Track> = design.routing.as_ref().map(|r| r.tracks.clone()).unwrap_or_default();
    let mut vias: Vec<Via> = design.routing.as_ref().map(|r| r.vias.clone()).unwrap_or_default();
    let zones: Vec<Zone> = design.routing.as_ref().map(|r| r.zones.clone()).unwrap_or_default();
    let pads = pad_geoms(design, model);
    let full_stack_centers: Vec<Point> = pads.iter().filter(|p| p.layer.is_none()).map(|p| p.pad.center).collect();

    let mut changes = Vec::new();

    remove_redundant(&mut tracks, &mut vias, &full_stack_centers, &opts, &mut changes);

    if opts.merge_segments {
        merge_collinear(&mut tracks, &vias, &pads, &zones, &mut changes);
    }

    if opts.delete_shorting {
        remove_shorting(&mut tracks, &mut vias, design, model, &mut changes);
    }

    if opts.delete_tracks_in_pads {
        remove_tracks_in_pads(&mut tracks, &pads, &mut changes);
    }

    let deleted_any = if opts.delete_dangling_tracks || opts.delete_dangling_vias {
        remove_dangling(&mut tracks, &mut vias, design, model, opts.delete_dangling_tracks, opts.delete_dangling_vias, &mut changes)
    } else {
        false
    };

    if deleted_any && opts.merge_segments {
        merge_collinear(&mut tracks, &vias, &pads, &zones, &mut changes);
    }

    CleanupReport { changes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::{pad_center, two_pad_model};
    use eda_model::ir::{FootprintInstance, Side};

    #[test]
    fn a_zero_length_track_is_removed_when_merge_is_on() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, a], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, CleanupOptions { merge_segments: true, ..Default::default() });
        assert_eq!(report.remove_track_ids(), vec!["t1".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::ZeroLengthTrack));
    }

    #[test]
    fn nothing_changes_with_every_option_off() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, a], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, CleanupOptions::default());
        assert!(report.changes.is_empty(), "{:?}", report.changes.iter().map(|c| c.kind).collect::<Vec<_>>());
    }

    #[test]
    fn duplicate_tracks_are_always_removed_regardless_of_other_options() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, b], arc_mid_offset: None });
        rt.tracks.push(Track { id: "t2".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![b, a], arc_mid_offset: None }); // reverse order, still a dup
        let report = compute_cleanup(&design, &model, CleanupOptions::default());
        assert_eq!(report.remove_track_ids(), vec!["t2".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::DuplicateTrack));
    }

    #[test]
    fn redundant_vias_at_the_same_spot_collapse_to_one() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: a, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.vias.push(Via { id: "v2".into(), net: "N1".into(), at: a, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let report = compute_cleanup(&design, &model, CleanupOptions { delete_redundant_vias: true, ..Default::default() });
        assert_eq!(report.remove_via_ids(), vec!["v2".to_string()]);
    }

    #[test]
    fn a_via_on_a_through_hole_pad_is_redundant() {
        let (mut design, mut model) = two_pad_model();
        model.footprints.push(eda_model::Footprint {
            name: "THPAD".into(),
            pads: vec![eda_model::Pad { number: "1".into(), at: (0, 0), size: (1000, 1000), shape: eda_model::PadShape::Circle, kind: PadKind::ThroughHole, drill: Some(500), drill_slot: None, rot: 0, roundrect_ratio: None }],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
        });
        model.parts[0].footprint = Some("THPAD".into());
        model.parts[0].package = Some("THPAD".into());
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().vias.push(Via { id: "v1".into(), net: "N1".into(), at: a, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let report = compute_cleanup(&design, &model, CleanupOptions { delete_redundant_vias: true, ..Default::default() });
        assert_eq!(report.remove_via_ids(), vec!["v1".to_string()]);
    }

    #[test]
    fn a_track_fully_inside_a_pad_is_removable() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        // A tiny stub that never leaves R1's own pad.
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 50, pts: vec![a, Point { x: a.x + 5, y: a.y }], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, CleanupOptions { delete_tracks_in_pads: true, ..Default::default() });
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
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, b], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, CleanupOptions { delete_shorting: true, ..Default::default() });
        assert_eq!(report.remove_track_ids(), vec!["t1".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::ShortingTrack));
    }

    #[test]
    fn a_dangling_stub_is_removed_when_asked() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let free_end = Point { x: a.x, y: a.y + 10_000 };
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, free_end], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, CleanupOptions { delete_dangling_tracks: true, ..Default::default() });
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
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, mid], arc_mid_offset: None });
        rt.tracks.push(Track { id: "t2".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![mid, b], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, CleanupOptions { merge_segments: true, ..Default::default() });
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
        let off = Point { x: mid.x, y: mid.y + 5_000 };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, mid], arc_mid_offset: None });
        rt.tracks.push(Track { id: "t2".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![mid, b], arc_mid_offset: None });
        // A third leg stubbing off the same joint -- a real node, must
        // not be silently absorbed by the merge.
        rt.tracks.push(Track { id: "t3".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![mid, off], arc_mid_offset: None });
        let report = compute_cleanup(&design, &model, CleanupOptions { merge_segments: true, ..Default::default() });
        assert!(report.changes.iter().all(|c| !matches!(c.kind, CleanupKind::MergedTracks)), "{:?}", report.changes.iter().map(|c| c.kind).collect::<Vec<_>>());
    }

    #[test]
    fn a_via_drops_a_dangling_via_connected_to_only_one_layer() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x, y: a.y + 3_000 };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, via_at], arc_mid_offset: None });
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let report = compute_cleanup(&design, &model, CleanupOptions { delete_dangling_vias: true, ..Default::default() });
        assert_eq!(report.remove_via_ids(), vec!["v1".to_string()]);
        assert!(matches!(report.changes[0].kind, CleanupKind::DanglingVia));
    }

    #[test]
    fn no_placement_section_does_not_panic() {
        let (mut design, model) = two_pad_model();
        design.placement = None;
        let report = compute_cleanup(&design, &model, CleanupOptions { delete_tracks_in_pads: true, delete_shorting: true, ..Default::default() });
        assert!(report.changes.is_empty());
    }

    #[test]
    fn unused_import_guard() {
        // Keep FootprintInstance/Side imports honest if a future edit
        // trims the fixtures above; cheap sentinel, not a real assertion.
        let _ = FootprintInstance { id: "x".into(), at: Point { x: 0, y: 0 }, rot: 0, side: Side::Top, label: Default::default() };
    }
}

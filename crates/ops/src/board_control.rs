//! Board control: the board-wide verbs of KiCad's `BOARD_EDITOR_CONTROL`
//! (`pcbnew/tools/board_editor_control.cpp`) that edit `design.json`: merging
//! zones, a zone's fill priority, the drill/place file origin and Repair
//! Board. Each is a pure function over the design; [`crate::Board::apply`]
//! calls them for `Cmd::MergeZones`, `Cmd::SetZonePriority`,
//! `Cmd::SetAuxOrigin` and `Cmd::RepairBoard`, so each lands as one undo step.
//!
//! What is ported, and where this differs from the source:
//!  * `ZoneMerge` / `mergeZones` / `BOARD::TestZoneIntersection`: the same
//!    filters (same net, same rule-area-ness, same layer), the same
//!    "intersects an already chosen zone" test (a touching edge or a corner
//!    inside), the same refusal when the union is more than one outline
//!    ("insufficient overlap"), the highest priority wins. A zone here is one
//!    ring on one layer, so a union with a hole is stored fractured (a slit
//!    ring), as `Cmd::ZoneCutout` stores one.
//!  * `ZonePriority{MoveToTop,Raise,Lower,MoveToBottom}`: the overlap test
//!    (`getOverlappingZones`), the priority map and the cascade search
//!    (`findCascadeZones`) as written; a move that would change nothing is
//!    an error here ("nothing to do"), where KiCad quietly does nothing, so an
//!    empty undo step is never pushed.
//!  * `RepairBoard`: duplicate item ids get new ones (`RepairDuplicateItemUuids`)
//!    and a net an item uses but the netlist lacks is added back
//!    ("Orphaned net re-parented"). The report lines are KiCad's.

use eda_model::units::Nm;
use eda_clipper2::Point64;
use eda_model::ir::{Design, Point, Zone};
use eda_model::{CheckResult, Net};
use eda_shape_poly_set::ShapePolySet;
use std::collections::{BTreeMap, BTreeSet};

fn fail(check: &str, location: &str, hint: &str) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, location, hint)]
}

// ------------------------------------------------------------------ geometry

fn orient(a: Point, b: Point, c: Point) -> i128 {
    (b.x.get() as i128 - a.x.get() as i128) * (c.y.get() as i128 - a.y.get() as i128) - (b.y.get() as i128 - a.y.get() as i128) * (c.x.get() as i128 - a.x.get() as i128)
}

fn on_segment(a: Point, b: Point, p: Point) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// `SEG::Collide( aSeg, 0 )`: the segments share a point (a touch counts).
fn segments_touch(p1: Point, p2: Point, q1: Point, q2: Point) -> bool {
    let (o1, o2, o3, o4) = (orient(p1, p2, q1), orient(p1, p2, q2), orient(q1, q2, p1), orient(q1, q2, p2));
    if ((o1 > 0 && o2 < 0) || (o1 < 0 && o2 > 0)) && ((o3 > 0 && o4 < 0) || (o3 < 0 && o4 > 0)) {
        return true;
    }
    (o1 == 0 && on_segment(p1, p2, q1)) || (o2 == 0 && on_segment(p1, p2, q2)) || (o3 == 0 && on_segment(q1, q2, p1)) || (o4 == 0 && on_segment(q1, q2, p2))
}

/// `SHAPE_LINE_CHAIN::PointInside`: even-odd crossing, a point on an edge counts as inside.
fn ring_contains(ring: &[Point], p: Point) -> bool {
    let n = ring.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if orient(a, b, p) == 0 && on_segment(a, b, p) {
            return true;
        }
        if (a.y > p.y) != (b.y > p.y) {
            // The crossing's x against p.x, without division: sign of (b.x-a.x)*(p.y-a.y) - (p.x-a.x)*(b.y-a.y), flipped by the edge's direction.
            let side = orient(a, b, p);
            if (b.y > a.y && side > 0) || (b.y < a.y && side < 0) {
                inside = !inside;
            }
        }
    }
    inside
}

fn bbox(ring: &[Point]) -> (Nm, Nm, Nm, Nm) {
    ring.iter().fold((Nm::MAX, Nm::MAX, Nm::MIN, Nm::MIN), |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)))
}

fn bboxes_meet(a: &[Point], b: &[Point]) -> bool {
    let (a, b) = (bbox(a), bbox(b));
    a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
}

fn edges_touch(a: &[Point], b: &[Point]) -> bool {
    (0..a.len()).any(|i| {
        let (p1, p2) = (a[i], a[(i + 1) % a.len()]);
        (0..b.len()).any(|j| segments_touch(p1, p2, b[j], b[(j + 1) % b.len()]))
    })
}

/// `BOARD::TestZoneIntersection`: a layer in common, bounding boxes meet, and either an edge of one touches an edge of the other
/// (`IterateSegmentsWithHoles`: the holes' edges too) or a corner of one, holes' corners included, is in the other (`SHAPE_POLY_SET::Contains`:
/// inside its outline and not in one of its holes). A zone wholly inside another's hole does not intersect it.
pub fn zones_intersect(a: &Zone, b: &Zone) -> bool {
    let rings = |z: &Zone| -> Vec<Vec<Point>> { std::iter::once(z.outline.clone()).chain(z.holes.iter().filter(|h| h.len() >= 3).cloned()).collect() };
    let contains = |z: &Zone, p: Point| ring_contains(&z.outline, p) && !z.holes.iter().filter(|h| h.len() >= 3).any(|h| ring_contains(h, p) && !on_ring(h, p));
    let (ra, rb) = (rings(a), rings(b));
    a.layer_set().iter().any(|l| b.is_on_layer(l))
        && a.outline.len() >= 3
        && b.outline.len() >= 3
        && bboxes_meet(&a.outline, &b.outline)
        && (ra.iter().any(|x| rb.iter().any(|y| edges_touch(x, y))) || rb.iter().flatten().any(|p| contains(a, *p)) || ra.iter().flatten().any(|p| contains(b, *p)))
}

/// Whether `p` is on an edge of `ring`.
fn on_ring(ring: &[Point], p: Point) -> bool {
    let n = ring.len();
    (0..n).any(|i| orient(ring[i], ring[(i + 1) % n], p) == 0 && on_segment(ring[i], ring[(i + 1) % n], p))
}

// --------------------------------------------------------------------- merge

/// `ZoneMerge`: the zones among `ids` (in that order) that can join the first one, merged into it. Returns the id of
/// the zone that carries the result (the first zone, which keeps its settings, with the highest priority of them all).
/// A selection of fewer than two zones, or a union that is not one outline, is refused with KiCad's message.
pub fn merge_zones(design: &mut Design, ids: &[String]) -> Result<String, Vec<CheckResult>> {
    let zones: &mut Vec<Zone> = &mut design.routing.as_mut().ok_or_else(|| fail("ops_unknown_zone", "zones", "the board has no zones"))?.zones;
    // The selection's zones, in order; anything that is not a zone is ignored, as in the source.
    let chosen: Vec<usize> = ids.iter().filter_map(|id| zones.iter().position(|z| &z.id == id)).collect();
    if chosen.len() < 2 {
        return Err(fail("ops_zone_merge", "zones", "select two or more zones to merge"));
    }
    let first = chosen[0];
    let mut to_merge: Vec<usize> = Vec::new();
    // What the source `wxLogMessage`s for each zone it skips, in order, once each.
    let mut logged: Vec<&str> = Vec::new();
    let mut log = |m: &'static str| {
        if !logged.contains(&m) {
            logged.push(m);
        }
    };
    for &c in &chosen {
        let zone = &zones[c];
        if zone.net != zones[first].net {
            log("Some zone netcodes did not match and were not merged.");
            continue;
        }
        if zone.is_rule_area != zones[first].is_rule_area {
            log("Some zones were rule areas and were not merged.");
            continue;
        }
        if zone.layer_set() != zones[first].layer_set() {
            log("Some zone layer sets did not match and were not merged.");
            continue;
        }
        let meets = c == first || to_merge.iter().any(|&m| zones_intersect(zone, &zones[m]));
        if !meets {
            log("Some zones did not intersect and were not merged.");
            continue;
        }
        if !to_merge.contains(&c) {
            to_merge.push(c);
        }
    }
    // Only the first zone left: KiCad still runs `mergeZones` on it (a `Simplify` of its outline, a "Merge Zones" commit) and shows its log lines; here
    // nothing changes and the log lines are the refusal (round 4 C-7).
    if to_merge.len() < 2 {
        let message = if logged.is_empty() { "select two or more zones to merge".to_string() } else { logged.join("\n") };
        return Err(fail("ops_zone_merge", &zones[first].id, &message));
    }

    // `ZONE::Outline()`: the outline and its holes.
    let chain = |r: &[Point]| r.iter().map(|p| Point64::new(p.x.get(), p.y.get())).collect::<Vec<_>>();
    let poly = |z: &Zone| {
        let mut set = ShapePolySet::from_outline(chain(&z.outline));
        for hole in z.holes.iter().filter(|h| h.len() >= 3) {
            set.add_hole(chain(hole), Some(0));
        }
        set
    };
    let mut union = poly(&zones[to_merge[0]]);
    for &m in &to_merge[1..] {
        union.boolean_add(&poly(&zones[m]));
    }
    union.simplify();
    // One polygon, possibly with holes: two means the overlap was a point, or inside a hole.
    if union.outline_count() != 1 {
        return Err(fail("ops_zone_merge", &zones[first].id, "Zones have insufficient overlap for merging."));
    }
    // Not fractured: the merged zone keeps its holes (`aOriginZone->Outline()->Simplify()` and no more).
    let ring: Vec<Point> = union.outline(0).iter().map(|p| Point { x: Nm::new(p.x), y: Nm::new(p.y) }).collect();
    let holes: Vec<Vec<Point>> = union.polys[0][1..].iter().filter(|h| h.len() >= 3).map(|h| h.iter().map(|p| Point { x: Nm::new(p.x), y: Nm::new(p.y) }).collect()).collect();
    if ring.len() < 3 {
        return Err(fail("ops_zone_merge", &zones[first].id, "Zones have insufficient overlap for merging."));
    }

    // The most aggressive fill ordering of the lot survives.
    let priority = to_merge.iter().map(|&m| zones[m].priority).max().unwrap_or(0);
    let keep = zones[first].id.clone();
    zones[first].outline = ring;
    zones[first].holes = holes;
    zones[first].priority = priority;
    let drop: BTreeSet<usize> = to_merge.iter().copied().filter(|&m| m != first).collect();
    let mut at = 0;
    zones.retain(|_| {
        let keep_it = !drop.contains(&at);
        at += 1;
        keep_it
    });
    Ok(keep)
}

// ------------------------------------------------------------------ priority

/// Which way `Cmd::SetZonePriority` moves a zone among the zones it overlaps
/// (`pcbnew.EditorControl.zonePriority{MoveToTop,Raise,Lower,MoveToBottom}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZonePriorityMove {
    Top,
    Raise,
    Lower,
    Bottom,
}

/// `getOverlappingZones`: the other copper zones that share a layer with this one, whose boxes meet, and whose areas collide
/// (`aZone->Outline()->Collide( candidate->Outline() )`, over the triangulation, so nothing in a hole) or of which one holds the other's first corner
/// (`Outline()->Contains( CVertex( 0 ) )`, a point in a hole being outside). That is [`zones_intersect`]'s test: a zone inside another's hole does not
/// overlap it (r7 C7-1).
pub fn overlapping_zones(zones: &[Zone], zi: usize) -> Vec<usize> {
    let zone = &zones[zi];
    (0..zones.len()).filter(|&ci| ci != zi && !zones[ci].is_rule_area && !zones[ci].teardrop && zones_intersect(zone, &zones[ci])).collect()
}

/// `buildPriorityMap`: every copper zone but `exclude`, by priority.
fn priority_map(zones: &[Zone], exclude: usize) -> BTreeMap<u32, Vec<usize>> {
    let mut by: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, z) in zones.iter().enumerate() {
        if i != exclude && !z.is_rule_area && !z.teardrop {
            by.entry(z.priority).or_default().push(i);
        }
    }
    by
}

/// `findCascadeZones`: from `from`, walk up (or down) collecting the zones at each consecutive occupied priority until a
/// free one. `viable` is false when the walk runs into 0 (or `u32::MAX`) still occupied: the shift would overflow.
fn cascade(by: &BTreeMap<u32, Vec<usize>>, from: u32, up: bool) -> (Vec<usize>, bool) {
    let (mut found, mut p) = (Vec::new(), from);
    while let Some(zs) = by.get(&p) {
        found.extend(zs);
        if up {
            if p == u32::MAX {
                return (found, false);
            }
            p += 1;
        } else {
            if p == 0 {
                return (found, false);
            }
            p -= 1;
        }
    }
    (found, true)
}

/// The four moves. The zone must be a copper zone (not a rule area or a teardrop).
pub fn set_zone_priority(design: &mut Design, id: &str, to: ZonePriorityMove) -> Result<(), Vec<CheckResult>> {
    let zones: &mut Vec<Zone> = &mut design.routing.as_mut().ok_or_else(|| fail("ops_unknown_zone", id, "the board has no zones"))?.zones;
    let zi = zones.iter().position(|z| z.id == id).ok_or_else(|| fail("ops_unknown_zone", id, "no zone with this id"))?;
    if zones[zi].is_rule_area || zones[zi].teardrop {
        return Err(fail("ops_zone_priority", id, "only a copper zone has a fill priority"));
    }
    let nothing = |what: &str| fail("ops_zone_priority", id, what);
    let overlapping = overlapping_zones(zones, zi);
    let mine = zones[zi].priority;
    match to {
        ZonePriorityMove::Top => {
            let max = overlapping.iter().map(|&o| zones[o].priority).fold(mine, u32::max);
            if mine >= max {
                return Err(nothing("this zone is already above every zone it overlaps"));
            }
            let by = priority_map(zones, zi);
            // A: take the top slot and push the zones there down; B: take the one above it and push up. Fewer displaced wins.
            let (down, down_ok) = cascade(&by, max, false);
            let (up, up_ok) = if max < u32::MAX { cascade(&by, max + 1, true) } else { (Vec::new(), false) };
            if !down_ok && !up_ok {
                return Err(nothing("no free priority to move this zone to the top"));
            }
            if down_ok && (!up_ok || down.len() <= up.len()) {
                zones[zi].priority = max;
                down.iter().for_each(|&z| zones[z].priority -= 1);
            } else {
                zones[zi].priority = max + 1;
                up.iter().for_each(|&z| zones[z].priority += 1);
            }
        }
        ZonePriorityMove::Raise => {
            // The overlapping zone with the lowest priority still above ours.
            let target = overlapping.iter().copied().filter(|&o| zones[o].priority > mine).min_by_key(|&o| zones[o].priority).ok_or_else(|| nothing("this zone is already above every zone it overlaps"))?;
            let above = zones[target].priority;
            if above < u32::MAX {
                zones[zi].priority = above + 1;
            } else {
                zones[zi].priority = u32::MAX;
                zones[target].priority = mine;
            }
        }
        ZonePriorityMove::Lower => {
            let target = overlapping.iter().copied().filter(|&o| zones[o].priority < mine).max_by_key(|&o| zones[o].priority).ok_or_else(|| nothing("this zone is already below every zone it overlaps"))?;
            let below = zones[target].priority;
            if below > 0 {
                zones[zi].priority = below - 1;
            } else {
                zones[zi].priority = 0;
                zones[target].priority = mine;
            }
        }
        ZonePriorityMove::Bottom => {
            let min = overlapping.iter().map(|&o| zones[o].priority).fold(mine, u32::min);
            if mine <= min {
                return Err(nothing("this zone is already below every zone it overlaps"));
            }
            let by = priority_map(zones, zi);
            // A: take the bottom slot and push the zones there up; B: take the one below it and push down.
            let (up, up_ok) = cascade(&by, min, true);
            let (down, down_ok) = if min > 0 { cascade(&by, min - 1, false) } else { (Vec::new(), false) };
            if !up_ok && !down_ok {
                return Err(nothing("no free priority to move this zone to the bottom"));
            }
            if up_ok && (!down_ok || up.len() <= down.len()) {
                zones[zi].priority = min;
                up.iter().for_each(|&z| zones[z].priority += 1);
            } else {
                zones[zi].priority = min - 1;
                down.iter().for_each(|&z| zones[z].priority -= 1);
            }
        }
    }
    Ok(())
}

// -------------------------------------------------------------------- origin

/// `DoSetDrillOrigin`: the drill/place file origin; `None` resets it to (0, 0), `DrillOrigin`'s reset branch.
pub fn set_aux_origin(design: &mut Design, at: Option<Point>) -> Result<(), Vec<CheckResult>> {
    let at = at.filter(|p| p.x != Nm::ZERO || p.y != Nm::ZERO);
    let current = design.drawings.as_ref().and_then(|d| d.aux_origin);
    if current == at {
        return Err(fail("ops_aux_origin", "aux_origin", if at.is_some() { "the drill/place file origin is already there" } else { "the drill/place file origin is already at (0, 0)" }));
    }
    design.drawings.get_or_insert_with(Default::default).aux_origin = at;
    Ok(())
}

/// What the Plot and Generate Drill Files dialogs store back in the board (`DIALOG_PLOT::applyPlotSettings`: `if( !m_plotOpts.IsSameAs(
/// tempOptions ) ) { SetPlotSettings( tempOptions ); OnModify(); }`, on Plot; `DIALOG_GENDRILL::updateConfig`, on Generate, the Origin
/// as `SetUseAuxOrigin( m_origin->GetSelection() == 1 )`): the checklist as the layer selection
/// ([`eda_model::plot_params::PlotParams::set_dialog_layer_selection`]) and "Use drill/place file origin" as `useauxorigin`. `None`
/// leaves that part as it is (the drill dialog has no checklist). Nothing changed is refused, so no empty undo step is pushed (KiCad does
/// not mark the board modified either). Differs: the dialogs' other controls are not ported, so only these two are stored.
pub fn set_plot_settings(design: &mut Design, board_copper: &[String], layers: Option<&[String]>, use_aux_origin: Option<bool>) -> Result<(), Vec<CheckResult>> {
    let before = eda_model::plot_params::PlotParams::of_design(design);
    let mut plot = before.clone();
    if let Some(layers) = layers {
        plot.set_dialog_layer_selection(layers, board_copper);
    }
    if let Some(aux) = use_aux_origin {
        plot.set_item("useauxorigin", if aux { "yes" } else { "no" });
    }
    if plot == before {
        return Err(fail("ops_plot_settings", "plot_params", "the plot settings are already these"));
    }
    design.drawings.get_or_insert_with(Default::default).plot_params = Some(plot);
    Ok(())
}

/// Export IPC-2581's BOM columns kept in the board for the next opening of the dialog (`DIALOG_EXPORT_2581::TransferDataFromWindow`:
/// `prj.m_IP2581Bom.id = GetOEM(); .. bomRev = m_textBomRev->GetValue();`, on Export). Like the plot settings this is no undo step (the
/// project file has none), see `crates/cli/src/board.rs`'s `outside_history`. All empty clears it; nothing changed is refused.
pub fn set_ipc2581_bom(design: &mut Design, bom: &eda_model::plot_params::Ipc2581Bom) -> Result<(), Vec<CheckResult>> {
    let bom = (*bom != eda_model::plot_params::Ipc2581Bom::default()).then(|| bom.clone());
    if design.drawings.as_ref().and_then(|d| d.ipc2581_bom.as_ref()) == bom.as_ref() {
        return Err(fail("ops_ipc2581_bom", "ipc2581_bom", "the IPC-2581 BOM columns are already these"));
    }
    design.drawings.get_or_insert_with(Default::default).ipc2581_bom = bom;
    Ok(())
}

/// `PCB_CONTROL::DoSetGridOrigin`: the point the editing grid is anchored at; `None` (or (0, 0)) is `GridResetOrigin`'s reset.
pub fn set_grid_origin(design: &mut Design, at: Option<Point>) -> Result<(), Vec<CheckResult>> {
    let at = at.filter(|p| p.x != Nm::ZERO || p.y != Nm::ZERO);
    let current = design.drawings.as_ref().and_then(|d| d.grid_origin);
    if current == at {
        return Err(fail("ops_grid_origin", "grid_origin", if at.is_some() { "the grid origin is already there" } else { "the grid origin is already at (0, 0)" }));
    }
    design.drawings.get_or_insert_with(Default::default).grid_origin = at;
    Ok(())
}

// -------------------------------------------------------------------- repair

/// What `RepairBoard` did, in KiCad's words.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepairReport {
    /// How many problems were repaired ("%d potential problems repaired.").
    pub repaired: usize,
    /// The detail lines ("%d duplicate IDs replaced.", "Orphaned net %s re-parented.").
    pub details: Vec<String>,
}

/// `BOARD_EDITOR_CONTROL::RepairBoard`, applied to `design`: replaces duplicate item ids (`RepairDuplicateItemUuids`,
/// the first of each id keeps it) and adds back, to the board's net list, every net an item uses that the list lacks.
/// `nets` is the netlist the board is judged against (the intent's, or the schematic-derived one that replaced it).
pub fn repair_board(design: &mut Design, nets: &[Net]) -> RepairReport {
    let mut report = RepairReport::default();

    // Duplicate ids, across every kind of item that carries one: the first of an id keeps it, the rest are blanked.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut replaced = 0usize;
    let mut duplicate = |id: &str| {
        let dup = !id.is_empty() && !seen.insert(id.to_string());
        replaced += dup as usize;
        dup
    };
    if let Some(rt) = design.routing.as_mut() {
        for t in rt.tracks.iter_mut().filter(|t| duplicate(&t.id)) {
            t.id.clear();
        }
        for v in rt.vias.iter_mut().filter(|v| duplicate(&v.id)) {
            v.id.clear();
        }
        for z in rt.zones.iter_mut().filter(|z| duplicate(&z.id)) {
            z.id.clear();
        }
    }
    if let Some(dr) = design.drawings.as_mut() {
        for s in dr.shapes.iter_mut().filter(|s| duplicate(s.id())) {
            s.set_id(String::new());
        }
        for t in dr.texts.iter_mut().filter(|t| duplicate(&t.id)) {
            t.id.clear();
        }
        for g in dr.groups.iter_mut().filter(|g| duplicate(&g.id)) {
            g.id.clear();
        }
        for d in dr.dimensions.iter_mut().filter(|d| duplicate(&d.id)) {
            d.id.clear();
        }
    }
    if replaced > 0 {
        // The blanked ones get new deterministic ids, exactly as an item added by hand does.
        if let Some(rt) = design.routing.as_mut() {
            rt.assign_missing_ids();
        }
        if let Some(dr) = design.drawings.as_mut() {
            dr.assign_missing_ids();
        }
        report.repaired += replaced;
        report.details.push(format!("{replaced} duplicate IDs replaced."));
    }

    // Nets an item names but the netlist does not have.
    let known: BTreeSet<&str> = nets.iter().map(|n| n.name.as_str()).collect();
    let mut orphans: BTreeSet<String> = BTreeSet::new();
    if let Some(rt) = design.routing.as_ref() {
        for net in rt.tracks.iter().map(|t| &t.net).chain(rt.vias.iter().map(|v| &v.net)).chain(rt.zones.iter().map(|z| &z.net)) {
            if !net.is_empty() && !known.contains(net.as_str()) {
                orphans.insert(net.clone());
            }
        }
    }
    if !orphans.is_empty() {
        let mut list = nets.to_vec();
        for net in &orphans {
            list.push(Net { name: net.clone(), pins: vec![] });
            report.details.push(format!("Orphaned net {net} re-parented."));
            report.repaired += 1;
        }
        design.nets = Some(list);
    }
    report
}

// ------------------------------------------------------------------ export footprints

/// What [`export_footprints_to_library`] did: the names written as new entries and the entries written over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportFootprintsReport {
    pub added: Vec<String>,
    pub replaced: Vec<String>,
}

/// One library pad with a board pad edit laid on: the geometry through the engine pad ([`eda_model::fp_edit::apply_pad_edit`], the rule the board uses), the
/// margins into the pad's kept file forms. A pad whose type changed takes its new kind's layers (an empty list: the writer's defaults for the kind) and stops
/// being an edge connector.
fn library_pad_with_edit(pad: &eda_model::ir::LibraryPad, e: &eda_model::fp_edit::PadEdit) -> eda_model::ir::LibraryPad {
    let mut engine = pad.to_engine_pad();
    eda_model::fp_edit::apply_pad_edit(&mut engine, e);
    let geometry = eda_model::ir::LibraryPad::from_engine_pad(&engine);
    let mut out = pad.clone();
    out.shape = geometry.shape;
    out.size = geometry.size;
    out.offset = geometry.offset;
    out.drill = geometry.drill;
    out.drill_slot = geometry.drill_slot;
    out.rot = geometry.rot;
    out.roundrect_ratio = geometry.roundrect_ratio;
    out.trapezoid_delta = geometry.trapezoid_delta;
    out.chamfer_ratio = geometry.chamfer_ratio;
    out.chamfer_corners = geometry.chamfer_corners;
    out.custom = geometry.custom;
    out.stack = geometry.stack;
    if geometry.kind != pad.kind {
        out.kind = geometry.kind;
        out.layers = Vec::new();
        out.kicad.connector = false;
    }
    if let Some(m) = e.solder_mask_margin {
        out.kicad.solder_mask_margin = Some(m);
    }
    if let Some(m) = e.solder_paste_margin {
        out.kicad.solder_paste_margin = Some(m);
    }
    if let Some(r) = e.solder_paste_margin_ratio {
        out.kicad.solder_paste_margin_ratio = Some(r);
    }
    out
}

/// `PCB_EDIT_FRAME::ExportFootprintsToLibrary` (File > Export > Footprints...): every footprint of the board is saved to the library as the board has it,
/// over an entry of the same name (`adapter->SaveFootprint( nickname, fpCopy, true )`, `true` being overwrite), in board order, so the last instance of a name
/// wins; a footprint with no library item name is skipped ("Handle old boards."). The library is the project footprint library (`design.footprint_library`):
/// the studio has the one, not a library table to choose from, so KiCad's "Export footprints to library:" question and its "Update board footprints to link to
/// exported footprints" option have nothing to do (the board's footprints already name it).
///
/// "As the board has it" is the entry the board resolves with that instance's own pad edits and 3D models laid on (GAPS 9, `DrawingsSection::footprint_edits`):
/// - an entry that is published (the board's footprints follow it) gets the edits laid on its pads, so what the editor keeps of the file stays;
/// - any other footprint -- an entry not published, which the board does not show, or no entry yet -- is written from the footprint the board shows
///   ([`eda_model::ConstraintModel::footprint_of`], the instance's edits included), as KiCad overwrites the library item with the board's copy: its pads,
///   art, courtyard and models replace an unpublished entry's (whose name, description, attributes and fields stay).
///
/// What the instance's edit has beyond its pads and models (its Reference and Value layouts, user fields and attributes) is not written: the library entry keeps
/// its own, where KiCad's copy takes the board's (with the reference reset to `REF**`, which an entry here always is).
pub fn export_footprints_to_library(design: &mut Design, model: &eda_model::ConstraintModel) -> Result<ExportFootprintsReport, Vec<CheckResult>> {
    // `GetBoard()->GetFirstFootprint() == nullptr`: "No footprints to export!"
    let named: Vec<&eda_model::Part> = model.parts.iter().filter(|p| p.footprint.as_deref().is_some_and(|n| !n.is_empty())).collect();
    if model.parts.is_empty() {
        return Err(fail("ops_export_footprints", "board", "No footprints to export!"));
    }
    // The last instance of each name, in board order.
    let mut last: BTreeMap<String, &eda_model::Part> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for part in named {
        let name = part.footprint.clone().unwrap_or_default();
        if !last.contains_key(&name) {
            order.push(name.clone());
        }
        last.insert(name, part);
    }
    let mut report = ExportFootprintsReport::default();
    for name in order {
        let part = last[&name];
        let edit = design.footprint_edit(&part.reference).cloned();
        let lib = design.footprint_library.get_or_insert_with(Default::default);
        match lib.by_name_mut(&name) {
            Some(entry) if entry.published => {
                if let Some(edit) = &edit {
                    let engine = entry.to_engine_footprint();
                    let mut pads = entry.pads.clone();
                    for e in &edit.pads {
                        if let Some(i) = eda_model::fp_edit::pad_index(&engine, &e.number, e.nth) {
                            pads[i] = library_pad_with_edit(&entry.pads[i], e);
                        }
                    }
                    entry.pads = pads;
                    if let Some(models) = &edit.models3d {
                        entry.models3d = models.clone();
                        entry.model = models.first().map(|m| m.path.clone());
                    }
                }
                report.replaced.push(name);
            }
            existing => {
                let Some(fp) = model.footprint_of(part) else { continue };
                let mut fresh = eda_model::ir::LibraryFootprint::from_engine_footprint(&fp);
                fresh.name = name.clone();
                if let Some(entry) = existing {
                    // the board's pads, art, courtyard and models over the entry's; its name, description, attributes and fields stay
                    entry.pads = fresh.pads;
                    entry.graphics = fresh.graphics;
                    entry.texts = fresh.texts;
                    entry.courtyard = fresh.courtyard;
                    entry.model = fresh.model;
                    entry.models3d = fresh.models3d;
                    entry.pad_edit = None;
                    entry.assign_missing_ids();
                    report.replaced.push(name);
                } else {
                    fresh.assign_missing_ids();
                    lib.footprints.push(fresh);
                    report.added.push(name);
                }
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use eda_model::units::Nm;
    use super::*;

    fn pt(x: Nm, y: Nm) -> Point {
        Point { x, y }
    }

    fn square(id: &str, layer: &str, net: &str, x: Nm, y: Nm, side: Nm, priority: u32) -> Zone {
        Zone { id: id.into(), net: net.into(), layer: layer.into(), outline: vec![pt(x, y), pt(x + side, y), pt(x + side, y + side), pt(x, y + side)], priority, ..Default::default() }
    }

    /// Built from JSON so a field the IR gains later (with its serde default) does not break these.
    fn design(zones: Vec<Zone>) -> Design {
        let mut d: Design = serde_json::from_value(serde_json::json!({ "schema": 1, "provenance": { "engine_version": "0", "intent_hash": "x", "seed": 0 }, "routing": { "tracks": [], "vias": [] } })).unwrap();
        d.routing.as_mut().unwrap().zones = zones;
        d
    }

    fn zones(d: &Design) -> &Vec<Zone> {
        &d.routing.as_ref().unwrap().zones
    }

    fn priorities(d: &Design) -> Vec<(String, u32)> {
        zones(d).iter().map(|z| (z.id.clone(), z.priority)).collect()
    }

    #[test]
    fn segments_that_touch_collide_and_ones_that_miss_do_not() {
        assert!(segments_touch(pt(Nm::ZERO, Nm::ZERO), pt(Nm::new(10), Nm::new(10)), pt(Nm::ZERO, Nm::new(10)), pt(Nm::new(10), Nm::ZERO)), "a cross");
        assert!(segments_touch(pt(Nm::ZERO, Nm::ZERO), pt(Nm::new(10), Nm::ZERO), pt(Nm::new(10), Nm::ZERO), pt(Nm::new(10), Nm::new(10))), "end to end");
        assert!(segments_touch(pt(Nm::ZERO, Nm::ZERO), pt(Nm::new(10), Nm::ZERO), pt(Nm::new(5), Nm::ZERO), pt(Nm::new(5), Nm::new(10))), "a T");
        assert!(segments_touch(pt(Nm::ZERO, Nm::ZERO), pt(Nm::new(10), Nm::ZERO), pt(Nm::new(5), Nm::ZERO), pt(Nm::new(15), Nm::ZERO)), "collinear and overlapping");
        assert!(!segments_touch(pt(Nm::ZERO, Nm::ZERO), pt(Nm::new(10), Nm::ZERO), pt(Nm::new(11), Nm::ZERO), pt(Nm::new(15), Nm::ZERO)), "collinear, apart");
        assert!(!segments_touch(pt(Nm::ZERO, Nm::ZERO), pt(Nm::new(10), Nm::ZERO), pt(Nm::ZERO, Nm::new(1)), pt(Nm::new(10), Nm::new(1))), "parallel");
    }

    #[test]
    fn a_point_is_inside_a_ring_the_way_pointinside_says() {
        let ring = [pt(Nm::ZERO, Nm::ZERO), pt(Nm::new(10), Nm::ZERO), pt(Nm::new(10), Nm::new(10)), pt(Nm::ZERO, Nm::new(10))];
        assert!(ring_contains(&ring, pt(Nm::new(5), Nm::new(5))));
        assert!(ring_contains(&ring, pt(Nm::new(10), Nm::new(5))), "on an edge");
        assert!(ring_contains(&ring, pt(Nm::ZERO, Nm::ZERO)), "on a corner");
        assert!(!ring_contains(&ring, pt(Nm::new(11), Nm::new(5))));
        assert!(!ring_contains(&ring, pt(Nm::new(5), Nm::new(-1))));
        // A concave ring: the notch is outside.
        let c = [pt(Nm::ZERO, Nm::ZERO), pt(Nm::new(10), Nm::ZERO), pt(Nm::new(10), Nm::new(10)), pt(Nm::new(6), Nm::new(10)), pt(Nm::new(6), Nm::new(4)), pt(Nm::new(4), Nm::new(4)), pt(Nm::new(4), Nm::new(10)), pt(Nm::ZERO, Nm::new(10))];
        assert!(!ring_contains(&c, pt(Nm::new(5), Nm::new(8))));
        assert!(ring_contains(&c, pt(Nm::new(2), Nm::new(8))));
    }

    #[test]
    fn two_zones_intersect_when_edges_touch_or_one_holds_the_other_and_only_on_one_layer() {
        let a = square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 0);
        assert!(zones_intersect(&a, &square("b", "F.Cu", "GND", Nm::new(5_000), Nm::new(5_000), Nm::new(10_000), 0)), "overlap");
        assert!(zones_intersect(&a, &square("b", "F.Cu", "GND", Nm::new(2_000), Nm::new(2_000), Nm::new(2_000), 0)), "inside: no edge crosses, a corner is inside");
        assert!(zones_intersect(&a, &square("b", "F.Cu", "GND", Nm::new(10_000), Nm::ZERO, Nm::new(5_000), 0)), "sharing an edge");
        assert!(!zones_intersect(&a, &square("b", "F.Cu", "GND", Nm::new(20_000), Nm::ZERO, Nm::new(5_000), 0)), "apart");
        assert!(!zones_intersect(&a, &square("b", "B.Cu", "GND", Nm::new(5_000), Nm::new(5_000), Nm::new(10_000), 0)), "another layer");
    }

    #[test]
    fn merging_overlapping_zones_keeps_the_first_and_its_settings_and_takes_the_highest_priority() {
        let mut d = design(vec![square("z1", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 1), square("z2", "F.Cu", "GND", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 3), square("z3", "F.Cu", "GND", Nm::new(100_000), Nm::ZERO, Nm::new(5_000), 9)]);
        let kept = merge_zones(&mut d, &["z1".into(), "z2".into(), "z3".into()]).unwrap();
        assert_eq!(kept, "z1");
        assert_eq!(priorities(&d), vec![("z1".to_string(), 3), ("z3".to_string(), 9)], "z2 is gone, z3 (apart) is untouched and was not merged");
        let merged = &zones(&d)[0];
        let (x0, y0, x1, y1) = bbox(&merged.outline);
        assert_eq!((x0, y0, x1, y1), (Nm::ZERO, Nm::ZERO, Nm::new(15_000), Nm::new(10_000)), "the union of the two squares");
        let area = {
            let mut s = ShapePolySet::from_outline(merged.outline.iter().map(|p| Point64::new(p.x.get(), p.y.get())).collect());
            s.simplify();
            s.area()
        };
        assert!((area - 150_000_000.0).abs() < 1.0, "{area}");
    }

    #[test]
    fn merging_skips_zones_of_another_net_layer_or_kind_and_chains_through_a_zone_that_touches_a_chosen_one() {
        let other_net = square("n", "F.Cu", "VCC", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 0);
        let mut keepout = square("k", "F.Cu", "GND", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 0);
        keepout.is_rule_area = true;
        let mut d = design(vec![
            square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 0),
            other_net,
            keepout,
            square("b", "B.Cu", "GND", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 0),
            // c touches only b2, which is chosen before it, so c chains in through b2.
            square("b2", "F.Cu", "GND", Nm::new(9_000), Nm::ZERO, Nm::new(6_000), 0),
            square("c", "F.Cu", "GND", Nm::new(14_000), Nm::ZERO, Nm::new(6_000), 0),
        ]);
        merge_zones(&mut d, &["a".into(), "n".into(), "k".into(), "b".into(), "b2".into(), "c".into()]).unwrap();
        let left: Vec<&str> = zones(&d).iter().map(|z| z.id.as_str()).collect();
        assert_eq!(left, ["a", "n", "k", "b"], "a absorbed b2, then c (which only touches b2)");
        assert_eq!(bbox(&zones(&d)[0].outline), (Nm::ZERO, Nm::ZERO, Nm::new(20_000), Nm::new(10_000)));
    }

    #[test]
    fn merging_zones_that_do_not_touch_or_touch_only_at_a_corner_is_refused() {
        let mut d = design(vec![square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 0), square("b", "F.Cu", "GND", Nm::new(50_000), Nm::ZERO, Nm::new(10_000), 0)]);
        let e = merge_zones(&mut d, &["a".into(), "b".into()]).unwrap_err();
        assert_eq!(e[0].check, "ops_zone_merge");
        assert_eq!(zones(&d).len(), 2, "nothing changed");
        // Corner to corner: the intersection test passes (they touch), but the union is two outlines.
        let mut d = design(vec![square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 0), square("b", "F.Cu", "GND", Nm::new(10_000), Nm::new(10_000), Nm::new(10_000), 0)]);
        let e = merge_zones(&mut d, &["a".into(), "b".into()]).unwrap_err();
        assert_eq!(e[0].hint.as_deref(), Some("Zones have insufficient overlap for merging."));
        assert_eq!(zones(&d).len(), 2);
        // One zone is not a selection to merge.
        assert!(merge_zones(&mut d, &["a".into()]).is_err());
    }

    /// Round 4 C-7: `TestZoneIntersection` walks the holes too (`IterateSegmentsWithHoles`, `Contains` without the holes): a zone wholly inside another's
    /// hole does not intersect it; and the refusal is in the words of the source's log lines.
    #[test]
    fn a_zone_inside_anothers_hole_does_not_intersect_it_and_the_refusal_is_kicads_log_line() {
        let mut ring = square("r", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(30_000), 0);
        ring.holes = vec![vec![pt(Nm::new(5_000), Nm::new(5_000)), pt(Nm::new(25_000), Nm::new(5_000)), pt(Nm::new(25_000), Nm::new(25_000)), pt(Nm::new(5_000), Nm::new(25_000))]];
        let inner = square("i", "F.Cu", "GND", Nm::new(10_000), Nm::new(10_000), Nm::new(5_000), 0);
        assert!(!zones_intersect(&ring, &inner), "inside the hole");
        assert!(zones_intersect(&ring, &square("e", "F.Cu", "GND", Nm::new(20_000), Nm::new(20_000), Nm::new(10_000), 0)), "across the hole's edge");
        let mut d = design(vec![ring, inner, square("v", "F.Cu", "VCC", Nm::ZERO, Nm::ZERO, Nm::new(5_000), 0)]);
        let e = merge_zones(&mut d, &["r".into(), "i".into(), "v".into()]).unwrap_err();
        assert_eq!(e[0].hint.as_deref(), Some("Some zones did not intersect and were not merged.\nSome zone netcodes did not match and were not merged."));
        assert_eq!(zones(&d).len(), 3);
    }

    fn ring_area(ring: &[Point]) -> f64 {
        let twice: i128 = (0..ring.len()).map(|i| ring[i].x.area(ring[(i + 1) % ring.len()].y) - ring[(i + 1) % ring.len()].x.area(ring[i].y)).sum();
        (twice as f64 / 2.0).abs()
    }

    #[test]
    fn a_merge_that_closes_a_hole_stays_one_zone_with_the_hole() {
        // A U (a notch open at the top) and a bar across the notch's mouth: the void under the bar is a hole.
        let u = Zone { id: "u".into(), net: "GND".into(), layer: "F.Cu".into(), outline: vec![pt(Nm::ZERO, Nm::ZERO), pt(Nm::new(30_000), Nm::ZERO), pt(Nm::new(30_000), Nm::new(30_000)), pt(Nm::new(20_000), Nm::new(30_000)), pt(Nm::new(20_000), Nm::new(10_000)), pt(Nm::new(10_000), Nm::new(10_000)), pt(Nm::new(10_000), Nm::new(30_000)), pt(Nm::ZERO, Nm::new(30_000))], ..Default::default() };
        let bar = Zone { id: "bar".into(), net: "GND".into(), layer: "F.Cu".into(), outline: vec![pt(Nm::new(8_000), Nm::new(25_000)), pt(Nm::new(22_000), Nm::new(25_000)), pt(Nm::new(22_000), Nm::new(35_000)), pt(Nm::new(8_000), Nm::new(35_000))], ..Default::default() };
        let mut d = design(vec![u, bar]);
        merge_zones(&mut d, &["u".into(), "bar".into()]).unwrap();
        assert_eq!(zones(&d).len(), 1);
        let z = &zones(&d)[0];
        // Not fractured (`mergeZones` simplifies the union and keeps its holes): the outline round the lot and the void under the bar as a hole.
        assert_eq!(z.holes.len(), 1, "{z:?}");
        // U (700e6) + bar (140e6) - their overlap (20e6), the hole (10x15 = 150e6) inside the outer ring and not in the U to begin with.
        assert!((ring_area(&z.outline) - ring_area(&z.holes[0]) - 820e6).abs() < 1.0, "{} {}", ring_area(&z.outline), ring_area(&z.holes[0]));
    }

    #[test]
    fn priority_raise_goes_just_above_the_next_overlapping_zone_and_leaves_the_others_alone() {
        let mut d = design(vec![
            square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 1),
            square("b", "F.Cu", "GND", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 4),
            square("c", "F.Cu", "GND", Nm::new(5_000), Nm::new(5_000), Nm::new(10_000), 7),
            square("far", "F.Cu", "GND", Nm::new(90_000), Nm::ZERO, Nm::new(1_000), 2),
        ]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Raise).unwrap();
        assert_eq!(priorities(&d), [("a", 5), ("b", 4), ("c", 7), ("far", 2)].map(|(i, p)| (i.to_string(), p)).to_vec(), "just above b, the lowest above it; nothing else moved");
        set_zone_priority(&mut d, "a", ZonePriorityMove::Raise).unwrap();
        assert_eq!(priorities(&d)[0], ("a".to_string(), 8), "then just above c");
        let e = set_zone_priority(&mut d, "a", ZonePriorityMove::Raise).unwrap_err();
        assert_eq!(e[0].check, "ops_zone_priority", "already on top");
    }

    /// `getOverlappingZones` tests `Outline()->Collide( .. )` (triangles, none in a hole) and `Contains( CVertex( 0 ) )` ("If the point is inside a hole it
    /// is outside of the polygon"): a zone in another's hole does not overlap it (r7 C7-1).
    #[test]
    fn a_zone_inside_anothers_hole_does_not_overlap_it() {
        let mut b = square("b", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, mm(20.0), 0);
        b.holes = vec![vec![pt(mm(5.0), mm(5.0)), pt(mm(15.0), mm(5.0)), pt(mm(15.0), mm(15.0)), pt(mm(5.0), mm(15.0))]];
        let a = square("a", "F.Cu", "GND", mm(8.0), mm(8.0), mm(4.0), 1);
        let mut d = design(vec![b.clone(), a]);
        assert!(overlapping_zones(zones(&d), 1).is_empty(), "in the hole");
        assert!(set_zone_priority(&mut d, "a", ZonePriorityMove::Lower).is_err(), "nothing to lower under");
        assert_eq!(priorities(&d), vec![("b".to_string(), 0), ("a".to_string(), 1)]);
        // one that reaches over the hole's edge does
        let over = square("o", "F.Cu", "GND", mm(3.0), mm(8.0), mm(4.0), 1);
        let d = design(vec![b, over]);
        assert_eq!(overlapping_zones(zones(&d), 1), vec![0]);
    }

    #[test]
    fn priority_lower_goes_just_below_the_next_overlapping_zone() {
        let mut d = design(vec![square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 9), square("b", "F.Cu", "GND", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 4), square("c", "F.Cu", "GND", Nm::new(5_000), Nm::new(5_000), Nm::new(10_000), 2)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Lower).unwrap();
        assert_eq!(priorities(&d)[0].1, 3, "just below b, the highest below it");
        set_zone_priority(&mut d, "a", ZonePriorityMove::Lower).unwrap();
        assert_eq!(priorities(&d)[0].1, 1, "then just below c");
        assert!(set_zone_priority(&mut d, "a", ZonePriorityMove::Lower).is_err(), "already at the bottom");
    }

    #[test]
    fn raising_over_a_zone_at_the_ceiling_swaps_the_two_and_lowering_under_one_at_zero_swaps_too() {
        let mut d = design(vec![square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 5), square("b", "F.Cu", "GND", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), u32::MAX)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Raise).unwrap();
        assert_eq!(priorities(&d), vec![("a".to_string(), u32::MAX), ("b".to_string(), 5)]);
        let mut d = design(vec![square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 5), square("b", "F.Cu", "GND", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 0)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Lower).unwrap();
        assert_eq!(priorities(&d), vec![("a".to_string(), 0), ("b".to_string(), 5)]);
    }

    #[test]
    fn move_to_top_takes_the_slot_that_displaces_fewer_zones() {
        // a (1) overlaps b (2) and c (3). Top: A takes 3 and pushes c to 2 -- but b is at 2, so b goes to 1: 2 moved.
        // B takes 4 and pushes nothing (4 is free): 0 moved. B wins.
        let mut d = design(vec![square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 1), square("b", "F.Cu", "GND", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 2), square("c", "F.Cu", "GND", Nm::new(5_000), Nm::new(5_000), Nm::new(10_000), 3)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Top).unwrap();
        assert_eq!(priorities(&d), [("a", 4), ("b", 2), ("c", 3)].map(|(i, p)| (i.to_string(), p)).to_vec());
        // Already on top.
        assert!(set_zone_priority(&mut d, "a", ZonePriorityMove::Top).is_err());
        // When the slot above is taken by a zone that does not overlap, pushing it up costs one more than pushing the
        // overlapping ones down only if fewer: a (1), b (2) overlap; far sits at 3 (not overlapping). A: take 2, push b to 1 (1 moved).
        // B: take 3, push far to 4 (1 moved). A wins a tie (down <= up).
        let mut d = design(vec![square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 1), square("b", "F.Cu", "GND", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 2), square("far", "F.Cu", "GND", Nm::new(90_000), Nm::ZERO, Nm::new(1_000), 3)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Top).unwrap();
        assert_eq!(priorities(&d), [("a", 2), ("b", 1), ("far", 3)].map(|(i, p)| (i.to_string(), p)).to_vec(), "ties go down");
    }

    #[test]
    fn move_to_bottom_is_the_mirror_image_and_a_cascade_into_zero_is_not_viable() {
        let mut d = design(vec![square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 9), square("b", "F.Cu", "GND", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 5), square("c", "F.Cu", "GND", Nm::new(5_000), Nm::new(5_000), Nm::new(10_000), 4)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Bottom).unwrap();
        // A: take 4 and push c (4) to 5 and b (5) to 6: 2 moved. B: take 3, push nothing (3 is free). B wins.
        assert_eq!(priorities(&d), [("a", 3), ("b", 5), ("c", 4)].map(|(i, p)| (i.to_string(), p)).to_vec());
        // Zero is occupied below: the only way is up. a (3) overlaps b (0).
        let mut d = design(vec![square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 3), square("b", "F.Cu", "GND", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 0)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Bottom).unwrap();
        assert_eq!(priorities(&d), [("a", 0), ("b", 1)].map(|(i, p)| (i.to_string(), p)).to_vec(), "a takes 0, b is pushed up (pushing down from -1 is not viable)");
    }

    #[test]
    fn the_priority_moves_only_apply_to_copper_zones_and_only_count_zones_on_the_same_layer() {
        let mut keepout = square("k", "F.Cu", "", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 0);
        keepout.is_rule_area = true;
        let mut d = design(vec![keepout, square("a", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 0), square("b", "B.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 5)]);
        assert!(set_zone_priority(&mut d, "k", ZonePriorityMove::Raise).is_err(), "a rule area has no fill priority");
        assert!(set_zone_priority(&mut d, "a", ZonePriorityMove::Raise).is_err(), "b is on another layer, the keepout is not copper: nothing to rise over");
        assert!(set_zone_priority(&mut d, "nope", ZonePriorityMove::Raise).is_err());
    }

    #[test]
    fn the_drill_origin_is_set_and_reset_and_setting_what_is_there_is_refused() {
        let mut d = design(vec![]);
        set_aux_origin(&mut d, Some(pt(Nm::new(1_000), Nm::new(2_000)))).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().aux_origin, Some(pt(Nm::new(1_000), Nm::new(2_000))));
        assert!(set_aux_origin(&mut d, Some(pt(Nm::new(1_000), Nm::new(2_000)))).is_err());
        set_aux_origin(&mut d, None).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().aux_origin, None);
        assert!(set_aux_origin(&mut d, None).is_err(), "already at (0, 0)");
        // (0, 0) is a reset, not a stored origin.
        set_aux_origin(&mut d, Some(pt(Nm::new(5), Nm::new(5)))).unwrap();
        set_aux_origin(&mut d, Some(pt(Nm::ZERO, Nm::ZERO))).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().aux_origin, None);
    }

    #[test]
    fn the_grid_origin_is_set_and_reset_and_setting_what_is_there_is_refused() {
        let mut d = design(vec![]);
        set_grid_origin(&mut d, Some(pt(Nm::new(1_250), Nm::new(2_500)))).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().grid_origin, Some(pt(Nm::new(1_250), Nm::new(2_500))));
        assert!(set_grid_origin(&mut d, Some(pt(Nm::new(1_250), Nm::new(2_500)))).is_err());
        set_grid_origin(&mut d, None).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().grid_origin, None);
        assert!(set_grid_origin(&mut d, None).is_err(), "already at (0, 0)");
        // (0, 0) is a reset, not a stored origin, and the grid origin is not the drill/place file origin.
        set_grid_origin(&mut d, Some(pt(Nm::new(5), Nm::new(5)))).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().aux_origin, None);
        set_grid_origin(&mut d, Some(pt(Nm::ZERO, Nm::ZERO))).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().grid_origin, None);
    }

    /// `ExportFootprintsToLibrary`: every board footprint saved over the library entry of its name, as the board has it (r7 X7-1).
    #[test]
    fn export_footprints_writes_the_boards_edited_footprint_over_the_library_entry() {
        use eda_model::fp_edit::{FootprintEdit, PadEdit};
        let part = |r: &str| -> eda_model::Part { serde_json::from_value(serde_json::json!({ "reference": r, "package": "0402", "footprint": "0402", "pins": [] })).unwrap() };
        let mut model = eda_model::ConstraintModel { parts: vec![part("C1"), part("C2")], ..Default::default() };
        let mut d = design(vec![]);
        // no entry yet: added from what the board shows
        let r = export_footprints_to_library(&mut d, &model).unwrap();
        assert_eq!(r.added, vec!["0402".to_string()]);
        let builtin = eda_model::footprint::builtin("0402").unwrap();
        let entry = d.footprint_library.as_ref().unwrap().by_name("0402").unwrap();
        assert_eq!(entry.pads.len(), 2);
        assert_eq!(entry.pads[0].size, builtin.pads[0].size);

        // an entry that is there and not published: the board's copy over it, the last instance of the name winning
        let mut pad_edit = PadEdit::none("1", 1);
        pad_edit.size = Some((mm(1.2), mm(1.4)));
        model.instance_footprints.insert("C2".into(), eda_model::fp_edit::patched_footprint(&builtin, std::slice::from_ref(&pad_edit)));
        d.footprint_library.as_mut().unwrap().by_name_mut("0402").unwrap().description = "mine".into();
        let r = export_footprints_to_library(&mut d, &model).unwrap();
        assert_eq!(r.replaced, vec!["0402".to_string()], "\"already there\" is overwritten (SaveFootprint( .., true ))");
        let entry = d.footprint_library.as_ref().unwrap().by_name("0402").unwrap();
        assert_eq!(entry.pads.iter().find(|p| p.number == "1").unwrap().size, (mm(1.2), mm(1.4)), "C2's edited pad");
        assert_eq!(entry.description, "mine", "what the board does not have stays");

        // a published entry: the instance's pad edit is laid on its pads, which keep what the editor kept of the file
        {
            let entry = d.footprint_library.as_mut().unwrap().by_name_mut("0402").unwrap();
            entry.published = true;
            for p in entry.pads.iter_mut() {
                p.size = (mm(0.6), mm(0.5));
                p.kicad.tokens = vec!["(pinfunction \"A\")".into()];
            }
        }
        model.instance_footprints.clear();
        let mut edit = FootprintEdit::new("C2");
        let mut e = PadEdit::none("2", 1);
        e.size = Some((mm(0.9), mm(0.8)));
        e.solder_mask_margin = Some(mm(0.05));
        edit.pads = vec![e];
        d.drawings.get_or_insert_with(Default::default).footprint_edits = vec![edit];
        export_footprints_to_library(&mut d, &model).unwrap();
        let entry = d.footprint_library.as_ref().unwrap().by_name("0402").unwrap();
        let p2 = entry.pads.iter().find(|p| p.number == "2").unwrap();
        assert_eq!(p2.size, (mm(0.9), mm(0.8)));
        assert_eq!(p2.kicad.solder_mask_margin, Some(mm(0.05)));
        assert_eq!(p2.kicad.tokens, vec!["(pinfunction \"A\")".to_string()]);
        assert_eq!(entry.pads.iter().find(|p| p.number == "1").unwrap().size, (mm(0.6), mm(0.5)), "an unedited pad is the entry's");

        // a board with no footprints
        assert_eq!(export_footprints_to_library(&mut design(vec![]), &eda_model::ConstraintModel::default()).unwrap_err()[0].hint.as_deref(), Some("No footprints to export!"));
    }

    #[test]
    fn repair_gives_duplicate_ids_new_ones_and_reports_kicads_lines() {
        let mut d = design(vec![square("zon_1", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 0), square("zon_1", "B.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 0), square("zon_1", "F.Cu", "GND", Nm::new(50_000), Nm::ZERO, Nm::new(10_000), 0)]);
        let nets = vec![Net { name: "GND".into(), pins: vec![] }];
        let r = repair_board(&mut d, &nets);
        assert_eq!(r, RepairReport { repaired: 2, details: vec!["2 duplicate IDs replaced.".into()] });
        let ids: BTreeSet<&str> = zones(&d).iter().map(|z| z.id.as_str()).collect();
        assert_eq!(ids.len(), 3, "all different now: {ids:?}");
        assert_eq!(zones(&d)[0].id, "zon_1", "the first keeps its id");
        assert!(zones(&d).iter().all(|z| !z.id.is_empty()));
        // Nothing left to repair.
        assert_eq!(repair_board(&mut d, &nets), RepairReport::default());
    }

    #[test]
    fn the_four_commands_read_from_json_and_apply_through_a_board() {
        use crate::{Board, Cmd};
        let model = eda_model::ConstraintModel::default();
        let mut d = design(vec![square("a", "F.Cu", "", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 1), square("b", "F.Cu", "", Nm::new(5_000), Nm::ZERO, Nm::new(10_000), 4), square("ghost", "F.Cu", "NOPE", Nm::new(90_000), Nm::ZERO, Nm::new(1_000), 0)]);
        d.placement = Some(serde_json::from_value(serde_json::json!({ "outline": [{ "x": 0, "y": 0 }, { "x": 1, "y": 0 }, { "x": 1, "y": 1 }], "footprints": [] })).unwrap());
        let mut board = Board::new(d, &model, mm(0.1), mm(0.3));
        let cmd = |j: serde_json::Value| serde_json::from_value::<Cmd>(j).unwrap();
        board.apply(&cmd(serde_json::json!({ "op": "set_zone_priority", "id": "a", "to": "raise" }))).unwrap();
        assert_eq!(priorities(board.design())[0], ("a".to_string(), 5));
        board.apply(&cmd(serde_json::json!({ "op": "merge_zones", "ids": ["a", "b"] }))).unwrap();
        assert_eq!(zones(board.design()).len(), 2, "b merged into a; ghost stays");
        board.apply(&cmd(serde_json::json!({ "op": "set_aux_origin", "at": { "x": 3_000, "y": 4_000 } }))).unwrap();
        assert_eq!(board.design().drawings.as_ref().unwrap().aux_origin, Some(pt(Nm::new(3_000), Nm::new(4_000))));
        board.apply(&cmd(serde_json::json!({ "op": "set_aux_origin", "at": null }))).unwrap();
        assert_eq!(board.design().drawings.as_ref().unwrap().aux_origin, None);
        // The ghost zone's net is not in the (empty) netlist: one problem, then none.
        board.apply(&cmd(serde_json::json!({ "op": "repair_board" }))).unwrap();
        assert_eq!(board.design().nets.as_ref().unwrap()[0].name, "NOPE");
        // The next load reads the board's own netlist back (`board::load` lets `design.nets` replace the intent's).
        let design = board.into_design();
        let model = eda_model::ConstraintModel { nets: design.nets.clone().unwrap(), ..Default::default() };
        let again = Board::new(design, &model, mm(0.1), mm(0.3)).apply(&Cmd::RepairBoard).unwrap_err();
        assert_eq!(again[0].hint.as_deref(), Some("No board problems found."));
    }

    #[test]
    fn repair_adds_back_a_net_that_items_use_but_the_netlist_lacks() {
        let mut d = design(vec![square("z", "F.Cu", "GHOST", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 0), square("y", "F.Cu", "GND", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 0), square("x", "F.Cu", "", Nm::ZERO, Nm::ZERO, Nm::new(10_000), 0)]);
        let nets = vec![Net { name: "GND".into(), pins: vec!["U1.1".into()] }];
        let r = repair_board(&mut d, &nets);
        assert_eq!(r, RepairReport { repaired: 1, details: vec!["Orphaned net GHOST re-parented.".into()] });
        let list = d.nets.clone().unwrap();
        assert_eq!(list.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["GND", "GHOST"]);
        assert_eq!(list[0].pins, vec!["U1.1".to_string()], "the existing nets keep their pins");
        assert!(list[1].pins.is_empty());
        // The no-net zone ("") is not an orphan; a second run finds nothing.
        assert_eq!(repair_board(&mut d, &list), RepairReport::default());
    }
}

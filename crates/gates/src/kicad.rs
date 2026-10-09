//! The gates KiCad owns, answered by kicad-cli.
//!
//! Courtyard overlap and copper-to-edge clearance are KiCad design rules
//! (`courtyards_overlap`, `copper_edge_clearance`), so no Rust code here
//! measures them: one `kicad-cli pcb drc` run on the exported design, its
//! report filtered by type and translated into the gate names this crate
//! has always used:
//!
//! | KiCad type              | gate                           | what it covers          |
//! |-------------------------|--------------------------------|-------------------------|
//! | `copper_edge_clearance` | `placement_pad_edge_clearance` | pads (placement time)   |
//! | `courtyards_overlap`    | `placement_courtyard_overlap`  | footprints              |
//! | `copper_edge_clearance` | `routing_edge_clearance`       | tracks and vias         |
//!
//! A kicad-cli run takes seconds, so none of this is in
//! [`check_placement`](crate::check_placement)/[`check_routing`](crate::check_routing)
//! (those run on every step of the placer and every studio refresh, and
//! stay in-process). The judges call this: `eda board check`, `--strict`,
//! and the pipeline's stage gates. A missing kicad-cli is a failing
//! `kicad_cli_missing` gate, never a silent skip -- a gate that quietly
//! stops running is worse than no gate.

use eda_kicad_engine::{drc_scratch, DrcReport, Violation};
use eda_model::ir::Design;
use eda_model::{CheckResult, CheckStatus, ConstraintModel};

/// One KiCad violation as the gate result for `check_name`. `location`
/// joins every item's id (our own id where kicad-cli's uuid maps back to
/// one, else KiCad's own description of it), mirroring the "A/B" pair
/// convention every gate here uses -- sorted, because KiCad's item order is
/// its own and a location is how `--strict` tells a new failure from an old
/// one; the board outline (`outline`) is dropped when another item is named,
/// so a pad too close to the edge is located at the pad. `hint` is KiCad's
/// own description. KiCad's severity carries over: an error fails the gate,
/// a warning warns.
fn gate_of(v: &Violation, check_name: &str) -> Option<CheckResult> {
    // A violation the user waived (an exclusion in the design) is reported, flagged, but it is not a finding.
    if v.excluded {
        return None;
    }
    let status = match v.severity.as_str() {
        "error" => CheckStatus::Fail,
        "warning" => CheckStatus::Warn,
        _ => return None,
    };
    // The board outline is dropped when another item is named: the polygon's sides (`outline`) and the Edge.Cuts shapes of an outline made of
    // them (an arc, a circle, a rectangle: kicad-cli describes each "... on Edge.Cuts"), so a pad too close to the edge is located at the pad.
    let on_edge = |it: &eda_kicad_engine::Item| it.description.contains("Edge.Cuts");
    let has_copper = v.items.iter().any(|it| !on_edge(it));
    let mut ids: Vec<String> = v.items.iter().filter(|it| !(has_copper && on_edge(it))).map(|it| it.id.clone().unwrap_or_else(|| it.description.clone())).collect();
    if ids.len() > 1 {
        ids.retain(|id| id != "outline");
    }
    ids.sort();
    let location = ids.join("/");
    Some(CheckResult { check: check_name.to_string(), status, location: Some(location), hint: Some(v.description.clone()), detail: None })
}

/// The copper item of an edge-clearance violation. kicad-cli names two items,
/// the Edge.Cuts segment ("Segment on Edge.Cuts") and the copper too close to
/// it ("Pad 1 [net] of R1 ...", "Track [net] on F.Cu ...", "Via [net] ..."),
/// in either order.
fn copper_item(v: &Violation) -> Option<&eda_kicad_engine::Item> {
    v.items.iter().find(|it| !it.description.contains("Edge.Cuts"))
}

fn copper_is(v: &Violation, prefixes: &[&str]) -> bool {
    copper_item(v).is_some_and(|it| prefixes.iter().any(|p| it.description.starts_with(p)))
}

/// The violations of one KiCad type that `keep` accepts, as gate results
/// named `check_name`, with a single `pass` when there are none.
fn gate(report: &DrcReport, kind: &str, check_name: &str, keep: impl Fn(&Violation) -> bool, out: &mut Vec<CheckResult>) {
    let mut any = false;
    for v in report.violations.iter().filter(|v| v.kind == kind && keep(v)) {
        if let Some(r) = gate_of(v, check_name) {
            any = true;
            out.push(r);
        }
    }
    if !any {
        out.push(CheckResult::pass(check_name));
    }
}

/// The placement gates KiCad owns, from a DRC report: pad-to-edge
/// clearance (pads only: copper that routing adds is `routing_edge_clearance`'s)
/// and courtyard overlap.
pub fn placement_from(report: &DrcReport) -> Vec<CheckResult> {
    let mut out = Vec::new();
    gate(report, "copper_edge_clearance", "placement_pad_edge_clearance", |v| copper_is(v, &["Pad "]), &mut out);
    gate(report, "courtyards_overlap", "placement_courtyard_overlap", |_| true, &mut out);
    out
}

/// The routing gate KiCad owns, from a DRC report: track and via copper too
/// close to the board outline.
pub fn routing_from(report: &DrcReport) -> Vec<CheckResult> {
    let mut out = Vec::new();
    gate(report, "copper_edge_clearance", "routing_edge_clearance", |v| copper_is(v, &["Track ", "Via "]), &mut out);
    out
}

/// The placement gates KiCad owns, on a placement-only export of `design`
/// (routing stripped: the placement gates never look at a track, via or
/// zone, and a kicad-cli run is cheaper without them).
pub fn check_placement(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    let placement_only = Design { routing: None, ..design.clone() };
    match drc_scratch(&placement_only, model) {
        Ok(report) => placement_from(&report),
        Err(e) => e,
    }
}

/// The routing gate KiCad owns, on `design` as it is.
pub fn check_routing(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    match drc_scratch(design, model) {
        Ok(report) => routing_from(&report),
        Err(e) => e,
    }
}

/// Every gate KiCad owns, from one kicad-cli run on `design` as it is: the
/// placement ones always, the routing one when there is routing.
pub fn check(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    // No routing to judge: the placement-only run is the whole answer.
    if design.routing.is_none() {
        return check_placement(design, model);
    }
    match drc_scratch(design, model) {
        Ok(report) => {
            let mut out = placement_from(&report);
            out.extend(routing_from(&report));
            out
        }
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_kicad_engine::Item;

    fn violation(kind: &str, severity: &str, first_item: &str, id: Option<&str>) -> Violation {
        Violation { kind: kind.into(), description: format!("{kind} description"), severity: severity.into(), items: vec![Item { description: first_item.into(), pos: (0, 0), id: id.map(str::to_string), uuid: "u".into() }], ..Default::default() }
    }

    /// kicad-cli's edge-clearance shape: the Edge.Cuts segment and the copper, in either order.
    fn edge_violation(copper: &str, id: &str, edge_first: bool) -> Violation {
        let mut v = violation("copper_edge_clearance", "error", copper, Some(id));
        let edge = Item { description: "Segment on Edge.Cuts".into(), pos: (0, 0), id: Some("outline".into()), uuid: "e".into() };
        if edge_first {
            v.items.insert(0, edge);
        } else {
            v.items.push(edge);
        }
        v
    }

    fn report(violations: Vec<Violation>) -> DrcReport {
        DrcReport { engine: "kicad-cli test".into(), zones_refilled_by_kicad: false, violations, unconnected_items: vec![], schematic_parity: vec![], parity: None, ignored_checks: vec![] }
    }

    #[test]
    fn edge_clearance_splits_by_what_the_copper_item_is() {
        let r = report(vec![
            edge_violation("Pad 1 [GND] of R1 on F.Cu", "R1.1", true),
            edge_violation("Track [GND] on F.Cu, length 3 mm", "t1#0", true),
            edge_violation("Via [GND] on F.Cu - B.Cu", "v1", false),
            edge_violation("Zone [GND] on F.Cu", "z1", true),
        ]);
        let place = placement_from(&r);
        let pad: Vec<_> = place.iter().filter(|c| c.check == "placement_pad_edge_clearance").collect();
        assert_eq!(pad.len(), 1, "only the pad is the placement gate's: {place:?}");
        assert_eq!(pad[0].location.as_deref(), Some("R1.1"), "located at the pad, not at the outline");
        assert_eq!(pad[0].status, CheckStatus::Fail);
        let route = routing_from(&r);
        let mut at: Vec<_> = route.iter().filter(|c| c.status == CheckStatus::Fail).map(|c| c.location.clone().unwrap_or_default()).collect();
        at.sort();
        assert_eq!(at, vec!["t1#0", "v1"], "{route:?}");
    }

    #[test]
    fn an_edge_shape_that_is_not_the_polygon_outline_is_dropped_from_the_location_too() {
        // An arc or a circle of the outline has a shape id of its own, not "outline"; the pad is still where the finding is.
        let mut v = edge_violation("Pad 1 [GND] of R1 on F.Cu", "R1.1", true);
        v.items[0] = Item { description: "Arc on Edge.Cuts".into(), pos: (0, 0), id: Some("shp_3f9a".into()), uuid: "e".into() };
        let r = placement_from(&report(vec![v]));
        let pad: Vec<_> = r.iter().filter(|c| c.check == "placement_pad_edge_clearance").collect();
        assert_eq!(pad.len(), 1);
        assert_eq!(pad[0].location.as_deref(), Some("R1.1"), "{pad:?}");
    }

    #[test]
    fn a_clean_report_passes_every_gate_once() {
        let r = report(vec![]);
        let names: Vec<_> = placement_from(&r).iter().chain(routing_from(&r).iter()).map(|c| (c.check.clone(), c.status)).collect();
        assert_eq!(
            names,
            vec![
                ("placement_pad_edge_clearance".to_string(), CheckStatus::Pass),
                ("placement_courtyard_overlap".to_string(), CheckStatus::Pass),
                ("routing_edge_clearance".to_string(), CheckStatus::Pass),
            ]
        );
    }

    #[test]
    fn kicads_own_severity_carries_over() {
        let r = report(vec![violation("courtyards_overlap", "warning", "Footprint R1", Some("R1")), violation("courtyards_overlap", "ignore", "Footprint R2", Some("R2"))]);
        let out = placement_from(&r);
        let overlaps: Vec<_> = out.iter().filter(|c| c.check == "placement_courtyard_overlap").collect();
        assert_eq!(overlaps.len(), 1, "{out:?}");
        assert_eq!(overlaps[0].status, CheckStatus::Warn);
    }

    /// The whole path against the real kicad-cli (skipped when there is none):
    /// the exporter writes the courtyards, kicad-cli judges them, the gate
    /// reports. Two parts stacked on one spot fail `placement_courtyard_overlap`;
    /// moved apart they pass. Guards the one way this gate could silently
    /// stop firing -- an export kicad-cli cannot read courtyards from.
    #[test]
    fn stacked_parts_fail_the_courtyard_gate_through_kicad_cli() {
        use eda_model::ir::{FootprintInstance, PlacementSection, Point, Provenance, Side};
        use eda_model::{BoardRules, Net, Part, Pin, PinKind};
        if eda_kicad_engine::find_cli().is_none() {
            eprintln!("kicad-cli not found; skipping");
            return;
        }
        let part = |r: &str| Part {
            reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("0805".into()), footprint: None,
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
            body_um: None, symbol: None, datasheet: None, edge: None,
        };
        let model = ConstraintModel {
            parts: vec![part("R1"), part("R2")],
            nets: vec![Net { name: "A".into(), pins: vec!["R1.1".into(), "R2.1".into()] }],
            board: BoardRules { layers: vec!["F.Cu".into(), "B.Cu".into()], ..Default::default() },
            ..Default::default()
        };
        let design = |second_x: i64| Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: Some(PlacementSection {
                outline: vec![Point { x: 0, y: 0 }, Point { x: 40_000, y: 0 }, Point { x: 40_000, y: 40_000 }, Point { x: 0, y: 40_000 }],
                footprints: vec![
                    FootprintInstance { id: "R1".into(), at: Point { x: 10_000, y: 20_000 }, rot: 0, side: Side::Top, label: Default::default() },
                    FootprintInstance { id: "R2".into(), at: Point { x: second_x, y: 20_000 }, rot: 0, side: Side::Top, label: Default::default() },
                ],
                modules: vec![],
            }),
            routing: None,
            drawings: None,
        };
        let overlap = |checks: &[CheckResult]| checks.iter().filter(|c| c.check == "placement_courtyard_overlap" && c.status == CheckStatus::Fail).count();
        let stacked = check_placement(&design(10_000), &model);
        assert_eq!(overlap(&stacked), 1, "{stacked:?}");
        assert_eq!(stacked.iter().find(|c| c.check == "placement_courtyard_overlap").and_then(|c| c.location.as_deref()), Some("R1/R2"), "the report maps back to our own part ids: {stacked:?}");
        let apart = check_placement(&design(25_000), &model);
        assert_eq!(overlap(&apart), 0, "{apart:?}");
        assert!(apart.iter().any(|c| c.check == "placement_courtyard_overlap" && c.status == CheckStatus::Pass), "{apart:?}");
    }

    #[test]
    fn an_item_that_is_not_ours_is_located_by_kicads_description() {
        let v = violation("courtyards_overlap", "error", "Footprint ?", None);
        assert_eq!(gate_of(&v, "placement_courtyard_overlap").unwrap().location.as_deref(), Some("Footprint ?"));
    }
}

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
/// convention every gate here uses; `hint` is KiCad's own description.
/// KiCad's severity carries over: an error fails the gate, a warning warns.
fn gate_of(v: &Violation, check_name: &str) -> Option<CheckResult> {
    let status = match v.severity.as_str() {
        "error" => CheckStatus::Fail,
        "warning" => CheckStatus::Warn,
        _ => return None,
    };
    let location = v.items.iter().map(|it| it.id.clone().unwrap_or_else(|| it.description.clone())).collect::<Vec<_>>().join("/");
    Some(CheckResult { check: check_name.to_string(), status, location: Some(location), hint: Some(v.description.clone()), detail: None })
}

/// Whether a violation's first item is a track or a via (as opposed to a
/// pad, a zone or a footprint). kicad-cli names items "Track [net] on
/// F.Cu, ...", "Via [net] ...", "Pad 1 [net] of R1 ...".
fn first_item_is_track_or_via(v: &Violation) -> bool {
    v.items.first().is_some_and(|it| it.description.starts_with("Track ") || it.description.starts_with("Via "))
}

fn first_item_is_zone(v: &Violation) -> bool {
    v.items.first().is_some_and(|it| it.description.starts_with("Zone "))
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
    gate(report, "copper_edge_clearance", "placement_pad_edge_clearance", |v| !first_item_is_track_or_via(v) && !first_item_is_zone(v), &mut out);
    gate(report, "courtyards_overlap", "placement_courtyard_overlap", |_| true, &mut out);
    out
}

/// The routing gate KiCad owns, from a DRC report: track and via copper too
/// close to the board outline.
pub fn routing_from(report: &DrcReport) -> Vec<CheckResult> {
    let mut out = Vec::new();
    gate(report, "copper_edge_clearance", "routing_edge_clearance", first_item_is_track_or_via, &mut out);
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
        Violation { kind: kind.into(), description: format!("{kind} description"), severity: severity.into(), items: vec![Item { description: first_item.into(), pos: (0, 0), id: id.map(str::to_string), uuid: "u".into() }] }
    }

    fn report(violations: Vec<Violation>) -> DrcReport {
        DrcReport { engine: "kicad-cli test".into(), zones_refilled_by_kicad: false, violations, unconnected_items: vec![] }
    }

    #[test]
    fn edge_clearance_splits_by_what_the_first_item_is() {
        let r = report(vec![
            violation("copper_edge_clearance", "error", "Pad 1 [GND] of R1 on F.Cu", Some("R1.1")),
            violation("copper_edge_clearance", "error", "Track [GND] on F.Cu, length 3 mm", Some("t1#0")),
            violation("copper_edge_clearance", "error", "Via [GND] on F.Cu - B.Cu", Some("v1")),
            violation("copper_edge_clearance", "error", "Zone [GND] on F.Cu", Some("z1")),
        ]);
        let place = placement_from(&r);
        let pad: Vec<_> = place.iter().filter(|c| c.check == "placement_pad_edge_clearance").collect();
        assert_eq!(pad.len(), 1, "{place:?}");
        assert_eq!(pad[0].location.as_deref(), Some("R1.1"));
        assert_eq!(pad[0].status, CheckStatus::Fail);
        let route = routing_from(&r);
        assert_eq!(route.iter().filter(|c| c.status == CheckStatus::Fail).count(), 2, "{route:?}");
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

    #[test]
    fn an_item_that_is_not_ours_is_located_by_kicads_description() {
        let v = violation("courtyards_overlap", "error", "Footprint ?", None);
        assert_eq!(gate_of(&v, "placement_courtyard_overlap").unwrap().location.as_deref(), Some("Footprint ?"));
    }
}

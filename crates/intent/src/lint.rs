//! T1.5 electrical lint: checks that run on a [`ConstraintModel`] *before*
//! any geometry exists. Purely structural/electrical — no coordinates.

use eda_model::{CheckResult, ConstraintModel, PinKind};
use std::collections::HashMap;

/// Heuristics for recognizing power/ground nets by name, used only for the
/// warn-level "no ground/power net at all" checks below.
fn looks_like_ground(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n == "GND" || n == "GROUND" || n == "AGND" || n == "DGND" || n == "VSS" || n.starts_with("GND")
}

fn looks_like_power(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n == "VCC" || n == "VDD" || n == "VBUS" || n == "VBAT" || n.starts_with('V') && n.len() > 1
}

/// Run all electrical lint checks and return every finding (pass entries are
/// not included; only fail/warn are reported, mirroring the other
/// checkers' convention of "silence means pass" for per-instance checks —
/// callers that want a positive pass record can synthesize one from an
/// empty Vec).
pub fn lint(model: &ConstraintModel) -> Vec<CheckResult> {
    let mut out = Vec::new();

    out.extend(check_pin_not_connected(model));
    out.extend(check_net_single_pin(model));
    out.extend(check_no_ground_net(model));
    out.extend(check_no_power_net(model));
    out.extend(check_power_pin_direct_short(model));
    out.extend(check_duplicate_reference(model));
    out.extend(check_pin_on_multiple_nets(model));
    out.extend(check_missing_footprint(model));
    out.extend(check_pin_has_no_pad(model));
    out.extend(check_footprint_body_mismatch(model));
    out.extend(check_control_pin_unconnected(model));

    out
}

/// True for pin names that read as a "control-ish" input to an IC (enable,
/// chip-select, reset...). Kept as its own small pattern list (rather than
/// depending on `eda-engine::geometry::is_control_pin_name`, which would
/// create a `eda-intent` -> `eda-engine` dependency this crate doesn't
/// otherwise need) — the two lists are intentionally identical, see that
/// function's doc comment.
fn is_control_pin_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["EN", "CE", "SHDN", "RESET", "RST", "CS"].iter().any(|p| n.contains(p))
}

/// A pin whose name reads as a control input (EN/CE/SHDN/RESET/CS) but is on
/// no net at all is an electrical issue, not just a cosmetic one: floating
/// control pins on real ICs are undefined logic level (undocumented internal
/// pull, if any). Fail.
fn check_control_pin_unconnected(model: &ConstraintModel) -> Vec<CheckResult> {
    let mut connected: std::collections::HashSet<String> = std::collections::HashSet::new();
    for net in &model.nets {
        for p in &net.pins {
            connected.insert(p.clone());
        }
    }
    let mut out = Vec::new();
    for part in &model.parts {
        for pin in &part.pins {
            if pin.kind == PinKind::Nc {
                continue;
            }
            let Some(name) = &pin.name else { continue };
            if !is_control_pin_name(name) {
                continue;
            }
            let loc = format!("{}.{}", part.reference, pin.number);
            if !connected.contains(&loc) {
                out.push(CheckResult::fail(
                    "source_control_pin_unconnected",
                    loc.clone(),
                    format!("control pin {} ('{}') is not attached to any net", loc, name),
                ));
            }
        }
    }
    out
}

fn check_pin_not_connected(model: &ConstraintModel) -> Vec<CheckResult> {
    let mut connected: std::collections::HashSet<String> = std::collections::HashSet::new();
    for net in &model.nets {
        for p in &net.pins {
            connected.insert(p.clone());
        }
    }
    let mut out = Vec::new();
    for part in &model.parts {
        for pin in &part.pins {
            if pin.kind == PinKind::Nc {
                continue;
            }
            let loc = format!("{}.{}", part.reference, pin.number);
            if !connected.contains(&loc) {
                out.push(CheckResult::fail(
                    "source_pin_not_connected",
                    loc.clone(),
                    format!("pin {} is not attached to any net", loc),
                ));
            }
        }
    }
    out
}

fn check_net_single_pin(model: &ConstraintModel) -> Vec<CheckResult> {
    model
        .nets
        .iter()
        .filter(|n| n.pins.len() == 1)
        .map(|n| {
            CheckResult::fail(
                "source_net_single_pin",
                n.name.clone(),
                format!("net '{}' has only one pin attached ({})", n.name, n.pins[0]),
            )
        })
        .collect()
}

fn check_no_ground_net(model: &ConstraintModel) -> Vec<CheckResult> {
    let has_ground = model.nets.iter().any(|n| looks_like_ground(&n.name))
        || model
            .parts
            .iter()
            .any(|p| p.pins.iter().any(|pin| pin.kind == PinKind::Ground));
    if has_ground {
        Vec::new()
    } else {
        vec![CheckResult {
            check: "source_no_ground_net".into(),
            status: eda_model::CheckStatus::Warn,
            location: None,
            hint: Some("no net or ground-kind pin found in the design".into()), detail: None
        }]
    }
}

fn check_no_power_net(model: &ConstraintModel) -> Vec<CheckResult> {
    let has_power = model.nets.iter().any(|n| looks_like_power(&n.name))
        || model
            .parts
            .iter()
            .any(|p| p.pins.iter().any(|pin| pin.kind == PinKind::Power));
    if has_power {
        Vec::new()
    } else {
        vec![CheckResult {
            check: "source_no_power_net".into(),
            status: eda_model::CheckStatus::Warn,
            location: None,
            hint: Some("no net or power-kind pin found in the design".into()), detail: None
        }]
    }
}

fn check_power_pin_direct_short(model: &ConstraintModel) -> Vec<CheckResult> {
    let mut out = Vec::new();
    for net in &model.nets {
        let mut has_power = false;
        let mut has_ground = false;
        for pin_ref in &net.pins {
            let Some((reference, number)) = pin_ref.split_once('.') else {
                continue;
            };
            let Some(part) = model.part(reference) else {
                continue;
            };
            let Some(pin) = part.pins.iter().find(|p| p.number == number) else {
                continue;
            };
            match pin.kind {
                PinKind::Power => has_power = true,
                PinKind::Ground => has_ground = true,
                _ => {}
            }
        }
        if has_power && has_ground {
            out.push(CheckResult::fail(
                "source_power_pin_direct_short",
                net.name.clone(),
                format!(
                    "net '{}' ties a power pin directly to a ground pin",
                    net.name
                ),
            ));
        }
    }
    out
}

fn check_duplicate_reference(model: &ConstraintModel) -> Vec<CheckResult> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for part in &model.parts {
        *seen.entry(part.reference.as_str()).or_insert(0) += 1;
    }
    seen.into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(reference, count)| {
            CheckResult::fail(
                "source_duplicate_reference",
                reference.to_string(),
                format!(
                    "reference designator '{}' is used by {} parts",
                    reference, count
                ),
            )
        })
        .collect()
}

/// A pin can carry exactly one net. Listing it on two is the intent
/// contradicting itself; downstream the first net silently wins the pad
/// and the second becomes unroutable, which is much harder to diagnose.
fn check_pin_on_multiple_nets(model: &ConstraintModel) -> Vec<CheckResult> {
    let mut nets_of: HashMap<&str, Vec<&str>> = HashMap::new();
    for net in &model.nets {
        for pin in &net.pins {
            nets_of.entry(pin.as_str()).or_default().push(net.name.as_str());
        }
    }
    let mut out: Vec<CheckResult> = nets_of
        .into_iter()
        .filter(|(_, nets)| nets.len() > 1)
        .map(|(pin, nets)| {
            CheckResult::fail(
                "source_pin_multiple_nets",
                pin.to_string(),
                format!("pin is listed on {} nets: {}", nets.len(), nets.join(", ")),
            )
        })
        .collect();
    out.sort_by(|a, b| a.location.cmp(&b.location));
    out
}

fn check_missing_footprint(model: &ConstraintModel) -> Vec<CheckResult> {
    model
        .parts
        .iter()
        .filter(|p| p.package.is_none() && p.footprint.is_none())
        .map(|p| CheckResult {
            check: "source_missing_footprint".into(),
            status: eda_model::CheckStatus::Warn,
            location: Some(p.reference.clone()),
            hint: Some(format!(
                "part '{}' has no package or footprint set",
                p.reference
            )), detail: None
        })
        .collect()
}

/// Every pin that is *on a net* must exist as a pad on the land pattern
/// the part is placed on.
///
/// Two deliberate narrowings, both learned from false positives:
///
/// A footprint with *more* pads than the part has pins is normal and
/// legal -- exposed thermal pads, mounting pads and NC pins the intent
/// never mentions all look like that. Only a pin with no pad is a defect.
///
/// And only a pin carrying a net. `nc_pins.yaml` declares an NC pin 4 on
/// a 3-pad SOT-223 (our built-in models the tab as the oversized pad 2),
/// and the first cut of this check failed it. Nothing routes to an
/// unconnected pin, so a missing pad for one costs nothing; failing it
/// would have made the gate a thing to work around rather than to trust.
///
/// What is left fails rather than warns, because the board is
/// unbuildable: copper reaches a terminal the physical part does not
/// have, every geometric gate passes, and the next place to find it is
/// the factory.
fn check_pin_has_no_pad(model: &ConstraintModel) -> Vec<CheckResult> {
    let mut on_a_net: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for net in &model.nets {
        for p in &net.pins {
            on_a_net.insert(p.as_str());
        }
    }
    let mut out = Vec::new();
    for part in &model.parts {
        let Some(fp) = model.footprint_of(part) else { continue };
        let pads: std::collections::HashSet<&str> = fp.pads.iter().map(|p| p.number.as_str()).collect();
        let missing: Vec<&str> = part
            .pins
            .iter()
            .map(|p| p.number.as_str())
            .filter(|n| !pads.contains(n) && on_a_net.contains(format!("{}.{}", part.reference, n).as_str()))
            .collect();
        if missing.is_empty() {
            continue;
        }
        out.push(CheckResult {
            check: "source_pin_has_no_pad".into(),
            status: eda_model::CheckStatus::Fail,
            location: Some(part.reference.clone()),
            hint: Some(format!(
                "{} routes pin(s) {} that footprint {:?} has no pad for ({} pin(s) against {} pad(s)); copper would reach a terminal the part does not have",
                part.reference,
                missing.join(", "),
                fp.name,
                part.pins.len(),
                fp.pads.len()
            )),
            detail: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_on_two_nets_is_flagged() {
        let model = ConstraintModel {
            parts: vec![crate_test_part("U1", 2)],
            nets: vec![
                eda_model::Net { name: "A".into(), pins: vec!["U1.1".into(), "U1.2".into()] },
                eda_model::Net { name: "B".into(), pins: vec!["U1.2".into()] },
            ],
            ..Default::default()
        };
        let hits: Vec<_> = check_pin_on_multiple_nets(&model);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].location.as_deref(), Some("U1.2"));
    }

    fn crate_test_part(reference: &str, n: usize) -> eda_model::Part {
        eda_model::Part {
            reference: reference.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some("0603".into()),
            footprint: None,
            pins: (1..=n).map(|i| eda_model::Pin { number: i.to_string(), name: None, kind: eda_model::PinKind::Signal }).collect(),
            body_um: None,
            edge: None,
        }
    }
    use eda_model::{Net, Part, Pin};

    fn part(reference: &str, pins: Vec<Pin>) -> Part {
        Part {
            reference: reference.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some("SOT-23".into()),
            footprint: None,
            pins,
            body_um: None,
            edge: None,
        }
    }

    fn pin(number: &str, kind: PinKind) -> Pin {
        Pin {
            number: number.into(),
            name: None,
            kind,
        }
    }

    // --- source_pin_not_connected ---
    #[test]
    fn pin_not_connected_triggers() {
        let model = ConstraintModel {
            parts: vec![part("U1", vec![pin("1", PinKind::Signal)])],
            nets: vec![],
            ..Default::default()
        };
        let res = check_pin_not_connected(&model);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].check, "source_pin_not_connected");
        assert_eq!(res[0].location.as_deref(), Some("U1.1"));
    }

    #[test]
    fn pin_not_connected_absent_when_wired() {
        let model = ConstraintModel {
            parts: vec![part("U1", vec![pin("1", PinKind::Signal)])],
            nets: vec![Net {
                name: "SIG".into(),
                pins: vec!["U1.1".into()],
            }],
            ..Default::default()
        };
        assert!(check_pin_not_connected(&model).is_empty());
    }

    #[test]
    fn nc_pin_not_flagged() {
        let model = ConstraintModel {
            parts: vec![part("U1", vec![pin("1", PinKind::Nc)])],
            nets: vec![],
            ..Default::default()
        };
        assert!(check_pin_not_connected(&model).is_empty());
    }

    // --- source_net_single_pin ---
    #[test]
    fn net_single_pin_triggers() {
        let model = ConstraintModel {
            nets: vec![Net {
                name: "N1".into(),
                pins: vec!["U1.1".into()],
            }],
            ..Default::default()
        };
        let res = check_net_single_pin(&model);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].check, "source_net_single_pin");
    }

    #[test]
    fn net_two_pins_not_flagged() {
        let model = ConstraintModel {
            nets: vec![Net {
                name: "N1".into(),
                pins: vec!["U1.1".into(), "C1.1".into()],
            }],
            ..Default::default()
        };
        assert!(check_net_single_pin(&model).is_empty());
    }

    // --- source_no_ground_net ---
    #[test]
    fn no_ground_net_triggers() {
        let model = ConstraintModel::default();
        let res = check_no_ground_net(&model);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].check, "source_no_ground_net");
        assert_eq!(res[0].status, eda_model::CheckStatus::Warn);
    }

    #[test]
    fn ground_net_present_not_flagged() {
        let model = ConstraintModel {
            nets: vec![Net {
                name: "GND".into(),
                pins: vec![],
            }],
            ..Default::default()
        };
        assert!(check_no_ground_net(&model).is_empty());
    }

    // --- source_no_power_net ---
    #[test]
    fn no_power_net_triggers() {
        let model = ConstraintModel::default();
        let res = check_no_power_net(&model);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].check, "source_no_power_net");
    }

    #[test]
    fn power_net_present_not_flagged() {
        let model = ConstraintModel {
            nets: vec![Net {
                name: "VCC".into(),
                pins: vec![],
            }],
            ..Default::default()
        };
        assert!(check_no_power_net(&model).is_empty());
    }

    // --- source_power_pin_direct_short ---
    #[test]
    fn power_ground_short_triggers() {
        let model = ConstraintModel {
            parts: vec![
                part("U1", vec![pin("1", PinKind::Power)]),
                part("U2", vec![pin("1", PinKind::Ground)]),
            ],
            nets: vec![Net {
                name: "BAD".into(),
                pins: vec!["U1.1".into(), "U2.1".into()],
            }],
            ..Default::default()
        };
        let res = check_power_pin_direct_short(&model);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].check, "source_power_pin_direct_short");
    }

    #[test]
    fn power_alone_not_flagged() {
        let model = ConstraintModel {
            parts: vec![part("U1", vec![pin("1", PinKind::Power)])],
            nets: vec![Net {
                name: "VCC".into(),
                pins: vec!["U1.1".into()],
            }],
            ..Default::default()
        };
        assert!(check_power_pin_direct_short(&model).is_empty());
    }

    // --- source_duplicate_reference ---
    #[test]
    fn duplicate_reference_triggers() {
        let model = ConstraintModel {
            parts: vec![part("U1", vec![]), part("U1", vec![])],
            ..Default::default()
        };
        let res = check_duplicate_reference(&model);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].check, "source_duplicate_reference");
    }

    #[test]
    fn unique_references_not_flagged() {
        let model = ConstraintModel {
            parts: vec![part("U1", vec![]), part("U2", vec![])],
            ..Default::default()
        };
        assert!(check_duplicate_reference(&model).is_empty());
    }

    // --- source_missing_footprint ---
    #[test]
    fn missing_footprint_triggers() {
        let mut p = part("U1", vec![]);
        p.package = None;
        let model = ConstraintModel {
            parts: vec![p],
            ..Default::default()
        };
        let res = check_missing_footprint(&model);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].check, "source_missing_footprint");
        assert_eq!(res[0].status, eda_model::CheckStatus::Warn);
    }

    // --- source_control_pin_unconnected ---
    #[test]
    fn control_pin_unconnected_triggers() {
        let model = ConstraintModel {
            parts: vec![Part {
                reference: "U1".into(),
                mpn: None,
                lcsc: None,
                value: None,
                package: Some("SOT-23".into()),
                footprint: None,
                pins: vec![Pin { number: "3".into(), name: Some("EN".into()), kind: PinKind::Signal }],
                body_um: None,
                edge: None,
            }],
            nets: vec![],
            ..Default::default()
        };
        let res = check_control_pin_unconnected(&model);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].check, "source_control_pin_unconnected");
        assert_eq!(res[0].location.as_deref(), Some("U1.3"));
    }

    #[test]
    fn control_pin_tied_to_a_net_not_flagged() {
        let model = ConstraintModel {
            parts: vec![Part {
                reference: "U1".into(),
                mpn: None,
                lcsc: None,
                value: None,
                package: Some("SOT-23".into()),
                footprint: None,
                pins: vec![
                    Pin { number: "1".into(), name: Some("VIN".into()), kind: PinKind::Power },
                    Pin { number: "3".into(), name: Some("EN".into()), kind: PinKind::Signal },
                ],
                body_um: None,
                edge: None,
            }],
            nets: vec![Net { name: "VIN".into(), pins: vec!["U1.1".into(), "U1.3".into()] }],
            ..Default::default()
        };
        assert!(check_control_pin_unconnected(&model).is_empty());
    }

    #[test]
    fn non_control_signal_pin_unconnected_not_flagged_by_this_check() {
        let model = ConstraintModel {
            parts: vec![part("U1", vec![pin("5", PinKind::Signal)])],
            nets: vec![],
            ..Default::default()
        };
        assert!(check_control_pin_unconnected(&model).is_empty());
    }

    #[test]
    fn footprint_present_not_flagged() {
        let model = ConstraintModel {
            parts: vec![part("U1", vec![])],
            ..Default::default()
        };
        assert!(check_missing_footprint(&model).is_empty());
    }
}

/// The land pattern has to be able to hold the part that was ordered.
///
/// Everything else in this pipeline matches a footprint to a part by
/// *name*, and a name is not a measurement. Two failures that costs
/// nothing to imagine and a board spin to discover:
///
///   - `0603` imperial is 1.6 x 0.8 mm. `0603` metric is 0.6 x 0.3 mm,
///     which is imperial `0201`. Same string, parts 5x apart in size.
///   - A USB-C receptacle and a 16-pin 2.54 mm header both have sixteen
///     pins, so every pin-count check passes and nothing fits.
///
/// DRC cannot see either, because DRC checks copper against copper and
/// never against the physical component.
///
/// `Part::body_um` is what makes this checkable: the body size the
/// distributor reports for that exact MPN, recorded when the part was
/// sourced. A part without it is skipped rather than failed -- the gate
/// reports what it can verify, and an intent that has not sourced its
/// parts yet is an ordinary state, not a defect.
///
/// The tolerance is deliberately loose. A rigorous answer needs IPC land
/// pattern rules and the part's terminal geometry, neither of which is in
/// a distributor feed. `RATIO` only asks whether these two things could
/// plausibly be the same component -- it is built to catch the 5x class
/// of error without inventing authority it does not have. A gate that
/// fires on real parts gets switched off, and then it catches nothing.
const BODY_RATIO: f64 = 3.0;

fn check_footprint_body_mismatch(model: &ConstraintModel) -> Vec<CheckResult> {
    let mut out = Vec::new();
    for part in &model.parts {
        let Some((bw, bh)) = part.body_um else { continue };
        let Some(fp) = model.footprint_of(part) else { continue };
        if fp.pads.is_empty() || bw <= 0 || bh <= 0 {
            continue;
        }
        // Bounding box of the copper the part has to land on.
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for pad in &fp.pads {
            x0 = x0.min(pad.at.0 - pad.size.0 / 2);
            y0 = y0.min(pad.at.1 - pad.size.1 / 2);
            x1 = x1.max(pad.at.0 + pad.size.0 / 2);
            y1 = y1.max(pad.at.1 + pad.size.1 / 2);
        }
        // Compare like with like: a part may be rotated relative to the
        // land pattern, so match long-to-long and short-to-short rather
        // than width-to-width, which would flag every 90-degree part.
        let (pl, ps) = {
            let (a, b) = ((x1 - x0) as f64, (y1 - y0) as f64);
            (a.max(b), a.min(b))
        };
        let (bl, bs) = {
            let (a, b) = (bw as f64, bh as f64);
            (a.max(b), a.min(b))
        };
        let long = pl / bl;
        let short = ps / bs;
        if long > BODY_RATIO || long < 1.0 / BODY_RATIO || short > BODY_RATIO || short < 1.0 / BODY_RATIO {
            out.push(CheckResult {
                check: "source_footprint_body_mismatch".into(),
                status: eda_model::CheckStatus::Fail,
                location: Some(part.reference.clone()),
                hint: Some(format!(
                    "{} is a {:.2} x {:.2} mm part on footprint {:?}, whose pads span {:.2} x {:.2} mm \
                     ({:.1}x by {:.1}x). These cannot be the same component: the package name matched, \
                     the geometry does not.",
                    part.reference,
                    bl / 1000.0,
                    bs / 1000.0,
                    fp.name,
                    pl / 1000.0,
                    ps / 1000.0,
                    long,
                    short
                )),
                detail: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod pad_tests {
    use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};

    fn part_on(reference: &str, package: &str, pins: usize) -> Part {
        Part {
            reference: reference.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some(package.into()),
            footprint: None,
            pins: (1..=pins).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Signal }).collect(),
            body_um: None,
            edge: None,
        }
    }

    fn fails(m: &ConstraintModel) -> Vec<String> {
        super::check_pin_has_no_pad(m).into_iter().filter_map(|c| c.location).collect()
    }

    #[test]
    fn a_routed_pin_with_no_pad_fails() {
        // A 4-pin part on our 3-pad SOT-223, with pin 4 actually carrying
        // a net: copper reaches a terminal the package does not have.
        let mut m = ConstraintModel::default();
        m.parts = vec![part_on("U1", "SOT-223", 4)];
        m.nets = vec![Net { name: "N1".into(), pins: vec!["U1.4".into()] }];
        assert_eq!(fails(&m), vec!["U1"]);
    }

    #[test]
    fn an_unconnected_pin_with_no_pad_is_fine() {
        // The nc_pins.yaml case, and the reason this check is narrowed:
        // nothing routes to an NC pin, so a missing pad costs nothing.
        let mut m = ConstraintModel::default();
        m.parts = vec![part_on("U1", "SOT-223", 4)];
        m.nets = vec![Net { name: "N1".into(), pins: vec!["U1.1".into()] }];
        assert!(fails(&m).is_empty());
    }

    #[test]
    fn a_footprint_with_spare_pads_is_fine() {
        // Thermal and mounting pads mean more pads than pins is normal.
        let mut m = ConstraintModel::default();
        m.parts = vec![part_on("U1", "SOIC-8", 3)];
        m.nets = vec![Net { name: "N1".into(), pins: vec!["U1.1".into(), "U1.2".into(), "U1.3".into()]  }];
        assert!(fails(&m).is_empty());
    }

    #[test]
    fn a_part_with_no_resolvable_footprint_is_left_to_its_own_check() {
        // source_missing_footprint owns that case; reporting it twice
        // just makes the list longer.
        let mut m = ConstraintModel::default();
        let mut p = part_on("U1", "SOT-223", 4);
        p.package = None;
        m.parts = vec![p];
        m.nets = vec![Net { name: "N1".into(), pins: vec!["U1.4".into()] }];
        assert!(fails(&m).is_empty());
    }
}

#[cfg(test)]
mod body_tests {
    use eda_model::{ConstraintModel, Part};

    fn part(reference: &str, package: &str, body: Option<(i64, i64)>) -> Part {
        Part {
            reference: reference.into(),
            mpn: Some("X".into()),
            lcsc: None,
            value: None,
            package: Some(package.into()),
            footprint: None,
            pins: vec![],
            body_um: body,
            edge: None,
        }
    }

    fn fails(p: Part) -> Vec<String> {
        let mut m = ConstraintModel::default();
        m.parts = vec![p];
        super::check_footprint_body_mismatch(&m).into_iter().filter_map(|c| c.location).collect()
    }

    #[test]
    fn a_real_0603_on_an_0603_footprint_passes() {
        // Imperial 0603: 1.6 x 0.8 mm. This must not fire, or the gate
        // gets switched off and then it catches nothing.
        assert!(fails(part("R1", "0603", Some((1600, 800)))).is_empty());
    }

    #[test]
    fn a_real_0402_on_an_0402_footprint_passes() {
        assert!(fails(part("C1", "0402", Some((1000, 500)))).is_empty());
    }

    #[test]
    fn a_metric_0603_on_an_imperial_0603_footprint_fails() {
        // The classic: metric 0603 is 0.6 x 0.3 mm, which is imperial
        // 0201. Same string on the reel and in the library, parts 5x
        // apart, and every name-based check passes.
        assert_eq!(fails(part("R1", "0603", Some((600, 300)))), vec!["R1"]);
    }

    #[test]
    fn a_usb_c_receptacle_on_a_pin_header_fails() {
        // L1's J1. Sixteen pins either way, so the pin-count gate passes
        // it; a USB-C receptacle body is nothing like a 16-way 2.54 mm
        // header, which spans about 40 mm.
        assert_eq!(fails(part("J1", "PINHEADER-16", Some((9000, 7300)))), vec!["J1"]);
    }

    #[test]
    fn a_part_that_has_not_been_sourced_is_skipped_not_failed() {
        // No body size means nothing was verified, which is an ordinary
        // state for an intent mid-design -- not a defect.
        assert!(fails(part("R1", "0603", None)).is_empty());
    }

    #[test]
    fn a_rotated_part_is_not_flagged_for_being_rotated() {
        // Long-to-long, short-to-short: a part described 90 degrees from
        // its land pattern is the same component.
        assert!(fails(part("R1", "0603", Some((800, 1600)))).is_empty());
    }
}

//! Second importer: real Zener (`.zen`) files, evaluated by shelling out to
//! Diode Computers' `pcb` CLI (github.com/diodeinc/pcb) and parsing its
//! structured netlist output into a [`ConstraintModel`].
//!
//! This is wire-compatible with real `.zen` files (unlike [`crate::zen`],
//! the hand-rolled fallback DSL) because it delegates evaluation entirely to
//! the upstream toolchain: `<pcb-binary> build --netlist <path>` runs the
//! real Starlark/Zener evaluator, resolves `@stdlib` / package imports, and
//! prints the resulting schematic as canonical JSON
//! (`pcb_sch::Schematic::to_json`) on stdout. We never link against
//! `pcb-sch` or `pcb-zen-core` — the JSON shape is treated as a small,
//! documented, checked-in-fixture-tested contract instead.
//!
//! Exact invocation: `<bin> build --netlist <path-to-.zen>` (the `--netlist`
//! flag is `hide = true` in pcbc's clap definition — undocumented in
//! `--help`, discovered by reading `crates/pcbc/src/build.rs` upstream — but
//! stable enough to shell out to: `pcb build --netlist foo.zen` is exactly
//! what the real `pcb` shim re-execs into `pcbc build --netlist foo.zen`).
//!
//! # Binary discovery
//!
//! 1. `$PCB_BIN` env var, if set.
//! 2. `which pcb` on `$PATH`.
//! 3. `~/.local/bin/pcb` (the installer's default location).
//!
//! Any failure (binary missing, spawn error, non-zero exit, unparseable
//! output) comes back as `Vec<CheckResult>` — this module never panics.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_model::{CheckResult, Cluster, ConstraintModel, Net, Part, Pin, PinKind};
use serde::Deserialize;

/// Evaluate a real `.zen` file via the `pcb` CLI into a [`ConstraintModel`].
pub fn import_zen_cli(path: &Path) -> Result<ConstraintModel, Vec<CheckResult>> {
    let bin = locate_binary().ok_or_else(|| {
        vec![CheckResult::fail(
            "zen_cli_missing",
            path.display().to_string(),
            "pcb CLI binary not found: set $PCB_BIN, install to $PATH, or install to \
             ~/.local/bin/pcb (see github.com/diodeinc/pcb install.sh)",
        )]
    })?;

    let output = Command::new(&bin)
        .arg("build")
        .arg("--netlist")
        .arg(path)
        .output()
        .map_err(|e| {
            vec![CheckResult::fail(
                "zen_cli_missing",
                path.display().to_string(),
                format!("failed to spawn {}: {e}", bin.display()),
            )]
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let hint = if stderr.trim().is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(vec![CheckResult::fail(
            "zen_cli_eval_error",
            path.display().to_string(),
            hint,
        )]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_netlist_json(&stdout).map_err(|e| {
        vec![CheckResult::fail(
            "zen_cli_bad_output",
            path.display().to_string(),
            format!("could not parse pcb build --netlist output as JSON: {e}"),
        )]
    })
}

fn locate_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PCB_BIN") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(out) = Command::new("which").arg("pcb").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join(".local/bin/pcb");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

// ------------------------------------------------------------- JSON shape
//
// Mirrors (a subset of) `pcb_sch::Schematic` / `pcb_sch::Instance` /
// `pcb_sch::Net` from github.com/diodeinc/pcb `crates/pcb-sch/src/lib.rs`,
// without depending on that crate. `AttributeValue` there is a plain
// `#[derive(Serialize, Deserialize)]` enum with data-carrying variants, so
// serde's default externally-tagged representation applies:
// `{"String": "10k"}`, `{"Number": 20251024.0}`, `{"Array": [...]}`, etc.
// We only care about the `String` case for the attributes we read.

#[derive(Debug, Deserialize)]
struct RawSchematic {
    instances: HashMap<String, RawInstance>,
    nets: HashMap<String, RawNet>,
    #[serde(default)]
    #[allow(dead_code)]
    root_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawInstance {
    kind: String,
    #[serde(default)]
    reference_designator: Option<String>,
    #[serde(default)]
    children: HashMap<String, String>,
    #[serde(default)]
    attributes: HashMap<String, RawAttr>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
enum RawAttr {
    String(String),
    #[allow(dead_code)]
    Number(f64),
    #[allow(dead_code)]
    Boolean(bool),
    #[allow(dead_code)]
    Port(String),
    #[allow(dead_code)]
    Array(Vec<serde_json::Value>),
    #[allow(dead_code)]
    Json(serde_json::Value),
}

impl RawAttr {
    fn as_str(&self) -> Option<&str> {
        match self {
            RawAttr::String(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawNet {
    #[serde(default)]
    #[allow(dead_code)]
    name: Option<String>,
    #[serde(default)]
    ports: Vec<String>,
}

/// Parse `pcb build --netlist` stdout into a [`ConstraintModel`].
///
/// Split out from [`import_zen_cli`] so it can be unit-tested against a
/// checked-in captured JSON sample without needing the `pcb` binary.
fn parse_netlist_json(json: &str) -> Result<ConstraintModel, String> {
    let raw: RawSchematic = serde_json::from_str(json).map_err(|e| e.to_string())?;

    // Assign a stable reference designator per instance ref for every
    // Component instance; used both for Part.reference and for pin
    // resolution below.
    let mut refdes_of: HashMap<&str, String> = HashMap::new();
    for (inst_ref, inst) in &raw.instances {
        if inst.kind == "Component" {
            let refdes = inst
                .reference_designator
                .clone()
                .unwrap_or_else(|| last_path_segment(inst_ref).to_string());
            refdes_of.insert(inst_ref.as_str(), refdes);
        }
    }

    // Parts.
    let mut parts: Vec<Part> = raw
        .instances
        .iter()
        .filter(|(_, inst)| inst.kind == "Component")
        .map(|(inst_ref, inst)| {
            let reference = refdes_of.get(inst_ref.as_str()).cloned().unwrap();
            let mut pin_names: Vec<&String> = inst.children.keys().collect();
            pin_names.sort();
            let pins = pin_names
                .into_iter()
                .map(|number| Pin {
                    number: number.clone(),
                    name: None,
                    kind: infer_pin_kind(number),
                })
                .collect();
            Part {
                reference,
                mpn: attr_str(inst, "mpn"),
                lcsc: attr_str(inst, "lcsc"),
                value: attr_str(inst, "value"),
                package: attr_str(inst, "package"),
                footprint: attr_str(inst, "footprint"),
                pins,
                body_um: None,
                edge: None,
            }
        })
        .collect();
    parts.sort_by(|a, b| a.reference.cmp(&b.reference));

    // Nets: resolve each port InstanceRef to "REFDES.PIN" via longest-prefix
    // match against known Component instance refs (mirrors
    // `Schematic::component_ref_and_pin_for_port` upstream).
    let mut nets: Vec<Net> = raw
        .nets
        .iter()
        .map(|(net_name, net)| {
            let mut pins: Vec<String> = net
                .ports
                .iter()
                .filter_map(|port_ref| resolve_pin(&refdes_of, port_ref))
                .collect();
            pins.sort();
            Net {
                name: net_name.clone(),
                pins,
            }
        })
        .collect();
    nets.sort_by(|a, b| a.name.cmp(&b.name));

    // Clusters: one per Module instance whose transitive descendants include
    // 2+ Components. Single-component wrapper modules (e.g. a generic
    // `Resistor(...)` call, which Zener represents as a Module wrapping one
    // Component) are skipped as trivial/non-informative.
    let mut clusters: Vec<Cluster> = Vec::new();
    for (inst_ref, inst) in &raw.instances {
        if inst.kind != "Module" || Some(inst_ref.as_str()) == raw.root_ref.as_deref() {
            continue;
        }
        let mut members = Vec::new();
        collect_component_refdes(&raw.instances, &refdes_of, inst_ref, &mut members);
        members.sort();
        members.dedup();
        if members.len() >= 2 {
            let anchor = members[0].clone();
            clusters.push(Cluster {
                anchor,
                members,
                orientation: Default::default(),
            });
        }
    }
    clusters.sort_by(|a, b| a.anchor.cmp(&b.anchor));

    Ok(ConstraintModel {
        parts,
        nets,
        clusters,
        placement_rules: Vec::new(),
        stackup: None,
        impedance_targets: Vec::new(),
        footprints: Vec::new(),
        board: Default::default(),
        allow: Default::default(),
        solver: Default::default(),
    })
}

fn attr_str(inst: &RawInstance, key: &str) -> Option<String> {
    inst.attributes.get(key).and_then(RawAttr::as_str).map(String::from)
}

fn last_path_segment(instance_ref: &str) -> &str {
    instance_ref.rsplit('.').next().unwrap_or(instance_ref)
}

/// Best-effort pin-kind inference from the pin/port name alone (the raw
/// netlist JSON does not carry an electrical pin-kind for ports/pads — see
/// module doc comment "what's lost").
fn infer_pin_kind(pin_name: &str) -> PinKind {
    match pin_name.to_ascii_uppercase().as_str() {
        "GND" | "VSS" | "AGND" | "DGND" => PinKind::Ground,
        "VCC" | "VDD" | "VIN" | "VOUT" | "VBUS" | "V+" | "V-" => PinKind::Power,
        "NC" => PinKind::Nc,
        _ => PinKind::Signal,
    }
}

/// Resolve a port `InstanceRef` string to `"REFDES.PIN"` by finding the
/// longest known Component instance-ref that is a path-prefix of it.
fn resolve_pin(refdes_of: &HashMap<&str, String>, port_ref: &str) -> Option<String> {
    let mut best: Option<(&str, &str)> = None; // (component_ref, refdes)
    for (comp_ref, refdes) in refdes_of {
        if let Some(rest) = port_ref.strip_prefix(*comp_ref) {
            if (rest.is_empty() || rest.starts_with('.'))
                && best.is_none_or(|(bk, _)| comp_ref.len() > bk.len())
            {
                best = Some((comp_ref, refdes.as_str()));
            }
        }
    }
    let (comp_ref, refdes) = best?;
    let pin = port_ref[comp_ref.len()..].trim_start_matches('.');
    if pin.is_empty() {
        return None;
    }
    Some(format!("{refdes}.{pin}"))
}

/// Recursively collect refdes of every Component reachable from
/// `instance_ref` through its `children` map.
fn collect_component_refdes(
    instances: &HashMap<String, RawInstance>,
    refdes_of: &HashMap<&str, String>,
    instance_ref: &str,
    out: &mut Vec<String>,
) {
    let Some(inst) = instances.get(instance_ref) else {
        return;
    };
    if inst.kind == "Component" {
        if let Some(refdes) = refdes_of.get(instance_ref) {
            out.push(refdes.clone());
        }
        return;
    }
    for child_ref in inst.children.values() {
        collect_component_refdes(instances, refdes_of, child_ref, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/zen_cli")
            .join(name)
    }

    /// Parses a real `pcb build --netlist VoltageDivider.zen` capture
    /// (checked in, no `pcb` binary required) into the expected
    /// ConstraintModel shape.
    #[test]
    fn parses_captured_voltage_divider_netlist() {
        let json = std::fs::read_to_string(fixture("voltage_divider_netlist.json")).unwrap();
        let model = parse_netlist_json(&json).expect("parse should succeed");

        let mut refs: Vec<&str> = model.parts.iter().map(|p| p.reference.as_str()).collect();
        refs.sort();
        assert_eq!(refs, vec!["R1", "R2"]);

        let r1 = model.part("R1").unwrap();
        assert_eq!(r1.value.as_deref(), Some("10k"));
        assert_eq!(r1.package.as_deref(), Some("0603"));
        assert!(r1.footprint.as_deref().unwrap().contains("R_0603_1608Metric"));
        let mut pin_numbers: Vec<&str> = r1.pins.iter().map(|p| p.number.as_str()).collect();
        pin_numbers.sort();
        assert_eq!(pin_numbers, vec!["1", "2"]);

        let mut net_names: Vec<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
        net_names.sort();
        assert_eq!(net_names, vec!["GND", "VIN", "VOUT"]);

        let vout = model.nets.iter().find(|n| n.name == "VOUT").unwrap();
        assert_eq!(vout.pins, vec!["R1.2", "R2.1"]);
        let vin = model.nets.iter().find(|n| n.name == "VIN").unwrap();
        assert_eq!(vin.pins, vec!["R1.1"]);
        let gnd = model.nets.iter().find(|n| n.name == "GND").unwrap();
        assert_eq!(gnd.pins, vec!["R2.2"]);
    }

    #[test]
    fn rejects_garbage_json() {
        assert!(parse_netlist_json("not json").is_err());
    }

    fn pcb_bin() -> Option<PathBuf> {
        locate_binary()
    }

    /// End-to-end: real `.zen` fixture -> real `pcb` CLI -> ConstraintModel.
    /// Ignored by default since it needs a working `pcb`/`pcbc` binary
    /// (plus its adjacent `lib/std`) on this machine.
    #[test]
    #[ignore = "requires a working pcb/pcbc binary; run with `cargo test -- --ignored`"]
    fn imports_real_voltage_divider_fixture() {
        let Some(_bin) = pcb_bin() else {
            panic!("PCB_BIN/which pcb/~/.local/bin/pcb not found");
        };
        let path = fixture("VoltageDivider.zen");
        let model = import_zen_cli(&path).expect("import should succeed");
        assert_eq!(model.parts.len(), 2);
        assert_eq!(model.nets.len(), 3);
    }

    /// End-to-end error path: a syntactically invalid `.zen` file should
    /// come back as a `zen_cli_eval_error` CheckResult with stderr in the
    /// hint, never a panic.
    #[test]
    #[ignore = "requires a working pcb/pcbc binary; run with `cargo test -- --ignored`"]
    fn reports_eval_error_for_broken_fixture() {
        if pcb_bin().is_none() {
            panic!("PCB_BIN/which pcb/~/.local/bin/pcb not found");
        }
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/zen_cli_bad/Bad.zen");
        let errs = import_zen_cli(&path).expect_err("expected eval error");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].check, "zen_cli_eval_error");
        assert!(!errs[0].hint.as_deref().unwrap_or_default().is_empty());
    }

    #[test]
    fn missing_binary_reports_check_result() {
        // Force a nonexistent binary path so this test is deterministic
        // regardless of what's installed on the host.
        std::env::set_var("PCB_BIN", "/definitely/not/a/real/path/pcb");
        let path = PathBuf::from("whatever.zen");
        let errs = import_zen_cli(&path).expect_err("expected missing-binary error");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].check, "zen_cli_missing");
        std::env::remove_var("PCB_BIN");
    }
}

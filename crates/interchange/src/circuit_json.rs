//! Circuit JSON writer.
//!
//! Ports the small slice of the tscircuit `circuit-json` schema
//! (github.com/tscircuit/circuit-json) that our IR can populate faithfully:
//! source_component_base (+ simple_resistor / simple_capacitor variants),
//! source_port, source_net, source_trace, schematic_component,
//! schematic_port, schematic_wire, and — when placement/routing are present —
//! pcb_component, pcb_port, pcb_trace, pcb_smtpad.
//!
//! IDs are derived deterministically from our stable IDs (reference
//! designators, net names), never from insertion order or randomness: two
//! runs over the same `(Design, ConstraintModel)` produce byte-identical
//! arrays.
//!
//! Known gap: our IR carries no symbol-library pin geometry (`SymbolInstance`
//! has only a component `at`/`rot`, no per-pin offsets), so `schematic_port`
//! and `pcb_port` centers are approximated at the owning component's center.
//! Real per-pin offsets are a candidate IR/model addition (see crate docs).

use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel, Part};
use serde_json::{json, Value};

fn um_to_mm(um: i64) -> f64 {
    (um as f64) / 1000.0
}

/// ftype + numeric-ish value guess for the simple passive variants. This is
/// a heuristic over `Part.reference`/`Part.value` — the spec's richer
/// variant set (inductor, diode, etc.) is not modeled here; anything that
/// doesn't look like R/C falls back to a generic `simple_bug`.
fn passive_variant(part: &Part) -> (&'static str, Option<&'static str>) {
    if part.reference.starts_with('R') {
        ("simple_resistor", Some("resistance"))
    } else if part.reference.starts_with('C') {
        ("simple_capacitor", Some("capacitance"))
    } else {
        ("simple_bug", None)
    }
}

pub fn to_circuit_json(design: &Design, model: &ConstraintModel) -> Result<Value, Vec<CheckResult>> {
    let mut elements: Vec<Value> = Vec::new();

    // Deterministic ordering: sort by stable ID.
    let mut parts: Vec<&Part> = model.parts.iter().collect();
    parts.sort_by(|a, b| a.reference.cmp(&b.reference));

    let mut nets = model.nets.clone();
    nets.sort_by(|a, b| a.name.cmp(&b.name));

    // reference -> source_component_id
    let mut component_id_of: std::collections::BTreeMap<&str, String> = Default::default();
    for (i, part) in parts.iter().enumerate() {
        component_id_of.insert(part.reference.as_str(), format!("source_component_{i}"));
    }

    // source_component_base (+ passive variant fields)
    for (i, part) in parts.iter().enumerate() {
        let id = format!("source_component_{i}");
        let (ftype, value_field) = passive_variant(part);
        let mut obj = json!({
            "type": "source_component",
            "source_component_id": id,
            "ftype": ftype,
            "name": part.reference,
        });
        if let Some(mpn) = &part.mpn {
            obj["manufacturer_part_number"] = json!(mpn);
        }
        if let (Some(field), Some(value)) = (value_field, &part.value) {
            obj[field] = json!(value);
        }
        elements.push(obj);
    }

    // "REF.PIN" -> source_port_id, in a deterministic (component, pin) order
    let mut port_id_of: std::collections::BTreeMap<String, String> = Default::default();
    let mut port_counter = 0usize;
    for part in &parts {
        let comp_id = &component_id_of[part.reference.as_str()];
        let mut pins = part.pins.clone();
        pins.sort_by(|a, b| a.number.cmp(&b.number));
        for pin in &pins {
            let port_id = format!("source_port_{port_counter}");
            port_counter += 1;
            let key = format!("{}.{}", part.reference, pin.number);
            port_id_of.insert(key, port_id.clone());
            let mut obj = json!({
                "type": "source_port",
                "source_port_id": port_id,
                "source_component_id": comp_id,
                "pin_number": pin.number,
            });
            if let Some(name) = &pin.name {
                obj["name"] = json!(name);
            }
            elements.push(obj);
        }
    }

    // source_net + source_trace
    let mut net_id_of: std::collections::BTreeMap<&str, String> = Default::default();
    for (i, net) in nets.iter().enumerate() {
        let id = format!("source_net_{i}");
        net_id_of.insert(net.name.as_str(), id.clone());
        elements.push(json!({
            "type": "source_net",
            "source_net_id": id,
            "name": net.name,
        }));
    }
    for (i, net) in nets.iter().enumerate() {
        let mut pin_refs = net.pins.clone();
        pin_refs.sort();
        let port_ids: Vec<String> = pin_refs
            .iter()
            .filter_map(|p| port_id_of.get(p).cloned())
            .collect();
        elements.push(json!({
            "type": "source_trace",
            "source_trace_id": format!("source_trace_{i}"),
            "source_net_id": net_id_of[net.name.as_str()],
            "connected_source_port_ids": port_ids,
        }));
    }

    // schematic_component / schematic_port (approximated at component center
    // — see module docs) — only when a schematic section exists.
    if let Some(schematic) = &design.schematic {
        let mut symbols = schematic.symbols.clone();
        symbols.sort_by(|a, b| a.id.cmp(&b.id));
        for (i, sym) in symbols.iter().enumerate() {
            let comp_id = format!("schematic_component_{i}");
            elements.push(json!({
                "type": "schematic_component",
                "schematic_component_id": comp_id,
                "source_component_id": component_id_of.get(sym.id.as_str()),
                "center": { "x": um_to_mm(sym.at.x), "y": um_to_mm(sym.at.y) },
                "rotation": sym.rot,
                "mirrored": sym.mirrored,
            }));
            if let Some(part) = model.part(&sym.id) {
                let mut pins = part.pins.clone();
                pins.sort_by(|a, b| a.number.cmp(&b.number));
                for (j, pin) in pins.iter().enumerate() {
                    let key = format!("{}.{}", sym.id, pin.number);
                    elements.push(json!({
                        "type": "schematic_port",
                        "schematic_port_id": format!("schematic_port_{i}_{j}"),
                        "source_port_id": port_id_of.get(&key),
                        "schematic_component_id": comp_id,
                        "center": { "x": um_to_mm(sym.at.x), "y": um_to_mm(sym.at.y) },
                    }));
                }
            }
        }

        let mut wires = schematic.wires.clone();
        wires.sort_by(|a, b| (&a.net, a.pts.first()).cmp(&(&b.net, b.pts.first())));
        for (i, wire) in wires.iter().enumerate() {
            elements.push(json!({
                "type": "schematic_wire",
                "schematic_wire_id": format!("schematic_wire_{i}"),
                "source_net_id": net_id_of.get(wire.net.as_str()),
                "route": wire.pts.iter().map(|p| json!({"x": um_to_mm(p.x), "y": um_to_mm(p.y)})).collect::<Vec<_>>(),
            }));
        }
    }

    // pcb_component / pcb_port / pcb_smtpad — only when placement exists.
    if let Some(placement) = &design.placement {
        let mut fps = placement.footprints.clone();
        fps.sort_by(|a, b| a.id.cmp(&b.id));
        for (i, fp) in fps.iter().enumerate() {
            let pcb_comp_id = format!("pcb_component_{i}");
            let layer = match fp.side {
                eda_model::ir::Side::Top => "top",
                eda_model::ir::Side::Bottom => "bottom",
            };
            elements.push(json!({
                "type": "pcb_component",
                "pcb_component_id": pcb_comp_id,
                "source_component_id": component_id_of.get(fp.id.as_str()),
                "center": { "x": um_to_mm(fp.at.x), "y": um_to_mm(fp.at.y) },
                "rotation": fp.rot,
                "layer": layer,
            }));
            if let Some(part) = model.part(&fp.id) {
                let Some(footprint) = model.footprint_of(part) else {
                    return Err(vec![CheckResult::fail(
                        "circuit_json_footprint",
                        &fp.id,
                        "part has no resolvable footprint geometry",
                    )]);
                };
                let rad = (fp.rot as f64) / 1000.0 * std::f64::consts::PI / 180.0;
                let (sin, cos) = rad.sin_cos();
                let mirror = if fp.side == eda_model::ir::Side::Bottom { -1.0 } else { 1.0 };
                let mut pads = footprint.pads.clone();
                pads.sort_by(|a, b| a.number.cmp(&b.number));
                for (j, pad) in pads.iter().enumerate() {
                    let key = format!("{}.{}", fp.id, pad.number);
                    let lx = pad.at.0 as f64 * mirror;
                    let ly = pad.at.1 as f64;
                    let x = fp.at.x as f64 + lx * cos - ly * sin;
                    let y = fp.at.y as f64 + lx * sin + ly * cos;
                    let (w, h) = (pad.size.0 as f64, pad.size.1 as f64);
                    let bw = w * cos.abs() + h * sin.abs();
                    let bh = w * sin.abs() + h * cos.abs();
                    elements.push(json!({
                        "type": "pcb_port",
                        "pcb_port_id": format!("pcb_port_{i}_{j}"),
                        "source_port_id": port_id_of.get(&key),
                        "pcb_component_id": pcb_comp_id,
                        "x": um_to_mm(x.round() as i64),
                        "y": um_to_mm(y.round() as i64),
                        "layer": layer,
                    }));
                    let shape = match pad.shape {
                        eda_model::PadShape::Circle => "circle",
                        _ => "rect",
                    };
                    if pad.kind == eda_model::PadKind::ThroughHole {
                        elements.push(json!({
                            "type": "pcb_plated_hole",
                            "pcb_plated_hole_id": format!("pcb_plated_hole_{i}_{j}"),
                            "pcb_component_id": pcb_comp_id,
                            "pcb_port_id": format!("pcb_port_{i}_{j}"),
                            "x": um_to_mm(x.round() as i64),
                            "y": um_to_mm(y.round() as i64),
                            "shape": "circle",
                            "outer_diameter": um_to_mm(bw.round() as i64),
                            "hole_diameter": um_to_mm(pad.drill.unwrap_or(pad.size.0 / 2)),
                            "layers": ["top", "bottom"],
                        }));
                    } else {
                        elements.push(json!({
                            "type": "pcb_smtpad",
                            "pcb_smtpad_id": format!("pcb_smtpad_{i}_{j}"),
                            "pcb_component_id": pcb_comp_id,
                            "pcb_port_id": format!("pcb_port_{i}_{j}"),
                            "x": um_to_mm(x.round() as i64),
                            "y": um_to_mm(y.round() as i64),
                            "layer": layer,
                            "shape": shape,
                            "width": um_to_mm(bw.round() as i64),
                            "height": um_to_mm(bh.round() as i64),
                        }));
                    }
                }
            }
        }
    }

    if let Some(routing) = &design.routing {
        let mut tracks = routing.tracks.clone();
        tracks.sort_by(|a, b| (&a.net, &a.layer, a.pts.first()).cmp(&(&b.net, &b.layer, b.pts.first())));
        for (i, track) in tracks.iter().enumerate() {
            elements.push(json!({
                "type": "pcb_trace",
                "pcb_trace_id": format!("pcb_trace_{i}"),
                "source_net_id": net_id_of.get(track.net.as_str()),
                "route": track.pts.iter().map(|p| json!({
                    "x": um_to_mm(p.x), "y": um_to_mm(p.y), "layer": track.layer,
                })).collect::<Vec<_>>(),
                "width": um_to_mm(track.width),
            }));
        }
    }

    Ok(json!(elements))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Point, SchematicSection, SymbolInstance, Provenance};
    use eda_model::{Net, Pin, PinKind};

    fn model() -> ConstraintModel {
        ConstraintModel {
            parts: vec![
                Part {
                    reference: "R1".into(),
                    mpn: None,
                    value: Some("10k".into()),
                    footprint: None,
                    package: Some("0603".into()),
                    pins: vec![
                        Pin { number: "1".into(), name: None, kind: PinKind::Passive },
                        Pin { number: "2".into(), name: None, kind: PinKind::Passive },
                    ],
                },
                Part {
                    reference: "C1".into(),
                    mpn: None,
                    value: Some("100nF".into()),
                    footprint: None,
                    package: Some("0603".into()),
                    pins: vec![
                        Pin { number: "1".into(), name: None, kind: PinKind::Passive },
                        Pin { number: "2".into(), name: None, kind: PinKind::Passive },
                    ],
                },
            ],
            nets: vec![Net { name: "VIN".into(), pins: vec!["R1.1".into(), "C1.1".into()] }],
            ..Default::default()
        }
    }

    fn design() -> Design {
        Design {
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(SchematicSection {
                symbols: vec![
                    SymbolInstance { id: "R1".into(), at: Point { x: 1_000, y: 2_000 }, rot: 0, mirrored: false },
                    SymbolInstance { id: "C1".into(), at: Point { x: 3_000, y: 2_000 }, rot: 0, mirrored: false },
                ],
                wires: vec![],
                labels: vec![],
            }),
            placement: None,
            routing: None,
        }
    }

    #[test]
    fn element_counts_match_model() {
        let m = model();
        let d = design();
        let v = to_circuit_json(&d, &m).unwrap();
        let arr = v.as_array().unwrap();
        let source_components = arr.iter().filter(|e| e["type"] == "source_component").count();
        assert_eq!(source_components, 2);
        let schematic_components = arr.iter().filter(|e| e["type"] == "schematic_component").count();
        assert_eq!(schematic_components, 2);
        let source_ports = arr.iter().filter(|e| e["type"] == "source_port").count();
        assert_eq!(source_ports, 4);
    }

    #[test]
    fn deterministic_across_runs() {
        let m = model();
        let d = design();
        let v1 = to_circuit_json(&d, &m).unwrap();
        let v2 = to_circuit_json(&d, &m).unwrap();
        assert_eq!(serde_json::to_string(&v1).unwrap(), serde_json::to_string(&v2).unwrap());
    }

    #[test]
    fn ports_reference_their_components() {
        let m = model();
        let d = design();
        let v = to_circuit_json(&d, &m).unwrap();
        let arr = v.as_array().unwrap();
        let r1_id = arr.iter().find(|e| e["type"] == "source_component" && e["name"] == "R1").unwrap()["source_component_id"].clone();
        let ports: Vec<_> = arr.iter().filter(|e| e["type"] == "source_port" && e["source_component_id"] == r1_id).collect();
        assert_eq!(ports.len(), 2);
    }

    #[test]
    fn json_serializes_stably() {
        let m = model();
        let d = design();
        let v = to_circuit_json(&d, &m).unwrap();
        let s = serde_json::to_string(&v).unwrap();
        let back: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v, back);
    }
}

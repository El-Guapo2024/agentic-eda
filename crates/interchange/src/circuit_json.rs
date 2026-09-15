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

/// Map our KiCad-style stackup layer names ("F.Cu", "B.Cu", "In1.Cu", ...)
/// to the official circuit-json `layer_ref` enum ("top", "bottom",
/// "inner1", ...).
fn layer_ref(layer: &str) -> String {
    match layer {
        "F.Cu" => "top".to_string(),
        "B.Cu" => "bottom".to_string(),
        other => {
            if let Some(n) = other.strip_prefix("In").and_then(|s| s.strip_suffix(".Cu")) {
                format!("inner{n}")
            } else {
                other.to_lowercase()
            }
        }
    }
}

/// ftype + numeric-ish value guess for the simple passive variants. This is
/// a heuristic over `Part.reference`/`Part.value`. The official schema's
/// `any_source_component` union has no generic/untyped fallback member, so
/// anything that doesn't look like R/C is emitted as `simple_chip` — the
/// schema's catch-all for ICs and other multi-pin parts.
fn passive_variant(part: &Part) -> (&'static str, Option<&'static str>) {
    if part.reference.starts_with('R') {
        ("simple_resistor", Some("resistance"))
    } else if part.reference.starts_with('C') {
        ("simple_capacitor", Some("capacitance"))
    } else {
        ("simple_chip", None)
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
            // `name` is required by the schema; fall back to the pin
            // number when the IR has no symbolic pin name.
            let name = pin.name.clone().unwrap_or_else(|| pin.number.clone());
            let mut obj = json!({
                "type": "source_port",
                "source_port_id": port_id,
                "source_component_id": comp_id,
                "name": name,
            });
            // `pin_number` is typed as a schema number, not a string — only
            // emit it when the pin designator actually parses as one (e.g.
            // "1", not "A1" on a BGA).
            if let Ok(n) = pin.number.parse::<f64>() {
                obj["pin_number"] = json!(n);
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
            // We have no source_group concept in the IR; the schema
            // requires the field but allows it empty.
            "member_source_group_ids": Vec::<String>::new(),
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
            "connected_source_port_ids": port_ids,
            "connected_source_net_ids": [net_id_of[net.name.as_str()].clone()],
        }));
    }

    // schematic_component / schematic_port (approximated at component center
    // — see module docs) — only when a schematic section exists.
    if let Some(schematic) = &design.schematic {
        let mut symbols = schematic.symbols.clone();
        symbols.sort_by(|a, b| a.id.cmp(&b.id));
        for (i, sym) in symbols.iter().enumerate() {
            let comp_id = format!("schematic_component_{i}");
            // Our IR has no per-symbol pin geometry (see module docs), so
            // there is no real bounding box to report. `size` is required
            // by the schema; approximate with a fixed placeholder box —
            // this is a known gap, not a measured value.
            elements.push(json!({
                "type": "schematic_component",
                "schematic_component_id": comp_id,
                "source_component_id": component_id_of.get(sym.id.as_str()).cloned().unwrap_or_default(),
                "center": { "x": um_to_mm(sym.at.x), "y": um_to_mm(sym.at.y) },
                "size": { "width": 1.0, "height": 1.0 },
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
                        "source_port_id": port_id_of.get(&key).cloned().unwrap_or_default(),
                        "schematic_component_id": comp_id,
                        "center": { "x": um_to_mm(sym.at.x), "y": um_to_mm(sym.at.y) },
                    }));
                }
            }
        }

        // The official schema has no `schematic_wire` element — routed
        // schematic connections are `schematic_trace`s, built from
        // consecutive-point edges.
        let mut wires = schematic.wires.clone();
        wires.sort_by(|a, b| (&a.net, a.pts.first()).cmp(&(&b.net, b.pts.first())));
        for (i, wire) in wires.iter().enumerate() {
            let edges: Vec<Value> = wire
                .pts
                .windows(2)
                .map(|w| {
                    json!({
                        "from": { "x": um_to_mm(w[0].x), "y": um_to_mm(w[0].y) },
                        "to": { "x": um_to_mm(w[1].x), "y": um_to_mm(w[1].y) },
                    })
                })
                .collect();
            let source_trace_id = nets
                .iter()
                .position(|n| n.name == wire.net)
                .map(|idx| format!("source_trace_{idx}"));
            let mut obj = json!({
                "type": "schematic_trace",
                "schematic_trace_id": format!("schematic_trace_{i}"),
                "edges": edges,
                "junctions": Vec::<Value>::new(),
            });
            if let Some(id) = source_trace_id {
                obj["source_trace_id"] = json!(id);
            }
            elements.push(obj);
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
            let Some(part) = model.part(&fp.id) else {
                return Err(vec![CheckResult::fail(
                    "circuit_json_footprint",
                    &fp.id,
                    "placed footprint has no matching part",
                )]);
            };
            let Some(footprint) = model.footprint_of(part) else {
                return Err(vec![CheckResult::fail(
                    "circuit_json_footprint",
                    &fp.id,
                    "part has no resolvable footprint geometry",
                )]);
            };
            // `width`/`height` are required by the schema; use the
            // footprint's courtyard when present, else the pad bounding
            // box (both in the local, unrotated frame).
            let (width_um, height_um) = footprint.courtyard.unwrap_or_else(|| {
                let mut min_x = i64::MAX;
                let mut max_x = i64::MIN;
                let mut min_y = i64::MAX;
                let mut max_y = i64::MIN;
                for pad in &footprint.pads {
                    let hw = pad.size.0 / 2;
                    let hh = pad.size.1 / 2;
                    min_x = min_x.min(pad.at.0 - hw);
                    max_x = max_x.max(pad.at.0 + hw);
                    min_y = min_y.min(pad.at.1 - hh);
                    max_y = max_y.max(pad.at.1 + hh);
                }
                if min_x > max_x {
                    (0, 0)
                } else {
                    (max_x - min_x, max_y - min_y)
                }
            });
            elements.push(json!({
                "type": "pcb_component",
                "pcb_component_id": pcb_comp_id,
                "source_component_id": component_id_of.get(fp.id.as_str()).cloned().unwrap_or_default(),
                "center": { "x": um_to_mm(fp.at.x), "y": um_to_mm(fp.at.y) },
                "rotation": fp.rot,
                "layer": layer,
                "width": um_to_mm(width_um),
                "height": um_to_mm(height_um),
            }));
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
                let source_port_id = port_id_of.get(&key).cloned().unwrap_or_default();
                let pcb_port_id = format!("pcb_port_{i}_{j}");
                elements.push(json!({
                    "type": "pcb_port",
                    "pcb_port_id": pcb_port_id,
                    "source_port_id": source_port_id,
                    "pcb_component_id": pcb_comp_id,
                    "x": um_to_mm(x.round() as i64),
                    "y": um_to_mm(y.round() as i64),
                    "layers": [layer],
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
                        "pcb_port_id": pcb_port_id,
                        "x": um_to_mm(x.round() as i64),
                        "y": um_to_mm(y.round() as i64),
                        "shape": "circle",
                        "outer_diameter": um_to_mm(bw.round() as i64),
                        "hole_diameter": um_to_mm(pad.drill.expect("through-hole pad without a drill passed Footprint::validate")),
                        "layers": ["top", "bottom"],
                    }));
                } else if shape == "circle" {
                    elements.push(json!({
                        "type": "pcb_smtpad",
                        "pcb_smtpad_id": format!("pcb_smtpad_{i}_{j}"),
                        "pcb_component_id": pcb_comp_id,
                        "pcb_port_id": pcb_port_id,
                        "x": um_to_mm(x.round() as i64),
                        "y": um_to_mm(y.round() as i64),
                        "layer": layer,
                        "shape": "circle",
                        "radius": um_to_mm((bw.max(bh) / 2.0).round() as i64),
                    }));
                } else {
                    elements.push(json!({
                        "type": "pcb_smtpad",
                        "pcb_smtpad_id": format!("pcb_smtpad_{i}_{j}"),
                        "pcb_component_id": pcb_comp_id,
                        "pcb_port_id": pcb_port_id,
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

    if let Some(routing) = &design.routing {
        let mut tracks = routing.tracks.clone();
        tracks.sort_by(|a, b| (&a.net, &a.layer, a.pts.first()).cmp(&(&b.net, &b.layer, b.pts.first())));
        for (i, track) in tracks.iter().enumerate() {
            let source_trace_id = nets
                .iter()
                .position(|n| n.name == track.net)
                .map(|idx| format!("source_trace_{idx}"));
            let mut obj = json!({
                "type": "pcb_trace",
                "pcb_trace_id": format!("pcb_trace_{i}"),
                "route": track.pts.iter().map(|p| json!({
                    "route_type": "wire",
                    "x": um_to_mm(p.x),
                    "y": um_to_mm(p.y),
                    "width": um_to_mm(track.width),
                    "layer": layer_ref(&track.layer),
                })).collect::<Vec<_>>(),
            });
            if let Some(id) = source_trace_id {
                obj["source_trace_id"] = json!(id);
            }
            elements.push(obj);
        }

        let mut vias = routing.vias.clone();
        vias.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));
        for (i, via) in vias.iter().enumerate() {
            let source_trace_id = nets
                .iter()
                .position(|n| n.name == via.net)
                .map(|idx| format!("source_trace_{idx}"));
            let mut obj = json!({
                "type": "pcb_via",
                "pcb_via_id": format!("pcb_via_{i}"),
                "x": um_to_mm(via.at.x),
                "y": um_to_mm(via.at.y),
                "outer_diameter": um_to_mm(via.diameter),
                "hole_diameter": um_to_mm(via.drill),
                "layers": [layer_ref(&via.from_layer), layer_ref(&via.to_layer)],
            });
            if let Some(id) = source_trace_id {
                obj["source_trace_id"] = json!(id);
            }
            elements.push(obj);
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
                    edge: None,
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
                    edge: None,
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

    // The official `any_source_component` union has no bare/generic member —
    // every variant requires a specific `ftype` literal. Anything that
    // isn't a recognized R/C must fall back to a member the schema actually
    // accepts ("simple_chip"), not an invented ftype like "simple_bug".
    #[test]
    fn unknown_part_falls_back_to_schema_valid_ftype() {
        let (ftype, _) = passive_variant(&Part {
            reference: "U1".into(),
            mpn: None,
            value: None,
            footprint: None,
            package: None,
            pins: vec![],
            edge: None,
        });
        assert_eq!(ftype, "simple_chip");
    }

    // `source_port.name` is a required string in the schema (not optional);
    // when the IR pin has no symbolic name we must still emit one.
    #[test]
    fn source_port_always_has_a_name() {
        let m = model();
        let d = design();
        let v = to_circuit_json(&d, &m).unwrap();
        let arr = v.as_array().unwrap();
        for port in arr.iter().filter(|e| e["type"] == "source_port") {
            assert!(port["name"].is_string(), "source_port missing required name: {port}");
        }
    }

    // `source_net.member_source_group_ids` and
    // `source_trace.connected_source_net_ids` are required arrays in the
    // schema; our IR has no source-group concept, but the fields must
    // still be present (empty is fine for the net; the trace must link
    // back to its own net id).
    #[test]
    fn source_net_and_trace_have_required_arrays() {
        let m = model();
        let d = design();
        let v = to_circuit_json(&d, &m).unwrap();
        let arr = v.as_array().unwrap();
        for net in arr.iter().filter(|e| e["type"] == "source_net") {
            assert!(net["member_source_group_ids"].is_array());
        }
        let trace = arr.iter().find(|e| e["type"] == "source_trace").unwrap();
        let net_ids = trace["connected_source_net_ids"].as_array().unwrap();
        assert_eq!(net_ids.len(), 1);
        assert!(net_ids[0].is_string());
    }

    // There is no `schematic_wire` element in the official schema — routed
    // schematic connections must be emitted as `schematic_trace` with an
    // `edges` array built from consecutive wire points.
    #[test]
    fn schematic_wires_become_schematic_traces_with_edges() {
        let m = model();
        let mut d = design();
        d.schematic.as_mut().unwrap().wires.push(eda_model::ir::Wire {
            net: "VIN".into(),
            pins: vec![],
            pts: vec![Point { x: 1_000, y: 2_000 }, Point { x: 3_000, y: 2_000 }],
        });
        let v = to_circuit_json(&d, &m).unwrap();
        let arr = v.as_array().unwrap();
        assert!(arr.iter().all(|e| e["type"] != "schematic_wire"));
        let trace = arr.iter().find(|e| e["type"] == "schematic_trace").unwrap();
        let edges = trace["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 1);
        assert!(edges[0]["from"].is_object());
        assert!(edges[0]["to"].is_object());
        assert!(trace["junctions"].is_array());
    }

    // `pcb_port.layers` is an array in the schema, not a scalar `layer`.
    #[test]
    fn pcb_port_uses_layers_array() {
        use eda_model::ir::{PlacementSection, Side};
        let m = ConstraintModel {
            footprints: vec![eda_model::Footprint {
                name: "0603".into(),
                pads: vec![
                    eda_model::Pad {
                        number: "1".into(),
                        at: (-800_000 / 1000, 0),
                        size: (900, 900),
                        shape: eda_model::PadShape::Rect,
                        kind: eda_model::PadKind::Smd,
                        drill: None,
                    },
                    eda_model::Pad {
                        number: "2".into(),
                        at: (800_000 / 1000, 0),
                        size: (900, 900),
                        shape: eda_model::PadShape::Rect,
                        kind: eda_model::PadKind::Smd,
                        drill: None,
                    },
                ],
                courtyard: None,
            }],
            ..model()
        };
        let mut d = design();
        d.placement = Some(PlacementSection {
            outline: vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 10_000 }],
            footprints: vec![eda_model::ir::FootprintInstance {
                id: "R1".into(),
                at: Point { x: 5_000, y: 5_000 },
                rot: 0,
                side: Side::Top, label: Default::default()
            }],
            modules: Vec::new(),
        });
        let v = to_circuit_json(&d, &m).unwrap();
        let arr = v.as_array().unwrap();
        let port = arr.iter().find(|e| e["type"] == "pcb_port").unwrap();
        assert!(port["layers"].is_array(), "pcb_port must use `layers` array: {port}");
        assert!(port.get("layer").is_none(), "pcb_port must not use scalar `layer`");
    }
}

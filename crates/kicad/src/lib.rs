//! eda-kicad — exports `Design::schematic` to KiCad 9 `.kicad_sch` text.
//!
//! Hand-rolled s-expression emitter (no `pcb-sexpr`/`pcb-kicad-sch`: the
//! format is small enough, and this way there's no external dependency on
//! an unreleased crate or a pinned git rev whose API might not fit our
//! integer-um `Design` model). Every symbol instance gets its own
//! `lib_symbol` (one instance == one reference designator == one part, in
//! v1), whose geometry is *pre-baked*: rotation/mirroring is applied once
//! by us (matching `eda_engine::geometry`/`eda_render`'s transform exactly)
//! to produce local, unrotated-in-KiCad-space points, and the KiCad
//! `(symbol ... (at x y 0))` instance itself is placed with rotation 0 and
//! no mirror. This sidesteps needing to replicate KiCad's own
//! rotate/mirror semantics bit-for-bit while still landing pins at exactly
//! our port points.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use eda_layout::{Port, Side};
use eda_model::ir::{Design, NetLabel, SymbolInstance, Wire};
use eda_model::{CheckResult, ConstraintModel, Part, PinKind};

mod pcb;
pub use pcb::export_kicad_pcb;

const STUB_MM: f64 = 1.27;

/// Fixed provenance for the title block. Passed explicitly (never system
/// time) so exports are byte-deterministic given the same inputs.
#[derive(Debug, Clone)]
pub struct ExportMeta<'a> {
    pub date: &'a str,
    pub title: &'a str,
}

pub fn export_kicad_sch(
    design: &Design,
    model: &ConstraintModel,
    meta: &ExportMeta,
) -> Result<String, Vec<CheckResult>> {
    let Some(sch) = &design.schematic else {
        return Err(vec![CheckResult::fail("kicad.no_schematic", "design", "design has no schematic section")]);
    };

    let mut errors = Vec::new();
    let parts_by_ref: BTreeMap<&str, &Part> = model.parts.iter().map(|p| (p.reference.as_str(), p)).collect();

    let mut symbols: Vec<&SymbolInstance> = sch.symbols.iter().collect();
    symbols.sort_by(|a, b| a.id.cmp(&b.id));
    for sym in &symbols {
        if !parts_by_ref.contains_key(sym.id.as_str()) {
            errors.push(CheckResult::fail("kicad.unknown_part", sym.id.clone(), "symbol id has no matching part in the constraint model"));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut wires: Vec<&Wire> = sch.wires.iter().collect();
    wires.sort_by(|a, b| (&a.net, &a.pts).cmp(&(&b.net, &b.pts)));

    let mut labels: Vec<&NetLabel> = sch.labels.iter().collect();
    labels.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));

    let mut out = String::new();

    // ---- header ----
    let sheet_uuid = duid("sheet:/");
    writeln!(out, "(kicad_sch").unwrap();
    writeln!(out, "\t(version 20250114)").unwrap();
    writeln!(out, "\t(generator \"eda-kicad\")").unwrap();
    writeln!(out, "\t(generator_version \"9.0\")").unwrap();
    writeln!(out, "\t(uuid \"{sheet_uuid}\")").unwrap();
    writeln!(out, "\t(paper \"A4\")").unwrap();
    writeln!(out, "\t(title_block").unwrap();
    writeln!(out, "\t\t(title {})", sexpr_str(meta.title)).unwrap();
    writeln!(out, "\t\t(date {})", sexpr_str(meta.date)).unwrap();
    writeln!(out, "\t\t(comment 1 {})", sexpr_str(&format!("engine_version: {}", design.provenance.engine_version))).unwrap();
    writeln!(out, "\t\t(comment 2 {})", sexpr_str(&format!("intent_hash: {}", design.provenance.intent_hash))).unwrap();
    writeln!(out, "\t\t(comment 3 {})", sexpr_str(&format!("seed: {}", design.provenance.seed))).unwrap();
    writeln!(out, "\t)").unwrap();

    // ---- lib_symbols ----
    writeln!(out, "\t(lib_symbols").unwrap();
    for sym in &symbols {
        let part = parts_by_ref[sym.id.as_str()];
        write_lib_symbol(&mut out, sym, part);
    }
    writeln!(out, "\t)").unwrap();

    // ---- symbol instances ----
    // KiCad 9 nests the sheet-path -> reference mapping *inside* each
    // symbol (an `instances` block), not in a separate top-level
    // `symbol_instances` list — that older shape parses as an unknown
    // token here and kicad-cli refuses to load the file.
    for sym in &symbols {
        let part = parts_by_ref[sym.id.as_str()];
        let lib_id = format!("eda:{}", sym.id);
        let x = mm(sym.at.x);
        let y = mm(sym.at.y);
        let uuid = duid(&format!("sym:{}", sym.id));
        writeln!(out, "\t(symbol (lib_id {}) (at {x} {y} 0) (unit 1)", sexpr_str(&lib_id)).unwrap();
        writeln!(out, "\t\t(exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no)").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        write_property(&mut out, "Reference", &sym.id, 0.0, -2.0);
        write_property(&mut out, "Value", part.value.as_deref().unwrap_or(&sym.id), 0.0, 2.0);
        write_property(&mut out, "Footprint", part.footprint.as_deref().unwrap_or(""), 0.0, 4.0);
        for pin in &part.pins {
            let pin_uuid = duid(&format!("pin:{}:{}", sym.id, pin.number));
            writeln!(out, "\t\t(pin {} (uuid \"{pin_uuid}\"))", sexpr_str(&pin.number)).unwrap();
        }
        writeln!(out, "\t\t(instances").unwrap();
        writeln!(out, "\t\t\t(project \"eda-kicad\"").unwrap();
        writeln!(out, "\t\t\t\t(path \"/{sheet_uuid}\"").unwrap();
        writeln!(out, "\t\t\t\t\t(reference {})", sexpr_str(&sym.id)).unwrap();
        writeln!(out, "\t\t\t\t\t(unit 1)").unwrap();
        writeln!(out, "\t\t\t\t)").unwrap();
        writeln!(out, "\t\t\t)").unwrap();
        writeln!(out, "\t\t)").unwrap();
        writeln!(out, "\t)").unwrap();
    }

    // ---- wires (each polyline segment as one KiCad wire) ----
    let mut junction_hits: BTreeMap<(i64, i64), u32> = BTreeMap::new();
    for w in &wires {
        for pair in w.pts.windows(2) {
            *junction_hits.entry((pair[0].x, pair[0].y)).or_default() += 1;
            *junction_hits.entry((pair[1].x, pair[1].y)).or_default() += 1;
        }
    }
    for (i, w) in wires.iter().enumerate() {
        for (j, pair) in w.pts.windows(2).enumerate() {
            let x1 = mm(pair[0].x);
            let y1 = mm(pair[0].y);
            let x2 = mm(pair[1].x);
            let y2 = mm(pair[1].y);
            let uuid = duid(&format!("wire:{}:{}:{}", w.net, i, j));
            writeln!(out, "\t(wire").unwrap();
            writeln!(out, "\t\t(pts (xy {x1} {y1}) (xy {x2} {y2}))").unwrap();
            writeln!(out, "\t\t(stroke (width 0) (type default))").unwrap();
            writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
            writeln!(out, "\t)").unwrap();
        }
    }

    // ---- junctions: points where >=3 wire-segment endpoints meet ----
    for (pt, count) in &junction_hits {
        if *count >= 3 {
            let x = mm(pt.0);
            let y = mm(pt.1);
            let uuid = duid(&format!("junction:{}:{}", pt.0, pt.1));
            writeln!(out, "\t(junction (at {x} {y}) (diameter 0) (color 0 0 0 0)").unwrap();
            writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
            writeln!(out, "\t)").unwrap();
        }
    }

    // ---- labels: global_label for power/ground nets, plain label otherwise ----
    for l in &labels {
        let x = mm(l.at.x);
        let y = mm(l.at.y);
        let uuid = duid(&format!("label:{}:{}:{}", l.net, l.at.x, l.at.y));
        if is_power_net(&l.net, model) {
            writeln!(out, "\t(global_label {} (shape input) (at {x} {y} 0)", sexpr_str(&l.net)).unwrap();
            writeln!(out, "\t\t(effects (font (size 1.27 1.27)) (justify left))").unwrap();
            writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
            writeln!(out, "\t)").unwrap();
        } else {
            writeln!(out, "\t(label {} (at {x} {y} 0)", sexpr_str(&l.net)).unwrap();
            writeln!(out, "\t\t(effects (font (size 1.27 1.27)) (justify left))").unwrap();
            writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
            writeln!(out, "\t)").unwrap();
        }
    }

    // ---- sheet instances (required by KiCad 9 for a valid project-less sheet) ----
    writeln!(out, "\t(sheet_instances").unwrap();
    writeln!(out, "\t\t(path \"/\" (page \"1\"))").unwrap();
    writeln!(out, "\t)").unwrap();
    writeln!(out, "\t(embedded_fonts no)").unwrap();

    writeln!(out, ")").unwrap();
    Ok(out)
}

fn write_property(out: &mut String, key: &str, value: &str, x: f64, y: f64) {
    writeln!(
        out,
        "\t\t(property {} {} (at {x} {y} 0)\n\t\t\t(effects (font (size 1.27 1.27)))\n\t\t)",
        sexpr_str(key),
        sexpr_str(value)
    )
    .unwrap();
}

fn write_lib_symbol(out: &mut String, sym: &SymbolInstance, part: &Part) {
    let lib_id = format!("eda:{}", sym.id);
    let (width, height) = eda_engine::geometry::node_size(part);
    let (ports, pin_port) = eda_engine::geometry::build_ports(part, width, height);
    let mut pin_of_port: Vec<Option<usize>> = vec![None; ports.len()];
    for (pin_idx, port_idx) in pin_port.iter().enumerate() {
        if let Some(pi) = port_idx {
            pin_of_port[*pi] = Some(pin_idx);
        }
    }

    writeln!(out, "\t\t(symbol {}", sexpr_str(&lib_id)).unwrap();
    writeln!(out, "\t\t\t(exclude_from_sim no) (in_bom yes) (on_board yes)").unwrap();
    write_property(out, "Reference", "U", 0.0, 0.0);
    write_property(out, "Value", &sym.id, 0.0, 0.0);

    // ---- unit _0_1: the box outline ----
    writeln!(out, "\t\t\t(symbol {}", sexpr_str(&format!("{}_0_1", sym.id))).unwrap();
    let corners = [(0.0, 0.0), (width as f64, 0.0), (width as f64, height as f64), (0.0, height as f64), (0.0, 0.0)];
    write!(out, "\t\t\t\t(polyline\n\t\t\t\t\t(pts").unwrap();
    for (lx, ly) in corners {
        let (bx, by) = baked_local(sym, width as f64, lx, ly);
        write!(out, " (xy {} {})", fmt_mm_f(bx), fmt_mm_f(by)).unwrap();
    }
    writeln!(out, ")\n\t\t\t\t\t(stroke (width 0.254) (type default))\n\t\t\t\t\t(fill (type none))\n\t\t\t\t)").unwrap();
    writeln!(out, "\t\t\t)").unwrap();

    // ---- unit _1_1: the pins ----
    writeln!(out, "\t\t\t(symbol {}", sexpr_str(&format!("{}_1_1", sym.id))).unwrap();
    for (port_idx, port) in ports.iter().enumerate() {
        let Some(pin_idx) = pin_of_port[port_idx] else { continue };
        let pin = &part.pins[pin_idx];
        let (plx, ply) = local_port_point(port, width, height);
        let (slx, sly) = local_stub_tip(port, plx, ply);
        let (bx, by) = baked_local(sym, width as f64, slx, sly);
        let etype = electrical_type(pin.kind);
        let name = pin.name.clone().unwrap_or_else(|| "~".to_string());
        writeln!(
            out,
            "\t\t\t\t(pin {etype} line (at {} {} 0) (length {STUB_MM})\n\t\t\t\t\t(name {} (effects (font (size 1.27 1.27))))\n\t\t\t\t\t(number {} (effects (font (size 1.27 1.27))))\n\t\t\t\t)",
            fmt_mm_f(bx),
            fmt_mm_f(by),
            sexpr_str(&name),
            sexpr_str(&pin.number),
        )
        .unwrap();
    }
    writeln!(out, "\t\t\t)").unwrap();

    writeln!(out, "\t\t)").unwrap();
}

fn electrical_type(kind: PinKind) -> &'static str {
    match kind {
        PinKind::Power | PinKind::Ground => "power_in",
        PinKind::Signal => "bidirectional",
        PinKind::Passive => "passive",
        PinKind::Nc => "no_connect",
    }
}

fn is_power_net(net_name: &str, model: &ConstraintModel) -> bool {
    let Some(net) = model.nets.iter().find(|n| n.name == net_name) else { return false };
    net.pins.iter().any(|pin_ref| {
        let Some((_, part_ref)) = pin_ref.split_once('.') else { return false };
        let _ = part_ref;
        let Some(reference) = pin_ref.split('.').next() else { return false };
        let Some(number) = pin_ref.rsplit('.').next() else { return false };
        model
            .part(reference)
            .and_then(|p| p.pins.iter().find(|pin| pin.number == number))
            .map(|pin| matches!(pin.kind, PinKind::Power | PinKind::Ground))
            .unwrap_or(false)
    })
}

/// Local (box-space, um) point for `port`, matching `eda_render::local_port_point`.
fn local_port_point(port: &Port, width: i64, height: i64) -> (f64, f64) {
    match port.side {
        Side::Top => (port.offset as f64, 0.0),
        Side::Bottom => (port.offset as f64, height as f64),
        Side::Left => (0.0, port.offset as f64),
        Side::Right => (width as f64, port.offset as f64),
    }
}

/// Local (box-space, um) pin-stub tip, matching `eda_render::stub_tip`.
fn local_stub_tip(port: &Port, lx: f64, ly: f64) -> (f64, f64) {
    let stub = eda_engine::geometry::STUB as f64;
    match port.side {
        Side::Top => (lx, ly - stub),
        Side::Bottom => (lx, ly + stub),
        Side::Left => (lx - stub, ly),
        Side::Right => (lx + stub, ly),
    }
}

/// Applies the symbol's mirror+rotation (but NOT translation) to a local
/// (um) point, exactly matching `eda_render::SymbolBox::to_abs` minus the
/// final `+ sym.at`. Returns mm.
fn baked_local(sym: &SymbolInstance, width: f64, lx: f64, ly: f64) -> (f64, f64) {
    let lx = if sym.mirrored { width - lx } else { lx };
    let theta = (sym.rot as f64 / 1000.0) * std::f64::consts::PI / 180.0;
    let rx = lx * theta.cos() - ly * theta.sin();
    let ry = lx * theta.sin() + ly * theta.cos();
    (rx / 1000.0, ry / 1000.0)
}

pub(crate) fn mm(um: i64) -> String {
    fmt_mm_f(um as f64 / 1000.0)
}

pub(crate) fn fmt_mm_f(v: f64) -> String {
    // Round to 0.1 um to kill float noise from trig, keep well under the
    // 1 um round-trip tolerance.
    let rounded = (v * 10_000.0).round() / 10_000.0;
    let s = format!("{rounded:.4}");
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".to_string() } else { s.to_string() }
}

pub(crate) fn sexpr_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Deterministic UUID-shaped id derived from a stable string (blake3, not a
/// true RFC 4122 v5, but stable/collision-resistant and structurally valid
/// so KiCad accepts it as a UUID field).
pub(crate) fn duid(seed: &str) -> String {
    let hash = blake3::hash(seed.as_bytes());
    let b = hash.as_bytes();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&b[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_engine::{derive_schematic, EngineOptions};
    use eda_model::{Net, Pin};

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }
    fn part(reference: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, value: Some(format!("{reference}_val")), package: None, footprint: Some("Foo:Bar".into()), pins, body_um: None, edge: None }
    }
    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
    }

    fn ldo_model() -> ConstraintModel {
        let u1 = part(
            "U1",
            vec![
                pin("1", "VIN", PinKind::Power),
                pin("2", "GND", PinKind::Ground),
                pin("3", "EN", PinKind::Signal),
                pin("4", "VOUT", PinKind::Power),
                pin("5", "NC", PinKind::Nc),
            ],
        );
        let cin = part("CIN", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        let cout = part("COUT", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        ConstraintModel {
            parts: vec![u1, cin, cout],
            nets: vec![
                net("VIN", &["U1.1", "CIN.1", "U1.3"]),
                net("VOUT", &["U1.4", "COUT.1"]),
                net("GND", &["U1.2", "CIN.2", "COUT.2"]),
            ],
            ..Default::default()
        }
    }

    fn export(model: &ConstraintModel) -> String {
        let design = derive_schematic(model, &EngineOptions::new(1, "hash")).unwrap();
        let meta = ExportMeta { date: "2026-01-01", title: "LDO test" };
        export_kicad_sch(&design, model, &meta).unwrap()
    }

    fn balanced_parens(s: &str) -> bool {
        let mut depth = 0i32;
        let mut in_str = false;
        let mut escaped = false;
        for c in s.chars() {
            if in_str {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_str = false;
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth < 0 {
                return false;
            }
        }
        depth == 0 && !in_str
    }

    #[test]
    fn balanced_parens_output() {
        let out = export(&ldo_model());
        assert!(balanced_parens(&out), "unbalanced parens:\n{out}");
    }

    #[test]
    fn deterministic_export() {
        let model = ldo_model();
        let a = export(&model);
        let b = export(&model);
        assert_eq!(a, b);
    }

    #[test]
    fn symbol_and_wire_counts_match() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
        let out = export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        assert_eq!(out.matches("(symbol (lib_id").count(), sch.symbols.len());
        let expected_wire_segments: usize = sch.wires.iter().map(|w| w.pts.len().saturating_sub(1)).sum();
        assert_eq!(out.matches("\t(wire\n").count(), expected_wire_segments);
    }

    #[test]
    fn pin_coordinates_round_trip_within_1um() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        let u1 = sch.symbols.iter().find(|s| s.id == "U1").unwrap();
        let part = model.part("U1").unwrap();
        let (width, height) = eda_engine::geometry::node_size(part);
        let (ports, pin_port) = eda_engine::geometry::build_ports(part, width, height);
        // VIN is pin "1" -> some port; compute expected world stub tip.
        let port_idx = pin_port[0].unwrap();
        let port = ports[port_idx];
        let (plx, ply) = local_port_point(&port, width, height);
        let (slx, sly) = local_stub_tip(&port, plx, ply);
        let (bx_mm, by_mm) = baked_local(u1, width as f64, slx, sly);
        let expected_x_um = u1.at.x + (bx_mm * 1000.0).round() as i64;
        let expected_y_um = u1.at.y + (by_mm * 1000.0).round() as i64;

        let meta = ExportMeta { date: "2026-01-01", title: "t" };
        let out = export_kicad_sch(&design, &model, &meta).unwrap();
        // Find the emitted pin line for number "1" within U1's own
        // "_1_1" pin sub-symbol block and parse its (at x y 0).
        let scope_start = out.find("\"U1_1_1\"").expect("U1 pin block emitted");
        let scope = &out[scope_start..];
        let marker = "(number \"1\"";
        let idx = scope.find(marker).expect("pin 1 emitted");
        let before = &scope[..idx];
        let at_idx = before.rfind("(at ").expect("preceding (at ...)");
        let at_str = &before[at_idx + 4..];
        let end = at_str.find(" 0)").unwrap();
        let coords: Vec<f64> = at_str[..end].split(' ').map(|t| t.parse().unwrap()).collect();
        let got_x_um = (coords[0] * 1000.0).round() as i64 + u1.at.x;
        let got_y_um = (coords[1] * 1000.0).round() as i64 + u1.at.y;
        assert!((got_x_um - expected_x_um).abs() <= 1, "x mismatch: {got_x_um} vs {expected_x_um}");
        assert!((got_y_um - expected_y_um).abs() <= 1, "y mismatch: {got_y_um} vs {expected_y_um}");
    }

    #[test]
    fn power_nets_use_global_label() {
        let model = ldo_model();
        let out = export(&model);
        // GND is a >=3-pin all-power/ground net, so the engine emits it as
        // a label (not wires); it must show up as a global_label.
        if out.contains("\"GND\"") {
            assert!(out.contains("(global_label \"GND\""), "GND should be a global_label:\n{out}");
        }
    }

    #[test]
    fn missing_schematic_errors() {
        let design = Design {
            schema: 1,
            provenance: eda_model::ir::Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            placement: None,
            routing: None,
        };
        let model = ConstraintModel::default();
        let err = export_kicad_sch(&design, &model, &ExportMeta { date: "d", title: "t" }).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn unknown_part_errors() {
        let mut model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
        model.parts.clear(); // now no part resolves any symbol id
        let err = export_kicad_sch(&design, &model, &ExportMeta { date: "d", title: "t" }).unwrap_err();
        assert!(err.iter().any(|e| e.check == "kicad.unknown_part"));
    }

    #[test]
    fn uuids_are_stable_and_look_like_uuids() {
        let a = duid("sym:U1");
        let b = duid("sym:U1");
        let c = duid("sym:U2");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 36);
        assert_eq!(a.chars().filter(|&c| c == '-').count(), 4);
    }
}

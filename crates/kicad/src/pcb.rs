//! eda-kicad — exports `Design::placement`/`routing` to KiCad 9
//! `.kicad_pcb` text.
//!
//! Same philosophy as `lib.rs`'s schematic exporter: hand-rolled
//! s-expression emission, deterministic uuids via `duid`, everything
//! sorted by stable keys before emission.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use eda_model::ir::{Design, FootprintInstance, Side, Track, Via};
use eda_model::{CheckResult, ConstraintModel, Pad, PadKind, PadShape, Part};

use crate::{duid, fmt_mm_f, mm, sexpr_str};

pub fn export_kicad_pcb(design: &Design, model: &ConstraintModel, _meta: &super::ExportMeta) -> Result<String, Vec<CheckResult>> {
    let Some(pl) = &design.placement else {
        return Err(vec![CheckResult::fail("kicad.no_placement", "design", "design has no placement section")]);
    };

    let mut errors = Vec::new();
    let parts_by_ref: BTreeMap<&str, &Part> = model.parts.iter().map(|p| (p.reference.as_str(), p)).collect();

    let mut footprints: Vec<&FootprintInstance> = pl.footprints.iter().collect();
    footprints.sort_by(|a, b| a.id.cmp(&b.id));
    for fp in &footprints {
        if !parts_by_ref.contains_key(fp.id.as_str()) {
            errors.push(CheckResult::fail("kicad.unknown_part", fp.id.clone(), "footprint id has no matching part in the constraint model"));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // Net name -> number, sorted by name, 1-indexed (0 is the unconnected net).
    let mut net_names: Vec<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
    net_names.sort();
    net_names.dedup();
    let net_num: BTreeMap<&str, usize> = net_names.iter().enumerate().map(|(i, n)| (*n, i + 1)).collect();

    let mut out = String::new();
    writeln!(out, "(kicad_pcb").unwrap();
    writeln!(out, "\t(version 20241229)").unwrap();
    writeln!(out, "\t(generator \"eda-kicad\")").unwrap();
    writeln!(out, "\t(generator_version \"9.0\")").unwrap();
    writeln!(out, "\t(general").unwrap();
    writeln!(out, "\t\t(thickness 1.6)").unwrap();
    writeln!(out, "\t\t(legacy_teardrops no)").unwrap();
    writeln!(out, "\t)").unwrap();
    writeln!(out, "\t(paper \"A4\")").unwrap();

    // ---- layers ----
    writeln!(out, "\t(layers").unwrap();
    let copper_layers = &model.board.layers;
    let n_cu = copper_layers.len().max(2);
    for (i, name) in copper_layers.iter().enumerate() {
        let ltype = if i == 0 { "signal" } else if i + 1 == copper_layers.len() { "signal" } else { "signal" };
        let ord = if i == 0 { 0 } else if i + 1 == copper_layers.len() { 31 } else { i };
        writeln!(out, "\t\t({ord} {} {ltype})", sexpr_str(name)).unwrap();
    }
    let _ = n_cu;
    for (ord, name, ltype) in [
        (32, "B.Adhes", "user"),
        (33, "F.Adhes", "user"),
        (34, "B.Paste", "user"),
        (35, "F.Paste", "user"),
        (36, "B.SilkS", "user"),
        (37, "F.SilkS", "user"),
        (38, "B.Mask", "user"),
        (39, "F.Mask", "user"),
        (44, "Edge.Cuts", "user"),
        (49, "F.Fab", "user"),
        (50, "B.Fab", "user"),
    ] {
        writeln!(out, "\t\t({ord} {} {ltype})", sexpr_str(name)).unwrap();
    }
    writeln!(out, "\t)").unwrap();

    // ---- setup ----
    let clearance_mm = mm(model.board.clearance);
    let track_mm = mm(model.board.track_width);
    let via_dia_mm = mm(model.board.via_diameter);
    let via_drill_mm = mm(model.board.via_drill);
    writeln!(out, "\t(setup").unwrap();
    writeln!(out, "\t\t(pad_to_mask_clearance 0.0)").unwrap();
    writeln!(out, "\t\t(allow_soldermask_bridges_in_footprints no)").unwrap();
    writeln!(out, "\t\t(aux_axis_origin 0 0)").unwrap();
    writeln!(out, "\t\t(grid_origin 0 0)").unwrap();
    writeln!(out, "\t\t(pcbplotparams").unwrap();
    writeln!(out, "\t\t\t(layerselection 0x00010fc_ffffffff)").unwrap();
    writeln!(out, "\t\t\t(plot_on_all_layers_selection 0x0000000_00000000)").unwrap();
    writeln!(out, "\t\t\t(disableapertmacros no)").unwrap();
    writeln!(out, "\t\t\t(usegerberextensions no)").unwrap();
    writeln!(out, "\t\t\t(usegerberattributes yes)").unwrap();
    writeln!(out, "\t\t\t(usegerberadvancedattributes yes)").unwrap();
    writeln!(out, "\t\t\t(creategerberjobfile yes)").unwrap();
    writeln!(out, "\t\t\t(dashed_line_dash_ratio 12.000000)").unwrap();
    writeln!(out, "\t\t\t(dashed_line_gap_ratio 3.000000)").unwrap();
    writeln!(out, "\t\t\t(svgprecision 4)").unwrap();
    writeln!(out, "\t\t\t(plotframeref no)").unwrap();
    writeln!(out, "\t\t\t(mode 1)").unwrap();
    writeln!(out, "\t\t\t(useauxorigin no)").unwrap();
    writeln!(out, "\t\t\t(hpglpennumber 1)").unwrap();
    writeln!(out, "\t\t\t(hpglpenspeed 20)").unwrap();
    writeln!(out, "\t\t\t(hpglpendiameter 15.000000)").unwrap();
    writeln!(out, "\t\t\t(pdf_front_fp_property_popups yes)").unwrap();
    writeln!(out, "\t\t\t(pdf_back_fp_property_popups yes)").unwrap();
    writeln!(out, "\t\t\t(dxfpolygonmode yes)").unwrap();
    writeln!(out, "\t\t\t(dxfimperialunits yes)").unwrap();
    writeln!(out, "\t\t\t(dxfusepcbnewfont yes)").unwrap();
    writeln!(out, "\t\t\t(psnegative no)").unwrap();
    writeln!(out, "\t\t\t(psa4output no)").unwrap();
    writeln!(out, "\t\t\t(plotreference yes)").unwrap();
    writeln!(out, "\t\t\t(plotvalue yes)").unwrap();
    writeln!(out, "\t\t\t(plotfptext yes)").unwrap();
    writeln!(out, "\t\t\t(plotinvisibletext no)").unwrap();
    writeln!(out, "\t\t\t(sketchpadsonfab no)").unwrap();
    writeln!(out, "\t\t\t(subtractmaskfromsilk no)").unwrap();
    writeln!(out, "\t\t\t(outputformat 1)").unwrap();
    writeln!(out, "\t\t\t(mirror no)").unwrap();
    writeln!(out, "\t\t\t(drillshape 1)").unwrap();
    writeln!(out, "\t\t\t(scaleselection 1)").unwrap();
    writeln!(out, "\t\t\t(outputdirectory \"\")").unwrap();
    writeln!(out, "\t\t)").unwrap();
    writeln!(out, "\t)").unwrap();

    // ---- nets ----
    writeln!(out, "\t(net 0 \"\")").unwrap();
    for name in &net_names {
        writeln!(out, "\t(net {} {})", net_num[name], sexpr_str(name)).unwrap();
    }

    // ---- net_class (KiCad 9: net classes live at top level, `add_net` refers by name) ----
    writeln!(out, "\t(net_class \"Default\" \"This is the default net class.\"").unwrap();
    writeln!(out, "\t\t(clearance {clearance_mm})").unwrap();
    writeln!(out, "\t\t(trace_width {track_mm})").unwrap();
    writeln!(out, "\t\t(via_dia {via_dia_mm})").unwrap();
    writeln!(out, "\t\t(via_drill {via_drill_mm})").unwrap();
    writeln!(out, "\t\t(uvia_dia 0.3)").unwrap();
    writeln!(out, "\t\t(uvia_drill 0.1)").unwrap();
    for name in &net_names {
        writeln!(out, "\t\t(add_net {})", sexpr_str(name)).unwrap();
    }
    writeln!(out, "\t)").unwrap();

    // ---- footprints ----
    for fp in &footprints {
        let part = parts_by_ref[fp.id.as_str()];
        let Some(footprint) = model.footprint_of(part) else {
            errors.push(CheckResult::fail("kicad.no_footprint", fp.id.clone(), "part has no resolvable footprint geometry"));
            continue;
        };
        write_footprint(&mut out, fp, part, &footprint, &net_num, model);
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // ---- routing: segments + vias ----
    if let Some(routing) = &design.routing {
        let mut tracks: Vec<&Track> = routing.tracks.iter().collect();
        tracks.sort_by(|a, b| (&a.net, &a.layer, &a.pts).cmp(&(&b.net, &b.layer, &b.pts)));
        for (ti, t) in tracks.iter().enumerate() {
            let n = *net_num.get(t.net.as_str()).unwrap_or(&0);
            for (j, pair) in t.pts.windows(2).enumerate() {
                let x1 = mm(pair[0].x);
                let y1 = mm(pair[0].y);
                let x2 = mm(pair[1].x);
                let y2 = mm(pair[1].y);
                let w = mm(t.width);
                let uuid = duid(&format!("segment:{}:{}:{}:{}", t.net, t.layer, ti, j));
                writeln!(out, "\t(segment (start {x1} {y1}) (end {x2} {y2}) (width {w}) (layer {}) (net {n}) (uuid \"{uuid}\"))", sexpr_str(&t.layer)).unwrap();
            }
        }

        let mut vias: Vec<&Via> = routing.vias.iter().collect();
        vias.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));
        for v in &vias {
            let n = *net_num.get(v.net.as_str()).unwrap_or(&0);
            let x = mm(v.at.x);
            let y = mm(v.at.y);
            let dia = mm(v.diameter);
            let drill = mm(v.drill);
            let uuid = duid(&format!("via:{}:{}:{}", v.net, v.at.x, v.at.y));
            writeln!(
                out,
                "\t(via (at {x} {y}) (size {dia}) (drill {drill}) (layers {} {}) (net {n}) (uuid \"{uuid}\"))",
                sexpr_str(&v.from_layer),
                sexpr_str(&v.to_layer)
            )
            .unwrap();
        }
    }

    // ---- board outline (Edge.Cuts) ----
    if pl.outline.len() >= 2 {
        let n = pl.outline.len();
        for i in 0..n {
            let a = pl.outline[i];
            let b = pl.outline[(i + 1) % n];
            let x1 = mm(a.x);
            let y1 = mm(a.y);
            let x2 = mm(b.x);
            let y2 = mm(b.y);
            let uuid = duid(&format!("edge:{i}:{}:{}:{}:{}", a.x, a.y, b.x, b.y));
            writeln!(out, "\t(gr_line (start {x1} {y1}) (end {x2} {y2}) (layer \"Edge.Cuts\") (uuid \"{uuid}\"))").unwrap();
        }
    }

    writeln!(out, ")").unwrap();
    Ok(out)
}

fn write_footprint(
    out: &mut String,
    fp: &FootprintInstance,
    part: &Part,
    footprint: &eda_model::Footprint,
    net_num: &BTreeMap<&str, usize>,
    model: &ConstraintModel,
) {
    let layer = if fp.side == Side::Bottom { "B.Cu" } else { "F.Cu" };
    let x = mm(fp.at.x);
    let y = mm(fp.at.y);
    // KiCad's `(at x y angle)` rotates footprints clockwise for positive
    // angle (screen convention with y already pointing down), the opposite
    // sense from `eda_model::footprint::to_board`'s rotation matrix — so the
    // angle we emit must be negated to land pads exactly where the
    // router/placer computed them. Verified empirically against
    // `footprint::to_board` output and `kicad-cli pcb drc` unconnected-item
    // positions for a rotated part.
    let rot_deg = fmt_mm_f(-(fp.rot as f64) / 1000.0);
    let uuid = duid(&format!("footprint:{}", fp.id));
    let lib_name = part.footprint.clone().unwrap_or_else(|| footprint.name.clone());
    let lib_id = if lib_name.contains(':') { lib_name } else { format!("eda:{lib_name}") };

    writeln!(out, "\t(footprint {}", sexpr_str(&lib_id)).unwrap();
    writeln!(out, "\t\t(layer {})", sexpr_str(layer)).unwrap();
    writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
    writeln!(out, "\t\t(at {x} {y} {rot_deg})").unwrap();

    let ref_layer = if fp.side == Side::Bottom { "B.SilkS" } else { "F.SilkS" };
    let ref_uuid = duid(&format!("footprint:{}:ref", fp.id));
    writeln!(
        out,
        "\t\t(property \"Reference\" {} (at 0 -1 0) (layer {})\n\t\t\t(uuid \"{ref_uuid}\")\n\t\t\t(effects (font (size 1 1) (thickness 0.15)))\n\t\t)",
        sexpr_str(&fp.id),
        sexpr_str(ref_layer)
    )
    .unwrap();
    let val_uuid = duid(&format!("footprint:{}:val", fp.id));
    let fab_layer = if fp.side == Side::Bottom { "B.Fab" } else { "F.Fab" };
    writeln!(
        out,
        "\t\t(property \"Value\" {} (at 0 1 0) (layer {})\n\t\t\t(uuid \"{val_uuid}\")\n\t\t\t(effects (font (size 1 1) (thickness 0.15)))\n\t\t)",
        sexpr_str(part.value.as_deref().unwrap_or(&fp.id)),
        sexpr_str(fab_layer)
    )
    .unwrap();

    let mut pads: Vec<&Pad> = footprint.pads.iter().collect();
    pads.sort_by(|a, b| a.number.cmp(&b.number));

    // Net assignment: look up which net (if any) this pad's "REF.PIN" belongs to.
    let pin_net = |number: &str| -> Option<&str> {
        let key = format!("{}.{}", fp.id, number);
        model.nets.iter().find(|n| n.pins.iter().any(|p| p == &key)).map(|n| n.name.as_str())
    };

    for pad in &pads {
        let px = mm(pad.at.0);
        let py = mm(pad.at.1);
        let (w, h) = pad.size;
        let wmm = mm(w);
        let hmm = mm(h);
        let kind = match pad.kind {
            PadKind::Smd => "smd",
            PadKind::ThroughHole => "thru_hole",
        };
        let shape = match pad.shape {
            PadShape::Rect => "rect",
            PadShape::RoundRect => "roundrect",
            PadShape::Circle => "circle",
            PadShape::Oval => "oval",
        };
        let uuid = duid(&format!("pad:{}:{}", fp.id, pad.number));
        // KiCad stores a pad's orientation as an absolute angle (footprint
        // angle + pad's own, which is 0 for us), so the footprint angle
        // must be repeated here or a rotated part's pads render unrotated.
        write!(out, "\t\t(pad {} {kind} {shape} (at {px} {py} {rot_deg})", sexpr_str(&pad.number)).unwrap();
        write!(out, " (size {wmm} {hmm})").unwrap();
        if pad.shape == PadShape::RoundRect {
            write!(out, " (roundrect_rratio 0.25)").unwrap();
        }
        match pad.kind {
            PadKind::ThroughHole => {
                let drill = mm(pad.drill.unwrap_or(w.min(h) / 2));
                write!(out, " (drill {drill})").unwrap();
                write!(out, " (layers \"*.Cu\" \"*.Mask\")").unwrap();
            }
            PadKind::Smd => {
                let (cu, paste, mask) = if fp.side == Side::Bottom {
                    ("B.Cu", "B.Paste", "B.Mask")
                } else {
                    ("F.Cu", "F.Paste", "F.Mask")
                };
                write!(out, " (layers {} {} {})", sexpr_str(cu), sexpr_str(paste), sexpr_str(mask)).unwrap();
            }
        }
        if let Some(net_name) = pin_net(&pad.number) {
            if let Some(&n) = net_num.get(net_name) {
                write!(out, " (net {n} {})", sexpr_str(net_name)).unwrap();
            }
        }
        writeln!(out, " (uuid \"{uuid}\"))").unwrap();
    }

    writeln!(out, "\t)").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{PlacementSection, Point, Provenance, RoutingSection};
    use eda_model::{Net, Pin, PinKind};

    fn pin(number: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: None, kind }
    }
    fn part(reference: &str, package: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, value: Some(format!("{reference}_val")), package: Some(package.into()), footprint: Some(package.into()), pins }
    }
    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
    }

    fn fixture() -> (Design, ConstraintModel) {
        let u1 = part("U1", "SOT-23", vec![pin("1", PinKind::Power), pin("2", PinKind::Ground), pin("3", PinKind::Signal)]);
        let c1 = part("C1", "0603", vec![pin("1", PinKind::Passive), pin("2", PinKind::Ground)]);
        let model = ConstraintModel {
            parts: vec![u1, c1],
            nets: vec![net("VIN", &["U1.1", "C1.1"]), net("GND", &["U1.2", "C1.2"])],
            ..Default::default()
        };
        let placement = PlacementSection {
            outline: vec![
                Point { x: 0, y: 0 },
                Point { x: 20_000, y: 0 },
                Point { x: 20_000, y: 20_000 },
                Point { x: 0, y: 20_000 },
            ],
            footprints: vec![
                FootprintInstance { id: "U1".into(), at: Point { x: 5_000, y: 5_000 }, rot: 0, side: Side::Top },
                FootprintInstance { id: "C1".into(), at: Point { x: 10_000, y: 10_000 }, rot: 90_000, side: Side::Bottom },
            ],
        };
        let design = Design {
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            placement: Some(placement),
            routing: Some(RoutingSection {
                tracks: vec![Track { net: "VIN".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 5_000, y: 5_000 }, Point { x: 10_000, y: 5_000 }] }],
                vias: vec![],
                zones: vec![],
            }),
        };
        (design, model)
    }

    fn meta() -> super::super::ExportMeta<'static> {
        super::super::ExportMeta { date: "2026-01-01", title: "t" }
    }

    #[test]
    fn deterministic_export() {
        let (design, model) = fixture();
        let a = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let b = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn net_numbering_sorted() {
        let (design, model) = fixture();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(net 0 \"\")"));
        // GND < VIN alphabetically -> GND=1, VIN=2
        assert!(out.contains("(net 1 \"GND\")"));
        assert!(out.contains("(net 2 \"VIN\")"));
    }

    #[test]
    fn pad_count_matches_model() {
        let (design, model) = fixture();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let mut expected = 0usize;
        for fp in &design.placement.as_ref().unwrap().footprints {
            let part = model.part(&fp.id).unwrap();
            expected += model.footprint_of(part).unwrap().pads.len();
        }
        assert_eq!(out.matches("\t\t(pad ").count(), expected);
    }

    #[test]
    fn missing_placement_errors() {
        let (mut design, model) = fixture();
        design.placement = None;
        let err = export_kicad_pcb(&design, &model, &meta()).unwrap_err();
        assert!(err.iter().any(|e| e.check == "kicad.no_placement"));
    }
}

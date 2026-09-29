//! eda-kicad — exports `Design::placement`/`routing` to KiCad 9
//! `.kicad_pcb` text.
//!
//! Same philosophy as `lib.rs`'s schematic exporter: hand-rolled
//! s-expression emission, deterministic uuids via `duid`, everything
//! sorted by stable keys before emission.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use eda_model::ir::{Design, FootprintInstance, Side, Track, Via, Zone};
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

    // Net 0 is KiCad's *unconnected* net. Defaulting a track, via or zone
    // to it does not lose the copper -- it exports copper that claims to
    // belong to nothing, so the board KiCad checks is not the board our
    // gates passed. A GND pour landing on net 0 is a floating plane that
    // still draws, and `drc --refill-zones` would be answering a question
    // about a different design than the one we routed. The net has to be
    // in the netlist or the export fails.
    let mut errors: Vec<CheckResult> = Vec::new();
    let mut net_of = |what: &str, where_: &str, net: &str| -> usize {
        match net_num.get(net) {
            Some(n) => *n,
            None => {
                errors.push(CheckResult::fail(
                    "kicad.unknown_net",
                    where_.to_string(),
                    format!("{what} is on net {net:?}, which is not in the netlist; KiCad would import it as net 0 (unconnected)"),
                ));
                0
            }
        }
    };

    // ---- routing: segments + vias ----
    if let Some(routing) = &design.routing {
        let mut tracks: Vec<&Track> = routing.tracks.iter().collect();
        tracks.sort_by(|a, b| (&a.net, &a.layer, &a.pts).cmp(&(&b.net, &b.layer, &b.pts)));
        for (ti, t) in tracks.iter().enumerate() {
            let n = net_of("a track", &format!("{} on {}", t.net, t.layer), &t.net);
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
            let n = net_of("a via", &format!("{} at ({}, {})", v.net, v.at.x, v.at.y), &v.net);
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

        // ---- copper pours ----
        // The outline is ours; the fill is KiCad's. We emit the polygon
        // and the connectivity rules, and let the viewer's filler lay the
        // copper -- our own reachability check (routing_pour_unreachable)
        // is what decides whether the plane is honest, not the render.
        let mut zones: Vec<&Zone> = routing.zones.iter().collect();
        zones.sort_by(|a, b| (&a.net, &a.layer).cmp(&(&b.net, &b.layer)));
        for z in &zones {
            if z.outline.len() < 3 {
                continue;
            }
            let n = net_of("a copper pour", &format!("{} on {}", z.net, z.layer), &z.net);
            let uuid = duid(&format!("zone:{}:{}", z.net, z.layer));
            writeln!(out, "\t(zone (net {n}) (net_name {}) (layer {}) (uuid \"{uuid}\")", sexpr_str(&z.net), sexpr_str(&z.layer)).unwrap();
            writeln!(out, "\t\t(hatch edge 0.5)").unwrap();
            writeln!(out, "\t\t(connect_pads (clearance {clearance_mm}))").unwrap();
            writeln!(out, "\t\t(min_thickness {track_mm})").unwrap();
            writeln!(out, "\t\t(fill yes (thermal_gap {clearance_mm}) (thermal_bridge_width {track_mm}))").unwrap();
            writeln!(out, "\t\t(polygon (pts").unwrap();
            for p in &z.outline {
                writeln!(out, "\t\t\t(xy {} {})", mm(p.x), mm(p.y)).unwrap();
            }
            writeln!(out, "\t\t))").unwrap();
            writeln!(out, "\t)").unwrap();
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
    if !errors.is_empty() {
        return Err(errors);
    }
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
    // Label offset from the footprint origin, in the footprint's own frame:
    // just outside the courtyard on the side the placer chose.
    let (hw, hh) = footprint.courtyard_half();
    let half_w = 300 * fp.id.chars().count() as eda_model::ir::Um;
    let (ref_x, ref_y) = match fp.label {
        eda_model::ir::LabelSide::Above => (0, -(hh + 700)),
        eda_model::ir::LabelSide::Below => (0, hh + 700),
        eda_model::ir::LabelSide::Left => (-(hw + 200 + half_w), 0),
        eda_model::ir::LabelSide::Right => (hw + 200 + half_w, 0),
    };
    let (ref_x, ref_y) = (mm(ref_x), mm(ref_y));
    writeln!(
        out,
        "\t\t(property \"Reference\" {} (at {ref_x} {ref_y} 0) (layer {})\n\t\t\t(uuid \"{ref_uuid}\")\n\t\t\t(effects (font (size 1 1) (thickness 0.15)))\n\t\t)",
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
        // Our local frame is the part seen from the top; a bottom-side
        // part's x is mirrored before it is rotated (footprint::to_board,
        // which the placer, router and gates all go through). KiCad
        // mirrors nothing when it loads a footprint -- it rotates and
        // translates the pad's `(at x y)` whatever the layer -- so the file
        // has to carry the mirrored x. Written as-is, every bottom-side
        // pin landed across the part from the copper routed to it (a
        // 0603's two pins swapped nets). The angle needs nothing: our pads
        // are symmetric about their own axes, so mirroring one only moves
        // it.
        let px = mm(if fp.side == Side::Bottom { -pad.at.0 } else { pad.at.0 });
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
                // Footprint::validate guarantees a through-hole pad
                // declares its drill; inventing one here shipped a
                // different hole than the circuit-json writer invented.
                let drill = mm(pad.drill.expect("through-hole pad without a drill passed Footprint::validate"));
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
        Part { reference: reference.into(), mpn: None, value: Some(format!("{reference}_val")), package: Some(package.into()), footprint: Some(package.into()), pins, body_um: None, edge: None }
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
                FootprintInstance { id: "U1".into(), at: Point { x: 5_000, y: 5_000 }, rot: 0, side: Side::Top, label: Default::default() },
                FootprintInstance { id: "C1".into(), at: Point { x: 10_000, y: 10_000 }, rot: 90_000, side: Side::Bottom, label: Default::default() },
            ],
            modules: Vec::new(),
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
    fn a_track_on_a_net_that_is_not_in_the_netlist_fails_the_export() {
        // Net 0 is KiCad's *unconnected* net. Defaulting to it exported
        // copper claiming to belong to nothing, so the board KiCad checked
        // was not the board our gates passed -- and `drc --refill-zones`
        // would have been answering about a different design.
        let (mut design, model) = fixture();
        design.routing.as_mut().unwrap().tracks[0].net = "NOT_A_NET".into();
        let err = export_kicad_pcb(&design, &model, &meta()).unwrap_err();
        assert!(err.iter().any(|c| c.check == "kicad.unknown_net"), "got {err:?}");
    }

    #[test]
    fn a_pour_on_a_net_that_is_not_in_the_netlist_fails_the_export() {
        // The worst case: a plane silently exported as floating copper.
        let (mut design, model) = fixture();
        design.routing.as_mut().unwrap().zones = vec![Zone {
            net: "NOT_A_NET".into(),
            layer: "B.Cu".into(),
            outline: vec![
                Point { x: 0, y: 0 },
                Point { x: 20_000, y: 0 },
                Point { x: 20_000, y: 20_000 },
            ],
        }];
        let err = export_kicad_pcb(&design, &model, &meta()).unwrap_err();
        assert!(err.iter().any(|c| c.check == "kicad.unknown_net"), "got {err:?}");
    }

    #[test]
    fn missing_placement_errors() {
        let (mut design, model) = fixture();
        design.placement = None;
        let err = export_kicad_pcb(&design, &model, &meta()).unwrap_err();
        assert!(err.iter().any(|e| e.check == "kicad.no_placement"));
    }

    /// Pad centres, keyed "REF.PIN", as KiCad computes them when it loads
    /// the file: the pad's `(at x y)` rotated by its footprint's angle
    /// (KiCad's `RotatePoint`), then moved to the footprint's position. No
    /// mirroring, whatever layer the footprint is on -- checked against
    /// kicad-cli by `kicad_cli_drc_bottom_side_pads` in
    /// tests/kicad_cli_drc.rs.
    fn kicad_pad_centres(pcb: &str) -> BTreeMap<String, (f64, f64)> {
        fn at(line: &str) -> Vec<f64> {
            let rest = &line[line.find("(at ").unwrap() + 4..];
            rest[..rest.find(')').unwrap()].split_whitespace().map(|v| v.parse().unwrap()).collect()
        }
        let mut out = BTreeMap::new();
        let (mut origin, mut angle, mut reference) = ((0.0, 0.0), 0.0_f64, String::new());
        for line in pcb.lines() {
            let t = line.trim_start();
            if line.starts_with("\t\t(at ") {
                let v = at(line);
                origin = (v[0], v[1]);
                angle = v[2].to_radians();
            } else if let Some(rest) = t.strip_prefix("(property \"Reference\" \"") {
                reference = rest[..rest.find('"').unwrap()].to_string();
            } else if let Some(rest) = t.strip_prefix("(pad \"") {
                let v = at(line);
                let (sin, cos) = angle.sin_cos();
                let (x, y) = (v[0] * cos + v[1] * sin, -v[0] * sin + v[1] * cos);
                out.insert(format!("{reference}.{}", &rest[..rest.find('"').unwrap()]), (origin.0 + x, origin.1 + y));
            }
        }
        out
    }

    #[test]
    fn pads_land_where_the_engine_put_them_on_either_side() {
        // The engine mirrors a bottom-side part's local x before rotating
        // it (footprint::to_board); KiCad does not, so the file has to
        // carry the mirrored x. Written as-is, a bottom SOT-23's pins land
        // on the wrong column and a bottom 0603 swaps pins 1 and 2 --
        // nets and all -- so every track routed to them misses.
        let (mut design, mut model) = fixture();
        model.parts.push(part("U2", "SOT-23", vec![pin("1", PinKind::Signal), pin("2", PinKind::Signal), pin("3", PinKind::Signal)]));
        design.placement.as_mut().unwrap().footprints.push(FootprintInstance {
            id: "U2".into(),
            at: Point { x: 15_000, y: 5_000 },
            rot: 90_000,
            side: Side::Bottom,
            label: Default::default(),
        });
        let kicad = kicad_pad_centres(&export_kicad_pcb(&design, &model, &meta()).unwrap());
        let mut checked = 0;
        for fp in &design.placement.as_ref().unwrap().footprints {
            let part = model.part(&fp.id).unwrap();
            for pad in eda_model::footprint::placed_pads(&model, part, fp).unwrap() {
                let key = format!("{}.{}", fp.id, pad.number);
                let (x, y) = kicad[&key];
                let (ex, ey) = (pad.center.x as f64 / 1000.0, pad.center.y as f64 / 1000.0);
                assert!(
                    (x - ex).abs() < 0.001 && (y - ey).abs() < 0.001,
                    "{key} ({:?}, rot {}): KiCad puts it at ({x:.3}, {y:.3}), the engine routed to ({ex:.3}, {ey:.3})",
                    fp.side,
                    fp.rot
                );
                checked += 1;
            }
        }
        // U1 top SOT-23, C1 bottom 0603, U2 bottom SOT-23.
        assert_eq!(checked, 8);
    }
}

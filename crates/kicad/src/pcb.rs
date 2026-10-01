//! eda-kicad — exports `Design::placement`/`routing` to KiCad 9
//! `.kicad_pcb` text.
//!
//! Same philosophy as `lib.rs`'s schematic exporter: hand-rolled
//! s-expression emission, deterministic uuids via `duid`, everything
//! sorted by stable keys before emission.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use eda_model::ir::{Design, FootprintInstance, Shape, Side, Text, TextJustify, Track, Um, Via, Zone};
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
    //
    // A net matching one of `model.board.net_classes` gets its own KiCad
    // net class (its own clearance/trace width) instead of landing in
    // Default with everything else. Without this, a class like an
    // escape route -- narrower than the board default so it can clear a
    // fine-pitch connector row -- only ever changes what *our own* router
    // thinks its nets need: the file we write still tells `kicad-cli pcb
    // drc` that every net wants the board's width/clearance, and it
    // fails every track the escape class routed narrower than that.
    let default_nets: Vec<&str> = net_names.iter().copied().filter(|n| model.board.class_of(n).is_none()).collect();
    writeln!(out, "\t(net_class \"Default\" \"This is the default net class.\"").unwrap();
    writeln!(out, "\t\t(clearance {clearance_mm})").unwrap();
    writeln!(out, "\t\t(trace_width {track_mm})").unwrap();
    writeln!(out, "\t\t(via_dia {via_dia_mm})").unwrap();
    writeln!(out, "\t\t(via_drill {via_drill_mm})").unwrap();
    writeln!(out, "\t\t(uvia_dia 0.3)").unwrap();
    writeln!(out, "\t\t(uvia_drill 0.1)").unwrap();
    for name in &default_nets {
        writeln!(out, "\t\t(add_net {})", sexpr_str(name)).unwrap();
    }
    writeln!(out, "\t)").unwrap();

    for class in &model.board.net_classes {
        let nets_in_class: Vec<&str> = net_names.iter().copied().filter(|n| model.board.class_of(n).is_some_and(|c| c.name == class.name)).collect();
        if nets_in_class.is_empty() {
            continue;
        }
        let class_clearance_mm = mm(class.clearance.unwrap_or(model.board.clearance));
        let class_track_mm = mm(class.track_width.unwrap_or(model.board.track_width));
        // Additive `NetClass` fields (see `eda_model::NetClass`'s doc
        // comments): a class with its own via size used to always fall
        // through to the board-wide via here regardless of what it asked
        // for, the same gap `clearance`/`trace_width` above don't have.
        let class_via_dia_mm = mm(class.via_diameter.unwrap_or(model.board.via_diameter));
        let class_via_drill_mm = mm(class.via_drill.unwrap_or(model.board.via_drill));
        writeln!(out, "\t(net_class {} \"\"", sexpr_str(&class.name)).unwrap();
        writeln!(out, "\t\t(clearance {class_clearance_mm})").unwrap();
        writeln!(out, "\t\t(trace_width {class_track_mm})").unwrap();
        writeln!(out, "\t\t(via_dia {class_via_dia_mm})").unwrap();
        writeln!(out, "\t\t(via_drill {class_via_drill_mm})").unwrap();
        writeln!(out, "\t\t(uvia_dia 0.3)").unwrap();
        writeln!(out, "\t\t(uvia_drill 0.1)").unwrap();
        for name in &nets_in_class {
            writeln!(out, "\t\t(add_net {})", sexpr_str(name)).unwrap();
        }
        writeln!(out, "\t)").unwrap();
    }

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

    // ---- drawings: graphic shapes + free text ----
    // Edge.Cuts stays `placement.outline`'s business, below, unchanged --
    // a shape a caller adds on that layer is exported as-is (whatever its
    // own layer says), never folded into the outline; see the report for
    // how the two should relate once outline editing exists.
    if let Some(drawings) = &design.drawings {
        let mut shapes: Vec<&Shape> = drawings.shapes.iter().collect();
        shapes.sort_by(|a, b| a.id().cmp(b.id()));
        for s in &shapes {
            write_shape(&mut out, s);
        }
        let mut texts: Vec<&Text> = drawings.texts.iter().collect();
        texts.sort_by(|a, b| a.id.cmp(&b.id));
        for t in &texts {
            write_text(&mut out, t);
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

/// The `.kicad_pro` project file `kicad-cli pcb drc` reads its design-rule
/// *constraint floors* from (minimum track width, minimum clearance,
/// minimum hole clearance, ...) -- a different thing from the per-net-class
/// *nominal* widths the `.kicad_pcb` itself carries in `net_class` blocks.
///
/// Without this file, `kicad-cli` has no project to load and falls back to
/// its own hard-coded floors (0.2mm minimum track/clearance, 0.25mm minimum
/// hole clearance): exactly the values a board with no `net_classes` at all
/// happens to already meet, and exactly what an escape class narrower than
/// that -- the reason `net_classes` exists -- fails, even though the
/// `.kicad_pcb`'s own `net_class` block correctly says that net wants less.
/// `track_width`/`clearance` DRC "violations" on a board that both our own
/// router and gates already passed are this gap, not a real defect, so this
/// writes the floors down to what the board actually asks of them; nothing
/// here loosens a constraint past what `model.board` itself specifies.
///
/// Fields this crate's model has no opinion on (via annular width, hole-to-
/// hole spacing, micro-via size) keep KiCad's own stock defaults -- small
/// enough to clear anything a 2-layer board like this routes, and not
/// something narrowing a signal net's clearance has any bearing on.
pub fn export_kicad_pro(model: &ConstraintModel) -> String {
    let min_clearance = model.board.net_classes.iter().filter_map(|c| c.clearance).fold(model.board.clearance, Um::min);
    let min_track_width = model.board.net_classes.iter().filter_map(|c| c.track_width).fold(model.board.track_width, Um::min);
    // The router keeps every pad -- through-hole or not -- at least the
    // board's own default clearance from copper on another net (see
    // `crates/freeroute`'s clearance matrix); that is the real guarantee
    // behind a hole on this board, so it is what the hole-clearance floor
    // should ask for, never more.
    let min_hole_clearance = model.board.clearance;
    format!(
        "{{\n  \"board\": {{\n    \"design_settings\": {{\n      \"rules\": {{\n        \"min_clearance\": {},\n        \"min_track_width\": {},\n        \"min_via_annular_width\": 0.1,\n        \"min_via_diameter\": 0.3,\n        \"min_hole_clearance\": {},\n        \"min_hole_to_hole\": 0.25,\n        \"min_through_hole_diameter\": 0.2,\n        \"min_microvia_diameter\": 0.2,\n        \"min_microvia_drill\": 0.1\n      }}\n    }}\n  }}\n}}\n",
        mm(min_clearance),
        mm(min_track_width),
        mm(min_hole_clearance),
    )
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
    // Text on a back layer is read from below, so it is written mirrored:
    // KiCad's own writer puts `(justify mirror)` after the font for a
    // flipped footprint's fields (EDA_TEXT::Format), and its DRC flags
    // back-layer text without it (nonmirrored_text_on_back_layer).
    let justify = if fp.side == Side::Bottom { " (justify mirror)" } else { "" };
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
        "\t\t(property \"Reference\" {} (at {ref_x} {ref_y} 0) (layer {})\n\t\t\t(uuid \"{ref_uuid}\")\n\t\t\t(effects (font (size 1 1) (thickness 0.15)){justify})\n\t\t)",
        sexpr_str(&fp.id),
        sexpr_str(ref_layer)
    )
    .unwrap();
    let val_uuid = duid(&format!("footprint:{}:val", fp.id));
    let fab_layer = if fp.side == Side::Bottom { "B.Fab" } else { "F.Fab" };
    writeln!(
        out,
        "\t\t(property \"Value\" {} (at 0 1 0) (layer {})\n\t\t\t(uuid \"{val_uuid}\")\n\t\t\t(effects (font (size 1 1) (thickness 0.15)){justify})\n\t\t)",
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
        // has to carry the mirrored x. The pad's own rotation, if any, is
        // handled by `pad_file_angle` (mirroring flips its sense; see
        // there), so it needs no mirroring of its own here.
        let px = mm(if fp.side == Side::Bottom { -pad.at.0 } else { pad.at.0 });
        let py = mm(pad.at.1);
        let (w, h) = pad.size;
        let wmm = mm(w);
        let hmm = mm(h);
        let kind = match pad.kind {
            PadKind::Smd => "smd",
            PadKind::ThroughHole => "thru_hole",
            PadKind::NonPlatedHole => "np_thru_hole",
        };
        let shape = match pad.shape {
            PadShape::Rect => "rect",
            PadShape::RoundRect => "roundrect",
            PadShape::Circle => "circle",
            PadShape::Oval => "oval",
        };
        // Pad numbers repeat by design (a connector's shield tab, several
        // physical pads on one pin), so the number alone cannot make a
        // unique uuid; the position can, since two pads of one footprint
        // never coincide.
        let uuid = duid(&format!("pad:{}:{}:{}:{}", fp.id, pad.number, pad.at.0, pad.at.1));
        // KiCad stores a pad's orientation as its footprint's own angle
        // plus the pad's own (0 for the overwhelming majority of pads) --
        // see `pad_file_angle` for the mirroring subtlety.
        let pad_rot_deg = crate::pad_file_angle(fp.side, fp.rot, pad.rot);
        write!(out, "\t\t(pad {} {kind} {shape} (at {px} {py} {pad_rot_deg})", sexpr_str(&pad.number)).unwrap();
        write!(out, " (size {wmm} {hmm})").unwrap();
        if pad.shape == PadShape::RoundRect {
            write!(out, " (roundrect_rratio {})", fmt_mm_f(pad.roundrect_ratio.unwrap_or(0.25))).unwrap();
        }
        match pad.kind {
            PadKind::ThroughHole | PadKind::NonPlatedHole => {
                // Footprint::validate guarantees a through-hole/non-plated
                // pad declares a drill, round or slot; inventing one here
                // shipped a different hole than the circuit-json writer
                // invented.
                match (pad.drill, pad.drill_slot) {
                    (Some(d), _) => write!(out, " (drill {})", mm(d)).unwrap(),
                    (None, Some((w, h))) => write!(out, " (drill oval {} {})", mm(w), mm(h)).unwrap(),
                    (None, None) => unreachable!("Footprint::validate requires a drill on a through-hole/non-plated pad"),
                }
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
        // A non-plated hole has no net by definition -- nothing to look up.
        if pad.kind != PadKind::NonPlatedHole {
            if let Some(net_name) = pin_net(&pad.number) {
                if let Some(&n) = net_num.get(net_name) {
                    write!(out, " (net {n} {})", sexpr_str(net_name)).unwrap();
                }
            }
        }
        writeln!(out, " (uuid \"{uuid}\"))").unwrap();
    }

    // 3D model reference, identity offset/scale/rotate -- this app has no
    // per-instance model adjustment to carry (KiCad's own UI lets a user
    // nudge an individual footprint's model, but nothing here ever sets
    // one), so every footprint that has a model at all gets the plain,
    // unmodified reference its library (or the built-in package map)
    // named. Path is written exactly as stored -- footprint_lib.rs reads
    // it straight out of the source library file, and footprint::
    // builtin_model_path already writes it in KiCad's own
    // `${KICADn_3DMODEL_DIR}/Lib.3dshapes/File.step` form.
    if let Some(model_path) = &footprint.model {
        writeln!(out, "\t\t(model {}", sexpr_str(model_path)).unwrap();
        writeln!(out, "\t\t\t(offset\n\t\t\t\t(xyz 0 0 0)\n\t\t\t)").unwrap();
        writeln!(out, "\t\t\t(scale\n\t\t\t\t(xyz 1 1 1)\n\t\t\t)").unwrap();
        writeln!(out, "\t\t\t(rotate\n\t\t\t\t(xyz 0 0 0)\n\t\t\t)").unwrap();
        writeln!(out, "\t\t)").unwrap();
    }

    writeln!(out, "\t)").unwrap();
}

/// A free-standing graphic shape as KiCad's `gr_line`/`gr_arc`/`gr_rect`/
/// `gr_circle`/`gr_poly`, matching `PCB_IO_KICAD_SEXPR::format(const
/// PCB_SHAPE*)`: stroke width and type first, `fill` only for the three
/// shapes KiCad actually fills (rect/circle/poly -- a line or an arc has no
/// interior, and KiCad's own writer never emits `fill` for either).
fn write_shape(out: &mut String, shape: &Shape) {
    let sw = |w: Um| mm(w.max(0));
    let uuid = duid(&format!("shape:{}", shape.id()));
    let layer = sexpr_str(shape.layer());
    let fill = |filled: bool| if filled { "yes" } else { "no" };
    match shape {
        Shape::Segment { stroke_width, start, end, .. } => {
            writeln!(
                out,
                "\t(gr_line (start {} {}) (end {} {}) (stroke (width {}) (type solid)) (layer {layer}) (uuid \"{uuid}\"))",
                mm(start.x), mm(start.y), mm(end.x), mm(end.y), sw(*stroke_width)
            )
            .unwrap();
        }
        Shape::Arc { stroke_width, start, mid, end, .. } => {
            writeln!(
                out,
                "\t(gr_arc (start {} {}) (mid {} {}) (end {} {}) (stroke (width {}) (type solid)) (layer {layer}) (uuid \"{uuid}\"))",
                mm(start.x), mm(start.y), mm(mid.x), mm(mid.y), mm(end.x), mm(end.y), sw(*stroke_width)
            )
            .unwrap();
        }
        Shape::Rect { stroke_width, filled, start, end, .. } => {
            writeln!(
                out,
                "\t(gr_rect (start {} {}) (end {} {}) (stroke (width {}) (type solid)) (fill {}) (layer {layer}) (uuid \"{uuid}\"))",
                mm(start.x), mm(start.y), mm(end.x), mm(end.y), sw(*stroke_width), fill(*filled)
            )
            .unwrap();
        }
        Shape::Circle { stroke_width, filled, center, end, .. } => {
            writeln!(
                out,
                "\t(gr_circle (center {} {}) (end {} {}) (stroke (width {}) (type solid)) (fill {}) (layer {layer}) (uuid \"{uuid}\"))",
                mm(center.x), mm(center.y), mm(end.x), mm(end.y), sw(*stroke_width), fill(*filled)
            )
            .unwrap();
        }
        Shape::Polygon { stroke_width, filled, pts, .. } => {
            write!(out, "\t(gr_poly (pts").unwrap();
            for p in pts {
                write!(out, " (xy {} {})", mm(p.x), mm(p.y)).unwrap();
            }
            writeln!(out, ") (stroke (width {}) (type solid)) (fill {}) (layer {layer}) (uuid \"{uuid}\"))", sw(*stroke_width), fill(*filled)).unwrap();
        }
    }
}

/// Free board text as KiCad's `gr_text`, matching `PCB_IO_KICAD_SEXPR::
/// format(const PCB_TEXT*)`/`EDA_TEXT::Format`: `(effects (font (size h w)
/// (thickness t)) (justify ...))`, `justify` present only when the text is
/// not centred/unmirrored (exactly KiCad's own rule, so a plain centred
/// label round-trips without growing a token it never had).
fn write_text(out: &mut String, text: &Text) {
    let uuid = duid(&format!("text:{}", text.id));
    let angle_deg = fmt_mm_f(text.angle as f64 / 1000.0);
    let size_mm = mm(text.size_um);
    let thickness_mm = mm(text.stroke_width);
    let mut justify = String::new();
    match text.justify {
        TextJustify::Left => justify.push_str(" left"),
        TextJustify::Right => justify.push_str(" right"),
        TextJustify::Center => {}
    }
    if text.mirror {
        justify.push_str(" mirror");
    }
    let justify_tok = if justify.is_empty() { String::new() } else { format!(" (justify{justify})") };
    writeln!(
        out,
        "\t(gr_text {} (at {} {} {angle_deg}) (layer {}) (uuid \"{uuid}\")\n\t\t(effects (font (size {size_mm} {size_mm}) (thickness {thickness_mm})){justify_tok})\n\t)",
        sexpr_str(&text.content),
        mm(text.at.x),
        mm(text.at.y),
        sexpr_str(&text.layer)
    )
    .unwrap();
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
        Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(format!("{reference}_val")), package: Some(package.into()), footprint: Some(package.into()), pins, body_um: None, symbol: None, datasheet: None, edge: None }
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
            schematic: None, nets: None,
            placement: Some(placement),
            routing: Some(RoutingSection {
                tracks: vec![Track { id: String::new(), net: "VIN".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 5_000, y: 5_000 }, Point { x: 10_000, y: 5_000 }] }],
                vias: vec![],
                zones: vec![],
            }),
            drawings: None,
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
    fn exports_shapes_and_text_on_their_own_layers() {
        let (mut design, model) = fixture();
        design.drawings = Some(eda_model::ir::DrawingsSection {
            shapes: vec![
                Shape::Segment { id: "s1".into(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 1000, y: 0 } },
                Shape::Rect { id: "s2".into(), layer: "F.Fab".into(), stroke_width: 100, filled: true, start: Point { x: 0, y: 0 }, end: Point { x: 2000, y: 2000 } },
                Shape::Circle { id: "s3".into(), layer: "B.SilkS".into(), stroke_width: 100, filled: false, center: Point { x: 5000, y: 5000 }, end: Point { x: 6000, y: 5000 } },
                Shape::Polygon { id: "s4".into(), layer: "F.CrtYd".into(), stroke_width: 50, filled: true, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }, Point { x: 1000, y: 1000 }] },
                Shape::Arc { id: "s5".into(), layer: "Cmts.User".into(), stroke_width: 100, filled: false, start: Point { x: 0, y: 0 }, mid: Point { x: 707, y: 707 }, end: Point { x: 1000, y: 1000 } },
            ],
            texts: vec![
                Text { id: "t1".into(), content: "REV A".into(), at: Point { x: 1000, y: 2000 }, angle: 90_000, layer: "F.SilkS".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Left, mirror: true },
                Text { id: "t2".into(), content: "centred".into(), at: Point { x: 0, y: 0 }, angle: 0, layer: "F.Fab".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false },
            ],
        });
        let a = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let b = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(a, b, "export must be deterministic");

        assert!(a.contains("(gr_line (start 0 0) (end 1 0)") && a.contains("(layer \"F.SilkS\")"), "{a}");
        assert!(a.contains("(gr_rect (start 0 0) (end 2 2)") && a.contains("(fill yes)") && a.contains("(layer \"F.Fab\")"), "{a}");
        assert!(a.contains("(gr_circle (center 5 5) (end 6 5)") && a.contains("(fill no)"), "{a}");
        assert!(a.contains("(gr_poly (pts (xy 0 0) (xy 1 0) (xy 1 1))"), "{a}");
        assert!(a.contains("(gr_arc (start 0 0) (mid 0.707 0.707) (end 1 1)"), "{a}");
        // A line/arc never gets a `fill` token -- KiCad's own writer does not emit one for either.
        assert!(!a[a.find("(gr_line").unwrap()..a.find("(gr_line").unwrap() + 200].contains("fill"), "{a}");

        assert!(a.contains("(gr_text \"REV A\" (at 1 2 90)") && a.contains("(justify left mirror)"), "{a}");
        assert!(a.contains("(gr_text \"centred\" (at 0 0 0)"), "{a}");
        // A centred, unmirrored text must not grow a `justify` token it never had.
        let centred_start = a.find("\"centred\"").unwrap();
        let centred_block = &a[centred_start..(centred_start + 200).min(a.len())];
        assert!(!centred_block.contains("justify"), "{centred_block}");
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
            id: String::new(),
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

    #[test]
    fn back_side_text_is_mirrored_and_front_side_text_is_not() {
        // Text on a back layer is read from below, so KiCad writes a
        // flipped footprint's fields mirrored, and its DRC flags either
        // side written the other way.
        let (design, model) = fixture();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let mut lines = out.lines();
        let mut properties = 0;
        while let Some(line) = lines.next() {
            if line.starts_with("\t\t(property ") {
                let block = std::iter::once(line).chain(lines.by_ref().take_while(|l| *l != "\t\t)")).collect::<Vec<_>>().join("\n");
                assert_eq!(block.contains("(justify mirror)"), block.contains("(layer \"B."), "{block}");
                properties += 1;
            }
        }
        // Reference and value of U1 (top) and C1 (bottom).
        assert_eq!(properties, 4);
    }
}

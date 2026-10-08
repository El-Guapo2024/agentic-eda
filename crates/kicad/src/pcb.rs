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

use crate::pcb_items::{write_dimension, write_group, write_zone, DimensionArgs, ZoneArgs};
use crate::{fmt_mm_f, mm, sexpr_str};

/// [`export_kicad_pcb`], also returning every exported item's KiCad uuid
/// mapped to our own item id (track `id`/`id#segment`, via, zone,
/// footprint, `REF.PAD`, shape, text; Edge.Cuts lines map to `outline`) --
/// how a kicad-cli report is pointed back at `design.json`.
pub fn export_kicad_pcb_mapped(design: &Design, model: &ConstraintModel, meta: &super::ExportMeta) -> Result<(String, std::collections::HashMap<String, String>), Vec<CheckResult>> {
    crate::start_uuid_map();
    let r = export_kicad_pcb(design, model, meta);
    let map = crate::take_uuid_map();
    r.map(|s| (s, map))
}

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
        (46, "B.CrtYd", "user"),
        (47, "F.CrtYd", "user"),
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
    // The drill/place file origin (`pcbnew.EditorControl.drillOrigin`): what kicad-cli measures drill, position and
    // (when asked) Gerber coordinates from.
    let aux = design.drawings.as_ref().and_then(|d| d.aux_origin).unwrap_or(eda_model::ir::Point { x: 0, y: 0 });
    writeln!(out, "\t\t(aux_axis_origin {} {})", mm(aux.x), mm(aux.y)).unwrap();
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

    // `BOARD_ITEM::IsLocked()`: every locked item's id (`DrawingsSection::locked_ids`). Each item kind
    // writes `(locked yes)` where KiCad's own writer does, so a locked board comes back locked.
    let locked: std::collections::BTreeSet<&str> = design.drawings.as_ref().map(|d| d.locked_ids.iter().map(String::as_str).collect()).unwrap_or_default();
    let is_locked = |id: &str| !id.is_empty() && locked.contains(id);
    // Our item id -> the KiCad uuids it was written as (a polyline track is several segments): what a
    // group's `(members ..)` names.
    let mut written: BTreeMap<String, Vec<String>> = BTreeMap::new();

    // ---- footprints ----
    for fp in &footprints {
        let part = parts_by_ref[fp.id.as_str()];
        let Some(footprint) = model.footprint_of(part) else {
            errors.push(CheckResult::fail("kicad.no_footprint", fp.id.clone(), "part has no resolvable footprint geometry"));
            continue;
        };
        let uuid = write_footprint(&mut out, fp, part, &footprint, &net_num, model, is_locked(&fp.id));
        written.entry(fp.id.clone()).or_default().push(uuid);
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
    // in the netlist or the export fails. An item the design says has no
    // net at all (an empty name: a rule area, or copper an imported board
    // drew with no net) is KiCad's net 0 on purpose, and says so.
    let mut errors: Vec<CheckResult> = Vec::new();
    let mut net_of = |what: &str, where_: &str, net: &str| -> usize {
        if net.is_empty() {
            return 0;
        }
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
            let lock = if is_locked(&t.id) { " (locked yes)" } else { "" };
            let w = mm(t.width);
            if let Some((start, mid, end)) = t.arc() {
                // `PCB_ARC`: one `(arc (start) (mid) (end) (width))` item. `Track::arc()` only reports an arc
                // while the polyline still is its tessellation, so an edited arc is written as the segments
                // it has become and never as a stale curve.
                let uuid = crate::duid_for(&format!("arc:{}:{}:{}", t.net, t.layer, ti), &t.id);
                written.entry(t.id.clone()).or_default().push(uuid.clone());
                writeln!(
                    out,
                    "\t(arc (start {} {}) (mid {} {}) (end {} {}) (width {w}){lock} (layer {}) (net {n}) (uuid \"{uuid}\"))",
                    mm(start.x),
                    mm(start.y),
                    mm(mid.x),
                    mm(mid.y),
                    mm(end.x),
                    mm(end.y),
                    sexpr_str(&t.layer)
                )
                .unwrap();
                continue;
            }
            for (j, pair) in t.pts.windows(2).enumerate() {
                let x1 = mm(pair[0].x);
                let y1 = mm(pair[0].y);
                let x2 = mm(pair[1].x);
                let y2 = mm(pair[1].y);
                let uuid = crate::duid_for(&format!("segment:{}:{}:{}:{}", t.net, t.layer, ti, j), &if j == 0 { t.id.clone() } else { format!("{}#{j}", t.id) });
                written.entry(t.id.clone()).or_default().push(uuid.clone());
                writeln!(out, "\t(segment (start {x1} {y1}) (end {x2} {y2}) (width {w}){lock} (layer {}) (net {n}) (uuid \"{uuid}\"))", sexpr_str(&t.layer)).unwrap();
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
            let lock = if is_locked(&v.id) { " (locked yes)" } else { "" };
            let uuid = crate::duid_for(&format!("via:{}:{}:{}", v.net, v.at.x, v.at.y), &v.id);
            written.entry(v.id.clone()).or_default().push(uuid.clone());
            writeln!(
                out,
                "\t(via (at {x} {y}) (size {dia}) (drill {drill}) (layers {} {}){lock} (net {n}) (uuid \"{uuid}\"))",
                sexpr_str(&v.from_layer),
                sexpr_str(&v.to_layer)
            )
            .unwrap();
        }

        // ---- copper pours ----
        // The outline is ours; so is the fill now (`eda_zone_filler`,
        // stage 3/4 of the zone-filling port) -- computed fresh here and
        // written as `filled_polygon`, so KiCad shows (and plots/exports
        // gerbers for) our fill directly without needing its own refill
        // pass. `(filled_areas_thickness no)` tells KiCad the polygon
        // itself is already the final min-thickness-correct shape (true
        // for every fill `eda_zone_filler::fill_zone` produces), not a
        // centerline needing a stroke width added.
        let mut zones: Vec<&Zone> = routing.zones.iter().collect();
        zones.sort_by(|a, b| (&a.net, &a.layer).cmp(&(&b.net, &b.layer)));
        let drc_board_for_fill = eda_drc::board::build(design, model);
        let fills = eda_drc::fill::fill_all_zones(&drc_board_for_fill, &model.board);
        for z in &zones {
            if z.outline.len() < 3 {
                continue;
            }
            // A rule area has no net at all, whatever its `net` says (`ZONE::GetIsRuleArea()`); asking the
            // net check about it rejected the whole export with `kicad.unknown_net`.
            let n = if z.is_rule_area { 0 } else { net_of("a copper pour", &format!("{} on {}", z.net, z.layer), &z.net) };
            // Seeded by the zone's own id: two pours on one net and layer must not share a uuid.
            let uuid = crate::duid_for(&if z.id.is_empty() { format!("zone:{}:{}", z.net, z.layer) } else { format!("zone:{}", z.id) }, &z.id);
            written.entry(z.id.clone()).or_default().push(uuid.clone());
            // Every disjoint fragment of the fill, as the one ring `(filled_polygon ..)` stores.
            let fill: Vec<Vec<(i64, i64)>> = fills
                .get(&z.id)
                .map(|f| f.polys.iter().filter_map(|poly| poly.first()).filter(|ring| ring.len() >= 3).map(|ring| ring.iter().map(|p| (p.x, p.y)).collect()).collect())
                .unwrap_or_default();
            write_zone(&mut out, z, &ZoneArgs { net: n, locked: is_locked(&z.id), uuid: &uuid, fill: &fill });
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
            let uuid = write_shape(&mut out, s, is_locked(s.id()));
            written.entry(s.id().to_string()).or_default().push(uuid);
        }
        let mut texts: Vec<&Text> = drawings.texts.iter().collect();
        texts.sort_by(|a, b| a.id.cmp(&b.id));
        for t in &texts {
            let uuid = write_text(&mut out, t, is_locked(&t.id));
            written.entry(t.id.clone()).or_default().push(uuid);
        }
        // `PCB_DIMENSION_BASE`: the geometry and the text are worked out in one place
        // (`eda_connectivity::dimension`); KiCad recomputes both when it loads the file.
        let mut dims: Vec<&eda_model::ir::Dimension> = drawings.dimensions.iter().collect();
        dims.sort_by(|a, b| a.id.cmp(&b.id));
        for d in &dims {
            let geom = eda_connectivity::dimension::compute_dimension_geometry(d);
            let uuid = crate::duid_for(&format!("dimension:{}", d.id), &d.id);
            let text_uuid = crate::duid_for(&format!("dimension:{}:text", d.id), &d.id);
            written.entry(d.id.clone()).or_default().push(uuid.clone());
            write_dimension(
                &mut out,
                d,
                &DimensionArgs { uuid: &uuid, text_uuid: &text_uuid, locked: is_locked(&d.id), text: &geom.text, text_at: (geom.text_at.x, geom.text_at.y), text_angle_millideg: geom.text_angle },
            );
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
            let uuid = crate::duid_for(&format!("edge:{i}:{}:{}:{}:{}", a.x, a.y, b.x, b.y), "outline");
            writeln!(out, "\t(gr_line (start {x1} {y1}) (end {x2} {y2}) (layer \"Edge.Cuts\") (uuid \"{uuid}\"))").unwrap();
        }
    }

    // ---- groups ---- written last, as KiCad does: a group names its members by uuid, and the parser
    // resolves them once every item has been read.
    if let Some(drawings) = &design.drawings {
        let mut groups: Vec<&eda_model::ir::Group> = drawings.groups.iter().collect();
        groups.sort_by(|a, b| a.id.cmp(&b.id));
        for g in &groups {
            let members: Vec<String> = g.member_ids.iter().filter_map(|m| written.get(m)).flatten().cloned().collect();
            let uuid = crate::duid_for(&format!("group:{}", g.id), &g.id);
            write_group(&mut out, g, &uuid, is_locked(&g.id), members);
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
    // Per-type DRC severities: the imported project's own, plus "ignore" for
    // the two library-link checks -- every footprint here is embedded in the
    // board file (`eda:<name>`), so there is no library for KiCad to compare
    // it against and each one would report a meaningless warning.
    let mut severities: BTreeMap<&str, &str> = BTreeMap::from([("lib_footprint_issues", "ignore"), ("lib_footprint_mismatch", "ignore")]);
    for (key, value) in &model.board.rule_severities {
        severities.insert(key.as_str(), value.as_str());
    }
    format!(
        "{{\n  \"board\": {{\n    \"design_settings\": {{\n      \"rule_severities\": {},\n      \"rules\": {{\n        \"min_clearance\": {},\n        \"min_track_width\": {},\n        \"min_via_annular_width\": 0.1,\n        \"min_via_diameter\": 0.3,\n        \"min_hole_clearance\": {},\n        \"min_hole_to_hole\": 0.25,\n        \"min_through_hole_diameter\": 0.2,\n        \"min_microvia_diameter\": 0.2,\n        \"min_microvia_drill\": 0.1\n      }}\n    }}\n  }}\n}}\n",
        serde_json::to_string(&severities).unwrap_or_else(|_| "{}".into()),
        mm(min_clearance),
        mm(min_track_width),
        mm(min_hole_clearance),
    )
}

/// The design's own ERC pin-to-pin conflict matrix (Schematic Setup > ERC
/// pin map) when it stores a usable one -- 12 x 12, every cell 0 (ok), 1
/// (warning) or 2 (error) -- else `None`, and KiCad's own default applies.
pub fn custom_erc_pin_map(design: &Design) -> Option<Vec<Vec<u8>>> {
    let m = &design.schematic.as_ref()?.erc_pin_map.as_ref()?.matrix;
    (m.len() == 12 && m.iter().all(|r| r.len() == 12 && r.iter().all(|&c| c <= 2))).then(|| m.clone())
}

/// [`export_kicad_pro`] plus the schematic side of the project (`erc`): the
/// design's own pin map, so kicad-cli's ERC judges pin conflicts by the matrix
/// the user set up and not only by KiCad's default; and the two library-link
/// checks ignored (every symbol is embedded in the derived schematic, so
/// there is no library for KiCad to compare it against and each one would
/// report a meaningless `lib_symbol_mismatch`).
pub fn export_kicad_pro_for(design: &Design, model: &ConstraintModel) -> String {
    let base = export_kicad_pro(model);
    let mut erc = serde_json::json!({ "rule_severities": { "lib_symbol_issues": "ignore", "lib_symbol_mismatch": "ignore" } });
    if let Some(m) = custom_erc_pin_map(design) {
        erc["pin_map"] = serde_json::json!(m);
    }
    base.replacen("{\n", &format!("{{\n  \"erc\": {},\n", serde_json::to_string(&erc).unwrap_or_else(|_| "{}".into())), 1)
}

/// One footprint; returns the uuid it was written with (a group's `(members ..)` names it).
fn write_footprint(
    out: &mut String,
    fp: &FootprintInstance,
    part: &Part,
    footprint: &eda_model::Footprint,
    net_num: &BTreeMap<&str, usize>,
    model: &ConstraintModel,
    locked: bool,
) -> String {
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
    let uuid = crate::duid_for(&format!("footprint:{}", fp.id), &fp.id);
    let lib_name = part.footprint.clone().unwrap_or_else(|| footprint.name.clone());
    let lib_id = if lib_name.contains(':') { lib_name } else { format!("eda:{lib_name}") };

    writeln!(out, "\t(footprint {}", sexpr_str(&lib_id)).unwrap();
    if locked {
        writeln!(out, "\t\t(locked yes)").unwrap();
    }
    writeln!(out, "\t\t(layer {})", sexpr_str(layer)).unwrap();
    writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
    writeln!(out, "\t\t(at {x} {y} {rot_deg})").unwrap();

    let ref_layer = if fp.side == Side::Bottom { "B.SilkS" } else { "F.SilkS" };
    // Text on a back layer is read from below, so it is written mirrored:
    // KiCad's own writer puts `(justify mirror)` after the font for a
    // flipped footprint's fields (EDA_TEXT::Format), and its DRC flags
    // back-layer text without it (nonmirrored_text_on_back_layer).
    let justify = if fp.side == Side::Bottom { " (justify mirror)" } else { "" };
    let ref_uuid = crate::duid_for(&format!("footprint:{}:ref", fp.id), &fp.id);
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
    let val_uuid = crate::duid_for(&format!("footprint:{}:val", fp.id), &fp.id);
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
        let uuid = crate::duid_for(&format!("pad:{}:{}:{}:{}", fp.id, pad.number, pad.at.0, pad.at.1), &format!("{}.{}", fp.id, pad.number));
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
                let (cu, paste, mask) = if pad.on_back(fp.side) {
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

    // The courtyard, on the footprint's own side: what KiCad's DRC reads for
    // `courtyards_overlap`, `pth_inside_courtyard` and `npth_inside_courtyard`
    // (the gates that call kicad-cli for those get nothing to judge from a
    // footprint with none). The real outlines when the library footprint has
    // them (x mirrored for a bottom-side part, like a pad's), else the box
    // `Footprint::courtyard_half` derives -- the same one the placer keeps
    // clear.
    let crtyd_layer = if fp.side == Side::Bottom { "B.CrtYd" } else { "F.CrtYd" };
    if footprint.courtyard_outlines.is_empty() {
        let (hw, hh) = footprint.courtyard_half();
        let uuid = crate::duid_for(&format!("footprint:{}:crtyd", fp.id), &fp.id);
        writeln!(
            out,
            "\t\t(fp_rect (start {} {}) (end {} {}) (stroke (width 0.05) (type solid)) (fill none) (layer {}) (uuid \"{uuid}\"))",
            mm(-hw),
            mm(-hh),
            mm(hw),
            mm(hh),
            sexpr_str(crtyd_layer)
        )
        .unwrap();
    } else {
        for (i, outline) in footprint.courtyard_outlines.iter().enumerate() {
            let pts: Vec<String> = outline.iter().map(|&(x, y)| format!("(xy {} {})", mm(if fp.side == Side::Bottom { -x } else { x }), mm(y))).collect();
            let uuid = crate::duid_for(&format!("footprint:{}:crtyd:{i}", fp.id), &fp.id);
            writeln!(out, "\t\t(fp_poly (pts {}) (stroke (width 0.05) (type solid)) (fill none) (layer {}) (uuid \"{uuid}\"))", pts.join(" "), sexpr_str(crtyd_layer)).unwrap();
        }
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
    //
    // A footprint with no model of its own -- a project-library entry, a `.kicad_mod` read for its pads -- gets the one the part's generic package name
    // stands for (`eda_model::footprint::kicad_footprint_for`: `0603` on an `R` is `Resistor_SMD:R_0603_1608Metric`, `SOIC-16` is
    // `Package_SO:SOIC-16_3.9x9.9mm_P1.27mm`, ...): without a `(model ...)` kicad-cli's 3D export leaves the part out altogether.
    let model_path = footprint.model.clone().or_else(|| eda_model::footprint::model_path_for_part(part));
    if let Some(model_path) = &model_path {
        writeln!(out, "\t\t(model {}", sexpr_str(model_path)).unwrap();
        writeln!(out, "\t\t\t(offset\n\t\t\t\t(xyz 0 0 0)\n\t\t\t)").unwrap();
        writeln!(out, "\t\t\t(scale\n\t\t\t\t(xyz 1 1 1)\n\t\t\t)").unwrap();
        writeln!(out, "\t\t\t(rotate\n\t\t\t\t(xyz 0 0 0)\n\t\t\t)").unwrap();
        writeln!(out, "\t\t)").unwrap();
    }

    writeln!(out, "\t)").unwrap();
    uuid
}

/// A free-standing graphic shape as KiCad's `gr_line`/`gr_arc`/`gr_rect`/
/// `gr_circle`/`gr_poly`, matching `PCB_IO_KICAD_SEXPR::format(const
/// PCB_SHAPE*)`: stroke width and type first, `fill` only for the three
/// shapes KiCad actually fills (rect/circle/poly -- a line or an arc has no
/// interior, and KiCad's own writer never emits `fill` for either).
fn write_shape(out: &mut String, shape: &Shape, locked: bool) -> String {
    let sw = |w: Um| mm(w.max(0));
    let uuid = crate::duid_for(&format!("shape:{}", shape.id()), shape.id());
    let layer = sexpr_str(shape.layer());
    let fill = |filled: bool| if filled { "yes" } else { "no" };
    let lock = if locked { " (locked yes)" } else { "" };
    match shape {
        Shape::Segment { stroke_width, start, end, .. } => {
            writeln!(
                out,
                "\t(gr_line (start {} {}) (end {} {}) (stroke (width {}) (type solid)){lock} (layer {layer}) (uuid \"{uuid}\"))",
                mm(start.x), mm(start.y), mm(end.x), mm(end.y), sw(*stroke_width)
            )
            .unwrap();
        }
        Shape::Arc { stroke_width, start, mid, end, .. } => {
            writeln!(
                out,
                "\t(gr_arc (start {} {}) (mid {} {}) (end {} {}) (stroke (width {}) (type solid)){lock} (layer {layer}) (uuid \"{uuid}\"))",
                mm(start.x), mm(start.y), mm(mid.x), mm(mid.y), mm(end.x), mm(end.y), sw(*stroke_width)
            )
            .unwrap();
        }
        Shape::Rect { stroke_width, filled, start, end, .. } => {
            writeln!(
                out,
                "\t(gr_rect (start {} {}) (end {} {}) (stroke (width {}) (type solid)) (fill {}){lock} (layer {layer}) (uuid \"{uuid}\"))",
                mm(start.x), mm(start.y), mm(end.x), mm(end.y), sw(*stroke_width), fill(*filled)
            )
            .unwrap();
        }
        Shape::Circle { stroke_width, filled, center, end, .. } => {
            writeln!(
                out,
                "\t(gr_circle (center {} {}) (end {} {}) (stroke (width {}) (type solid)) (fill {}){lock} (layer {layer}) (uuid \"{uuid}\"))",
                mm(center.x), mm(center.y), mm(end.x), mm(end.y), sw(*stroke_width), fill(*filled)
            )
            .unwrap();
        }
        Shape::Polygon { stroke_width, filled, pts, .. } => {
            write!(out, "\t(gr_poly (pts").unwrap();
            for p in pts {
                write!(out, " (xy {} {})", mm(p.x), mm(p.y)).unwrap();
            }
            writeln!(out, ") (stroke (width {}) (type solid)) (fill {}){lock} (layer {layer}) (uuid \"{uuid}\"))", sw(*stroke_width), fill(*filled)).unwrap();
        }
        // `case SHAPE_T::BEZIER:` -- `(gr_curve (pts (xy start) (xy c1) (xy c2) (xy end)) ...)`; like a line, never filled.
        Shape::Bezier { stroke_width, start, c1, c2, end, .. } => {
            writeln!(
                out,
                "\t(gr_curve (pts (xy {} {}) (xy {} {}) (xy {} {}) (xy {} {})) (stroke (width {}) (type solid)){lock} (layer {layer}) (uuid \"{uuid}\"))",
                mm(start.x), mm(start.y), mm(c1.x), mm(c1.y), mm(c2.x), mm(c2.y), mm(end.x), mm(end.y), sw(*stroke_width)
            )
            .unwrap();
        }
    }
    uuid
}

/// Free board text as KiCad's `gr_text`, matching `PCB_IO_KICAD_SEXPR::
/// format(const PCB_TEXT*)`/`EDA_TEXT::Format`: `(effects (font (size h w)
/// (thickness t)) (justify ...))`, `justify` present only when the text is
/// not centred/unmirrored (exactly KiCad's own rule, so a plain centred
/// label round-trips without growing a token it never had).
fn write_text(out: &mut String, text: &Text, locked: bool) -> String {
    let uuid = crate::duid_for(&format!("text:{}", text.id), &text.id);
    let lock = if locked { " (locked yes)" } else { "" };
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
        "\t(gr_text {}{lock} (at {} {} {angle_deg}) (layer {}) (uuid \"{uuid}\")\n\t\t(effects (font (size {size_mm} {size_mm}) (thickness {thickness_mm})){justify_tok})\n\t)",
        sexpr_str(&text.content),
        mm(text.at.x),
        mm(text.at.y),
        sexpr_str(&text.layer)
    )
    .unwrap();
    uuid
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
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: Some(placement),
            routing: Some(RoutingSection {
                tracks: vec![Track { id: String::new(), net: "VIN".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 5_000, y: 5_000 }, Point { x: 10_000, y: 5_000 }], arc_mid_offset: None }],
                vias: vec![],
                zones: vec![],
                track_width_presets: vec![],
                via_presets: vec![],
                teardrop_settings: Default::default(),
            }),
            drawings: None,
        };
        (design, model)
    }

    fn meta() -> super::super::ExportMeta<'static> {
        super::super::ExportMeta { date: "2026-01-01", title: "t" }
    }

    #[test]
    fn the_drill_origin_is_written_as_the_aux_axis_origin_and_read_back_by_the_importer() {
        let (mut design, model) = fixture();
        assert!(export_kicad_pcb(&design, &model, &meta()).unwrap().contains("(aux_axis_origin 0 0)"), "none set: KiCad's default");
        design.drawings = Some(eda_model::ir::DrawingsSection { aux_origin: Some(Point { x: 12_500, y: -3_000 }), ..Default::default() });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(aux_axis_origin 12.5 -3"), "{out}");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        assert_eq!(back.drawings.and_then(|d| d.aux_origin), Some(Point { x: 12_500, y: -3_000 }), "an imported board keeps its drill/place file origin");
        // A board saved with the default origin imports with none set (no empty drawings section is invented for it).
        let (plain, _, _) = crate::import_kicad_pcb(&export_kicad_pcb(&fixture().0, &model, &meta()).unwrap()).unwrap();
        assert!(plain.drawings.and_then(|d| d.aux_origin).is_none());
    }

    #[test]
    fn deterministic_export() {
        let (design, model) = fixture();
        let a = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let b = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn every_footprint_exports_its_courtyard_on_its_own_side() {
        // kicad-cli judges `courtyards_overlap` from F.CrtYd/B.CrtYd graphics;
        // a footprint with none gives the gates that ask it nothing to see.
        // `(fill none)`, not `(fill no)`: kicad-cli reads the latter as no
        // courtyard at all (found the hard way -- every overlap went unreported).
        let (design, model) = fixture();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let fps = &design.placement.as_ref().unwrap().footprints;
        let (top, bottom) = (fps.iter().filter(|f| f.side == Side::Top).count(), fps.iter().filter(|f| f.side == Side::Bottom).count());
        assert!(top > 0 && bottom > 0, "the fixture has parts on both sides");
        assert_eq!(out.matches("(layer \"F.CrtYd\")").count(), top, "one courtyard per top-side footprint");
        assert_eq!(out.matches("(layer \"B.CrtYd\")").count(), bottom, "a bottom-side footprint's courtyard is on B.CrtYd");
        assert!(out.contains("(fp_rect (start ") && out.contains("(fill none) (layer \"F.CrtYd\")"), "{out}");
        assert!(out.contains("(46 \"B.CrtYd\" user)") && out.contains("(47 \"F.CrtYd\" user)"), "both courtyard layers declared");
    }

    #[test]
    fn the_project_carries_the_designs_own_erc_pin_map_and_the_embedded_library_severities() {
        let (mut design, model) = fixture();
        let plain: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).expect("valid json");
        assert!(plain["erc"]["pin_map"].is_null(), "no custom map: KiCad's own default applies, nothing is written");
        assert_eq!(plain["erc"]["rule_severities"]["lib_symbol_mismatch"], "ignore");
        assert_eq!(plain["erc"]["rule_severities"]["lib_symbol_issues"], "ignore");
        design.schematic = Some(eda_model::ir::SchematicSection {
            symbols: vec![], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![],
            erc_pin_map: Some(eda_model::ir::ErcPinMap { matrix: { let mut m = eda_model::ir::ErcPinMap::default_matrix(); m[1][1] = 0; m } }),
            user_fields: Default::default(), title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(), imported_from_kicad: false,
        });
        let custom = export_kicad_pro_for(&design, &model);
        let json: serde_json::Value = serde_json::from_str(&custom).expect("valid json");
        assert_eq!(json["erc"]["pin_map"][1][1], 0, "{custom}");
        assert_eq!(json["erc"]["pin_map"].as_array().map(Vec::len), Some(12));
        assert_eq!(json["erc"]["rule_severities"]["lib_symbol_mismatch"], "ignore");
        assert!(json["board"]["design_settings"]["rules"]["min_clearance"].is_number(), "the board half is untouched");
        // A malformed matrix is ignored wholesale, like KiCad's own loader.
        design.schematic.as_mut().unwrap().erc_pin_map = Some(eda_model::ir::ErcPinMap { matrix: vec![vec![0; 3]; 3] });
        let bad: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).expect("valid json");
        assert!(bad["erc"]["pin_map"].is_null());
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
                Shape::Bezier { id: "s6".into(), layer: "F.SilkS".into(), stroke_width: 120, filled: false, start: Point { x: 0, y: 0 }, c1: Point { x: 0, y: 2000 }, c2: Point { x: 3000, y: 2000 }, end: Point { x: 3000, y: 0 } },
            ],
            texts: vec![
                Text { id: "t1".into(), content: "REV A".into(), at: Point { x: 1000, y: 2000 }, angle: 90_000, layer: "F.SilkS".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Left, mirror: true },
                Text { id: "t2".into(), content: "centred".into(), at: Point { x: 0, y: 0 }, angle: 0, layer: "F.Fab".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false },
            ],
            ..Default::default()
        });
        let a = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let b = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(a, b, "export must be deterministic");

        assert!(a.contains("(gr_line (start 0 0) (end 1 0)") && a.contains("(layer \"F.SilkS\")"), "{a}");
        assert!(a.contains("(gr_rect (start 0 0) (end 2 2)") && a.contains("(fill yes)") && a.contains("(layer \"F.Fab\")"), "{a}");
        assert!(a.contains("(gr_circle (center 5 5) (end 6 5)") && a.contains("(fill no)"), "{a}");
        assert!(a.contains("(gr_poly (pts (xy 0 0) (xy 1 0) (xy 1 1))"), "{a}");
        assert!(a.contains("(gr_arc (start 0 0) (mid 0.707 0.707) (end 1 1)"), "{a}");
        // `case SHAPE_T::BEZIER:` -- `(gr_curve (pts (xy start) (xy c1) (xy c2) (xy end)) ...)`, never `fill`ed.
        let curve = a.find("(gr_curve (pts (xy 0 0) (xy 0 2) (xy 3 2) (xy 3 0))").unwrap_or_else(|| panic!("{a}"));
        assert!(a[curve..curve + 140].contains("(stroke (width 0.12)") && !a[curve..curve + 140].contains("fill"), "{a}");
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
            ..Default::default()
        }];
        let err = export_kicad_pcb(&design, &model, &meta()).unwrap_err();
        assert!(err.iter().any(|c| c.check == "kicad.unknown_net"), "got {err:?}");
    }

    #[test]
    fn a_rule_area_exports_as_a_keepout_zone_without_a_net() {
        // `pcb_io_kicad_sexpr.cpp` `format(const ZONE*)`: a rule area has no `(net ..)` at all and
        // carries `(keepout ..)` + `(placement ..)`. Written as a pour it asked the net check for
        // a net named "" and the whole export failed with `kicad.unknown_net`.
        let (mut design, model) = fixture();
        design.routing.as_mut().unwrap().zones = vec![Zone {
            id: "ra1".into(),
            net: String::new(),
            layer: "F.Cu".into(),
            outline: vec![Point { x: 1_000, y: 1_000 }, Point { x: 8_000, y: 1_000 }, Point { x: 8_000, y: 8_000 }, Point { x: 1_000, y: 8_000 }],
            is_rule_area: true,
            keepout_tracks: true,
            keepout_vias: true,
            ..Default::default()
        }];
        let out = export_kicad_pcb(&design, &model, &meta()).expect("a board with a rule area must export");
        let zone = out.split("\t(zone").nth(1).expect("one zone block");
        assert!(zone.contains("(keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour allowed) (footprints allowed))"), "{zone}");
        assert!(zone.contains("(placement") && zone.contains("(enabled no)"), "{zone}");
        assert!(!zone.contains("(net ") && !zone.contains("(net_name"), "a rule area has no net: {zone}");
        assert!(!zone.contains("filled_polygon"), "a rule area is never filled: {zone}");
    }

    fn square(x: Um, y: Um, side: Um) -> Vec<eda_model::ir::Point> {
        vec![
            eda_model::ir::Point { x, y },
            eda_model::ir::Point { x: x + side, y },
            eda_model::ir::Point { x: x + side, y: y + side },
            eda_model::ir::Point { x, y: y + side },
        ]
    }

    /// The text of the first `(zone ...)` block.
    fn first_zone(out: &str) -> &str {
        let at = out.find("\t(zone").expect("a zone block");
        let rest = &out[at..];
        &rest[..rest.find("\n\t(").map_or(rest.len(), |i| i + 1)]
    }

    #[test]
    fn every_zone_is_written_with_its_own_settings_and_reads_back_the_same() {
        // `pcb_io_kicad_sexpr.cpp` `format( const ZONE* )`: the zone's own priority, clearance, minimum width,
        // pad connection, thermal gap and spoke width, island removal, hatch fill, name and lock. All of them
        // used to come out as the board's clearance and track width.
        let (mut design, model) = fixture();
        let mut pour = Zone {
            id: "zp".into(),
            net: "GND".into(),
            layer: "B.Cu".into(),
            outline: square(1_000, 1_000, 10_000),
            clearance: 300,
            min_thickness: 150,
            thermal_gap: 420,
            thermal_spoke_width: 350,
            pad_connection: eda_model::ir::PadConnection::Full,
            priority: 3,
            island_removal_mode: eda_model::ir::IslandRemovalMode::Area,
            min_island_area: 2_500_000,
            fill_mode: eda_model::ir::FillMode::HatchPattern,
            hatch_thickness: 400,
            hatch_gap: 900,
            hatch_orientation_mdeg: 45_000,
            hatch_smoothing_level: 2,
            hatch_smoothing_value: 0.25,
            hatch_hole_min_area: 0.3,
            hatch_border_algorithm: 0,
            name: "GND plane".into(),
            border_style: eda_model::ir::ZoneBorderStyle::Full,
            smoothing: eda_model::ir::ZoneSmoothing::Fillet,
            corner_radius: 600,
            ..Default::default()
        };
        let mut thermal = Zone { id: "zt".into(), net: "VIN".into(), layer: "F.Cu".into(), outline: square(12_000, 1_000, 6_000), ..Default::default() };
        thermal.pad_connection = eda_model::ir::PadConnection::ThtThermal;
        design.routing.as_mut().unwrap().zones = vec![pour.clone(), thermal.clone()];
        design.drawings = Some(eda_model::ir::DrawingsSection { locked_ids: vec!["zp".into()], ..Default::default() });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();

        let gnd = out.split("\t(zone").skip(1).find(|z| z.contains("\"GND\"")).expect("the GND zone");
        for want in [
            "(locked yes)",
            "(name \"GND plane\")",
            "(hatch full 0.5)",
            "(priority 3)",
            "(connect_pads yes (clearance 0.3))",
            "(min_thickness 0.15)",
            "(mode hatch)",
            "(thermal_gap 0.42) (thermal_bridge_width 0.35)",
            "(smoothing fillet) (radius 0.6)",
            "(island_removal_mode 2) (island_area_min 2.5)",
            "(hatch_thickness 0.4) (hatch_gap 0.9) (hatch_orientation 45)",
            "(hatch_smoothing_level 2) (hatch_smoothing_value 0.25)",
            "(hatch_border_algorithm min_thickness) (hatch_min_hole_area 0.3)",
        ] {
            assert!(gnd.contains(want), "missing {want}:\n{gnd}");
        }
        let vin = out.split("\t(zone").skip(1).find(|z| z.contains("\"VIN\"")).expect("the VIN zone");
        assert!(vin.contains("(connect_pads thru_hole_only (clearance 0.5))"), "{vin}");
        assert!(!vin.contains("(priority") && !vin.contains("(locked") && !vin.contains("(name"), "defaults write nothing extra: {vin}");

        // And back: the importer reads every one of them.
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let zones = &back.routing.as_ref().unwrap().zones;
        let got = zones.iter().find(|z| z.net == "GND").expect("GND zone");
        pour.id = got.id.clone();
        assert_eq!(*got, pour);
        assert_eq!(zones.iter().find(|z| z.net == "VIN").unwrap().pad_connection, eda_model::ir::PadConnection::ThtThermal);
        assert!(back.drawings.as_ref().unwrap().locked_ids.contains(&got.id), "the zone comes back locked");
    }

    #[test]
    fn an_arc_track_is_written_as_one_arc_and_read_back_as_one() {
        use eda_model::ir::Point;
        let (mut design, model) = fixture();
        let arc = Track::new_arc("VIN".into(), "F.Cu".into(), 250, Point { x: 2_000, y: 2_000 }, Point { x: 4_000, y: 1_000 }, Point { x: 6_000, y: 2_000 });
        assert!(arc.pts.len() > 3, "the polyline is the tessellation");
        design.routing.as_mut().unwrap().tracks.push(arc);
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(out.matches("\t(arc ").count(), 1, "{out}");
        assert!(out.contains("(arc (start 2 2) (mid 4 1) (end 6 2) (width 0.25) (layer \"F.Cu\") (net 2)"), "{out}");
        assert_eq!(out.matches("\t(segment ").count(), 1, "only the plain track is a segment, not 32 chords of the arc: {out}");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let tracks = &back.routing.unwrap().tracks;
        let arcs: Vec<_> = tracks.iter().filter_map(|t| t.arc()).collect();
        assert_eq!(arcs, vec![(Point { x: 2_000, y: 2_000 }, Point { x: 4_000, y: 1_000 }, Point { x: 6_000, y: 2_000 })]);
    }

    #[test]
    fn a_track_that_was_reshaped_after_it_was_an_arc_is_written_as_segments() {
        use eda_model::ir::Point;
        let (mut design, model) = fixture();
        let mut arc = Track::new_arc("VIN".into(), "F.Cu".into(), 250, Point { x: 2_000, y: 2_000 }, Point { x: 4_000, y: 1_000 }, Point { x: 6_000, y: 2_000 });
        arc.pts.truncate(5); // an edit that cut the curve short: no longer the tessellation of its arc
        design.routing.as_mut().unwrap().tracks.push(arc);
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(out.matches("\t(arc ").count(), 0, "a stale arc is never written: {out}");
        assert_eq!(out.matches("\t(segment ").count(), 1 + 4);
    }

    #[test]
    fn locked_items_are_written_locked_and_come_back_locked() {
        use eda_model::ir::Point;
        let (mut design, model) = fixture();
        design.routing.as_mut().unwrap().vias.push(Via { id: String::new(), net: "VIN".into(), at: Point { x: 8_000, y: 8_000 }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        design.drawings = Some(eda_model::ir::DrawingsSection {
            shapes: vec![Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 1_000, y: 0 } }],
            texts: vec![Text { id: String::new(), content: "REV A".into(), at: Point { x: 1_000, y: 2_000 }, angle: 0, layer: "F.SilkS".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false }],
            ..Default::default()
        });
        design.assign_missing_ids();
        let rt = design.routing.as_ref().unwrap();
        let mut locked: Vec<String> = vec!["U1".into(), rt.tracks[0].id.clone(), rt.vias[0].id.clone()];
        locked.push(design.drawings.as_ref().unwrap().shapes[0].id().to_string());
        locked.push(design.drawings.as_ref().unwrap().texts[0].id.clone());
        design.drawings.as_mut().unwrap().locked_ids = locked.clone();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(out.matches("(locked yes)").count(), 5, "{out}");
        assert!(out.contains("(footprint \"eda:SOT-23\"\n\t\t(locked yes)\n\t\t(layer"), "{out}");

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let mut got = back.drawings.as_ref().unwrap().locked_ids.clone();
        got.sort();
        // The importer gives items ids of its own making, so compare by kind: a footprint, a track, a via, a shape, a text.
        assert_eq!(got.len(), 5, "{got:?}");
        assert!(got.contains(&"U1".to_string()));
        assert!(!got.contains(&"C1".to_string()), "C1 was not locked");
        let brt = back.routing.as_ref().unwrap();
        assert!(got.contains(&brt.tracks[0].id) && got.contains(&brt.vias[0].id));
    }

    #[test]
    fn a_dimension_is_written_with_its_format_and_style_and_reads_back_the_same() {
        use eda_model::ir::{ArrowDirection, Dimension, DimensionKind, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, Point};
        let (mut design, model) = fixture();
        let dim = |kind, start: Point, end: Point| Dimension {
            id: String::new(),
            layer: "Dwgs.User".into(),
            kind,
            start,
            end,
            prefix: "L=".into(),
            suffix: "".into(),
            override_text: None,
            units: DimensionUnits::Mm,
            units_format: DimensionUnitsFormat::BareSuffix,
            precision: 2,
            suppress_trailing_zeros: true,
            text_position: DimensionTextPosition::Outside,
            keep_text_aligned: true,
            text_angle: 0,
            text_size_um: 1_200,
            stroke_width: 150,
            arrow_length: 1_270,
            extension_offset: 500,
            extension_height: 580,
            arrow_direction: ArrowDirection::Inward,
            text_thickness_um: Some(180),
        };
        let mut dims = vec![
            dim(DimensionKind::Aligned { height: 2_000 }, Point { x: 2_000, y: 12_000 }, Point { x: 12_000, y: 12_000 }),
            dim(DimensionKind::Orthogonal { height: 1_500, horizontal: false }, Point { x: 2_000, y: 14_000 }, Point { x: 6_000, y: 18_000 }),
            dim(DimensionKind::Radial { leader_length: 3_000 }, Point { x: 15_000, y: 15_000 }, Point { x: 17_000, y: 15_000 }),
            dim(DimensionKind::Leader, Point { x: 10_000, y: 10_000 }, Point { x: 12_000, y: 8_000 }),
            dim(DimensionKind::Center, Point { x: 5_000, y: 5_000 }, Point { x: 6_000, y: 5_000 }),
        ];
        dims[0].override_text = Some("ten".into());
        dims[1].units = DimensionUnits::Mil;
        dims[1].keep_text_aligned = false;
        dims[1].text_position = DimensionTextPosition::Inline;
        let mut dr = eda_model::ir::DrawingsSection::default();
        dr.dimensions = dims.clone();
        dr.assign_missing_ids();
        dims = dr.dimensions.clone();
        design.drawings = Some(dr);
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(out.matches("\t(dimension ").count(), 5);
        for ty in ["aligned", "orthogonal", "radial", "leader", "center"] {
            assert!(out.contains(&format!("(dimension (type {ty})")), "{ty}: {out}");
        }
        assert!(out.contains("(height 2)") && out.contains("(orientation 1)") && out.contains("(leader_length 3)") && out.contains("(text_frame 0)"), "{out}");
        assert!(out.contains("(format (prefix \"L=\") (suffix \"\") (units 2) (units_format 1) (precision 2) (override_value \"ten\") (suppress_zeroes yes))"), "{out}");
        assert!(out.contains("(units 1)") && out.contains("(text_position_mode 1)"), "{out}");
        assert!(out.contains("(thickness 0.15) (arrow_length 1.27) (text_position_mode 0) (arrow_direction inward) (extension_height 0.58) (extension_offset 0.5) (keep_text_aligned yes)"), "{out}");
        assert!(out.contains("(effects (font (size 1.2 1.2) (thickness 0.18)))"), "{out}");

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let mut got = back.drawings.unwrap().dimensions;
        assert_eq!(got.len(), 5);
        // The manual angle is only meaningful when the text is not kept aligned.
        for g in &mut got {
            let want = dims.iter().find(|d| d.start == g.start && d.end == g.end).expect("same feature points");
            g.id = want.id.clone();
            if want.keep_text_aligned {
                g.text_angle = want.text_angle;
            }
            // Where KiCad has no such field, nothing is promised back: only the kinds with an extension height / arrow direction keep them.
            if matches!(want.kind, DimensionKind::Radial { .. } | DimensionKind::Leader | DimensionKind::Center) {
                g.extension_height = want.extension_height;
                g.arrow_direction = want.arrow_direction;
            }
            if matches!(want.kind, DimensionKind::Center) {
                g.prefix = want.prefix.clone();
                g.units = want.units;
                g.units_format = want.units_format;
                g.precision = want.precision;
                g.suppress_trailing_zeros = want.suppress_trailing_zeros;
                g.text_size_um = want.text_size_um;
                g.text_thickness_um = want.text_thickness_um;
            }
            assert_eq!(g.kind, want.kind);
            assert_eq!(g.layer, want.layer);
            assert_eq!((g.stroke_width, g.arrow_length, g.extension_offset), (want.stroke_width, want.arrow_length, want.extension_offset));
            assert_eq!((g.override_text.as_deref(), g.units, g.units_format, g.precision), (want.override_text.as_deref(), want.units, want.units_format, want.precision), "{:?}", want.kind);
            assert_eq!((g.text_position, g.keep_text_aligned), (want.text_position, want.keep_text_aligned));
        }
    }

    #[test]
    fn groups_are_written_with_their_members_uuids_and_read_back_with_their_ids() {
        use eda_model::ir::{Group, Point};
        let (mut design, model) = fixture();
        design.routing.as_mut().unwrap().vias.push(Via { id: String::new(), net: "VIN".into(), at: Point { x: 8_000, y: 8_000 }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        design.routing.as_mut().unwrap().tracks.push(Track { id: String::new(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 1_000, y: 6_000 }, Point { x: 5_000, y: 6_000 }, Point { x: 5_000, y: 9_000 }], arc_mid_offset: None });
        design.assign_missing_ids();
        let rt = design.routing.as_ref().unwrap();
        let (via_id, long_track) = (rt.vias[0].id.clone(), rt.tracks.iter().find(|t| t.net == "GND").unwrap().id.clone());
        let mut dr = eda_model::ir::DrawingsSection::default();
        dr.groups = vec![Group { id: String::new(), name: "decoupling".into(), member_ids: vec!["U1".into(), "C1".into(), via_id.clone(), long_track.clone(), "no such item".into()] }];
        dr.assign_missing_ids();
        design.drawings = Some(dr);
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let group = out.split("\t(group ").nth(1).expect("a group block");
        assert!(group.starts_with("\"decoupling\" (uuid \""), "{group}");
        // U1, C1, the via, both segments of the two-segment track; the stale id names nothing.
        assert_eq!(group.split("(members").nth(1).unwrap().matches('"').count() / 2, 5, "{group}");
        assert!(out.rfind("\t(group ").unwrap() > out.rfind("\t(segment ").unwrap(), "groups come last, like KiCad's own writer");

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let groups = back.drawings.as_ref().unwrap().groups.clone();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "decoupling");
        let brt = back.routing.as_ref().unwrap();
        assert_eq!(groups[0].member_ids.len(), 5, "{:?}", groups[0].member_ids);
        assert!(groups[0].member_ids.contains(&"U1".to_string()) && groups[0].member_ids.contains(&"C1".to_string()));
        assert!(groups[0].member_ids.contains(&brt.vias[0].id));
        for t in brt.tracks.iter().filter(|t| t.net == "GND") {
            assert!(groups[0].member_ids.contains(&t.id), "both segments of the track are members");
        }
    }

    #[test]
    fn a_teardrop_zone_is_flagged_and_carries_kicads_own_teardrop_settings() {
        // `TEARDROP_MANAGER::createTeardrop`: no clearance of its own, 0.0254 mm minimum width, full pad
        // connection, islands kept; `(attr (teardrop (type padvia)))` makes KiCad treat it as a teardrop.
        let (mut design, model) = fixture();
        design.routing.as_mut().unwrap().zones = vec![Zone {
            id: "td1".into(),
            net: "VIN".into(),
            layer: "F.Cu".into(),
            outline: vec![eda_model::ir::Point { x: 4_000, y: 4_000 }, eda_model::ir::Point { x: 5_000, y: 4_300 }, eda_model::ir::Point { x: 5_000, y: 5_300 }],
            teardrop: true,
            ..Default::default()
        }];
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let z = first_zone(&out);
        for want in ["(attr (teardrop (type padvia)))", "(hatch none 0.5)", "(connect_pads yes (clearance 0))", "(min_thickness 0.0254)", "(island_removal_mode 1)"] {
            assert!(z.contains(want), "missing {want}:\n{z}");
        }
        assert!(!z.contains("thermal_gap"), "a teardrop has no thermal relief: {z}");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        assert!(back.routing.unwrap().zones[0].teardrop, "the teardrop flag comes back");
    }

    #[test]
    fn copper_with_no_net_exports_as_net_zero_and_a_missing_net_still_fails() {
        // An imported board can carry copper that has no net (KiCad's net 0); that is not the silent default
        // the net check exists to catch, so it must not fail the export of the whole board.
        let (mut design, model) = fixture();
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: String::new(), net: String::new(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![eda_model::ir::Point { x: 1_000, y: 1_000 }, eda_model::ir::Point { x: 2_000, y: 1_000 }], arc_mid_offset: None });
        rt.zones = vec![Zone { id: "z0".into(), net: String::new(), layer: "F.Cu".into(), outline: square(10_000, 10_000, 3_000), ..Default::default() }];
        let out = export_kicad_pcb(&design, &model, &meta()).expect("netless copper exports");
        assert!(out.contains("(width 0.2) (layer \"F.Cu\") (net 0)"), "{out}");
        assert!(!first_zone(&out).contains("(net "), "{}", first_zone(&out));
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

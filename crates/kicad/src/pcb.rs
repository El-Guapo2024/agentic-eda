//! eda-kicad — exports `Design::placement`/`routing` to KiCad 9
//! `.kicad_pcb` text.
//!
//! Same philosophy as `lib.rs`'s schematic exporter: hand-rolled
//! s-expression emission, deterministic uuids via `duid`, everything
//! sorted by stable keys before emission.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use eda_model::ir::{Design, FootprintInstance, Shape, Side, Text, Track, Nm, Via, Zone};
use eda_model::layers::{LayerSetup, LayerType};
use eda_model::{CheckResult, ConstraintModel, Pad, PadKind, Part};

use crate::fp_fields::{pad_text, write_attr, write_fields, FpExtra};
use crate::pcb_items::{write_dimension, write_groups, write_zone, DimensionArgs, ZoneArgs};
use crate::{fmt_mm_f, mm, sexpr_str};

/// [`export_kicad_pcb`], also returning every exported item's KiCad uuid
/// mapped to our own item id (track `id`/`id#segment`, via, zone,
/// footprint, `REF.PAD`, shape, text; Edge.Cuts lines map to `outline`) --
/// how a kicad-cli report is pointed back at `design.json`.
pub fn export_kicad_pcb_mapped(design: &Design, model: &ConstraintModel, meta: &super::ExportMeta) -> Result<(String, std::collections::HashMap<String, String>), Vec<CheckResult>> {
    export_kicad_pcb_mapped_with(design, model, meta, None)
}

/// [`export_kicad_pcb_mapped`] with the fills the zones are written with: kicad-cli's (`crate::zone_fills`), `None` for none.
pub fn export_kicad_pcb_mapped_with(design: &Design, model: &ConstraintModel, meta: &super::ExportMeta, fills: Option<&crate::zone_fills::KicadFills>) -> Result<(String, std::collections::HashMap<String, String>), Vec<CheckResult>> {
    crate::start_uuid_map();
    let r = export_kicad_pcb_with(design, model, meta, fills);
    let map = crate::take_uuid_map();
    r.map(|s| (s, map))
}

/// The board with every zone unfilled: the fill is KiCad's (`crate::zone_fills`), made by kicad-cli, and a board that is not handed one has none to write.
pub fn export_kicad_pcb(design: &Design, model: &ConstraintModel, meta: &super::ExportMeta) -> Result<String, Vec<CheckResult>> {
    export_kicad_pcb_with(design, model, meta, None)
}

/// [`export_kicad_pcb`] with the fills of the zones: each zone is written with the rings kicad-cli made of it for as long as the zone is the one it filled (a zone that was
/// edited since is written unfilled, as KiCad writes a zone it has emptied), nanometre for nanometre and with `(island yes)` where kicad-cli said so.
pub fn export_kicad_pcb_with(design: &Design, model: &ConstraintModel, meta: &super::ExportMeta, fills: Option<&crate::zone_fills::KicadFills>) -> Result<String, Vec<CheckResult>> {
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

    // The pads of a pin the schematic has on no net of its own (a pin alone, a pin on a no-connect flag): KiCad's netlist gives each a net of its own
    // (`unconnected-(U1-NC1-Pad12)`, `SCH_PIN::GetDefaultNetName( .., aForceNoConnect = true )`), which Update PCB from Schematic puts on the pad
    // (`BOARD_NETLIST_UPDATER::updateComponentPadConnections`). A footprint read from a KiCad file keeps the pads the file gave no net.
    let unconnected = unconnected_pin_nets(design, model, &footprints, &parts_by_ref);
    let from_schematic = FromSchematic { unconnected: unconnected.clone(), root_sheetfile: format!("{}.kicad_sch", meta.title) };
    // Net name -> number, sorted by name, 1-indexed (0 is the unconnected net).
    let mut net_names: Vec<&str> = model.nets.iter().map(|n| n.name.as_str()).chain(unconnected.values().map(String::as_str)).collect();
    net_names.sort();
    net_names.dedup();
    let net_num: BTreeMap<&str, usize> = net_names.iter().enumerate().map(|(i, n)| (*n, i + 1)).collect();

    let mut out = String::new();
    writeln!(out, "(kicad_pcb").unwrap();
    let version = board_file_version(design);
    writeln!(out, "\t(version {version})").unwrap();
    writeln!(out, "\t(generator \"eda-kicad\")").unwrap();
    writeln!(out, "\t(generator_version \"{}\")", if version > KICAD9_BOARD_FILE_VERSION { "10.0" } else { "9.0" }).unwrap();
    writeln!(out, "\t(general").unwrap();
    // `(general (thickness ..))`: the board's thickness (`BOARD_DESIGN_SETTINGS::GetBoardThickness()`), KiCad's 1.6 mm unless Board Setup's
    // Physical Stackup says otherwise.
    writeln!(out, "\t\t(thickness {})", mm(model.board.board_thickness_nm)).unwrap();
    // `(legacy_teardrops yes)` while the board has teardrops: they are zones of the design, made here (`Cmd::AddAllTeardrops`) or read from a file, and kicad-cli's
    // refill (`ZONE_FILLER_TOOL::FillAllZones` -> `TEARDROP_MANAGER::UpdateTeardrops( .., forceFullUpdate )`) would otherwise delete every one and rebuild only those of
    // pads and vias whose own teardrop parameters say `(enabled yes)`, which the studio's do not: the fills, the Gerbers and the plots lost the teardrop copper. A board
    // with legacy teardrops keeps them (`if( m_board->LegacyTeardrops() ) return;`) and fills them as the zones they are.
    let has_teardrops = design.routing.as_ref().is_some_and(|r| r.zones.iter().any(|z| z.teardrop));
    writeln!(out, "\t\t(legacy_teardrops {})", if has_teardrops { "yes" } else { "no" }).unwrap();
    writeln!(out, "\t)").unwrap();
    // The board's paper and title block (Page Settings); A4 landscape and no title block when it never set them.
    let page = design.drawings.as_ref().and_then(|d| d.page.as_ref());
    writeln!(out, "\t{}", page.map(|p| p.to_sexpr()).unwrap_or_else(|| "(paper \"A4\")".to_string())).unwrap();
    if let Some(tb) = design.drawings.as_ref().and_then(|d| d.title_block.as_ref()) {
        crate::page::write_board_title_block(&mut out, tb);
    }

    // ---- layers ----
    write_layers(&mut out, &model.board.layers, &model.board.layer_setup);

    // ---- setup ----
    let clearance_mm = mm(model.board.clearance);
    let track_mm = mm(model.board.track_width);
    let via_dia_mm = mm(model.board.via_diameter);
    let via_drill_mm = mm(model.board.via_drill);
    writeln!(out, "\t(setup").unwrap();
    // The physical stackup, when the model has one (Board Setup's Physical Stackup, or an imported board's own).
    write_stackup(&mut out, model);
    // Solder mask and paste (`PCB_IO_KICAD_SEXPR::formatSetup`): the mask expansion is always written, the rest when set.
    let sm = &model.board.solder_mask;
    writeln!(out, "\t\t(pad_to_mask_clearance {})", mm(sm.expansion_nm)).unwrap();
    if sm.min_width_nm != Nm::ZERO {
        writeln!(out, "\t\t(solder_mask_min_width {})", mm(sm.min_width_nm)).unwrap();
    }
    if sm.paste_margin_nm != Nm::ZERO {
        writeln!(out, "\t\t(pad_to_paste_clearance {})", mm(sm.paste_margin_nm)).unwrap();
    }
    if sm.paste_margin_ratio != 0.0 {
        writeln!(out, "\t\t(pad_to_paste_clearance_ratio {})", fmt_mm_f(sm.paste_margin_ratio)).unwrap();
    }
    writeln!(out, "\t\t(allow_soldermask_bridges_in_footprints {})", if sm.allow_bridges_in_footprints { "yes" } else { "no" }).unwrap();
    writeln!(out, "\t\t(tenting (front {}) (back {}))", if sm.tent_vias_front { "yes" } else { "no" }, if sm.tent_vias_back { "yes" } else { "no" }).unwrap();
    // KiCad 10's defaults for the vias that say nothing of their own: covering, plugging, capping, filling.
    for token in design.drawings.iter().flat_map(|d| d.setup_tokens.iter()) {
        writeln!(out, "\t\t{token}").unwrap();
    }
    // KiCad 10's drill symbol profile (`formatDrillSymbolProfile`), when the board has one other than the default.
    if let Some(p) = design.drawings.as_ref().and_then(|d| d.board_items.drill_profile.as_ref()) {
        crate::drill_chart_io::write_profile(&mut out, p);
    }
    // Zone Hatch Offsets (`formatSetup`'s `(zone_defaults (property (layer ..) (hatch_position (xy x y))))`), in `PCB_LAYER_ID` order, for the copper layers the board has.
    {
        let mut offsets: Vec<(&String, &eda_model::setup_pages::HatchOffset)> = model.board.hatch_offsets.iter().filter(|(l, o)| !o.is_zero() && model.board.layers.contains(l)).collect();
        offsets.sort_by_key(|(l, _)| eda_model::layers::pcb_layer_id(l));
        if !offsets.is_empty() {
            write!(out, "\t\t(zone_defaults").unwrap();
            for (layer, o) in offsets {
                write!(out, " (property (layer {}) (hatch_position (xy {} {})))", sexpr_str(layer), mm(o.x_nm), mm(o.y_nm)).unwrap();
            }
            writeln!(out, ")").unwrap();
        }
    }
    // The drill/place file origin (`pcbnew.EditorControl.drillOrigin`): what kicad-cli measures drill, position and
    // (when asked) Gerber coordinates from.
    let aux = design.drawings.as_ref().and_then(|d| d.aux_origin).unwrap_or(eda_model::ir::Point { x: Nm::ZERO, y: Nm::ZERO });
    writeln!(out, "\t\t(aux_axis_origin {} {})", mm(aux.x), mm(aux.y)).unwrap();
    // The point the editing grid is anchored at (`common.Control.gridSetOrigin`): KiCad's `(grid_origin x y)` of the board setup.
    let grid = design.drawings.as_ref().and_then(|d| d.grid_origin).unwrap_or(eda_model::ir::Point { x: Nm::ZERO, y: Nm::ZERO });
    writeln!(out, "\t\t(grid_origin {} {})", mm(grid.x), mm(grid.y)).unwrap();
    // The board's plot settings (`PCB_PLOT_PARAMS::Format`): an imported board's own, else KiCad's defaults; the dash ratios are Text & Graphics > Formatting's.
    {
        // Read back as KiCad reads it (`PlotParams::of_design`): a legacy `(viasonmask ..)` is never written after `(tenting ..)`.
        let plot = eda_model::plot_params::PlotParams::of_design(design);
        writeln!(out, "\t\t(pcbplotparams").unwrap();
        writeln!(out, "\t\t\t(layerselection {})", eda_model::plot_params::encode_layer_selection(&plot.layer_selection)).unwrap();
        writeln!(out, "\t\t\t(plot_on_all_layers_selection {})", eda_model::plot_params::encode_layer_selection(&plot.plot_on_all_layers)).unwrap();
        let mut plot = plot;
        plot.set_item("dashed_line_dash_ratio", &format!("{:.6}", model.board.formatting.dash_length_ratio));
        plot.set_item("dashed_line_gap_ratio", &format!("{:.6}", model.board.formatting.gap_length_ratio));
        for item in &plot.items {
            writeln!(out, "\t\t\t{item}").unwrap();
        }
        writeln!(out, "\t\t)").unwrap();
    }
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
    // A board whose net classes name tuning profiles has none of these blocks: KiCad moves a board's legacy classes into the project, replacing the project's own and with
    // them the profile each names, so the classes are the project's then (`project_net_settings`).
    'legacy_net_classes: {
        if net_classes_in_project(model) {
            break 'legacy_net_classes;
        }
    let default_nets: Vec<&str> = net_names.iter().copied().filter(|n| model.board.class_of(n).is_none()).collect();
    writeln!(out, "\t(net_class \"Default\" \"This is the default net class.\"").unwrap();
    writeln!(out, "\t\t(clearance {clearance_mm})").unwrap();
    writeln!(out, "\t\t(trace_width {track_mm})").unwrap();
    writeln!(out, "\t\t(via_dia {via_dia_mm})").unwrap();
    writeln!(out, "\t\t(via_drill {via_drill_mm})").unwrap();
    write_class_micro_and_pair(&mut out, model.board.default_class.as_ref());
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
        write_class_micro_and_pair(&mut out, Some(class));
        for name in &nets_in_class {
            writeln!(out, "\t\t(add_net {})", sexpr_str(name)).unwrap();
        }
        writeln!(out, "\t)").unwrap();
    }
    }

    // `BOARD_ITEM::IsLocked()`: every locked item's id (`DrawingsSection::locked_ids`). Each item kind
    // writes `(locked yes)` where KiCad's own writer does, so a locked board comes back locked.
    let locked: std::collections::BTreeSet<&str> = design.drawings.as_ref().map(|d| d.locked_ids.iter().map(String::as_str).collect()).unwrap_or_default();
    // `BOARD_ITEM::IsLocked` (`pcbnew/board_item.cpp`) is also true when the group above is locked (`GetParentGroup()->IsLocked()`, up the chain), and every
    // `format()` writes `IsLocked()`: a member of a locked group -- or of a locked tuning pattern or via stitching, which are groups -- is written `(locked yes)`.
    let parent_of: BTreeMap<&str, &str> = design
        .drawings
        .as_ref()
        .map(|d| {
            let groups = d.groups.iter().map(|g| (g.id.as_str(), &g.member_ids));
            let patterns = d.tuning_patterns.iter().map(|p| (p.id.as_str(), &p.member_ids));
            let stitches = d.board_items.via_stitches.iter().map(|v| (v.id.as_str(), &v.member_ids));
            groups.chain(patterns).chain(stitches).flat_map(|(g, ms)| ms.iter().map(move |m| (m.as_str(), g))).collect()
        })
        .unwrap_or_default();
    let is_locked = |id: &str| {
        let mut at = id;
        for _ in 0..64 {
            if at.is_empty() {
                return false;
            }
            if locked.contains(at) {
                return true;
            }
            match parent_of.get(at) {
                Some(p) => at = p,
                None => return false,
            }
        }
        false
    };
    // Our item id -> the KiCad uuids it was written as (a polyline track is several segments): what a
    // group's `(members ..)` names.
    let mut written: BTreeMap<String, Vec<String>> = BTreeMap::new();

    // Net 0 is KiCad's *unconnected* net. Defaulting a track, via or zone
    // to it does not lose the copper -- it exports copper that claims to
    // belong to nothing, so the board KiCad checks is not the board our
    // gates passed. A GND pour landing on net 0 is a floating plane that
    // still draws, and `drc --refill-zones` would be answering a question
    // about a different design than the one we routed. The net has to be
    // in the netlist or the export fails. An item the design says has no
    // net at all (an empty name: a rule area, or copper an imported board
    // drew with no net) is KiCad's net 0 on purpose, and says so.
    let mut net_errors: Vec<CheckResult> = Vec::new();
    let mut net_of = |what: &str, where_: &str, net: &str| -> usize {
        if net.is_empty() {
            return 0;
        }
        match net_num.get(net) {
            Some(n) => *n,
            None => {
                net_errors.push(CheckResult::fail(
                    "kicad.unknown_net",
                    where_.to_string(),
                    format!("{what} is on net {net:?}, which is not in the netlist; KiCad would import it as net 0 (unconnected)"),
                ));
                0
            }
        }
    };

    // ---- copper pours, rule areas, teardrops: rendered up front ----
    // The outline is ours; the fill is KiCad's: the rings kicad-cli made of the zone (Fill All Zones), written as `filled_polygon`, so KiCad shows (and plots/exports
    // gerbers for) the fill it made without a refill pass of its own. A zone with none is written unfilled. A zone that belongs to a footprint (a rule area a
    // footprint carries) is written inside that footprint, where KiCad keeps it: a footprint's own rule area
    // does not test the footprint itself (`intersectsArea`'s `aArea->GetParent() == item`), so written at board
    // level it would report the footprint that owns it as a keepout violation.
    let mut zone_blocks: Vec<(Option<String>, String)> = Vec::new();
    if let Some(routing) = &design.routing {
        let mut zones: Vec<&Zone> = routing.zones.iter().collect();
        zones.sort_by(|a, b| (&a.net, &a.layer).cmp(&(&b.net, &b.layer)));
        // `BOARD::GetEnabledLayers`: the copper layers of the stackup and the technical and user layers Board Setup has on. A zone is written on those of its layer set.
        let enabled: std::collections::BTreeSet<String> = model.board.layers.iter().chain(model.board.layer_setup.enabled.iter()).cloned().collect();
        let mut kept_uuids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let teardrop_priority = teardrop_priorities(&routing.zones);
        for z in &zones {
            if z.outline.len() < 3 {
                continue;
            }
            // A teardrop of the studio's with the priority KiCad's teardrop manager would give it.
            let with_priority;
            let z: &Zone = match teardrop_priority.get(z.id.as_str()) {
                Some(&p) => {
                    with_priority = Zone { priority: p, ..(*z).clone() };
                    &with_priority
                }
                None => z,
            };
            // A rule area has no net at all, whatever its `net` says (`ZONE::GetIsRuleArea()`); asking the
            // net check about it rejected the whole export with `kicad.unknown_net`. Neither has a zone on no
            // copper layer (`if( !zone->IsOnCopperLayer() ) zone->SetNetCode( NETINFO_LIST::UNCONNECTED )`).
            let n = if z.is_rule_area || !z.is_on_copper_layer() { 0 } else { net_of("a copper pour", &format!("{} on {}", z.net, z.layer), &z.net) };
            // Seeded by the zone's own id: two pours on one net and layer must not share a uuid. A zone imported from a KiCad file keeps the uuid it had there (the first zone
            // to claim one): KiCad breaks the tie between zones of equal priority by comparing uuids (`ZONE::HigherPriority`), so a different uuid could turn one zone into the
            // other's winner.
            let uuid = if !z.uuid.is_empty() && kept_uuids.insert(z.uuid.clone()) {
                crate::note_uuid(&z.uuid, &z.id);
                z.uuid.clone()
            } else {
                crate::duid_for(&if z.id.is_empty() { format!("zone:{}:{}", z.net, z.layer) } else { format!("zone:{}", z.id) }, &z.id)
            };
            written.entry(z.id.clone()).or_default().push(uuid.clone());
            // The rings kicad-cli stored for the zone on each of its layers, while the zone is the one it filled.
            // None for a zone kicad-cli has no current fill of: it is written unfilled (`ZONE::IsFilled`), as is one whose file said so of its rings (`ZoneEntry::unfilled`).
            let layer_fills: Vec<(String, Vec<crate::zone_fills::KicadRing>)> = match fills.filter(|k| k.current(z).is_some()) {
                Some(k) => z.layer_set().into_iter().map(|layer| (layer.to_string(), k.rings(z, layer))).collect(),
                None => Vec::new(),
            };
            let filled = fills.and_then(|k| k.current(z)).is_some_and(|e| !e.unfilled);
            let mut block = String::new();
            let thieving = design.drawings.as_ref().and_then(|d| d.zone_thieving.iter().find(|t| t.id == z.id)).map(|t| t.settings.as_str());
            let layer_props = design.drawings.as_ref().and_then(|d| d.zone_layer_props.iter().find(|t| t.id == z.id)).map(|t| t.settings.as_str());
            let arc_rings = design.drawings.as_ref().and_then(|d| d.zone_arcs.iter().find(|a| a.id == z.id)).filter(|a| a.shape == eda_model::ir::ZoneArcs::shape_of(z)).map(|a| a.rings.as_slice());
            write_zone(&mut block, z, &ZoneArgs { net: n, locked: is_locked(&z.id), uuid: &uuid, fills: &layer_fills, filled, enabled_layers: Some(&enabled), thieving, arc_rings, layer_props });
            zone_blocks.push((z.parent_footprint.clone(), block));
        }
    }

    // ---- footprints ----
    for fp in &footprints {
        let part = parts_by_ref[fp.id.as_str()];
        let Some(footprint) = model.footprint_of(part) else {
            errors.push(CheckResult::fail("kicad.no_footprint", fp.id.clone(), "part has no resolvable footprint geometry"));
            continue;
        };
        let own_zones: String = zone_blocks.iter().filter(|(parent, _)| parent.as_deref() == Some(fp.id.as_str())).map(|(_, text)| text.as_str()).collect();
        let uuid = write_footprint(&mut out, fp, part, &footprint, &net_num, model, design, is_locked(&fp.id), &own_zones, &FpExtra::of(design, &fp.id, &footprint), &from_schematic);
        written.entry(fp.id.clone()).or_default().push(uuid);
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // ---- routing: segments + vias ----
    if let Some(routing) = &design.routing {
        let mut tracks: Vec<&Track> = routing.tracks.iter().collect();
        tracks.sort_by(|a, b| (&a.net, &a.layer, &a.pts).cmp(&(&b.net, &b.layer, &b.pts)));
        for (ti, t) in tracks.iter().enumerate() {
            let n = net_of("a track", &format!("{} on {}", t.net, t.layer), &t.net);
            let lock = if is_locked(&t.id) { " (locked yes)" } else { "" };
            let w = mm(t.width);
            let mask = design.drawings.as_ref().and_then(|d| d.track_mask(&t.id));
            let layer = cu_item_layer_words(&t.layer, mask.is_some(), mask.and_then(|m| m.margin));
            if let Some((start, mid, end)) = t.arc() {
                // `PCB_ARC`: one `(arc (start) (mid) (end) (width))` item. `Track::arc()` only reports an arc
                // while the polyline still is its tessellation, so an edited arc is written as the segments
                // it has become and never as a stale curve.
                let uuid = crate::duid_for(&format!("arc:{}:{}:{}", t.net, t.layer, ti), &t.id);
                written.entry(t.id.clone()).or_default().push(uuid.clone());
                writeln!(
                    out,
                    "\t(arc (start {} {}) (mid {} {}) (end {} {}) (width {w}){lock} {layer} (net {n}) (uuid \"{uuid}\"))",
                    mm(start.x),
                    mm(start.y),
                    mm(mid.x),
                    mm(mid.y),
                    mm(end.x),
                    mm(end.y),
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
                writeln!(out, "\t(segment (start {x1} {y1}) (end {x2} {y2}) (width {w}){lock} {layer} (net {n}) (uuid \"{uuid}\"))").unwrap();
            }
        }

        let mut vias: Vec<&Via> = routing.vias.iter().collect();
        vias.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));
        // Two vias of a net at one point (a microvia stack's, `PCB_VIA_STACK`) would mint one uuid: the second takes its layers into the seed, and any later one at the
        // same place on the same layers a count as well (25ec2335 `multichannel_nested` has three VCC vias at one point; `resolveGroups` finds one item per uuid).
        let mut via_seeds: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for v in &vias {
            let n = net_of("a via", &format!("{} at ({}, {})", v.net, v.at.x, v.at.y), &v.net);
            let x = mm(v.at.x);
            let y = mm(v.at.y);
            let dia = mm(v.diameter);
            let drill = mm(v.drill);
            let lock = if is_locked(&v.id) { " (locked yes)" } else { "" };
            let mut seed = format!("via:{}:{}:{}", v.net, v.at.x, v.at.y);
            if !via_seeds.insert(seed.clone()) {
                let layered = format!("{seed}:{}:{}", v.from_layer, v.to_layer);
                seed = layered.clone();
                let mut k = 2;
                while !via_seeds.insert(seed.clone()) {
                    seed = format!("{layered}:{k}");
                    k += 1;
                }
            }
            let uuid = crate::duid_for(&seed, &v.id);
            written.entry(v.id.clone()).or_default().push(uuid.clone());
            // The via's type and the tokens after its layers (`remove_unused_layers`, `tenting`, `padstack`, ..) as the file had them.
            let (kind, tokens) = crate::via_io::words(design.drawings.as_ref().and_then(|d| d.via_extra(&v.id)));
            writeln!(
                out,
                "\t(via{kind} (at {x} {y}) (size {dia}) (drill {drill}) (layers {} {}){tokens}{lock} (net {n}) (uuid \"{uuid}\"))",
                sexpr_str(&v.from_layer),
                sexpr_str(&v.to_layer)
            )
            .unwrap();
        }

        // ---- board-level zones (the ones rendered above that no footprint owns) ----
        for (parent, block) in &zone_blocks {
            if parent.is_none() {
                out.push_str(block);
            }
        }
    }

    // ---- drawings: graphic shapes + free text ----
    // A shape on Edge.Cuts is written like any other, as the line, arc, rectangle, circle, polygon or curve it is. Together with
    // `placement.outline` (below) they are the board's Edge.Cuts items, the ones kicad-cli chains into its outline
    // (`eda_model::outline::edge_cuts_shapes`).
    if let Some(drawings) = &design.drawings {
        let mut shapes: Vec<&Shape> = drawings.shapes.iter().collect();
        shapes.sort_by(|a, b| a.id().cmp(b.id()));
        for s in &shapes {
            let style = drawings.board_items.style_of(s.id());
            let net = match style.and_then(|st| st.net.as_deref()) {
                Some(name) => net_of("a copper shape", &format!("{name} on {}", s.layer()), name),
                None => 0,
            };
            let uuid = write_shape(&mut out, s, is_locked(s.id()), style, net);
            written.entry(s.id().to_string()).or_default().push(uuid);
        }
        let mut texts: Vec<&Text> = drawings.texts.iter().collect();
        texts.sort_by(|a, b| a.id.cmp(&b.id));
        for t in &texts {
            let uuid = write_text_with(&mut out, t, is_locked(&t.id), drawings.text_extra(&t.id));
            written.entry(t.id.clone()).or_default().push(uuid);
        }
        let mut barcodes: Vec<&eda_model::barcode::Barcode> = drawings.barcodes.iter().collect();
        barcodes.sort_by(|a, b| a.id.cmp(&b.id));
        for b in &barcodes {
            let uuid = crate::barcode_io::write(&mut out, b, is_locked(&b.id));
            written.entry(b.id.clone()).or_default().push(uuid);
        }
        // The text boxes, reference images and points (`eda_model::pcb_items`).
        crate::pcb_items_io::write_all(&mut out, &drawings.board_items, &is_locked, &mut written);
        // `PCB_TABLE`: the grid and the looks of its cells, with the corners the grid decides (`crate::pcb_table`).
        let mut tables: Vec<&eda_model::table::Table> = drawings.tables.iter().collect();
        tables.sort_by(|a, b| a.id.cmp(&b.id));
        for t in &tables {
            // A table that is a drill chart is written as one (`format( const PCB_GENERATED_TABLE* )`).
            let uuid = match drawings.board_items.drill_charts.iter().find(|c| c.table_id == t.id) {
                Some(c) => crate::drill_chart_io::write_chart(&mut out, t, c, is_locked(&t.id)),
                None => crate::pcb_table::write(&mut out, t, is_locked(&t.id)),
            };
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
    // The polygon is a closed loop of straight sides, written as `gr_line`s. When the outline is the Edge.Cuts shapes (an imported board
    // with arcs, circles, cutouts, ...), `placement.outline` is only their summary and is not written: they were, above.
    if pl.outline.len() >= 2 && !eda_model::outline::outline_is_shapes(design) {
        let n = pl.outline.len();
        // The width the lines were drawn at, when the board says (an imported outline of lines of one width).
        let stroke = design.drawings.as_ref().and_then(|d| d.outline_width_nm).map(|w| format!(" (stroke (width {}) (type default))", mm(w))).unwrap_or_default();
        for i in 0..n {
            let a = pl.outline[i];
            let b = pl.outline[(i + 1) % n];
            let x1 = mm(a.x);
            let y1 = mm(a.y);
            let x2 = mm(b.x);
            let y2 = mm(b.y);
            let uuid = crate::duid_for(&format!("edge:{i}:{}:{}:{}:{}", a.x, a.y, b.x, b.y), "outline");
            writeln!(out, "\t(gr_line (start {x1} {y1}) (end {x2} {y2}){stroke} (layer \"Edge.Cuts\") (uuid \"{uuid}\"))").unwrap();
        }
    }

    // ---- groups ---- written last, as KiCad does: a group names its members by uuid, and the parser
    // resolves them once every item has been read.
    if let Some(drawings) = &design.drawings {
        let groups: Vec<&eda_model::ir::Group> = drawings.groups.iter().collect();
        // A group may hold a generator (`resolveGroups`' `groupTypeObjects` has them): a tuning pattern that is written (one with a member on the
        // board) is named by its uuid.
        for p in &drawings.tuning_patterns {
            if p.member_ids.iter().any(|m| written.contains_key(m)) {
                written.entry(p.id.clone()).or_default().push(crate::duid_for(&format!("generated:{}", p.id), &p.id));
            }
        }
        write_groups(&mut out, &groups, &written, &|g| crate::duid_for(&format!("group:{}", g.id), &g.id), &is_locked);
        // ... and the generators after them (`PCB_IO_KICAD_SEXPR::format( BOARD )` writes them last): the tuning patterns.
        crate::pcb_generated::write_generated(&mut out, &drawings.tuning_patterns, &written, &|p| crate::duid_for(&format!("generated:{}", p.id), &p.id), &is_locked);
        // KiCad 10's via stitchings, generators too.
        crate::via_stitch_io::write_all(&mut out, &drawings.board_items.via_stitches, &written, &|v| crate::duid_for(&format!("generated:{}", v.id), &v.id), &is_locked);
        // The generators nothing reads (`via_stack`), with the uuids their members were just written as.
        for g in &drawings.raw_generators {
            let members: Vec<String> = g.member_ids.iter().flat_map(|id| written.get(id).into_iter().flatten()).map(|u| sexpr_str(u)).collect();
            let text = g.text.strip_suffix(')').unwrap_or(&g.text);
            writeln!(out, "\t{text} (members {}))", members.join(" ")).unwrap();
        }
    }

    // The items nothing reads (a text box, a reference image, a target ..), as the file had them.
    for item in design.drawings.iter().flat_map(|d| d.raw_items.iter()) {
        writeln!(out, "\t{item}").unwrap();
    }

    // The files embedded in the board, and those its footprints use: one list, last in the file as KiCad writes it (`PCB_IO_KICAD_SEXPR::format( const BOARD* )`), each file's
    // data as it was read.
    // `board_file_list` has dropped the board's own fonts when it does not embed them; the footprints' fonts stay (`ClearEmbeddedFonts` is the board's list only).
    {
        let list = eda_model::embedded_files::board_file_list(design);
        if list.fonts {
            writeln!(out, "\t(embedded_fonts yes)").unwrap();
        }
        let files: Vec<&eda_model::embedded_files::EmbeddedFile> = list.files.iter().collect();
        if !files.is_empty() {
            crate::embedded::write_block(&mut out, &files, 1, true);
        }
    }

    writeln!(out, ")").unwrap();
    if !net_errors.is_empty() {
        return Err(net_errors);
    }
    Ok(out)
}

/// The number the board file gives a non-copper layer in its `(layers ..)` entry. KiCad's parser reads layers by name and ignores these (copper by position), so
/// they only have to be distinct; they are the ones KiCad 8 wrote (`B_Adhes` 32 .. `B_Fab` 48, `F_Fab` 49, `User_1` 50 on).
fn layer_ordinal(layer: &str) -> i32 {
    match layer {
        "B.Adhes" => 32,
        "F.Adhes" => 33,
        "B.Paste" => 34,
        "F.Paste" => 35,
        "B.SilkS" => 36,
        "F.SilkS" => 37,
        "B.Mask" => 38,
        "F.Mask" => 39,
        "Dwgs.User" => 40,
        "Cmts.User" => 41,
        "Eco1.User" => 42,
        "Eco2.User" => 43,
        "Edge.Cuts" => 44,
        "Margin" => 45,
        "B.CrtYd" => 46,
        "F.CrtYd" => 47,
        "B.Fab" => 48,
        "F.Fab" => 49,
        other => 49 + eda_model::layers::user_layer_number(other).map_or(0, |n| n as i32),
    }
}

/// `(layers ..)` (`PCB_IO_KICAD_SEXPR::formatBoardLayers`): the board's copper layers, outer first, each with its type; then the enabled technical and user layers in
/// `TechAndUserUIOrder`, typed `user` unless a user layer is off-board at the front or the back. A layer the user renamed, and every layer whose standard name
/// KiCad shows differs from the one the file writes (`F.SilkS` is `F.Silkscreen`), carries that name as the entry's last word.
pub(crate) fn write_layers(out: &mut String, copper_layers: &[String], setup: &LayerSetup) {
    // A layer shown by the very name the file writes needs no name of its own -- unless KiCad's standard name for it differs (`F.SilkS` is `F.Silkscreen`): a layer of a
    // file from before 20200922 is shown by its file name (`parseLayer`), and only an explicit `(5 "F.SilkS" user "F.SilkS")` says so to a reader of this file.
    let shown = |layer: &str| {
        let name = setup.display_name(layer);
        if name == layer && eda_model::layers::standard_layer_name(layer) == layer { String::new() } else { format!(" {}", sexpr_str(&name)) }
    };
    writeln!(out, "\t(layers").unwrap();
    for (i, name) in copper_layers.iter().enumerate() {
        let ord = if i == 0 { 0 } else if i + 1 == copper_layers.len() { 31 } else { i };
        writeln!(out, "\t\t({ord} {} {}{})", sexpr_str(name), setup.layer_type(name).word(), shown(name)).unwrap();
    }
    for layer in setup.enabled_in_order() {
        let ty = match setup.layer_type(&layer) {
            t @ (LayerType::Front | LayerType::Back) if eda_model::layers::user_layer_number(&layer).is_some() => t.word(),
            _ => "user",
        };
        writeln!(out, "\t\t({} {} {ty}{})", layer_ordinal(&layer), sexpr_str(&layer), shown(&layer)).unwrap();
    }
    writeln!(out, "\t)").unwrap();
}

/// A net class's microvia size and differential-pair width and gap: `(uvia_dia ..) (uvia_drill ..)` always (KiCad's
/// 0.3 / 0.1 when the class sets none), `(diff_pair_width ..) (diff_pair_gap ..)` when it does -- what
/// `parseNETCLASS` reads.
fn write_class_micro_and_pair(out: &mut String, class: Option<&eda_model::NetClass>) {
    writeln!(out, "\t\t(uvia_dia {})", class.and_then(|c| c.microvia_diameter).map_or("0.3".to_string(), mm)).unwrap();
    writeln!(out, "\t\t(uvia_drill {})", class.and_then(|c| c.microvia_drill).map_or("0.1".to_string(), mm)).unwrap();
    if let Some(w) = class.and_then(|c| c.diff_pair_width) {
        writeln!(out, "\t\t(diff_pair_width {})", mm(w)).unwrap();
    }
    if let Some(g) = class.and_then(|c| c.diff_pair_gap) {
        writeln!(out, "\t\t(diff_pair_gap {})", mm(g)).unwrap();
    }
}

/// `BOARD_STACKUP::FormatBoardStackup`: `(stackup (layer "F.Cu" (type "copper") (thickness 0.035)) ..)` inside `(setup ..)`.
/// Written only for a stackup whose every layer says what kind it is (one Board Setup or a board file made): the layers of
/// a stackup an intent listed by name and thickness alone are not enough to describe the board to KiCad.
fn write_stackup(out: &mut String, model: &ConstraintModel) {
    let Some(stackup) = &model.stackup else { return };
    if stackup.layers.is_empty() || stackup.layers.iter().any(|l| l.kind.is_none()) {
        return;
    }
    writeln!(out, "\t\t(stackup").unwrap();
    for l in &stackup.layers {
        write!(out, "\t\t\t(layer {} (type {})", sexpr_str(&l.name), sexpr_str(l.kind.as_deref().unwrap_or(""))).unwrap();
        // `(color ..)` is written for a mask, a silkscreen and a dielectric whose colour is specified (`IsColorEditable`, `IsPrmSpecified`), before the thickness.
        if let Some(c) = l.color.as_deref().filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("not specified")) {
            let kind = l.kind.as_deref().unwrap_or("").to_ascii_lowercase();
            if kind.contains("silk screen") || kind.contains("solder mask") || kind == "core" || kind == "prepreg" || kind == "dielectric" {
                write!(out, " (color {})", sexpr_str(c)).unwrap();
            }
        }
        if let Some(t) = l.thickness_mm {
            write!(out, " (thickness {})", crate::mm_f(t)).unwrap();
        }
        if let Some(m) = &l.material {
            write!(out, " (material {})", sexpr_str(m)).unwrap();
        }
        if l.material.is_some() || l.epsilon_r.is_some() {
            if let Some(e) = l.epsilon_r {
                write!(out, " (epsilon_r {})", fmt_mm_f(e)).unwrap();
            }
        }
        if l.material.is_some() || l.loss_tangent.is_some() {
            if let Some(t) = l.loss_tangent {
                write!(out, " (loss_tangent {})", fmt_mm_f(t)).unwrap();
            }
        }
        writeln!(out, ")").unwrap();
    }
    if let Some(f) = &stackup.copper_finish {
        writeln!(out, "\t\t\t(copper_finish {})", sexpr_str(f)).unwrap();
    }
    writeln!(out, "\t\t\t(dielectric_constraints {})", if stackup.dielectric_constraints { "yes" } else { "no" }).unwrap();
    if stackup.edge_connector > 0 {
        writeln!(out, "\t\t\t(edge_connector {})", if stackup.edge_connector > 1 { "bevelled" } else { "yes" }).unwrap();
    }
    if stackup.edge_plating {
        writeln!(out, "\t\t\t(edge_plating yes)").unwrap();
    }
    writeln!(out, "\t\t)").unwrap();
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
    let b = &model.board;
    let mm_f = |nm: Nm| eda_model::units::to_mm(nm);
    let rules = if b.constraints_explicit {
        // The constraints were stated -- by the project the board came from or by Board Setup -- so they are written
        // exactly (`BOARD_DESIGN_SETTINGS`'s `rules.*` keys, `pcbnew/board_design_settings.cpp`).
        serde_json::json!({
            "max_error": mm_f(b.max_error_nm),
            "min_clearance": mm_f(b.min_clearance_nm),
            "min_connection": mm_f(b.min_connection_nm),
            "min_copper_edge_clearance": mm_f(b.copper_edge_clearance_nm.unwrap_or(eda_model::KICAD_EDGE_CLEARANCE_NM)),
            "min_groove_width": mm_f(b.min_groove_width_nm),
            "min_hole_clearance": mm_f(b.hole_clearance_nm),
            "min_hole_to_hole": mm_f(b.hole_to_hole_min_nm),
            "min_microvia_diameter": mm_f(b.microvia_diameter_min_nm),
            "min_microvia_drill": mm_f(b.microvia_drill_min_nm),
            "min_resolved_spokes": b.min_resolved_spokes,
            "min_silk_clearance": mm_f(b.silk_clearance_nm),
            "min_text_height": mm_f(b.min_silk_text_height_nm),
            "min_text_thickness": mm_f(b.min_silk_text_thickness_nm),
            "min_through_hole_diameter": mm_f(b.via_drill_min_nm),
            "min_track_width": mm_f(b.track_width_min_nm),
            "min_via_annular_width": mm_f(b.annular_width_min_nm),
            "min_via_diameter": mm_f(b.via_diameter_min_nm),
            "solder_mask_to_copper_clearance": mm_f(b.solder_mask.to_copper_clearance_nm),
            "use_height_for_length_calcs": b.use_height_for_length_calcs,
        })
    } else {
        // A board built from an intent states no minimums, so the floors are lowered to what the board itself asks of
        // them: the narrowest class it routes. The hole floors are the board's own `hole_clearance_nm` and
        // `hole_to_hole_min_nm`, KiCad's defaults unless the intent says otherwise (`DEFAULT_HOLECLEARANCE` and
        // `DEFAULT_HOLETOHOLEMIN`, 0.25 mm, `BOARD_DESIGN_SETTINGS::BOARD_DESIGN_SETTINGS`): the router keeps exactly
        // these (`pns::rule_resolver`'s `HOLE_CLEARANCE` and `HOLE_TO_HOLE`), so kicad-cli checks what was routed. (This
        // wrote the class clearance, 0.2 mm, as the hole clearance until 2026-10-27.)
        let min_clearance = b.net_classes.iter().filter_map(|c| c.clearance).fold(b.clearance, Nm::min);
        let min_track_width = b.net_classes.iter().filter_map(|c| c.track_width).fold(b.track_width, Nm::min);
        serde_json::json!({
            "min_clearance": mm_f(min_clearance),
            "min_track_width": mm_f(min_track_width),
            "min_via_annular_width": 0.1,
            "min_via_diameter": 0.3,
            "min_hole_clearance": mm_f(b.hole_clearance_nm),
            "min_hole_to_hole": mm_f(b.hole_to_hole_min_nm),
            "min_through_hole_diameter": 0.2,
            "min_microvia_diameter": 0.2,
            "min_microvia_drill": 0.1,
        })
    };
    let t = &b.text_graphics;
    let class = |prefix: &str, c: &eda_model::rules::LayerClassDefaults| -> Vec<(String, serde_json::Value)> {
        vec![
            (format!("{prefix}_line_width"), serde_json::json!(mm_f(c.line_width_nm))),
            (format!("{prefix}_text_size_h"), serde_json::json!(mm_f(c.text_width_nm))),
            (format!("{prefix}_text_size_v"), serde_json::json!(mm_f(c.text_height_nm))),
            (format!("{prefix}_text_thickness"), serde_json::json!(mm_f(c.text_thickness_nm))),
            (format!("{prefix}_text_italic"), serde_json::json!(c.italic)),
            (format!("{prefix}_text_upright"), serde_json::json!(c.upright)),
        ]
    };
    // `defaults.*`: the Text & Graphics grid (the zone and pad defaults are left to KiCad's own).
    let mut defaults = serde_json::Map::new();
    for (k, v) in class("silk", &t.silk).into_iter().chain(class("copper", &t.copper)).chain(class("fab", &t.fab)).chain(class("other", &t.others)) {
        defaults.insert(k, v);
    }
    defaults.insert("board_outline_line_width".into(), serde_json::json!(mm_f(t.edge_cuts_line_width_nm)));
    defaults.insert("courtyard_line_width".into(), serde_json::json!(mm_f(t.courtyard_line_width_nm)));

    // Text & Graphics > Formatting: the "apply board defaults to footprint .." boxes (`defaults.apply_defaults_to_fp_*`).
    let f = &b.formatting;
    if f.applies_any() {
        for (key, on) in [("fields", f.apply_to_fp_fields), ("text", f.apply_to_fp_text), ("shapes", f.apply_to_fp_shapes), ("dimensions", f.apply_to_fp_dimensions), ("barcodes", f.apply_to_fp_barcodes)] {
            defaults.insert(format!("apply_defaults_to_fp_{key}"), serde_json::json!(on));
        }
    }
    let mut design_settings = serde_json::json!({ "rule_severities": effective_rule_severities(model), "rules": rules });
    if b.constraints_explicit || !t.is_default() || f.applies_any() {
        design_settings["defaults"] = serde_json::Value::Object(defaults);
    }
    // Design Rules > Length-tuning Patterns (`tuning_pattern_settings`).
    if !b.tuning_patterns.is_default() {
        let group = |m: &eda_model::setup_pages::MeanderSettings| {
            serde_json::json!({
                "min_amplitude": mm_f(m.min_amplitude_nm),
                "max_amplitude": mm_f(m.max_amplitude_nm),
                "spacing": mm_f(m.spacing_nm),
                "corner_style": m.corner_style.code(),
                "corner_radius_percentage": m.corner_radius_percentage,
                "single_sided": m.single_sided,
            })
        };
        design_settings["tuning_pattern_settings"] = serde_json::json!({ "single_track_defaults": group(&b.tuning_patterns.single_track), "diff_pair_defaults": group(&b.tuning_patterns.diff_pair), "diff_pair_skew_defaults": group(&b.tuning_patterns.diff_pair_skew) });
    }
    let mut project = serde_json::json!({ "board": { "design_settings": design_settings } });
    // Design Rules > Tuning Profiles: the project's `tuning_profiles` (`common/project/tuning_profiles.cpp`), and the net classes when they name one.
    if !b.tuning_profiles.profiles.is_empty() {
        project["tuning_profiles"] = serde_json::json!({ "meta": { "version": 0 }, "tuning_profiles_impedance_geometric": b.tuning_profiles.profiles.iter().map(project_tuning_profile).collect::<Vec<_>>() });
    }
    if net_classes_in_project(model) {
        project["net_settings"] = project_net_settings(model);
    }
    // Component Classes: the project's assignment rules (`COMPONENT_CLASS_SETTINGS`), which a DRC rule reads as `A.Component_Class`.
    if !b.component_classes.is_default() {
        project["component_class_settings"] = b.component_classes.to_project_json();
    }
    let mut text = serde_json::to_string_pretty(&project).unwrap_or_else(|_| "{}".into());
    text.push('\n');
    text
}

/// Whether the derived project carries the net classes (`net_settings`) in place of the board file's legacy `(net_class ..)` blocks. It has to when a class names a tuning
/// profile, which only the project form can say (KiCad moves the legacy classes of a board into the project, replacing the project's own), and when a net is in more than one
/// class, whose settings KiCad composes from the classes the project assigns it to (`NET_SETTINGS::GetEffectiveNetClass`): a legacy block names one class for a net.
pub(crate) fn net_classes_in_project(model: &ConstraintModel) -> bool {
    !model.board.tuning_profiles.class_profiles.is_empty() || model.nets.iter().any(|n| model.board.matching_classes(&n.name).len() > 1)
}

/// One `TUNING_PROFILE` as the project file keeps it (`common/project/tuning_profiles.cpp`): widths and gaps in nanometres, delays in attoseconds per millimetre, via delays in
/// attoseconds, layers by the names the board file writes (`UNDEFINED` for a reference layer a row leaves out).
fn project_tuning_profile(p: &eda_model::setup_pages::TuningProfile) -> serde_json::Value {
    serde_json::json!({
        "profile_name": p.name,
        "type": p.profile_type.code(),
        "target_impedance": p.target_impedance,
        "enable_time_domain_tuning": p.enable_time_domain_tuning,
        "layer_entries": p.layer_entries.iter().map(|e| serde_json::json!({
            "signal_layer": e.signal_layer,
            "top_reference_layer": e.top_reference_layer.as_deref().unwrap_or("UNDEFINED"),
            "bottom_reference_layer": e.bottom_reference_layer.as_deref().unwrap_or("UNDEFINED"),
            "width": e.width_nm,
            "diff_pair_gap": e.diff_pair_gap_nm,
            "delay": e.delay_as_per_mm,
        })).collect::<Vec<_>>(),
        "via_prop_delay": p.via_propagation_delay_as,
        "via_overrides": p.via_overrides.iter().map(|v| serde_json::json!({
            "signal_layer_from": v.signal_layer_from,
            "signal_layer_to": v.signal_layer_to,
            "via_layer_from": v.via_layer_from,
            "via_layer_to": v.via_layer_to,
            "delay": v.delay_as,
        })).collect::<Vec<_>>(),
    })
}

/// The project's `net_settings` (`common/project/net_settings.cpp`): the Default class and every other class with the tuning profile each names, and one pattern per net that
/// has a class (the nets a class's patterns match, as the board file's `add_net` lists name them). A class that only exists because a profile is named for it is a class of its own.
fn project_net_settings(model: &ConstraintModel) -> serde_json::Value {
    let b = &model.board;
    let mm_f = |nm: Nm| eda_model::units::to_mm(nm);
    let profile_of = |class: &str| b.tuning_profiles.class_profiles.get(class).cloned().unwrap_or_default();
    let mut classes = Vec::new();
    let mut default = serde_json::json!({ "name": "Default", "priority": i32::MAX, "tuning_profile": profile_of("Default"), "clearance": mm_f(b.clearance), "track_width": mm_f(b.track_width), "via_diameter": mm_f(b.via_diameter), "via_drill": mm_f(b.via_drill) });
    if let Some(d) = &b.default_class {
        for (key, v) in [("microvia_diameter", d.microvia_diameter), ("microvia_drill", d.microvia_drill), ("diff_pair_width", d.diff_pair_width), ("diff_pair_gap", d.diff_pair_gap), ("diff_pair_via_gap", d.diff_pair_via_gap)] {
            if let Some(v) = v {
                default[key] = serde_json::json!(mm_f(v));
            }
        }
    }
    classes.push(default);
    // A class's place in the list is its priority (`panel_setup_netclasses.cpp`: `SetPriority( row )`; the Default class has the lowest); `NetClass::priority` is the router's own.
    for (row, c) in b.net_classes.iter().enumerate() {
        let mut j = serde_json::json!({ "name": c.name, "priority": row as i32, "tuning_profile": profile_of(&c.name) });
        for (key, v) in [("clearance", c.clearance), ("track_width", c.track_width), ("via_diameter", c.via_diameter), ("via_drill", c.via_drill), ("microvia_diameter", c.microvia_diameter), ("microvia_drill", c.microvia_drill), ("diff_pair_width", c.diff_pair_width), ("diff_pair_gap", c.diff_pair_gap), ("diff_pair_via_gap", c.diff_pair_via_gap)] {
            if let Some(v) = v {
                j[key] = serde_json::json!(mm_f(v));
            }
        }
        classes.push(j);
    }
    // A class the project names a profile for that the board has no class of (an imported project's unused class).
    for (class, _) in &b.tuning_profiles.class_profiles {
        if class != "Default" && !b.net_classes.iter().any(|c| &c.name == class) {
            classes.push(serde_json::json!({ "name": class, "priority": classes.len() as i32 - 1, "tuning_profile": profile_of(class) }));
        }
    }
    let mut net_names: Vec<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
    net_names.sort();
    net_names.dedup();
    // One pattern for each class a net is in, naming the net exactly (a name written so neither reading of a pattern gives its characters a meaning): KiCad then composes the classes of
    // a net the way the studio does (`BoardRules::effective_class`) whatever its own regular expressions would have said of the patterns.
    let patterns: Vec<serde_json::Value> = net_names.iter().flat_map(|n| b.matching_classes(n).into_iter().map(move |c| serde_json::json!({ "netclass": c.name, "pattern": exact_pattern(n) }))).collect();
    serde_json::json!({ "classes": classes, "meta": { "version": 5 }, "net_colors": null, "netclass_assignments": null, "netclass_patterns": patterns })
}

/// A net name as a pattern that matches that name and no other, read as a regular expression (`NET_SETTINGS`' matchers read every pattern as a wildcard pattern and as a regular
/// expression): each character a regular expression gives a meaning to is escaped.
pub fn exact_pattern(net: &str) -> String {
    let mut out = String::with_capacity(net.len());
    for c in net.chars() {
        if ".+*?()[]{}|^$\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The per-type DRC severities the derived project holds: the imported project's own or Board Setup's, with "ignore"
/// for the two library-link checks -- every footprint here is embedded in the board file (`eda:<name>`), so there is
/// no library for KiCad to compare it against and each one would report a meaningless warning -- unless the table
/// says otherwise. Keyed by the DRC settings key, valued `error` | `warning` | `ignore`.
pub fn effective_rule_severities(model: &ConstraintModel) -> BTreeMap<String, String> {
    let mut severities: BTreeMap<String, String> = BTreeMap::from([("lib_footprint_issues".to_string(), "ignore".to_string()), ("lib_footprint_mismatch".to_string(), "ignore".to_string())]);
    for (key, value) in &model.board.rule_severities {
        severities.insert(key.clone(), value.clone());
    }
    severities
}

/// The design's own ERC pin-to-pin conflict matrix (Schematic Setup > ERC
/// pin map) when it stores a usable one -- 12 x 12, every cell 0 (ok), 1
/// (warning) or 2 (error) -- else `None`, and KiCad's own default applies.
pub fn custom_erc_pin_map(design: &Design) -> Option<Vec<Vec<u8>>> {
    let m = &design.schematic.as_ref()?.erc_pin_map.as_ref()?.matrix;
    (m.len() == 12 && m.iter().all(|r| r.len() == 12 && r.iter().all(|&c| c <= 2))).then(|| m.clone())
}

/// The per-check ERC severities the derived project holds: Schematic Setup's choices (`SchExtras::erc_severities`), with "ignore" for the
/// library-link checks -- every symbol is embedded in the derived schematic, so there is no library for KiCad to compare it against and
/// each one would report a meaningless `lib_symbol_mismatch`; and a symbol's Footprint field names the installed footprint (`footprint_lib_id`),
/// whose library is in the global table kicad-cli is run without (`eda_kicad_engine::cli_command`), so `footprint_link_issues` (`TestFootprintLinkIssues`)
/// would say of every symbol "The current configuration does not include the footprint library" -- unless the table says otherwise. Keyed by the ERC
/// settings key, valued `error` | `warning` | `ignore`; only the checks that differ from KiCad's default are named (the project's own `rule_severities`
/// loader leaves a check it does not find at its default).
pub fn effective_erc_severities(design: &Design) -> BTreeMap<String, String> {
    let mut severities: BTreeMap<String, String> = BTreeMap::from([("lib_symbol_issues".to_string(), "ignore".to_string()), ("lib_symbol_mismatch".to_string(), "ignore".to_string()), ("footprint_link_issues".to_string(), "ignore".to_string())]);
    if let Some(sch) = design.schematic.as_ref() {
        for (key, value) in &sch.extras.erc_severities {
            severities.insert(key.clone(), value.clone());
        }
    }
    severities
}

/// The schema `board.design_settings` is at when it lists its exclusions as records: 3 (`BOARD_DESIGN_SETTINGS::migrateSchema2to3` is the step that makes them).
const DRC_EXCLUSION_RECORDS_SCHEMA: u32 = 3;
/// The schema the `erc` section is at when it lists its exclusions as records: 1 (a section at 0 lists pipe-delimited texts).
const ERC_EXCLUSION_RECORDS_SCHEMA: u32 = 1;

/// [`export_kicad_pro`] plus the parts of the project the design's own data decides:
///
/// - `erc`: the design's own pin map, so kicad-cli's ERC judges pin conflicts by the matrix the user set up and not only by KiCad's
///   default, and the per-check severities ([`effective_erc_severities`]);
/// - `schematic.connection_grid_size`: Schematic Setup's connection grid, in mils, when it is not KiCad's default;
/// - `erc.erc_exclusions`: the ERC findings the user waived, whose marker is known ([`crate::erc_exclusions`]), with their comments, as records at the schema that has them;
/// - `board.design_settings.drc_exclusions`: the violations the user waived ([`crate::drc_exclusions`]), as records at the schema that has them, so kicad-cli
///   reports each as excluded and leaves it out of a report that does not ask for exclusions.
pub fn export_kicad_pro_for(design: &Design, model: &ConstraintModel) -> String {
    let mut project: serde_json::Value = serde_json::from_str(&export_kicad_pro(model)).unwrap_or_else(|_| serde_json::json!({}));
    let mut erc = serde_json::json!({ "rule_severities": effective_erc_severities(design) });
    if let Some(m) = custom_erc_pin_map(design) {
        erc["pin_map"] = serde_json::json!(m);
    }
    // Schematic Setup > Formatting: the connection grid ERC's `endpoint_off_grid` judges by (`schematic.connection_grid_size`, in mils).
    if let Some(mils) = design.schematic.as_ref().and_then(|s| s.extras.connection_grid_mils) {
        project["schematic"]["connection_grid_size"] = serde_json::json!(f64::from(mils));
    }
    // Schematic Setup > Formatting > Inter-sheet References (`SCHEMATIC_SETTINGS`, `schematic.drawing.intersheets_ref_*`): how a global label's Sheet References read
    if let Some(iref) = design.schematic.as_ref().and_then(|s| s.extras.intersheet_refs.as_ref()) {
        let drawing = &mut project["schematic"]["drawing"];
        drawing["intersheets_ref_show"] = serde_json::json!(iref.show);
        drawing["intersheets_ref_own_page"] = serde_json::json!(iref.own_page);
        drawing["intersheets_ref_short"] = serde_json::json!(iref.short);
        drawing["intersheets_ref_prefix"] = serde_json::json!(iref.prefix);
        drawing["intersheets_ref_suffix"] = serde_json::json!(iref.suffix);
    }
    let waived_erc = crate::erc_exclusions::project_entries(design.schematic.as_ref().map_or(&[][..], |s| s.erc_exclusions.as_slice()));
    if !waived_erc.is_empty() {
        erc["erc_exclusions"] = serde_json::Value::Array(waived_erc);
        // The entries are structured records (`kiapi.schematic.ErcExclusion`), the form of a section at schema 1; at schema 0 -- what a section that says so is, and
        // what KiCad before the change wrote -- an entry is a pipe-delimited text, and a record is not read. So the section says which it is (measured on
        // kicad-cli 10.99.0-5334: records are read at 1, 2, 3 and without a version, texts only at 0).
        erc["meta"] = serde_json::json!({ "version": ERC_EXCLUSION_RECORDS_SCHEMA });
    }
    project["erc"] = erc;
    // The simulator's settings (`NGSPICE_SETTINGS`, a `NESTED_SETTINGS` at "ngspice" of `SCHEMATIC_SETTINGS`, schema version 0) as KiCad writes them, once the
    // simulator set them.
    if let Some(ng) = design.schematic.as_ref().and_then(|s| s.extras.ngspice.as_ref()) {
        project["schematic"]["ngspice"] = serde_json::json!({
            "fix_include_paths": ng.fix_include_paths,
            "meta": { "version": 0 },
            "model_mode": ng.model_mode,
            "workbook_filename": ng.workbook_filename,
        });
    }
    // Board Setup > Pre-defined Sizes' Tracks and Vias (`BOARD_DESIGN_SETTINGS::m_TrackWidthList` / `m_ViasDimensionsList`, the project's
    // `track_widths` and `via_dimensions`, each a `PARAM_LAMBDA` of `board_design_settings.cpp`): millimetres, with the index-0 "use net class"
    // dummy entry KiCad keeps at the head of each list (r5 R5-3)
    if let Some(widths) = design.routing.as_ref().map(|r| r.track_width_presets.as_slice()).filter(|w| !w.is_empty()) {
        let mut js = vec![serde_json::json!(0.0)];
        js.extend(widths.iter().map(|w| serde_json::json!(eda_model::units::to_mm(*w))));
        project["board"]["design_settings"]["track_widths"] = serde_json::Value::Array(js);
    }
    if let Some(vias) = design.routing.as_ref().map(|r| r.via_presets.as_slice()).filter(|v| !v.is_empty()) {
        let mm_f = eda_model::units::to_mm;
        let mut js = vec![serde_json::json!({ "diameter": 0.0, "drill": 0.0 })];
        js.extend(vias.iter().map(|v| serde_json::json!({ "diameter": mm_f(v.diameter), "drill": mm_f(v.drill) })));
        project["board"]["design_settings"]["via_dimensions"] = serde_json::Value::Array(js);
    }
    // Board Setup > Pre-defined Sizes' Differential Pairs (`BOARD_DESIGN_SETTINGS::m_DiffPairDimensionsList`, the project's `diff_pair_dimensions`):
    // width, gap and via gap in millimetres, with the index-0 "use net class" dummy row KiCad keeps at the head of its list.
    if let Some(pairs) = design.routing.as_ref().map(|r| r.diff_pair_presets.as_slice()).filter(|p| !p.is_empty()) {
        let mm_f = eda_model::units::to_mm;
        let mut dims = vec![serde_json::json!({ "width": 0.0, "gap": 0.0, "via_gap": 0.0 })];
        dims.extend(pairs.iter().map(|p| serde_json::json!({ "width": mm_f(p.width), "gap": mm_f(p.gap), "via_gap": mm_f(p.via_gap) })));
        project["board"]["design_settings"]["diff_pair_dimensions"] = serde_json::Value::Array(dims);
    }
    // Board Setup > Text & Graphics > Dimensions (`defaults.dimension_units`, `defaults.dimension_precision`, `defaults.dimensions.*`), when they are not KiCad's own
    // (`crate::import::merge_project_dimension_defaults` reads them back).
    if let Some((units, precision, dims)) = design.drawings.as_ref().and_then(|d| crate::import::project_dimension_defaults(&d.dimension_settings)) {
        let defaults = &mut project["board"]["design_settings"]["defaults"];
        defaults["dimension_units"] = units;
        defaults["dimension_precision"] = precision;
        defaults["dimensions"] = dims;
    }
    let waived = crate::drc_exclusions::project_entries(design.drawings.as_ref().map_or(&[][..], |d| d.drc_exclusions.as_slice()));
    if !waived.is_empty() {
        project["board"]["design_settings"]["drc_exclusions"] = serde_json::Value::Array(waived);
        // The entries are structured records (`kiapi.board.DrcExclusion`), the form of a section at schema 3. A section that says it is at 2 or less is what KiCad
        // before the change wrote: its entries are pipe-delimited texts, which 10.99 by build 5334 turns into records on load (`BOARD_DESIGN_SETTINGS::migrateSchema2to3`)
        // and reads nothing else from; a section without a version is read as current, and a text in it is dropped. So the section says which it is, and the studio
        // writes the records alone, not a text for a migration to convert (measured on kicad-cli 10.99.0-5334: records are read at 2, 3, 4 and without a version,
        // texts only at 2 or less).
        project["board"]["design_settings"]["meta"] = serde_json::json!({ "version": DRC_EXCLUSION_RECORDS_SCHEMA });
    }
    // The Text Variables page (`PROJECT::GetTextVars`, the project file's `text_variables`): once it has been used it is the project's whole set, and kicad-cli expands
    // `${NAME}` in the plots, the reports and the DRC from it. Until then the board folder's variables are the project's (`library_table::derive_project`).
    if let Some(vars) = design.drawings.as_ref().and_then(|d| d.rules.as_ref()).and_then(|r| r.text_variables.as_ref()) {
        project["text_variables"] = serde_json::json!(vars);
    }
    let mut text = serde_json::to_string_pretty(&project).unwrap_or_else(|_| "{}".into());
    text.push('\n');
    text
}

/// What the footprints of a board made here take from the schematic written beside it, beyond the model's nets.
#[derive(Default)]
pub(crate) struct FromSchematic {
    /// "REF.PIN" -> `unconnected-(..)`, the net of a pin the schematic has alone ([`unconnected_pin_nets`]).
    pub unconnected: BTreeMap<String, String>,
    /// The root sheet's file (`<title>.kicad_sch`): the `(sheetfile ..)` of a footprint whose symbol is on the root.
    pub root_sheetfile: String,
}

/// "REF.PIN" -> `unconnected-(..)` for every pin of a footprint made here that is on no net of `model` (see `export_kicad_pcb_with`).
fn unconnected_pin_nets(design: &Design, model: &ConstraintModel, footprints: &[&FootprintInstance], parts: &BTreeMap<&str, &Part>) -> BTreeMap<String, String> {
    let on_a_net: std::collections::BTreeSet<&str> = model.nets.iter().flat_map(|n| n.pins.iter().map(String::as_str)).collect();
    let read_from_a_file: std::collections::BTreeSet<&str> = design.drawings.iter().flat_map(|d| d.footprint_extras.iter().map(|e| e.id.as_str())).collect();
    let mut out = BTreeMap::new();
    for fp in footprints {
        let Some(part) = parts.get(fp.id.as_str()) else { continue };
        if read_from_a_file.contains(fp.id.as_str()) {
            continue;
        }
        let link = model.symbol_links.get(&fp.id);
        for pin in &part.pins {
            let key = format!("{}.{}", fp.id, pin.number);
            // (a pin no symbol draws -- of a unit no symbol places -- is in no netlist, and its pad on no net)
            if link.is_some_and(|l| !l.pins.contains_key(&pin.number)) {
                continue;
            }
            if !on_a_net.contains(key.as_str()) {
                let with_unit = model.symbol_links.get(&fp.id).and_then(|l| l.units.get(&pin.number)).map(String::as_str);
                let name = eda_engine::nets::unconnected_net_name(&key, pin.name.as_deref(), with_unit);
                out.insert(key, name);
            }
        }
    }
    out
}

/// `footprint` turned half a turn in its own frame: its pads, its art and its courtyard. With the footprint's orientation turned half a turn back it is the same
/// footprint on the board (see `write_footprint`). A pad's shape turns with the frame, which is no change to a shape that a half turn maps onto itself (every
/// shape with no offset, deltas, chamfers, primitives or padstack of its own): its own angle stays, and the file's absolute angle then turns with the
/// footprint, as KiCad's flip turns it. **Differs**: a pad with any of those keeps its absolute angle and is turned by its own angle instead -- the same copper,
/// the pad's own frame half a turn from KiCad's.
pub(crate) fn half_turned(footprint: &eda_model::Footprint) -> eda_model::Footprint {
    let mut f = footprint.clone();
    for pad in f.pads.iter_mut() {
        half_turn_pad(pad);
    }
    for outline in f.courtyard_outlines.iter_mut() {
        for p in outline.iter_mut() {
            *p = (-p.0, -p.1);
        }
    }
    let half_turn = FootprintInstance { id: String::new(), at: eda_model::ir::Point { x: Nm::ZERO, y: Nm::ZERO }, rot: 180_000, side: Side::Top, label: Default::default() };
    f.art = f.art.moved(&half_turn);
    f
}

/// A pad turned half a turn with its footprint's frame (see [`half_turned`]): its own angle kept when a half turn maps its shape onto itself.
pub(crate) fn half_turn_pad(pad: &mut Pad) {
    pad.at = (-pad.at.0, -pad.at.1);
    if !pad.ext.is_default() {
        pad.rot = (pad.rot + 180_000) % 360_000;
    }
}

/// A field's place turned half a turn with its footprint's frame: the same place on the board once the footprint's orientation turns back.
pub(crate) fn half_turn_layout(l: &mut eda_model::fp_edit::FieldLayout) {
    l.at = eda_model::ir::Point { x: -l.at.x, y: -l.at.y };
    l.angle = (l.angle + 180_000).rem_euclid(360_000);
}

/// An angle in millidegrees in (-180, 180] (`EDA_ANGLE::Normalize180`).
fn normalize180(a: i64) -> i64 {
    let mut a = a.rem_euclid(360_000);
    if a > 180_000 {
        a -= 360_000;
    }
    a
}

/// Is the back footprint `fp` written half a turn from the model's pose (see [`write_footprint`])? One made or flipped here is, one read in the
/// file's frame is not.
fn written_half_turned(design: &Design, fp: &FootprintInstance) -> bool {
    let read_in_the_files_frame = design.drawings.as_ref().is_some_and(|d| d.footprint_extras.iter().any(|e| e.id == fp.id && !e.library_frame));
    fp.side == Side::Bottom && !read_in_the_files_frame
}

/// KiCad's orientation of the footprint `fp`, in millidegrees counter-clockwise, in (-180, 180]: the angle of the `(at x y angle)` the writer gives it,
/// which is what KiCad's Properties panel (`FOOTPRINT::GetOrientationDegrees`) and Footprint Properties show. A back footprint made or flipped here
/// is half a turn from the model's `rot` (`FOOTPRINT::Flip`: `-m_orient`, then `Rotate( ANGLE_180 )` for a left-right flip).
pub fn kicad_footprint_orientation(design: &Design, fp: &FootprintInstance) -> i64 {
    let half = if written_half_turned(design, fp) { 180_000 } else { 0 };
    normalize180(-(fp.rot as i64 + half))
}

/// One footprint; returns the uuid it was written with (a group's `(members ..)` names it).
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_footprint(
    out: &mut String,
    fp: &FootprintInstance,
    part: &Part,
    footprint: &eda_model::Footprint,
    net_num: &BTreeMap<&str, usize>,
    model: &ConstraintModel,
    design: &Design,
    locked: bool,
    zones: &str,
    extra: &FpExtra,
    from_schematic: &FromSchematic,
) -> String {
    let unconnected = &from_schematic.unconnected;
    // A footprint on the back is written in KiCad's frame (`FOOTPRINT::Flip`: "we flip around the X axis, and after rotate 180 deg" -- y mirrored in the
    // footprint's own frame, the half turn in its orientation). This model mirrors x instead (`to_board`), the same board geometry a half turn apart: so a
    // footprint made here is written turned by half a turn in its own frame and back again in its orientation (`half_turned`), and every file position the
    // writer mirrors in x comes out mirrored in y. KiCad's Change Footprint and Update from Library, which flip the library's footprint and give it the old
    // orientation (`ExchangeFootprint`), then put it where it was, and its 3D models need no turn of their own. A footprint read from a KiCad file is in
    // that frame already (the reader keeps the file's numbers), and is written as it was read.
    // (one read in the library's frame, `FootprintExtra::library_frame`, is turned as one made here)
    let turn = written_half_turned(design, fp);
    let (as_placed, footprint_as_placed) = (fp, footprint);
    let (turned_fp, turned_footprint);
    let (fp, footprint) = if turn {
        turned_fp = FootprintInstance { rot: (fp.rot + 180_000) % 360_000, ..fp.clone() };
        turned_footprint = half_turned(footprint);
        (&turned_fp, &turned_footprint)
    } else {
        (fp, footprint)
    };
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
    // `FOOTPRINT::SetOrientation` keeps the angle in (-180, 180] (`Normalize180`), and the file has it so.
    let rot_deg = fmt_mm_f(normalize180(-(fp.rot as i64)) as f64 / 1000.0);
    // A footprint imported from a KiCad file keeps the uuid it had there (the first footprint to claim one), as a pour does: a text can name a footprint by its uuid
    // (`${uuid:REFERENCE}`) and `kicad-cli pcb diff` matches items by it.
    let kept_uuid = design.drawings.as_ref().and_then(|d| {
        let mine = d.footprint_extras.iter().position(|e| e.id == fp.id)?;
        let uuid = d.footprint_extras[mine].uuid.as_deref().filter(|u| !u.is_empty())?;
        (d.footprint_extras.iter().position(|e| e.uuid.as_deref() == Some(uuid)) == Some(mine)).then(|| uuid.to_string())
    });
    let uuid = match kept_uuid {
        Some(uuid) => {
            crate::note_uuid(&uuid, &fp.id);
            uuid
        }
        None => crate::duid_for(&format!("footprint:{}", fp.id), &fp.id),
    };
    // The name the derived symbol's Footprint field gives too (`footprint_lib_id`): the installed KiCad footprint a built-in package stands for.
    let lib_name = Some(eda_model::footprint::footprint_lib_id(part)).filter(|n| !n.is_empty()).unwrap_or_else(|| footprint.name.clone());
    // A `#2`, `#3` ... slot is the importer's way to keep apart instances of one library footprint whose pads, models or graphics differ (`dedup_footprint_key`): in the file it is
    // still that library's footprint.
    let lib_name = match lib_name.rsplit_once('#') {
        Some((base, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && model.footprints.iter().any(|f| f.name == base) => base.to_string(),
        _ => lib_name,
    };
    // A footprint named for no library is written with none (`LIB_ID::Format`: the bare item name), as the derived symbol names it.
    let own_extra = design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == fp.id));
    let lib_name_written = lib_name.clone();
    let lib_id = lib_name;

    writeln!(out, "\t(footprint {}", sexpr_str(&lib_id)).unwrap();
    if locked {
        writeln!(out, "\t\t(locked yes)").unwrap();
    }
    writeln!(out, "\t\t(layer {})", sexpr_str(layer)).unwrap();
    writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
    writeln!(out, "\t\t(at {x} {y} {rot_deg})").unwrap();

    // The Reference, the Value and the user fields (`format( PCB_FIELD )`), then the footprint's attributes. A footprint nothing was
    // edited on gets the Reference beside its courtyard and the Value below its origin, as it always has (`eda_model::fp_edit`).
    write_fields(out, as_placed, part, footprint_as_placed, extra, turn.then_some(fp));
    // The symbol's other fields, as Update PCB from Schematic gives them to a footprint made here (`BOARD_NETLIST_UPDATER::updateFootprintParameters`: a
    // field the footprint lacks is added hidden, on F.Fab or B.Fab, at the footprint's origin and orientation): Datasheet, Description, the simulation
    // fields, the user's. A field the footprint has of its own (Footprint Properties) is the footprint's.
    let link = model.symbol_links.get(&fp.id);
    if let Some(link) = link.filter(|_| own_extra.is_none()) {
        let fab = if fp.side == Side::Bottom { "B.Fab" } else { "F.Fab" };
        for (name, text) in &link.fields {
            if extra.edit.is_some_and(|e| e.fields.iter().any(|f| f.name == *name)) {
                continue;
            }
            let mut layout = eda_model::fp_edit::FieldLayout::new(eda_model::ir::Point { x: Nm::ZERO, y: Nm::ZERO }, fab);
            layout.visible = false;
            let uuid = crate::duid_for(&format!("footprint:{}:symfield:{name}", fp.id), &fp.id);
            crate::fp_fields::write_property(out, name, text, &layout, fp.side, fp.rot, &uuid);
        }
    }
    // The classes the file gave the footprint, and the sheet its symbol is on (`format( FOOTPRINT )` writes them after the properties).
    if let Some(e) = own_extra {
        // Its description, tags, jumper pad groups, Datasheet and Description fields ..., as the file had them, while the part is on the footprint they describe.
        if e.source_name == lib_name_written {
            for token in &e.tokens {
                writeln!(out, "\t\t{token}").unwrap();
            }
        }
        if !e.component_classes.is_empty() {
            write!(out, "\t\t(component_classes").unwrap();
            for class in &e.component_classes {
                write!(out, " (class {})", sexpr_str(class)).unwrap();
            }
            writeln!(out, ")").unwrap();
        }
        if let Some(name) = e.sheetname.as_deref().filter(|n| !n.is_empty()) {
            writeln!(out, "\t\t(sheetname {})", sexpr_str(name)).unwrap();
        }
        if let Some(file) = e.sheetfile.as_deref().filter(|n| !n.is_empty()) {
            writeln!(out, "\t\t(sheetfile {})", sexpr_str(file)).unwrap();
        }
    }
    // The link to the symbol in the schematic written beside the board (`FOOTPRINT::GetPath`, `GetSheetname`, `GetSheetfile`, as Update PCB from
    // Schematic sets them): KiCad's F8 matches a footprint to its symbol by the path unless told to relink by reference, and a footprint with none is
    // matched by nothing, so it would add a second footprint for every symbol. A footprint read from a file keeps the sheet the file named.
    if let Some(link) = link {
        writeln!(out, "\t\t(path {})", sexpr_str(&link.path)).unwrap();
        // Each of the two only when the file named none (a file that names a `(sheetfile)` and no `(sheetname)` had both written, and KiCad reads the last:
        // r3-board-io F12, `issue11814`).
        if own_extra.is_none_or(|e| e.sheetname.as_deref().is_none_or(str::is_empty)) {
            writeln!(out, "\t\t(sheetname {})", sexpr_str(&link.sheetname)).unwrap();
        }
        if own_extra.is_none_or(|e| e.sheetfile.as_deref().is_none_or(str::is_empty)) {
            let file = if link.sheetfile.is_empty() { from_schematic.root_sheetfile.as_str() } else { link.sheetfile.as_str() };
            if !file.is_empty() {
                writeln!(out, "\t\t(sheetfile {})", sexpr_str(file)).unwrap();
            }
        }
    }
    // The footprint's gates (`format( FOOTPRINT )` writes `(units ..)` after the sheet): the ones the file gave it, else the symbol's when it has two or more -- what the
    // netlist hands a footprint updated from the schematic -- so that KiCad's Swap Gate Nets finds them on a board made here.
    let units = own_extra
        .filter(|e| !e.units.is_empty())
        .map(|e| e.units.clone())
        .unwrap_or_else(|| {
            let derived = eda_model::fp_units::unit_info(design, model, &fp.id);
            if derived.len() >= 2 {
                derived
            } else {
                Vec::new()
            }
        });
    if !units.is_empty() {
        write!(out, "\t\t(units").unwrap();
        for u in &units {
            write!(out, " (unit (name {}) (pins", sexpr_str(&u.name)).unwrap();
            for pin in &u.pins {
                write!(out, " {}", sexpr_str(pin)).unwrap();
            }
            write!(out, "))").unwrap();
        }
        writeln!(out, ")").unwrap();
    }
    write_attr(out, &extra.attrs, own_extra.is_some_and(|e| e.allow_soldermask_bridges));

    // The pads in number order, each with its index in the footprint (what the zone-connection facts are addressed by).
    let mut pads: Vec<(usize, &Pad)> = footprint.pads.iter().enumerate().collect();
    pads.sort_by(|a, b| a.1.number.cmp(&b.1.number));

    // The footprint's own clearance and zone connection (`FOOTPRINT::SetLocalClearance`, `SetLocalZoneConnection`): the
    // facts a pad inherits. Resolved as `DRC_ENGINE::EvalZoneConnection` does, so kicad-cli's fill and DRC see what the studio shows.
    // The footprint's own solder mask margin (`FOOTPRINT::SetLocalSolderMaskMargin`), which the pads that have none of their own take (`PAD::GetSolderMaskExpansion`).
    if let Some(m) = own_extra.and_then(|e| e.solder_mask_margin) {
        writeln!(out, "\t\t(solder_mask_margin {})", mm(m)).unwrap();
    }
    if let Some(&(first, first_pad)) = pads.first() {
        let facts = design.pad_zone_facts(&fp.id, &footprint.name, first, footprint.pads.len(), &first_pad.number);
        if let Some(c) = facts.footprint_clearance {
            writeln!(out, "\t\t(clearance {})", mm(c)).unwrap();
        }
        if let Some(c) = facts.footprint_connection {
            writeln!(out, "\t\t(zone_connect {})", crate::zone_connection_to_file(c)).unwrap();
        }
    } else if let Some(e) = own_extra {
        // A footprint with no pad (a logo: `issue14549`'s `Logo1`) has no pad to resolve the facts through; `format( FOOTPRINT )` writes its own all the same
        // (r2-board-io F17).
        if let Some(c) = e.clearance {
            writeln!(out, "\t\t(clearance {})", mm(c)).unwrap();
        }
        if let Some(c) = e.zone_connection {
            writeln!(out, "\t\t(zone_connect {})", crate::zone_connection_to_file(c)).unwrap();
        }
    }

    // `(net_tie_pad_groups "1, 2" "3")` (`FOOTPRINT::SetNetTiePadGroups`): the pads a net tie joins. Without it KiCad's DRC reports the tie's short, and its zone fill keeps
    // clear of the tie's copper (`buildCopperItemClearances` exempts the tie from the zones of the nets it joins).
    if let Some(groups) = design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == fp.id)).map(|e| e.net_tie_pad_groups.as_slice()).filter(|g| !g.is_empty()) {
        write!(out, "\t\t(net_tie_pad_groups").unwrap();
        for group in groups {
            write!(out, " {}", sexpr_str(group)).unwrap();
        }
        writeln!(out, ")").unwrap();
    }

    // The footprint's silkscreen, fab drawings and texts (`fp_line`, `fp_arc`, ..., `fp_text user`), in its own frame -- a bottom-side footprint's mirrored
    // like its pads and on the other side's layers: after the fields and before the pads, where `format( FOOTPRINT )` writes the drawings.
    crate::fp_art_io::write_art(out, &footprint.art, fp);

    // Net assignment: look up which net (if any) this pad's "REF.PIN" belongs to.
    let pin_net = |number: &str| -> Option<&str> {
        let key = format!("{}.{}", fp.id, number);
        model.nets.iter().find(|n| n.pins.iter().any(|p| p == &key)).map(|n| n.name.as_str()).or_else(|| unconnected.get(&key).map(String::as_str))
    };

    // The layers a pad of an imported board is on besides copper, paste and mask (an older library's through-hole pads are on the silkscreen too, and kicad-cli's silkscreen checks
    // measure them there): the importer kept them per pad, in the footprint's pad order.
    let pad_infos = design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == fp.id)).map(|e| e.pads.as_slice()).filter(|p| p.len() == footprint.pads.len());

    // The k-th pad that carries a number is the one the studio and a `PadEdit` name `NUM#k`.
    let mut seen: BTreeMap<&str, u32> = BTreeMap::new();
    let mut pad_seeds: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // The uuid each pad is written with, by `NUM#k`: a group of the footprint names its pads so.
    let mut pad_uuids: BTreeMap<String, String> = BTreeMap::new();
    for &(pad_idx, pad) in &pads {
        let art_layers: &[String] = pad_infos.and_then(|p| p.get(pad_idx)).map(|i| i.art_layers.as_slice()).unwrap_or_default();
        let (no_paste, no_mask, connector) = pad_infos.and_then(|p| p.get(pad_idx)).map_or((false, false, false), |i| (i.no_paste, i.no_mask, i.connector));
        // The paste and mask of a hole or an edge connector pad that the file had and the default does not give, on the footprint's side.
        let hole_tech: Option<Vec<String>> = pad_infos.and_then(|p| p.get(pad_idx)).and_then(|i| i.hole_tech_layers.as_ref()).map(|t| t.iter().map(|l| eda_model::fp_art::layer_on_side(l, fp.side)).collect());
        let nth = {
            let n = seen.entry(pad.number.as_str()).or_insert(0);
            *n += 1;
            *n
        };
        let pad_extra = pad_text(extra.edit.and_then(|e| e.pad(&pad.number, nth)));
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
        // The shape the file's main fields hold: the pad's own on the top, the one its back has on the bottom (`PADSTACK::FlipLayers`: the front of the board is the
        // footprint's other side), see `padstack_io`.
        let file_main = pad.file_main_shape(fp.side);
        let (w, h) = file_main.size;
        let wmm = mm(w);
        let hmm = mm(h);
        let kind = match pad.kind {
            PadKind::Smd if connector => "connect",
            PadKind::Smd => "smd",
            PadKind::ThroughHole => "thru_hole",
            PadKind::NonPlatedHole => "np_thru_hole",
        };
        let shape = crate::pad_shape::shape_token(file_main.shape);
        // The delta, the offset, the corners and a custom pad's primitives (`PCB_IO_KICAD_SEXPR::format( const PAD* )`).
        let main_ext = eda_model::footprint::PadShapeExt {
            offset: file_main.offset,
            trapezoid_delta: file_main.trapezoid_delta,
            chamfer_ratio: file_main.chamfer_ratio,
            chamfer_corners: file_main.chamfer_corners,
            custom: file_main.custom.clone(),
            stack: Default::default(),
        };
        let shape_text = crate::pad_shape::write_pad_shape(file_main.shape, file_main.roundrect_ratio, &main_ext, fp.side);
        // Pad numbers repeat by design (a connector's shield tab, several
        // physical pads on one pin), so the number alone cannot make a
        // unique uuid; the position can, since two pads of one footprint
        // almost never coincide. The ones that do (a registration mark's crossed bars, all numbered "": `complex_hierarchy`'s MIRE has two
        // rectangles on one point) would share one uuid, and KiCad would tell the second from the first by a uuid of its own making: the later
        // pad's seed carries its index.
        let mut seed = format!("pad:{}:{}:{}:{}", fp.id, pad.number, pad.at.0, pad.at.1);
        if !pad_seeds.insert(seed.clone()) {
            seed = format!("{seed}:{pad_idx}");
            pad_seeds.insert(seed.clone());
        }
        let uuid = crate::duid_for(&seed, &format!("{}.{}", fp.id, pad.number));
        pad_uuids.insert(format!("pad:{}#{nth}", pad.number), uuid.clone());
        // KiCad stores a pad's orientation as its footprint's own angle
        // plus the pad's own (0 for the overwhelming majority of pads) --
        // see `pad_file_angle` for the mirroring subtlety.
        let pad_rot_deg = crate::pad_file_angle(fp.side, fp.rot, pad.rot);
        write!(out, "\t\t(pad {} {kind} {shape} (at {px} {py} {pad_rot_deg})", sexpr_str(&pad.number)).unwrap();
        write!(out, " (size {wmm} {hmm})").unwrap();
        out.push_str(&shape_text.delta);
        out.push_str(&shape_text.corners);
        match pad.kind {
            PadKind::ThroughHole | PadKind::NonPlatedHole => {
                // Footprint::validate guarantees a through-hole/non-plated
                // pad of a footprint built here declares a drill, round or slot;
                // inventing one here shipped a different hole than the
                // circuit-json writer invented.
                // `(drill D)` / `(drill oval W H)`, with the copper shape's offset inside it (`format( PAD )`).
                let offset = shape_text.offset.map(|(ox, oy)| format!(" (offset {} {})", mm(ox), mm(oy))).unwrap_or_default();
                match (pad.drill, pad.drill_slot) {
                    (Some(d), _) => write!(out, " (drill {}{offset})", mm(d)).unwrap(),
                    (None, Some((w, h))) => write!(out, " (drill oval {} {}{offset})", mm(w), mm(h)).unwrap(),
                    // A pad of a board's file that says no drill (the Allegro importer makes them): `format( PAD )` writes the token for a hole of some size only.
                    (None, None) => {}
                }
                // Copper on both sides and the mask on both, or the paste and mask the file's pad had (`PadMaskInfo::hole_tech_layers`: `format( const PAD* )` writes the pad's own set).
                let mut base: Vec<&str> = vec!["F.Cu", "B.Cu"];
                match &hole_tech {
                    Some(tech) => base.extend(tech.iter().map(String::as_str)),
                    None if !no_mask => base.extend(["F.Mask", "B.Mask"]),
                    None => {}
                }
                write!(out, " {}", crate::fp_art_io::pad_layers_token(&base, art_layers, fp.side)).unwrap();
            }
            PadKind::Smd => {
                if let Some((ox, oy)) = shape_text.offset {
                    write!(out, " (drill (offset {} {}))", mm(ox), mm(oy)).unwrap();
                }
                let (cu, paste, mask) = if pad.on_back(fp.side) {
                    ("B.Cu", "B.Paste", "B.Mask")
                } else {
                    ("F.Cu", "F.Paste", "F.Mask")
                };
                // An SMD pad on several copper layers: the set it had (`PadMaskInfo::smd_layers`), on the footprint's side, every copper layer of the board folded into
                // `*.Cu` (`formatLayers`: `if( ( aLayerMask & cu_board_mask ) == cu_board_mask )`, the copper bits then cleared).
                let smd_layers: Vec<String> = pad_infos.and_then(|p| p.get(pad_idx)).map(|i| i.smd_layers.iter().map(|l| eda_model::fp_art::layer_on_side(l, fp.side)).collect()).unwrap_or_default();
                let mut smd_layers: Vec<&str> = smd_layers.iter().map(String::as_str).collect();
                if model.board.layers.iter().all(|l| smd_layers.contains(&l.as_str())) {
                    smd_layers.retain(|l| !l.ends_with(".Cu") || *l == "F.Cu" || *l == "B.Cu");
                }
                // The layers the pad had: its side's copper, and its paste and mask unless the file's pad had none; an edge connector pad has mask on both sides and no paste.
                let several = !connector && !smd_layers.is_empty();
                let base: Vec<&str> = if several {
                    smd_layers
                } else if let (true, Some(tech)) = (connector, &hole_tech) {
                    std::iter::once(cu).chain(tech.iter().map(String::as_str)).collect()
                } else if connector {
                    std::iter::once(cu).chain((!no_mask).then_some(["F.Mask", "B.Mask"]).into_iter().flatten()).collect()
                } else {
                    std::iter::once(cu).chain((!no_paste).then_some(paste)).chain((!no_mask).then_some(mask)).collect()
                };
                if art_layers.is_empty() && !several {
                    write!(out, " (layers {})", base.iter().map(|l| sexpr_str(l)).collect::<Vec<_>>().join(" ")).unwrap();
                } else {
                    write!(out, " {}", crate::fp_art_io::pad_layers_token(&base, art_layers, fp.side)).unwrap();
                }
            }
        }
        // `(remove_unused_layers ..) (keep_end_layers ..)` of a plated through-hole pad (`format( const PAD* )`: after its layers).
        let stack_modelled = !pad.ext.stack.is_default();
        if pad.kind == PadKind::ThroughHole {
            out.push_str(&crate::padstack_io::unconnected_text(pad.ext.stack.unconnected));
        }
        // A non-plated hole has no net by definition -- nothing to look up.
        if pad.kind != PadKind::NonPlatedHole {
            if let Some(net_name) = pin_net(&pad.number) {
                if let Some(&n) = net_num.get(net_name) {
                    write!(out, " (net {n} {})", sexpr_str(net_name)).unwrap();
                }
            }
        }
        // `(pintype "free")` and the other pin types a pad was given (`PAD::SetPinType`): a free pad takes part in no net, and the copper clearance, solder mask and
        // connectivity checks leave it be (`PAD::IsFreePad`). Written after the net, as `format( const PAD* )` does.
        // A pad of a footprint made here has its pin's (`updateComponentPadConnections`: `SetPinFunction( net.GetPinFunction() )`, `SetPinType( net.GetPinType() )`).
        let from_pin = link.filter(|_| pad_infos.is_none() && pad.kind != PadKind::NonPlatedHole).and_then(|l| l.pins.get(&pad.number));
        let function = pad_infos.and_then(|p| p.get(pad_idx)).map(|i| i.pin_function.as_str()).or(from_pin.map(|(f, _)| f.as_str()));
        if let Some(function) = function.filter(|t| !t.is_empty()) {
            write!(out, " (pinfunction {})", sexpr_str(function)).unwrap();
        }
        let pin_type = pad_infos.and_then(|p| p.get(pad_idx)).map(|i| i.pin_type.as_str()).or(from_pin.map(|(_, t)| t.as_str()));
        if let Some(pin_type) = pin_type.filter(|t| !t.is_empty()) {
            write!(out, " (pintype {})", sexpr_str(pin_type)).unwrap();
        }
        // The pad's other tokens as the file had them (`remove_unused_layers`, `teardrops`, `tenting`, `property pad_prop_heatsink`, `die_length`, ..).
        for token in pad_infos.and_then(|p| p.get(pad_idx)).map(|i| i.tokens.as_slice()).unwrap_or_default() {
            // A design from before the padstack was modelled kept these as tokens; once the pad has a padstack of its own they are written from it.
            if stack_modelled && ["(remove_unused_layers", "(keep_end_layers", "(padstack"].iter().any(|t| token.starts_with(t)) {
                continue;
            }
            write!(out, " {token}").unwrap();
        }
        // The margins of a pad edit (`solder_mask_margin`, `solder_paste_margin`, `solder_paste_margin_ratio`), then the zone facts: KiCad's order.
        out.push_str(&pad_extra.margins);
        // The pad's own clearance, zone connection and thermal relief (`PCB_IO_KICAD_SEXPR::format( const PAD* )`): the angle
        // only when it is not the shape's default, 45 degrees for a circle and 90 otherwise.
        let zf = design.pad_zone_facts(&fp.id, &footprint.name, pad_idx, footprint.pads.len(), &pad.number);
        if let Some(c) = zf.pad_clearance {
            write!(out, " (clearance {})", mm(c)).unwrap();
        }
        if let Some(c) = zf.connection {
            write!(out, " (zone_connect {})", crate::zone_connection_to_file(c)).unwrap();
        }
        if let Some(w) = zf.thermal_spoke_width {
            write!(out, " (thermal_bridge_width {})", mm(w)).unwrap();
        }
        if let Some(a) = zf.thermal_spoke_angle_mdeg {
            let default_mdeg = pad.default_spoke_angle_mdeg() as eda_model::ir::Millideg;
            if a != default_mdeg {
                write!(out, " (thermal_bridge_angle {})", fmt_mm_f(a as f64 / 1000.0)).unwrap();
            }
        }
        if let Some(g) = zf.thermal_gap {
            write!(out, " (thermal_gap {})", mm(g)).unwrap();
        }
        out.push_str(&shape_text.custom);
        write!(out, " (uuid \"{uuid}\")").unwrap();
        // The other layers' shapes (`(padstack (mode ..) (layer ..) ..)`), after the uuid as `format( const PAD* )` writes them.
        out.push_str(&crate::padstack_io::write_padstack(pad, fp.side, &model.board.layers));
        writeln!(out, ")").unwrap();
    }

    // The non-plated holes the footprint has on no copper layer (`FootprintExtra::holes`: a `*.Mask`-only pad), as the pads they are: without them the board would lose its drilled holes,
    // and kicad-cli's zone fill would not keep clear of them.
    let holes: &[eda_model::ir::FootprintHole] = design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == fp.id)).map(|e| e.holes.as_slice()).unwrap_or_default();
    for (i, hole) in holes.iter().enumerate() {
        // (in the footprint's frame as the model holds it: turned with it when it is written in KiCad's frame)
        let mut turned_pad = hole.pad.clone();
        if turn {
            half_turn_pad(&mut turned_pad);
        }
        let pad = &turned_pad;
        let drill = match (pad.drill, pad.drill_slot) {
            (Some(d), _) => format!("(drill {})", mm(d)),
            (None, Some((w, h))) => format!("(drill oval {} {})", mm(w), mm(h)),
            (None, None) => continue,
        };
        let px = mm(if fp.side == Side::Bottom { -pad.at.0 } else { pad.at.0 });
        let shape_text = crate::pad_shape::write_pad_shape(pad.shape, pad.roundrect_ratio, &pad.ext, fp.side);
        let uuid = crate::duid_for(&format!("hole:{}:{i}:{}:{}", fp.id, pad.at.0, pad.at.1), &format!("{}.hole{i}", fp.id));
        write!(out, "\t\t(pad {} np_thru_hole {} (at {px} {} {}) (size {} {})", sexpr_str(&pad.number), crate::pad_shape::shape_token(pad.shape), mm(pad.at.1), crate::pad_file_angle(fp.side, fp.rot, pad.rot), mm(pad.size.0), mm(pad.size.1)).unwrap();
        out.push_str(&shape_text.delta);
        out.push_str(&shape_text.corners);
        // Its layers (`*.Mask` unless the file's hole had others, `FootprintHole::layers`) and the tokens it had (its mask margin, ..), as `format( const PAD* )` writes them.
        let layers = match &hole.layers {
            Some(names) => {
                let names: Vec<String> = names.iter().map(|l| eda_model::fp_art::layer_on_side(l, fp.side)).collect();
                if names.is_empty() { "(layers)".to_string() } else { crate::fp_art_io::pad_layers_token(&names.iter().map(String::as_str).collect::<Vec<_>>(), &[], fp.side) }
            }
            None => "(layers \"*.Mask\")".to_string(),
        };
        write!(out, " {drill} {layers}").unwrap();
        for token in &hole.tokens {
            write!(out, " {token}").unwrap();
        }
        if let Some(c) = hole.clearance {
            write!(out, " (clearance {})", mm(c)).unwrap();
        }
        writeln!(out, " (uuid \"{uuid}\"))").unwrap();
    }

    // The pads with no copper (`FootprintExtra::aux_pads`: the stencil's openings, mask apertures) as the file had them, while the part is on that footprint and that side;
    // the angle turned by what the footprint has turned since (the file's angle is the board's: `pad_file_angle`), the net by its name on this board.
    if let Some(e) = own_extra.filter(|e| !e.aux_pads.is_empty() && e.source_name == lib_name_written && e.courtyard_bottom == (fp.side == Side::Bottom)) {
        let turned = fp.rot as f64 - e.aux_pads_rot as f64;
        for aux in &e.aux_pads {
            let Ok(crate::sexpr::Sexpr::List(mut items)) = crate::sexpr::parse(&aux.text) else { continue };
            if turned != 0.0 {
                if let Some(at) = items.iter_mut().find_map(|c| match c {
                    crate::sexpr::Sexpr::List(l) if crate::sexpr::tag(l) == Some("at") => Some(l),
                    _ => None,
                }) {
                    let angle = crate::sexpr::num(at, 3).unwrap_or(0.0) - turned / 1000.0;
                    at.truncate(3);
                    at.push(crate::sexpr::Sexpr::Atom(fmt_mm_f(angle.rem_euclid(360.0))));
                }
            }
            let mut text = crate::sexpr::to_text(&crate::sexpr::Sexpr::List(items));
            if let Some(&n) = net_num.get(aux.net.as_str()).filter(|_| !aux.net.is_empty()) {
                // `(net N "name")` before the uuid, where `format( const PAD* )` writes it.
                let net = format!(" (net {n} {})", sexpr_str(&aux.net));
                match text.rfind(" (uuid ") {
                    Some(at) => text.insert_str(at, &net),
                    None => text.insert_str(text.len() - 1, &net),
                }
            }
            writeln!(out, "\t\t{text}").unwrap();
        }
    }

    // The courtyard, on the footprint's own side: what KiCad's DRC reads for
    // `courtyards_overlap`, `pth_inside_courtyard` and `npth_inside_courtyard`
    // (the gates that call kicad-cli for those get nothing to judge from a
    // footprint with none). The real outlines when the library footprint has
    // them (x mirrored for a bottom-side part, like a pad's), else the box
    // `Footprint::courtyard_half` derives -- the same one the placer keeps
    // clear.
    let crtyd_layer = if fp.side == Side::Bottom { "B.CrtYd" } else { "F.CrtYd" };
    // The courtyard graphics the file had, as it had them, while the part is on that footprint and that side: a courtyard of lines, arcs or a circle is not a polygon to
    // KiCad's comparison with the library's copy (`FOOTPRINT::FootprintNeedsUpdate`).
    let own_courtyard = own_extra.filter(|e| !e.courtyard_items.is_empty() && e.source_name == lib_name_written && e.courtyard_bottom == (fp.side == Side::Bottom));
    if let Some(e) = own_courtyard {
        for item in &e.courtyard_items {
            writeln!(out, "\t\t{item}").unwrap();
        }
    } else if !footprint.art.courtyard.is_empty() {
        // a library footprint's courtyard, item for item as the library draws it
        for (i, s) in footprint.art.courtyard.iter().enumerate() {
            crate::fp_art_io::write_shape(out, s, fp, &crate::duid_for(&format!("footprint:{}:crtyd:{i}", fp.id), &fp.id));
        }
    } else if footprint.courtyard_outlines.is_empty() && own_extra.is_some_and(|e| e.no_courtyard) {
        // The footprint the file had has no courtyard: none is made up for it.
    } else if footprint.courtyard_outlines.is_empty() {
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

    // The files the footprint lists as its own: names, types and checksums (the data is the board's), before its models as KiCad writes them. `(embedded_fonts yes)` when the
    // footprint embeds fonts, whether it lists files or not (`FormatBool( .., "embedded_fonts", .. )` is written for every footprint; the `no` is left out, as before, so that a board
    // without embedded files is written as it was).
    {
        if eda_model::embedded_files::of_footprint(design, &fp.id).is_some_and(|l| l.fonts) {
            writeln!(out, "\t\t(embedded_fonts yes)").unwrap();
        }
        let listed = eda_model::embedded_files::footprint_listed_files(design, &fp.id);
        if !listed.is_empty() {
            let refs: Vec<&eda_model::embedded_files::EmbeddedFile> = listed.iter().collect();
            crate::embedded::write_block(out, &refs, 2, false);
        }
    }

    // 3D models: every `(model ...)` the footprint has, with the placement it gives each (`Footprint::models3d`: a library footprint's, an imported
    // board's own), else the one its generic package name stands for -- the installed KiCad footprint's model with its own placement, fitted to the pads
    // this footprint has (`part_models`). Without a `(model ...)` kicad-cli's 3D export leaves the part out altogether.
    //
    // A bottom-side footprint is written the way KiCad stores one (`turn` above), its models as held. One a reader of before
    // `FootprintExtra::library_frame` read is in its file's frame mirrored in x: the half turn between the two goes into the model's own placement
    // (`Model3d::flipped_frame`). The path is written exactly as stored (`${KICADn_3DMODEL_DIR}/Lib.3dshapes/File.step`).
    let placed: Vec<eda_model::footprint::Model3d> = crate::part_models(model, part, footprint_as_placed).map(|p| p.models).unwrap_or_default().into_iter().map(|m| if fp.side == Side::Bottom && !turn { m.flipped_frame() } else { m }).collect();
    crate::footprint_lib::write_models(out, &placed, 2);

    // The footprint's own zones (rule areas it carries), after its pads and graphics as KiCad writes them.
    out.push_str(zones);
    // The footprint's own groups (`format( const FOOTPRINT* )` writes `GetGroups()` inside it), while the part is on the footprint they were read from: each
    // member by the uuid its item is written with here.
    if let Some(e) = own_extra.filter(|e| !e.groups.is_empty() && e.source_name == lib_name_written) {
        let field_names: Vec<&str> = extra.edit.map(|ed| ed.fields.iter().map(|f| f.name.as_str()).collect()).unwrap_or_default();
        for (gi, g) in e.groups.iter().enumerate() {
            let mut members: Vec<String> = g
                .members
                .iter()
                .filter_map(|m| {
                    let (kind, rest) = m.split_once(':')?;
                    match kind {
                        "art" => Some(crate::duid_for(&format!("footprint:{}:art:{rest}", fp.id), &fp.id)),
                        "arttext" => Some(crate::duid_for(&format!("footprint:{}:arttext:{rest}", fp.id), &fp.id)),
                        "pad" => pad_uuids.get(m).cloned(),
                        "uuid" => own_courtyard.is_some().then(|| rest.to_string()),
                        "field" => match rest {
                            "Reference" => Some(crate::duid_for(&format!("footprint:{}:ref", fp.id), &fp.id)),
                            "Value" => Some(crate::duid_for(&format!("footprint:{}:val", fp.id), &fp.id)),
                            name => field_names.iter().position(|n| *n == name).map(|i| crate::duid_for(&format!("footprint:{}:field:{i}", fp.id), &fp.id)),
                        },
                        _ => None,
                    }
                })
                .collect();
            if members.is_empty() {
                continue;
            }
            members.sort();
            members.dedup();
            let guuid = crate::duid_for(&format!("footprint:{}:group:{gi}", fp.id), &fp.id);
            let lock = if g.locked { " (locked yes)" } else { "" };
            let quoted: Vec<String> = members.iter().map(|m| sexpr_str(m)).collect();
            writeln!(out, "\t\t(group {} (uuid \"{guuid}\"){lock} (members {}))", sexpr_str(&g.name), quoted.join(" ")).unwrap();
        }
    }
    writeln!(out, "\t)").unwrap();
    uuid
}

/// `MAGIC_TEARDROP_ZONE_ID`: the priority KiCad's teardrops start from, above any zone a user draws.
const MAGIC_TEARDROP_ZONE_ID: u32 = 30000;

/// `TEARDROP_MANAGER::setTeardropPriorities` for the teardrops of the design that have no priority (the studio's own, `eda_connectivity::generate_teardrops`): per layer,
/// by decreasing outline area (then id), `MAGIC_TEARDROP_ZONE_ID` and up, skipping the priorities the layer's other teardrops (a file's, which keep theirs) hold. Without
/// it a teardrop would tie with a pour of priority 0 of another net, and the uuid would decide which yields (`ZONE::HigherPriority`).
fn teardrop_priorities(zones: &[Zone]) -> std::collections::HashMap<&str, u32> {
    let area = |z: &Zone| {
        let n = z.outline.len();
        (0..n).map(|i| z.outline[i].x.as_f64() * z.outline[(i + 1) % n].y.as_f64() - z.outline[(i + 1) % n].x.as_f64() * z.outline[i].y.as_f64()).sum::<f64>().abs() / 2.0
    };
    let layer_rank = |z: &Zone| eda_model::layers::pcb_layer_id(&z.layer).unwrap_or(u32::MAX);
    let mut created: Vec<&Zone> = zones.iter().filter(|z| z.teardrop && z.priority == 0 && !z.id.is_empty()).collect();
    created.sort_by(|a, b| layer_rank(a).cmp(&layer_rank(b)).then(area(b).total_cmp(&area(a))).then(a.id.cmp(&b.id)));
    let mut out = std::collections::HashMap::new();
    let mut current: Option<&str> = None;
    let mut next = MAGIC_TEARDROP_ZONE_ID;
    for z in created {
        if current != Some(z.layer.as_str()) {
            current = Some(z.layer.as_str());
            next = MAGIC_TEARDROP_ZONE_ID;
        }
        let taken = |p: u32| zones.iter().any(|o| o.teardrop && o.priority == p && o.layer == z.layer);
        while taken(next) {
            next += 1;
        }
        out.insert(z.id.as_str(), next);
        next += 1;
    }
    out
}

/// A free-standing graphic shape as KiCad's `gr_line`/`gr_arc`/`gr_rect`/
/// `gr_circle`/`gr_poly`, matching `PCB_IO_KICAD_SEXPR::format(const
/// PCB_SHAPE*)`: stroke width and type first, `fill` only for the three
/// shapes KiCad actually fills (rect/circle/poly -- a line or an arc has no
/// interior, and KiCad's own writer never emits `fill` for either).
///
/// `style` is what the board keeps of the shape beyond `Shape` ([`eda_model::pcb_items::ShapeStyle`]): its hatch, its line endings (`(start_shape ..)` and `(end_shape ..)`
/// after the stroke, `LINE_ENDING::Format`; an open shape's only) and its net, which `net` has already turned into the file's number (0 is none: `GetNetCode() > 0`).
/// KiCad 9's board file version (`SEXPR_BOARD_FILE_VERSION` 20241229, "Expand User layers to arbitrary count"): what the writer writes a board as, so that KiCad 9 opens it
/// (`parseBOARD_unchecked`'s `checkVersion` refuses a newer file outright).
pub(crate) const KICAD9_BOARD_FILE_VERSION: i64 = 20241229;

/// The first board file version whose vias inherit the board's covering, plugging, capping and filling when they say none of their own (20250228, "ipc-4761 via protection
/// features"; `parsePCB_VIA`: `if( m_requiredVersion < 20250228 )` sets them all off, "Versions before 10.0 had no protection features other than tenting, so those features must
/// be interpreted as OFF in legacy boards, not as unspecified (aka: inherit from board stackup)").
pub(crate) const FIRST_VIA_PROTECTION_VERSION: i64 = 20250228;

/// The first board file version that may hold a copper thieving zone (20260513: `parseZONE`'s `T_thieving`, "thieving requires file version >= 20260513").
pub(crate) const FIRST_THIEVING_VERSION: i64 = 20260513;

/// Whether some copper pour of the board is written as a thieving zone (`DrawingsSection::zone_thieving`, the zone not hatched since).
fn has_thieving_zone(design: &Design) -> bool {
    let Some(d) = design.drawings.as_ref() else { return false };
    !d.zone_thieving.is_empty()
        && design.routing.as_ref().is_some_and(|r| r.zones.iter().any(|z| !z.teardrop && !z.is_rule_area && z.fill_mode == eda_model::ir::FillMode::Polygons && d.zone_thieving.iter().any(|t| t.id == z.id)))
}

/// The file version a board is written as: KiCad 9's, which every reader of this crate's files opens, unless the board holds KiCad 10 data whose meaning the version decides
/// (`m_requiredVersion` in 25ec2335's `pcb_io_kicad_sexpr_parser.cpp`):
/// - board defaults for the vias' covering, plugging, capping or filling (`DrawingsSection::setup_tokens`): a via that says nothing inherits them only from 20250228 on, so the
///   board is written as 20250228. The one other gate between the two versions, the stroked zone fill of before 20250210 (`isStrokedFill`), is off in either, as every zone is
///   written `(filled_areas_thickness no)`; KiCad 9 cannot read such a setup anyway.
/// - a copper thieving zone (`DrawingsSection::zone_thieving`): 20260513, the version `parseZONE` asks of one. The gates on the way are met as above, but for 20250926's: a pad's
///   `(die_delay ..)` is read as a time after it (`parsePAD`), so one kept among a pad's tokens from an older file would change its meaning (no QA board has both).
/// - The later gates are met where the board is read instead, the tree normalised to KiCad 9's meaning: bold text (20260826, `crate::bold_text`), a footprint's library frame
///   (20260616, `crate::fp_transform`), a reference image's scale (20260623, `crate::pcb_image::file_scale`). A version past 20260616 would have every footprint read in its
///   library frame, which this writer does not write.
pub(crate) fn board_file_version(design: &Design) -> i64 {
    if has_thieving_zone(design) {
        FIRST_THIEVING_VERSION
    } else if design.drawings.as_ref().is_some_and(|d| !d.setup_tokens.is_empty()) {
        FIRST_VIA_PROTECTION_VERSION
    } else {
        KICAD9_BOARD_FILE_VERSION
    }
}

/// The layer words of a track, an arc or a board shape: `(layer "F.Cu")`, or, for one with a solder-mask opening on an outer copper layer, `(layers "F.Cu" "F.Mask")` and its own
/// `(solder_mask_margin ..)` when it has one (`pcb_io_kicad_sexpr.cpp::format( PCB_TRACK )` and `format( PCB_SHAPE )`: `formatLayers( GetLayerSet() )` when the set has more than one
/// layer, and `GetLayerSet` adds `F_Mask` to `F_Cu` and `B_Mask` to `B_Cu` only; the margin `if( HasSolderMask() && GetLocalSolderMaskMargin().has_value() &&
/// IsExternalCopperLayer( GetLayer() ) )`).
pub(crate) fn cu_item_layer_words(layer: &str, has_mask: bool, margin: Option<Nm>) -> String {
    let mask_layer = match layer {
        "F.Cu" if has_mask => "F.Mask",
        "B.Cu" if has_mask => "B.Mask",
        _ => return format!("(layer {})", sexpr_str(layer)),
    };
    let margin = margin.map(|m| format!(" (solder_mask_margin {})", mm(m))).unwrap_or_default();
    format!("(layers {} {}){margin}", sexpr_str(layer), sexpr_str(mask_layer))
}

pub(crate) fn write_shape(out: &mut String, shape: &Shape, locked: bool, style: Option<&eda_model::pcb_items::ShapeStyle>, net: usize) -> String {
    let sw = |w: Nm| mm(w.max(Nm::ZERO));
    let uuid = crate::duid_for(&format!("shape:{}", shape.id()), shape.id());
    let hatch = style.and_then(|st| st.hatch);
    let ends = style.map(|st| crate::line_ending_io::words(&st.endings, matches!(shape, Shape::Rect { .. } | Shape::Circle { .. } | Shape::Polygon { .. }))).unwrap_or_default();
    // `(net ..)` after the layer, before the uuid.
    let layer = cu_item_layer_words(shape.layer(), style.is_some_and(|st| st.solder_mask), style.and_then(|st| st.solder_mask_margin));
    let layer = if net > 0 { format!("{layer} (net {net})") } else { layer };
    // `(fill yes)`, `(fill no)` or `(fill hatch)`, `(fill reverse_hatch)`, `(fill cross_hatch)`: a solid fill wins over a hatch.
    let fill = |filled: bool| match (filled, hatch) {
        (true, _) => "yes",
        (false, Some(h)) => h.token(),
        (false, None) => "no",
    };
    let lock = if locked { " (locked yes)" } else { "" };
    match shape {
        Shape::Segment { stroke_width, start, end, .. } => {
            writeln!(
                out,
                "\t(gr_line (start {} {}) (end {} {}) (stroke (width {}) (type solid)){ends}{lock} {layer} (uuid \"{uuid}\"))",
                mm(start.x), mm(start.y), mm(end.x), mm(end.y), sw(*stroke_width)
            )
            .unwrap();
        }
        Shape::Arc { stroke_width, start, mid, end, .. } => {
            writeln!(
                out,
                "\t(gr_arc (start {} {}) (mid {} {}) (end {} {}) (stroke (width {}) (type solid)){ends}{lock} {layer} (uuid \"{uuid}\"))",
                mm(start.x), mm(start.y), mm(mid.x), mm(mid.y), mm(end.x), mm(end.y), sw(*stroke_width)
            )
            .unwrap();
        }
        Shape::Rect { stroke_width, filled, start, end, .. } => {
            // `if( aShape->GetCornerRadius() > 0 ) m_out->Print( " (radius %s)", .. )`, after the end.
            let radius = style.map(|st| st.corner_radius).filter(|&r| r > Nm::ZERO).map(|r| format!(" (radius {})", mm(r))).unwrap_or_default();
            writeln!(
                out,
                "\t(gr_rect (start {} {}) (end {} {}){radius} (stroke (width {}) (type solid)) (fill {}){lock} {layer} (uuid \"{uuid}\"))",
                mm(start.x), mm(start.y), mm(end.x), mm(end.y), sw(*stroke_width), fill(*filled)
            )
            .unwrap();
        }
        Shape::Circle { stroke_width, filled, center, end, .. } => {
            writeln!(
                out,
                "\t(gr_circle (center {} {}) (end {} {}) (stroke (width {}) (type solid)) (fill {}){lock} {layer} (uuid \"{uuid}\"))",
                mm(center.x), mm(center.y), mm(end.x), mm(end.y), sw(*stroke_width), fill(*filled)
            )
            .unwrap();
        }
        Shape::Polygon { stroke_width, filled, pts, .. } => {
            write!(out, "\t(gr_poly (pts").unwrap();
            for p in pts {
                write!(out, " (xy {} {})", mm(p.x), mm(p.y)).unwrap();
            }
            writeln!(out, ") (stroke (width {}) (type solid)) (fill {}){lock} {layer} (uuid \"{uuid}\"))", sw(*stroke_width), fill(*filled)).unwrap();
        }
        // `case SHAPE_T::BEZIER:` -- `(gr_curve (pts (xy start) (xy c1) (xy c2) (xy end)) ...)`; like a line, never filled.
        Shape::Bezier { stroke_width, start, c1, c2, end, .. } => {
            writeln!(
                out,
                "\t(gr_curve (pts (xy {} {}) (xy {} {}) (xy {} {}) (xy {} {})) (stroke (width {}) (type solid)){ends}{lock} {layer} (uuid \"{uuid}\"))",
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
pub(crate) fn write_text(out: &mut String, text: &Text, locked: bool) -> String {
    write_text_with(out, text, locked, None)
}

/// [`write_text`] with the looks `Text` has no fields for (`TextExtra`: a glyph width that is not the height, a vertical justification, bold, italic, a knockout, line
/// spacing, a font face, an automatic stroke width).
pub(crate) fn write_text_with(out: &mut String, text: &Text, locked: bool, extra: Option<&eda_model::board_text::TextExtra>) -> String {
    let uuid = crate::duid_for(&format!("text:{}", text.id), &text.id);
    let lock = if locked { " (locked yes)" } else { "" };
    let angle_deg = fmt_mm_f(text.angle as f64 / 1000.0);
    let knockout = if extra.is_some_and(|e| e.knockout) { " knockout" } else { "" };
    writeln!(
        out,
        "\t(gr_text {}{lock} (at {} {} {angle_deg}) (layer {}{knockout}) (uuid \"{uuid}\")\n\t\t{}\n\t)",
        sexpr_str(&text.content),
        mm(text.at.x),
        mm(text.at.y),
        sexpr_str(&text.layer),
        crate::board_text_io::effects(text, extra)
    )
    .unwrap();
    uuid
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{PlacementSection, Point, Provenance, RoutingSection, TextJustify};
    use eda_model::{Net, Pin, PinKind};

    fn pin(number: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: None, kind }
    }
    fn part(reference: &str, package: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(format!("{reference}_val")), package: Some(package.into()), footprint: Some(package.into()), pins, body_nm: None, symbol: None, datasheet: None, edge: None }
    }
    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
    }

    /// P13-1: a board with teardrops says `(legacy_teardrops yes)`, so kicad-cli's refill keeps them (`UpdateTeardrops` returns at once) and fills them as zones; the
    /// studio's teardrops, which have no priority, get `MAGIC_TEARDROP_ZONE_ID` and up per layer by decreasing area (`setTeardropPriorities`), and a teardrop that has
    /// one (a file's) keeps it and its number is skipped. A board without teardrops says `no`, as before.
    #[test]
    fn a_board_with_teardrops_keeps_them_through_kicad_clis_refill() {
        let (mut design, model) = fixture();
        assert!(export_kicad_pcb(&design, &model, &meta()).unwrap().contains("(legacy_teardrops no)"));
        let tri = |id: &str, size: Nm, priority: u32| Zone {
            id: id.into(),
            net: "GND".into(),
            layer: "F.Cu".into(),
            outline: vec![Point { x: Nm::from_mm(1.0), y: Nm::from_mm(1.0) }, Point { x: Nm::from_mm(1.0) + size, y: Nm::from_mm(1.0) }, Point { x: Nm::from_mm(1.0), y: Nm::from_mm(1.0) + size }],
            teardrop: true,
            priority,
            ..Default::default()
        };
        design.routing.as_mut().unwrap().zones = vec![tri("small", Nm::from_mm(0.5), 0), tri("big", Nm::from_mm(0.9), 0), tri("kicads", Nm::from_mm(0.7), 30000)];
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(legacy_teardrops yes)"), "{out}");
        let priorities: Vec<u32> = out.split("(zone").skip(1).filter_map(|z| z.split("(priority ").nth(1)).map(|p| p.split(')').next().unwrap().parse().unwrap()).collect();
        let mut sorted = priorities.clone();
        sorted.sort();
        assert_eq!(sorted, vec![30000, 30001, 30002], "{out}");
        assert_eq!(teardrop_priorities(&design.routing.as_ref().unwrap().zones).into_iter().collect::<std::collections::BTreeMap<_, _>>(), [("big", 30001), ("small", 30002)].into_iter().collect());
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
                Point { x: Nm::ZERO, y: Nm::ZERO },
                Point { x: Nm::from_mm(20.0), y: Nm::ZERO },
                Point { x: Nm::from_mm(20.0), y: Nm::from_mm(20.0) },
                Point { x: Nm::ZERO, y: Nm::from_mm(20.0) },
            ],
            footprints: vec![
                FootprintInstance { id: "U1".into(), at: Point { x: Nm::from_mm(5.0), y: Nm::from_mm(5.0) }, rot: 0, side: Side::Top, label: Default::default() },
                FootprintInstance { id: "C1".into(), at: Point { x: Nm::from_mm(10.0), y: Nm::from_mm(10.0) }, rot: 90_000, side: Side::Bottom, label: Default::default() },
            ],
            modules: Vec::new(),
        };
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: Some(placement),
            routing: Some(RoutingSection {
                tracks: vec![Track { id: String::new(), net: "VIN".into(), pins: vec![], layer: "F.Cu".into(), width: Nm::from_mm(0.2), pts: vec![Point { x: Nm::from_mm(5.0), y: Nm::from_mm(5.0) }, Point { x: Nm::from_mm(10.0), y: Nm::from_mm(5.0) }], arc_mid_offset: None }],
                vias: vec![],
                zones: vec![],
                track_width_presets: vec![],
                via_presets: vec![],
                diff_pair_presets: vec![],
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
        design.drawings = Some(eda_model::ir::DrawingsSection { aux_origin: Some(Point { x: Nm::from_mm(12.5), y: Nm::from_mm(-3.0) }), ..Default::default() });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(aux_axis_origin 12.5 -3"), "{out}");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        assert_eq!(back.drawings.and_then(|d| d.aux_origin), Some(Point { x: Nm::from_mm(12.5), y: Nm::from_mm(-3.0) }), "an imported board keeps its drill/place file origin");
        // A board saved with the default origin imports with none set (no empty drawings section is invented for it).
        let (plain, _, _) = crate::import_kicad_pcb(&export_kicad_pcb(&fixture().0, &model, &meta()).unwrap()).unwrap();
        assert!(plain.drawings.and_then(|d| d.aux_origin).is_none());
    }

    #[test]
    fn the_grid_origin_is_written_and_read_back_by_the_importer() {
        let (mut design, model) = fixture();
        assert!(export_kicad_pcb(&design, &model, &meta()).unwrap().contains("(grid_origin 0 0)"), "none set: KiCad's default");
        design.drawings = Some(eda_model::ir::DrawingsSection { grid_origin: Some(Point { x: Nm::from_mm(13.37), y: Nm::from_mm(-8.1) }), ..Default::default() });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(grid_origin 13.37 -8.1)"), "{out}");
        assert!(out.contains("(aux_axis_origin 0 0)"), "the drill/place file origin is its own setting");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        assert_eq!(back.drawings.as_ref().and_then(|d| d.grid_origin), Some(Point { x: Nm::from_mm(13.37), y: Nm::from_mm(-8.1) }));
        assert_eq!(back.drawings.and_then(|d| d.aux_origin), None);
        let (plain, _, _) = crate::import_kicad_pcb(&export_kicad_pcb(&fixture().0, &model, &meta()).unwrap()).unwrap();
        assert!(plain.drawings.and_then(|d| d.grid_origin).is_none(), "the default origin is not kept");
    }

    #[test]
    fn the_boards_paper_and_title_block_are_written_and_read_back_by_the_importer() {
        let (mut design, model) = fixture();
        let plain = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(plain.contains("(paper \"A4\")") && !plain.contains("(title_block"), "never set: A4 landscape, no title block");
        design.drawings = Some(eda_model::ir::DrawingsSection {
            page: Some(eda_model::page::PageSettings { paper: "A3".into(), portrait: true, user_size_nm: None }),
            title_block: Some(eda_model::ir::TitleBlock { title: "Blinky".into(), rev: "B".into(), comments: vec!["one".into()], ..Default::default() }),
            ..Default::default()
        });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(paper \"A3\" portrait)"), "{out}");
        assert!(out.contains("(title_block\n\t\t(title \"Blinky\")\n\t\t(rev \"B\")\n\t\t(comment 1 \"one\")\n\t)"), "{out}");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let drawings = back.drawings.unwrap();
        assert_eq!(drawings.page, Some(eda_model::page::PageSettings { paper: "A3".into(), portrait: true, user_size_nm: None }));
        assert_eq!(drawings.title_block, Some(eda_model::ir::TitleBlock { title: "Blinky".into(), rev: "B".into(), comments: vec!["one".into()], ..Default::default() }));
        // A user size goes out in mm and comes back as the same size.
        design.drawings.as_mut().unwrap().page = Some(eda_model::page::PageSettings { paper: "User".into(), portrait: false, user_size_nm: Some((Nm::from_mm(300.0), Nm::from_mm(200.5))) });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(paper \"User\" 300 200.5)"), "{out}");
        assert_eq!(crate::import_kicad_pcb(&out).unwrap().0.drawings.unwrap().page.unwrap().user_size_nm, Some((Nm::from_mm(300.0), Nm::from_mm(200.5))));
        // A board saved with the defaults imports with none of them (no empty drawings section is invented for it).
        let (default_back, _, _) = crate::import_kicad_pcb(&plain).unwrap();
        assert!(default_back.drawings.is_none() || default_back.drawings.as_ref().is_some_and(|d| d.page.is_none() && d.title_block.is_none()));
    }

    #[test]
    fn a_footprint_nothing_was_edited_on_is_written_as_always_with_its_type_from_its_pads() {
        let (design, model) = fixture();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        // Both parts have surface-mount pads: the type KiCad's libraries would give them, so `--smd-only` finds them.
        assert_eq!(out.matches("(attr smd)").count(), 2, "{out}");
        // The Reference above the courtyard, horizontal in the file whatever the footprint's angle; the Value 1 mm below the origin.
        assert!(out.contains("(property \"Reference\" \"U1\" (at 0 -"), "{out}");
        assert!(out.contains(" 0) (layer \"F.SilkS\")\n"), "{out}");
        assert!(out.contains("(property \"Value\" \"U1_val\" (at 0 1 0) (layer \"F.Fab\")"), "{out}");
        // The bottom-side part: back layers, mirrored text.
        assert!(out.contains("(layer \"B.SilkS\")") && out.contains("(layer \"B.Fab\")") && out.contains("(justify mirror)"), "{out}");
    }

    #[test]
    fn the_fields_attributes_and_pad_edits_of_a_footprint_are_written_and_read_back_by_the_importer() {
        use eda_model::fp_edit::{FieldLayout, FootprintAttrs, FootprintEdit, FootprintKind, PadEdit, UserField};
        let (mut design, mut model) = fixture();
        let mut reference = FieldLayout::new(Point { x: Nm::from_mm(1.5), y: Nm::from_mm(-2.5) }, "F.Fab");
        reference.size = (Nm::from_mm(1.2), Nm::from_mm(0.9));
        reference.thickness = Nm::from_mm(0.18);
        reference.angle = 90_000;
        reference.halign = -1;
        reference.italic = true;
        let mut value = FieldLayout::new(Point { x: Nm::from_mm(-1.0), y: Nm::from_mm(3.0) }, "F.SilkS");
        value.visible = false;
        let mut vendor = FieldLayout::new(Point { x: Nm::ZERO, y: Nm::from_mm(4.0) }, "F.Fab");
        vendor.visible = false;
        let mut pad1 = PadEdit::none("1", 1);
        pad1.offset = Some(Point { x: Nm::from_mm(0.2), y: Nm::from_mm(0.1) });
        pad1.solder_mask_margin = Some(Nm::from_mm(0.04));
        let mut u1 = FootprintEdit::new("U1");
        u1.reference = Some(reference.clone());
        u1.value = Some(value.clone());
        u1.fields = vec![UserField { name: "Vendor".into(), text: "ACME".into(), layout: vendor.clone() }];
        u1.attrs = Some(FootprintAttrs { kind: FootprintKind::Unspecified, board_only: false, exclude_from_pos_files: true, exclude_from_bom: true, dnp: true, allow_missing_courtyard: true });
        u1.set_pad(pad1.clone());
        // As the board's loader folds the pad edits into the instance's own footprint (`fold_footprint_edits`).
        let library = model.library_footprint_of(model.part("U1").unwrap()).unwrap();
        model.instance_footprints.insert("U1".into(), eda_model::fp_edit::patched_footprint(&library, &u1.pads));
        design.drawings = Some(eda_model::ir::DrawingsSection { footprint_edits: vec![u1], ..Default::default() });

        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(property \"Reference\" \"U1\" (at 1.5 -2.5 90) (layer \"F.Fab\")"), "{out}");
        assert!(out.contains("(effects (font (size 0.9 1.2) (thickness 0.18) italic) (justify left))"), "{out}");
        assert!(out.contains("(property \"Value\" \"U1_val\" (at -1 3 0) (layer \"F.SilkS\") (hide yes)"), "{out}");
        assert!(out.contains("(property \"Vendor\" \"ACME\" (at 0 4 0) (layer \"F.Fab\") (hide yes)"), "{out}");
        assert!(out.contains("(attr exclude_from_pos_files exclude_from_bom allow_missing_courtyard dnp)"), "U1's own attributes: {out}");
        assert!(out.contains("(drill (offset 0.2 0.1))") && out.contains("(solder_mask_margin 0.04)"), "{out}");

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let edits = &back.drawings.as_ref().unwrap().footprint_edits;
        let u1_back = edits.iter().find(|e| e.id == "U1").expect("U1's edit comes back");
        assert_eq!(u1_back.reference.as_ref(), Some(&reference));
        assert_eq!(u1_back.value.as_ref(), Some(&value));
        assert_eq!(u1_back.fields.len(), 1);
        assert_eq!((u1_back.fields[0].name.as_str(), u1_back.fields[0].text.as_str(), &u1_back.fields[0].layout), ("Vendor", "ACME", &vendor));
        assert_eq!(u1_back.attrs, Some(FootprintAttrs { kind: FootprintKind::Unspecified, board_only: false, exclude_from_pos_files: true, exclude_from_bom: true, dnp: true, allow_missing_courtyard: true }));
        assert_eq!(u1_back.pad("1", 1), Some(&pad1));
        // C1 (bottom, turned): its Reference and Value come back too, unchanged from where the writer put them.
        let c1_back = edits.iter().find(|e| e.id == "C1").expect("an imported footprint keeps the text the file has");
        assert_eq!(c1_back.reference.as_ref().unwrap().layer, "B.SilkS");
        assert!(c1_back.reference.as_ref().unwrap().mirror);
    }

    /// p13-p16 P14-2: a footprint flipped here is written as KiCad's `FOOTPRINT::Flip` leaves one -- y mirrored in its own frame, the half turn in its
    /// orientation, its pads' absolute angles turned with it, its 3D model as the library has it -- where it was written mirrored in x with the half turn in
    /// the model. The copper is where it was: read back, every pad is on the same point of the board.
    #[test]
    fn a_flipped_footprint_is_written_in_kicads_frame() {
        let (mut design, model) = fixture();
        let c1 = design.placement.as_mut().unwrap().footprints.iter_mut().find(|f| f.id == "C1").unwrap();
        c1.rot = 0;
        let lib = model.footprint_of(model.part("C1").unwrap()).unwrap();
        let pad1 = lib.pads.iter().find(|p| p.number == "1").unwrap().at;
        assert!(pad1.0 < Nm::ZERO && pad1.1 == Nm::ZERO, "the 0603's pad 1 is on the left: {pad1:?}");
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let start = out.find("(footprint \"0603\"").expect("C1");
        let block = &out[start..start + out[start..].find("\n\t)").unwrap()];
        assert!(block.contains("(layer \"B.Cu\")") && block.contains("(at 10 10 180)"), "the half turn is in the orientation: {block}");
        let x = mm(pad1.0);
        assert!([180, -180].iter().any(|a| block.contains(&format!("(pad \"1\" smd roundrect (at {x} 0 {a})"))), "pad 1 at its library x, y mirrored, turned with the footprint: {block}");
        // its model as it is held, with no half turn of its own (`Model3d::flipped_frame` is the reader's)
        let held = crate::part_models(&model, model.part("C1").unwrap(), &lib).unwrap().models;
        let mut want = String::new();
        crate::footprint_lib::write_models(&mut want, &held, 2);
        assert!(block.contains(want.trim_end()), "{block}\n{want}");
        // the same pads on the board: read back, each is where the studio has it
        let (back, back_model, _) = crate::import_kicad_pcb(&out).unwrap();
        let placed = |d: &Design, m: &ConstraintModel| -> Vec<(String, Point)> {
            let f = d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "C1").unwrap();
            let mut pads: Vec<(String, Point)> = m.footprint_of(m.part("C1").unwrap()).unwrap().pads.iter().map(|p| (p.number.clone(), eda_model::footprint::to_board(f, p.at))).collect();
            pads.sort_by(|a, b| a.0.cmp(&b.0));
            pads
        };
        assert_eq!(placed(&back, &back_model), placed(&design, &model));
    }

    #[test]
    fn a_bottom_side_footprint_edit_goes_across_and_back_to_the_same_place_on_the_board() {
        use eda_model::fp_edit::{FieldLayout, FootprintEdit};
        let (mut design, model) = fixture();
        // C1 sits on the bottom, turned 90: its field's angle and position cross the mirror and the turn and come back the same.
        let mut l = FieldLayout::new(Point { x: Nm::from_mm(0.8), y: Nm::from_mm(-1.9) }, "B.SilkS");
        l.angle = 30_000;
        let mut c1 = FootprintEdit::new("C1");
        c1.reference = Some(l.clone());
        design.drawings = Some(eda_model::ir::DrawingsSection { footprint_edits: vec![c1], ..Default::default() });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let got = back.drawings.as_ref().unwrap().footprint_edits.iter().find(|e| e.id == "C1").unwrap().reference.clone().unwrap();
        // The footprint is written in KiCad's frame (`write_footprint`'s half turn) and read back in the library's (`FootprintExtra::library_frame`): the
        // field comes back as it was, and the footprint's orientation too.
        let instance = |d: &Design| d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "C1").unwrap().clone();
        assert_eq!(instance(&back).rot, instance(&design).rot);
        assert_eq!(got, l);
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
        assert!(out.contains("(46 \"B.CrtYd\" user \"B.Courtyard\")") && out.contains("(47 \"F.CrtYd\" user \"F.Courtyard\")"), "both courtyard layers declared, with the names KiCad shows for them (formatBoardLayers)");
    }

    #[test]
    fn an_intent_boards_project_asks_the_hole_clearance_the_router_keeps() {
        // `BOARD_DESIGN_SETTINGS::BOARD_DESIGN_SETTINGS`: `m_HoleClearance = mmToIU( DEFAULT_HOLECLEARANCE )`, 0.25 mm
        // (`board_design_settings.h`), and `m_HoleToHoleMin` 0.25 mm. The router (`pns::rule_resolver`, `HOLE_CLEARANCE`)
        // keeps `board.hole_clearance_nm`; the project kicad-cli checks it with has to ask the same, not the class clearance.
        let (_, model) = fixture();
        assert!(!model.board.constraints_explicit);
        let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro(&model)).expect("valid json");
        let rules = &pro["board"]["design_settings"]["rules"];
        assert_eq!(rules["min_hole_clearance"], serde_json::json!(0.25), "KiCad's DEFAULT_HOLECLEARANCE, the router's hole_clearance_nm");
        assert_eq!(rules["min_hole_to_hole"], serde_json::json!(0.25), "KiCad's DEFAULT_HOLETOHOLEMIN");
        let mut tighter = model.clone();
        tighter.board.hole_clearance_nm = Nm::new(300_000);
        tighter.board.hole_to_hole_min_nm = Nm::new(400_000);
        let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro(&tighter)).expect("valid json");
        assert_eq!(pro["board"]["design_settings"]["rules"]["min_hole_clearance"], serde_json::json!(0.3));
        assert_eq!(pro["board"]["design_settings"]["rules"]["min_hole_to_hole"], serde_json::json!(0.4));
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
            user_fields: Default::default(), field_layout: Default::default(), title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(), imported_from_kicad: false,
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
    fn the_project_lists_the_waived_violations_and_the_chosen_erc_severities() {
        use eda_model::ir::{DrawingsSection, DrcExclusion};
        let (mut design, model) = fixture();
        let plain: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).expect("valid json");
        assert!(plain["board"]["design_settings"]["drc_exclusions"].is_null(), "nothing waived: the key is not written");
        assert!(plain["board"]["design_settings"]["meta"].is_null() && plain["erc"]["meta"].is_null(), "and neither section says which schema it is at");
        assert_eq!(plain["erc"]["rule_severities"].as_object().map(|o| o.len()), Some(3), "only the three library checks, which are always ignored");

        design.drawings = Some(DrawingsSection {
            drc_exclusions: vec![
                DrcExclusion { check: "annular_width".into(), items: vec!["via-uuid".into()], ids: vec!["v1".into()], positions_nm: vec![[Nm::from_mm(12.459), Nm::from_mm(18.39)]], comment: "tight on purpose".into() },
                DrcExclusion { check: "clearance".into(), items: vec!["a".into(), "b".into()], ids: vec![], positions_nm: vec![], comment: String::new() },
            ],
            ..Default::default()
        });
        design.schematic = Some(eda_model::ir::SchematicSection {
            symbols: vec![], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![],
            erc_exclusions: vec![
                eda_model::ir::ErcExclusion {
                    check: "footprint_link_issues".into(),
                    location: "R1".into(),
                    comment: "bare footprint on purpose".into(),
                    marker: Some(eda_model::ir::ErcMarker { error_type: "ERCET_FOOTPRINT_LINK_ISSUES".into(), at_nm: [Nm::from_mm(113.03), Nm::from_mm(100.33)], items: vec!["a4f97020-e09b-4e24-b611-b376f3c27df7".into()], ..Default::default() }),
                },
                eda_model::ir::ErcExclusion { check: "wire_dangling".into(), location: "W1".into(), ..Default::default() },
            ],
            erc_pin_map: None,
            user_fields: Default::default(), title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![],
            extras: eda_model::sch_extras::SchExtras { erc_severities: [("pin_not_connected".to_string(), "warning".to_string()), ("lib_symbol_mismatch".to_string(), "warning".to_string())].into(), ..Default::default() },
            imported_from_kicad: false,
            field_layout: Default::default(),
        });
        let json: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).expect("valid json");
        let entries = json["board"]["design_settings"]["drc_exclusions"].as_array().expect("a list");
        assert_eq!(entries.len(), 1, "an exclusion with no position to try is the studio's alone: {entries:?}");
        assert_eq!(entries[0]["marker"], serde_json::json!({ "errorType": "DRCET_ANNULAR_WIDTH", "position": { "xNm": "12459000", "yNm": "18390000" }, "items": [{ "value": "via-uuid" }] }));
        assert_eq!(entries[0]["comment"], "tight on purpose");
        assert_eq!(json["board"]["design_settings"]["meta"], serde_json::json!({ "version": 3 }), "records are what a board's design settings at schema 3 list (at 2 they are texts)");
        assert_eq!(json["erc"]["meta"], serde_json::json!({ "version": 1 }), "and what an erc section at schema 1 lists (at 0 they are texts)");
        assert!(json["schematic"]["connection_grid_size"].is_null(), "KiCad's default grid is not written");
        let waived = json["erc"]["erc_exclusions"].as_array().expect("a list");
        assert_eq!(waived.len(), 1, "an ERC exclusion whose marker is not known is the studio's alone: {waived:?}");
        assert_eq!(
            waived[0],
            serde_json::json!({
                "marker": { "errorType": "ERCET_FOOTPRINT_LINK_ISSUES", "position": { "xNm": "113030000", "yNm": "100330000" }, "items": [{ "value": "a4f97020-e09b-4e24-b611-b376f3c27df7" }] },
                "comment": "bare footprint on purpose",
            })
        );
        assert_eq!(json["erc"]["rule_severities"]["pin_not_connected"], "warning", "the schematic's own choice");
        assert_eq!(json["erc"]["rule_severities"]["lib_symbol_mismatch"], "warning", "and it can undo the embedded-library default");
        assert_eq!(json["erc"]["rule_severities"]["lib_symbol_issues"], "ignore");
        assert!(json["board"]["design_settings"]["rules"]["min_clearance"].is_number(), "the board half is untouched");
    }

    #[test]
    fn the_project_carries_the_connection_grid_the_schematic_setup_chose() {
        let (mut design, model) = fixture();
        let mut sch = eda_model::ir::SchematicSection {
            symbols: vec![], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], erc_pin_map: None,
            user_fields: Default::default(), title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(), imported_from_kicad: false,
            field_layout: Default::default(),
        };
        design.schematic = Some(sch.clone());
        let plain: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
        assert!(plain["schematic"].is_null(), "nothing chosen: KiCad's own 50 mils applies and the project says nothing");
        sch.extras.connection_grid_mils = Some(25);
        design.schematic = Some(sch);
        let json: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
        assert_eq!(json["schematic"]["connection_grid_size"], 25.0, "in mils, as `connection_grid_size` is in the project file");
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

    /// A pad's zone connection, relief gap, spoke width and angle and clearance, and its footprint's connection and clearance,
    /// are written as KiCad writes them -- so kicad-cli's fill and DRC see the board's own edits -- and the importer reads
    /// every one of them back.
    #[test]
    fn pad_and_footprint_zone_overrides_are_written_and_read_back() {
        use eda_model::ir::{FootprintZoneFacts, FootprintZoneOverrides, PadConnection, PadZoneOverride};
        let (mut design, model) = fixture();
        design.drawings = Some(eda_model::ir::DrawingsSection {
            zone_overrides: vec![FootprintZoneOverrides {
                id: "U1".into(),
                footprint: Some(FootprintZoneFacts { zone_connection: Some(PadConnection::None), clearance: Some(Nm::from_mm(0.8)) }),
                pads: vec![PadZoneOverride { number: "1".into(), clearance: Some(Nm::from_mm(0.6)), zone_connection: Some(PadConnection::Full), thermal_gap: Some(Nm::from_mm(0.4)), thermal_spoke_width: Some(Nm::from_mm(0.3)), thermal_spoke_angle_mdeg: Some(45_000) }],
            }],
            ..Default::default()
        });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        for want in ["(clearance 0.8)", "(zone_connect 0)", "(clearance 0.6)", "(zone_connect 2)", "(thermal_gap 0.4)", "(thermal_bridge_width 0.3)", "(thermal_bridge_angle 45)"] {
            assert!(out.contains(want), "missing {want}");
        }
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let extra = back.drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == "U1").unwrap();
        assert_eq!((extra.zone_connection, extra.clearance), (Some(PadConnection::None), Some(Nm::from_mm(0.8))));
        let p1 = &extra.pads[0];
        assert_eq!((p1.zone_connection, p1.thermal_gap, p1.thermal_spoke_width, p1.thermal_spoke_angle_mdeg, p1.clearance), (Some(PadConnection::Full), Some(Nm::from_mm(0.4)), Some(Nm::from_mm(0.3)), Some(45_000), Some(Nm::from_mm(0.6))));
        assert_eq!(extra.pads[1].zone_connection, None, "the other pads inherit");
    }

    /// A pad that carries an edit of ours (the shape's offset, a solder mask margin) and the zone filler's overrides (clearance, zone
    /// connection, thermal relief) is ONE `(pad ..)` form with both, in KiCad's order, and the importer reads each half back into its own
    /// overlay: `footprint_edits` for the extras, `zone_overrides`' source (`footprint_extras`) for the zone facts. A footprint edit
    /// (attributes, fields) and the footprint's own zone connection are written together too.
    #[test]
    fn one_pad_form_carries_the_pad_edit_and_the_zone_overrides_and_both_are_read_back() {
        use eda_model::fp_edit::{FootprintAttrs, FootprintEdit, FootprintKind, PadEdit};
        use eda_model::ir::{FootprintZoneFacts, FootprintZoneOverrides, PadConnection, PadZoneOverride};
        let (mut design, mut model) = fixture();
        let mut pad1 = PadEdit::none("1", 1);
        pad1.offset = Some(Point { x: Nm::from_mm(0.2), y: Nm::from_mm(0.1) });
        pad1.solder_mask_margin = Some(Nm::from_mm(0.04));
        pad1.solder_paste_margin_ratio = Some(-0.1);
        let mut u1 = FootprintEdit::new("U1");
        u1.attrs = Some(FootprintAttrs { kind: FootprintKind::Smd, dnp: true, ..Default::default() });
        u1.set_pad(pad1.clone());
        let library = model.library_footprint_of(model.part("U1").unwrap()).unwrap();
        model.instance_footprints.insert("U1".into(), eda_model::fp_edit::patched_footprint(&library, &u1.pads));
        design.drawings = Some(eda_model::ir::DrawingsSection {
            footprint_edits: vec![u1],
            zone_overrides: vec![FootprintZoneOverrides {
                id: "U1".into(),
                footprint: Some(FootprintZoneFacts { zone_connection: Some(PadConnection::None), clearance: Some(Nm::from_mm(0.8)) }),
                pads: vec![PadZoneOverride { number: "1".into(), clearance: Some(Nm::from_mm(0.6)), zone_connection: Some(PadConnection::Full), thermal_gap: Some(Nm::from_mm(0.4)), thermal_spoke_width: Some(Nm::from_mm(0.3)), thermal_spoke_angle_mdeg: Some(45_000) }],
            }],
            ..Default::default()
        });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let pad_line = out.lines().find(|l| l.contains("(pad \"1\"") && l.contains("(thermal_gap")).unwrap_or_else(|| panic!("pad 1 of U1 with its zone facts:\n{out}")).to_string();
        assert!(pad_line.contains("(drill (offset 0.2 0.1))"), "the shape offset of the pad edit: {pad_line}");
        let at = |what: &str| pad_line.find(what).unwrap_or_else(|| panic!("{what} in {pad_line}"));
        assert!(
            at("(solder_mask_margin 0.04)") < at("(solder_paste_margin_ratio -0.1)") && at("(solder_paste_margin_ratio -0.1)") < at("(clearance 0.6)") && at("(clearance 0.6)") < at("(zone_connect 2)") && at("(zone_connect 2)") < at("(thermal_bridge_width 0.3)") && at("(thermal_bridge_width 0.3)") < at("(thermal_bridge_angle 45)") && at("(thermal_bridge_angle 45)") < at("(thermal_gap 0.4)"),
            "the margins of the edit, then the zone facts, in KiCad's order: {pad_line}"
        );
        // The footprint's own: the attributes of the edit and the footprint-level clearance and zone connection.
        assert!(out.contains("(attr smd dnp)"), "{out}");
        assert!(out.contains("\t\t(clearance 0.8)") && out.contains("\t\t(zone_connect 0)"), "{out}");

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let dr = back.drawings.as_ref().unwrap();
        // Our half: the offset and the margins are the pad edit again, and the clearance is not (a pad has one clearance, the zone overlay's).
        assert_eq!(dr.footprint_edits.iter().find(|e| e.id == "U1").and_then(|e| e.pad("1", 1)), Some(&pad1));
        // The zone half: what the importer keeps of the zone facts.
        let extra = dr.footprint_extras.iter().find(|e| e.id == "U1").expect("U1's imported facts");
        assert_eq!((extra.zone_connection, extra.clearance), (Some(PadConnection::None), Some(Nm::from_mm(0.8))));
        let p1 = &extra.pads[0];
        assert_eq!((p1.zone_connection, p1.thermal_gap, p1.thermal_spoke_width, p1.thermal_spoke_angle_mdeg, p1.clearance), (Some(PadConnection::Full), Some(Nm::from_mm(0.4)), Some(Nm::from_mm(0.3)), Some(45_000), Some(Nm::from_mm(0.6))));
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
                Shape::Segment { id: "s1".into(), layer: "F.SilkS".into(), stroke_width: Nm::from_mm(0.15), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: Nm::from_mm(1.0), y: Nm::ZERO } },
                Shape::Rect { id: "s2".into(), layer: "F.Fab".into(), stroke_width: Nm::from_mm(0.1), filled: true, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: Nm::from_mm(2.0), y: Nm::from_mm(2.0) } },
                Shape::Circle { id: "s3".into(), layer: "B.SilkS".into(), stroke_width: Nm::from_mm(0.1), filled: false, center: Point { x: Nm::from_mm(5.0), y: Nm::from_mm(5.0) }, end: Point { x: Nm::from_mm(6.0), y: Nm::from_mm(5.0) } },
                Shape::Polygon { id: "s4".into(), layer: "F.CrtYd".into(), stroke_width: Nm::from_mm(0.05), filled: true, pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::from_mm(1.0), y: Nm::ZERO }, Point { x: Nm::from_mm(1.0), y: Nm::from_mm(1.0) }] },
                Shape::Arc { id: "s5".into(), layer: "Cmts.User".into(), stroke_width: Nm::from_mm(0.1), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, mid: Point { x: Nm::from_mm(0.707), y: Nm::from_mm(0.707) }, end: Point { x: Nm::from_mm(1.0), y: Nm::from_mm(1.0) } },
                Shape::Bezier { id: "s6".into(), layer: "F.SilkS".into(), stroke_width: Nm::from_mm(0.12), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, c1: Point { x: Nm::ZERO, y: Nm::from_mm(2.0) }, c2: Point { x: Nm::from_mm(3.0), y: Nm::from_mm(2.0) }, end: Point { x: Nm::from_mm(3.0), y: Nm::ZERO } },
            ],
            texts: vec![
                Text { id: "t1".into(), content: "REV A".into(), at: Point { x: Nm::from_mm(1.0), y: Nm::from_mm(2.0) }, angle: 90_000, layer: "F.SilkS".into(), size_nm: Nm::from_mm(1.0), stroke_width: Nm::from_mm(0.15), justify: TextJustify::Left, mirror: true },
                Text { id: "t2".into(), content: "centred".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: 0, layer: "F.Fab".into(), size_nm: Nm::from_mm(1.0), stroke_width: Nm::from_mm(0.15), justify: TextJustify::Center, mirror: false },
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
        let centred_block = &a[centred_start..(centred_start + 200_000).min(a.len())];
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
                Point { x: Nm::ZERO, y: Nm::ZERO },
                Point { x: Nm::from_mm(20.0), y: Nm::ZERO },
                Point { x: Nm::from_mm(20.0), y: Nm::from_mm(20.0) },
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
            outline: vec![Point { x: Nm::from_mm(1.0), y: Nm::from_mm(1.0) }, Point { x: Nm::from_mm(8.0), y: Nm::from_mm(1.0) }, Point { x: Nm::from_mm(8.0), y: Nm::from_mm(8.0) }, Point { x: Nm::from_mm(1.0), y: Nm::from_mm(8.0) }],
            is_rule_area: true,
            keepout_tracks: true,
            keepout_vias: true,
            ..Default::default()
        }];
        let out = export_kicad_pcb(&design, &model, &meta()).expect("a board with a rule area must export");
        let zone = out.split("\t(zone").nth(1).expect("one zone block");
        assert!(zone.contains("(keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour allowed) (footprints allowed))"), "{zone}");
        assert!(zone.contains("(placement (enabled no) (sheetname \"\"))"), "{zone}");
        assert!(!zone.contains("(net ") && !zone.contains("(net_name"), "a rule area has no net: {zone}");
        assert!(!zone.contains("filled_polygon"), "a rule area is never filled: {zone}");
    }

    fn square(x: Nm, y: Nm, side: Nm) -> Vec<eda_model::ir::Point> {
        vec![
            eda_model::ir::Point { x, y },
            eda_model::ir::Point { x: x + side, y },
            eda_model::ir::Point { x: x + side, y: y + side },
            eda_model::ir::Point { x, y: y + side },
        ]
    }

    /// A made-up fill of kicad-cli's for every zone: a square ring on each layer of each (the writer puts what it is handed, whatever the ring).
    fn kicad_fills_of(design: &Design) -> crate::zone_fills::KicadFills {
        use crate::zone_fills::{zone_signature, KicadFills, KicadRing, ZoneEntry};
        let ring = KicadRing { island: false, pts: vec![[2_000_000, 2_000_000], [4_000_000, 2_000_000], [4_000_000, 4_000_000], [2_000_000, 4_000_000]] };
        let zones = design.routing.as_ref().unwrap().zones.iter().filter(|z| !z.is_rule_area).map(|z| (z.id.clone(), ZoneEntry { sig: zone_signature(z), layers: z.layer_set().into_iter().map(|l| (l.to_string(), vec![ring.clone()])).collect(), unfilled: false, shape: None })).collect();
        KicadFills::new("kicad-cli".into(), "r".into(), 1, zones)
    }

    /// The text of the first `(zone ...)` block.
    fn first_zone(out: &str) -> &str {
        let at = out.find("\t(zone").expect("a zone block");
        let rest = &out[at..];
        &rest[..rest.find("\n\t(").map_or(rest.len(), |i| i + 1)]
    }

    /// A non-plated hole on no copper layer (`issue5990`'s mounting holes are `*.Mask` only) and a net tie's pad groups are the board's, not the zone filler's: they reach the
    /// derived board, so the fill kicad-cli makes of it keeps clear of the hole and exempts the tie, and the drill file has the hole.
    #[test]
    fn a_hole_on_no_copper_layer_and_a_net_ties_pad_groups_are_written_and_read_back() {
        let (mut design, model) = fixture();
        let hole = eda_model::Pad {
            number: String::new(),
            at: (Nm::from_mm(1.5), Nm::from_mm(-2.0)),
            size: (Nm::from_mm(3.4), Nm::from_mm(3.4)),
            shape: eda_model::PadShape::Circle,
            kind: PadKind::NonPlatedHole,
            drill: Some(Nm::from_mm(3.2)),
            drill_slot: None,
            rot: 0,
            roundrect_ratio: None,
            opposite_side: false,
            ext: Default::default(),
        };
        let extra = eda_model::ir::FootprintExtra {
            id: "U1".into(),
            holes: vec![eda_model::ir::FootprintHole { pad: hole.clone(), clearance: Some(Nm::from_mm(1.2)), layers: None, tokens: vec![] }],
            net_tie_pad_groups: vec!["1, 2".into(), "3".into()],
            ..Default::default()
        };
        design.drawings = Some(eda_model::ir::DrawingsSection { footprint_extras: vec![extra], ..Default::default() });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let u1 = &out[out.find("(footprint \"SOT-23\"").expect("U1")..];
        let u1 = &u1[..u1.find("\n\t(footprint").map_or(u1.len(), |i| i + 1)];
        assert!(u1.contains("(net_tie_pad_groups \"1, 2\" \"3\")"), "{u1}");
        let pad = u1.lines().find(|l| l.contains("np_thru_hole")).expect("the hole as a pad");
        for want in ["(at 1.5 -2 ", "(drill 3.2)", "(layers \"*.Mask\")", "(clearance 1.2)"] {
            assert!(pad.contains(want), "{want} in {pad}");
        }

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let e = back.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == "U1")).expect("U1's extras");
        assert_eq!(e.net_tie_pad_groups, vec!["1, 2".to_string(), "3".to_string()]);
        assert_eq!(e.holes.len(), 1, "the hole comes back as the footprint's own, not as a pad with copper");
        assert_eq!((e.holes[0].pad.at, e.holes[0].pad.drill, e.holes[0].clearance), (hole.at, Some(Nm::from_mm(3.2)), Some(Nm::from_mm(1.2))));
        // A footprint with neither writes neither.
        let plain = export_kicad_pcb(&fixture().0, &model, &meta()).unwrap();
        assert!(!plain.contains("np_thru_hole") && !plain.contains("net_tie_pad_groups"));
    }

    /// There is no filler here: a zone is written with the rings kicad-cli made of it (Fill All Zones keeps them beside the design) for as long as the zone is the one it
    /// filled -- to the nanometre, with `(island yes)` where kicad-cli said so -- and unfilled otherwise, as KiCad writes a zone it has emptied.
    #[test]
    fn a_zone_is_written_with_kicad_clis_fill_while_it_is_the_zone_that_was_filled_and_unfilled_after() {
        use crate::zone_fills::{zone_signature, KicadFills, KicadRing, ZoneEntry};
        let (mut design, model) = fixture();
        let zone = Zone { id: "zg".into(), net: "GND".into(), layer: "F.Cu".into(), outline: square(Nm::from_mm(1.0), Nm::from_mm(1.0), Nm::from_mm(10.0)), ..Default::default() };
        design.routing.as_mut().unwrap().zones = vec![zone.clone()];
        let ring = |island: bool| KicadRing { island, pts: vec![[2_000_000, 2_000_000], [8_123_456, 2_000_000], [8_123_456, 7_654_321], [2_000_000, 7_654_321]] };
        let entry = ZoneEntry { sig: zone_signature(&zone), layers: std::collections::BTreeMap::from([("F.Cu".to_string(), vec![ring(false), ring(true)])]), unfilled: false, shape: None };
        let kicad = KicadFills::new("kicad-cli 10.99.0".into(), "r1".into(), 2_500, std::collections::BTreeMap::from([("zg".to_string(), entry)]));

        let out = export_kicad_pcb_with(&design, &model, &meta(), Some(&kicad)).unwrap();
        let block = first_zone(&out);
        assert_eq!(block.matches("(filled_polygon").count(), 2, "exactly kicad-cli's rings: {block}");
        assert!(block.contains("(xy 8.123456 7.654321)"), "to the nanometre: {block}");
        assert_eq!(block.matches("(island yes)").count(), 1, "and the island is marked");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        assert_eq!(back.routing.as_ref().unwrap().zones.len(), 1, "the board with such a fill still loads with its zone");
        assert_eq!(crate::zone_fills::scan_zone_fills(&out).len(), 2, "and the fills read back from the file");

        // No fills handed over: every zone is written unfilled.
        let plain = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(first_zone(&plain).matches("(filled_polygon").count(), 0);

        // Edits elsewhere on the board leave the fill where it is (KiCad with auto-refill off); an edit to the zone itself empties it.
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: Nm::from_mm(0.2), pts: vec![Point { x: Nm::from_mm(2.0), y: Nm::from_mm(9.0) }, Point { x: Nm::from_mm(9.0), y: Nm::from_mm(9.0) }], arc_mid_offset: None });
        let after_a_track = export_kicad_pcb_with(&design, &model, &meta(), Some(&kicad)).unwrap();
        assert_eq!(first_zone(&after_a_track).matches("(filled_polygon").count(), 2, "a track added since is no reason to take the copper away");
        design.routing.as_mut().unwrap().zones[0].min_thickness += Nm::from_mm(0.01);
        let edited = export_kicad_pcb_with(&design, &model, &meta(), Some(&kicad)).unwrap();
        assert_eq!(first_zone(&edited).matches("(filled_polygon").count(), 0, "a zone edited since its fill is written without it");
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
            outline: square(Nm::from_mm(1.0), Nm::from_mm(1.0), Nm::from_mm(10.0)),
            clearance: Nm::from_mm(0.3),
            min_thickness: Nm::from_mm(0.15),
            thermal_gap: Nm::from_mm(0.42),
            thermal_spoke_width: Nm::from_mm(0.35),
            pad_connection: eda_model::ir::PadConnection::Full,
            priority: 3,
            island_removal_mode: eda_model::ir::IslandRemovalMode::Area,
            min_island_area: 2_500_000_000_000,
            fill_mode: eda_model::ir::FillMode::HatchPattern,
            hatch_thickness: Nm::from_mm(0.4),
            hatch_gap: Nm::from_mm(0.9),
            hatch_orientation_mdeg: 45_000,
            hatch_smoothing_level: 2,
            hatch_smoothing_value: 0.25,
            hatch_hole_min_area: 0.3,
            hatch_border_algorithm: 0,
            name: "GND plane".into(),
            border_style: eda_model::ir::ZoneBorderStyle::Full,
            smoothing: eda_model::ir::ZoneSmoothing::Fillet,
            corner_radius: Nm::from_mm(0.6),
            ..Default::default()
        };
        let mut thermal = Zone { id: "zt".into(), net: "VIN".into(), layer: "F.Cu".into(), outline: square(Nm::from_mm(12.0), Nm::from_mm(1.0), Nm::from_mm(6.0)), ..Default::default() };
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
        // The zone comes back with the uuid it was written with (what KiCad breaks a tie of equal priorities by), which is not part of what was set.
        assert!(!got.uuid.is_empty());
        pour.uuid = got.uuid.clone();
        assert_eq!(*got, pour);
        assert_eq!(zones.iter().find(|z| z.net == "VIN").unwrap().pad_connection, eda_model::ir::PadConnection::ThtThermal);
        assert!(back.drawings.as_ref().unwrap().locked_ids.contains(&got.id), "the zone comes back locked");
    }

    /// `ZONE::GetLayerSet()` (`format( const ZONE* )` / `parseZONE`): a zone on the front and back copper is one `(zone ..)` with `(layers "F.Cu" "B.Cu")` and a
    /// `(filled_polygon (layer ..))` per layer; it reads back as one zone with its layer set, not as two zones.
    #[test]
    fn a_zone_on_two_copper_layers_is_one_zone_with_a_fill_on_each_layer_and_reads_back_as_one() {
        let (mut design, model) = fixture();
        let mut pour = Zone { id: "zl".into(), net: "GND".into(), outline: square(Nm::from_mm(1.0), Nm::from_mm(1.0), Nm::from_mm(10.0)), ..Default::default() };
        pour.set_layer_set(&["B.Cu", "F.Cu"]);
        assert_eq!((pour.layer.as_str(), pour.layers.clone()), ("F.Cu", vec!["F.Cu".to_string(), "B.Cu".to_string()]), "the set is kept in UI order, the front first");
        design.routing.as_mut().unwrap().zones = vec![pour.clone()];
        let out = export_kicad_pcb_with(&design, &model, &meta(), Some(&kicad_fills_of(&design))).unwrap();
        assert_eq!(out.matches("\t(zone").count(), 1, "one zone, not one per layer:\n{out}");
        let zone = first_zone(&out);
        assert!(zone.contains("(layers \"F.Cu\" \"B.Cu\")") && !zone.contains("(layer \"F.Cu\") (uuid"), "{zone}");
        assert!(zone.contains("(net 1) (net_name \"GND\")"), "{zone}");
        for layer in ["F.Cu", "B.Cu"] {
            assert!(zone.contains(&format!("(filled_polygon\n\t\t\t(layer \"{layer}\")")), "a fill on {layer}:\n{zone}");
        }
        // The front's fill comes first, `GetLayerSet().Seq()` being in `PCB_LAYER_ID` order (F.Cu 0, B.Cu 2).
        assert!(zone.find("(layer \"F.Cu\")\n").unwrap() < zone.find("(layer \"B.Cu\")\n").unwrap());

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let zones = &back.routing.as_ref().unwrap().zones;
        assert_eq!(zones.len(), 1, "one zone comes back");
        assert_eq!((zones[0].layer.as_str(), zones[0].layer_set()), ("F.Cu", vec!["F.Cu", "B.Cu"]));
        assert_eq!(zones[0].net, "GND");
    }

    /// A zone with no copper layer (`ZONE::IsOnCopperLayer` false) has no net and, on a front and back pair of one technical layer, is written with the wildcard
    /// `formatLayers` folds the pair into; `*.Mask`, `F&B.Cu` and `*.Cu` are all read back to the layers they stand for.
    #[test]
    fn a_non_copper_zone_has_no_net_is_written_with_its_wildcard_and_reads_back_with_its_layers() {
        let (mut design, model) = fixture();
        let silk = Zone { id: "zs".into(), net: "GND".into(), layer: "F.SilkS".into(), outline: square(Nm::from_mm(1.0), Nm::from_mm(1.0), Nm::from_mm(6.0)), ..Default::default() };
        let mut mask = Zone { id: "zm".into(), outline: square(Nm::from_mm(8.0), Nm::from_mm(1.0), Nm::from_mm(6.0)), ..Default::default() };
        mask.set_layer_set(&["B.Mask", "F.Mask"]);
        design.routing.as_mut().unwrap().zones = vec![silk, mask];
        let out = export_kicad_pcb_with(&design, &model, &meta(), Some(&kicad_fills_of(&design))).unwrap();
        let silk_block = out.split("\t(zone").skip(1).find(|z| z.contains("(layer \"F.SilkS\")")).expect("the silkscreen zone");
        assert!(!silk_block.contains("(net ") && !silk_block.contains("(net_name"), "a non-copper zone has no net, whatever its net says: {silk_block}");
        assert!(silk_block.contains("(filled_polygon") && silk_block.contains("(layer \"F.SilkS\")"), "and it is filled on its layer: {silk_block}");
        let mask_block = out.split("\t(zone").skip(1).find(|z| z.contains("*.Mask")).expect("the mask zone");
        assert!(mask_block.contains("(layers \"*.Mask\")"), "{mask_block}");
        for layer in ["F.Mask", "B.Mask"] {
            assert!(mask_block.contains(&format!("(layer \"{layer}\")")), "a fill on {layer}:\n{mask_block}");
        }

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let zones = &back.routing.as_ref().unwrap().zones;
        assert_eq!(zones.len(), 2, "both come back: a zone on a technical layer is no longer left out");
        let silk = zones.iter().find(|z| z.layer == "F.SilkS").unwrap();
        assert!(!silk.is_on_copper_layer() && silk.net.is_empty());
        let mask = zones.iter().find(|z| z.layer == "F.Mask").unwrap();
        assert_eq!(mask.layer_set(), vec!["F.Mask", "B.Mask"]);

        // The other spellings of a layer set: `F&B.Cu` and `*.Cu` on this two-layer board, and a copper layer the board does not have is left out.
        let wild = out.replace("(layers \"*.Mask\")", "(layers \"F&B.Cu\" \"In1.Cu\")");
        let (back, _, _) = crate::import_kicad_pcb(&wild).unwrap();
        let zone = back.routing.as_ref().unwrap().zones.iter().find(|z| z.id == "zm" || z.outline[0].x == Nm::from_mm(8.0)).unwrap().clone();
        assert_eq!(zone.layer_set(), vec!["F.Cu", "B.Cu"], "In1.Cu is not a layer of this board");
        let all = out.replace("(layers \"*.Mask\")", "(layers \"*.Cu\")");
        let (back, _, _) = crate::import_kicad_pcb(&all).unwrap();
        let zone = back.routing.as_ref().unwrap().zones.iter().find(|z| z.outline[0].x == Nm::from_mm(8.0)).unwrap().clone();
        assert_eq!(zone.layer_set(), vec!["F.Cu", "B.Cu"]);
    }

    /// On a four-layer board a zone on all of them is written layer by layer in `PCB_LAYER_ID` order (F.Cu 0, B.Cu 2, In1.Cu 4, In2.Cu 6) and read back in the order
    /// the Zone Manager lists them (the copper stack from the front); a fill is written for each layer that has one, whatever order the set is in.
    #[test]
    fn a_zone_on_a_four_layer_stack_keeps_its_layers_in_both_orders() {
        let (mut design, mut model) = fixture();
        model.board.layers = eda_model::rules::copper_layer_names(4);
        let mut pour = Zone { id: "z4".into(), net: "GND".into(), outline: square(Nm::from_mm(1.0), Nm::from_mm(1.0), Nm::from_mm(10.0)), ..Default::default() };
        pour.set_layer_set(&["In2.Cu", "B.Cu", "F.Cu", "In1.Cu"]);
        assert_eq!(pour.layer_set(), vec!["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
        design.routing.as_mut().unwrap().zones = vec![pour];
        let out = export_kicad_pcb_with(&design, &model, &meta(), Some(&kicad_fills_of(&design))).unwrap();
        assert!(out.contains("(layers \"F.Cu\" \"B.Cu\" \"In1.Cu\" \"In2.Cu\")"), "{}", first_zone(&out));
        let order: Vec<&str> = first_zone(&out).lines().filter(|l| l.contains("(layer \"") && !l.contains("(zone")).map(|l| l.trim()).collect();
        assert_eq!(order, vec!["(layer \"F.Cu\")", "(layer \"B.Cu\")", "(layer \"In1.Cu\")", "(layer \"In2.Cu\")"], "one fill per layer, in file order");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let zones = &back.routing.as_ref().unwrap().zones;
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].layer_set(), vec!["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
    }

    /// A rule area spans the layers it is drawn on, copper or not (`issue22826`'s keepouts are on copper, silkscreen, mask and paste at once): the set is kept as it is
    /// written, and the technical layers on a board that has turned them off are left out of the file, as `format( const ZONE* )` masks the set with the enabled layers.
    #[test]
    fn a_rule_area_keeps_its_technical_layers_and_the_file_leaves_out_the_ones_the_board_has_off() {
        let (mut design, mut model) = fixture();
        let mut keepout = Zone { id: "ka".into(), outline: square(Nm::from_mm(1.0), Nm::from_mm(1.0), Nm::from_mm(6.0)), is_rule_area: true, keepout_copper_pour: true, ..Default::default() };
        keepout.set_layer_set(&["F.Cu", "B.Cu", "F.SilkS", "F.Mask"]);
        design.routing.as_mut().unwrap().zones = vec![keepout.clone()];
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(layers \"F.Cu\" \"F.Mask\" \"B.Cu\" \"F.SilkS\")"), "KiCad's PCB_LAYER_ID order:\n{}", first_zone(&out));
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        assert_eq!(back.routing.as_ref().unwrap().zones[0].layer_set(), vec!["F.Cu", "B.Cu", "F.SilkS", "F.Mask"]);

        model.board.layer_setup.enabled.retain(|l| l != "F.SilkS");
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(layers \"F.Cu\" \"F.Mask\" \"B.Cu\")"), "F.SilkS is not a layer of this board:\n{}", first_zone(&out));
    }

    #[test]
    fn an_arc_track_is_written_as_one_arc_and_read_back_as_one() {
        use eda_model::ir::Point;
        let (mut design, model) = fixture();
        let arc = Track::new_arc("VIN".into(), "F.Cu".into(), Nm::from_mm(0.25), Point { x: Nm::from_mm(2.0), y: Nm::from_mm(2.0) }, Point { x: Nm::from_mm(4.0), y: Nm::from_mm(1.0) }, Point { x: Nm::from_mm(6.0), y: Nm::from_mm(2.0) });
        assert!(arc.pts.len() > 3, "the polyline is the tessellation");
        design.routing.as_mut().unwrap().tracks.push(arc);
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(out.matches("\t(arc ").count(), 1, "{out}");
        assert!(out.contains("(arc (start 2 2) (mid 4 1) (end 6 2) (width 0.25) (layer \"F.Cu\") (net 2)"), "{out}");
        assert_eq!(out.matches("\t(segment ").count(), 1, "only the plain track is a segment, not 32 chords of the arc: {out}");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let tracks = &back.routing.unwrap().tracks;
        let arcs: Vec<_> = tracks.iter().filter_map(|t| t.arc()).collect();
        assert_eq!(arcs, vec![(Point { x: Nm::from_mm(2.0), y: Nm::from_mm(2.0) }, Point { x: Nm::from_mm(4.0), y: Nm::from_mm(1.0) }, Point { x: Nm::from_mm(6.0), y: Nm::from_mm(2.0) })]);
    }

    #[test]
    fn a_track_that_was_reshaped_after_it_was_an_arc_is_written_as_segments() {
        use eda_model::ir::Point;
        let (mut design, model) = fixture();
        let mut arc = Track::new_arc("VIN".into(), "F.Cu".into(), Nm::from_mm(0.25), Point { x: Nm::from_mm(2.0), y: Nm::from_mm(2.0) }, Point { x: Nm::from_mm(4.0), y: Nm::from_mm(1.0) }, Point { x: Nm::from_mm(6.0), y: Nm::from_mm(2.0) });
        arc.pts.truncate(5); // an edit that cut the curve short: no longer the tessellation of its arc
        design.routing.as_mut().unwrap().tracks.push(arc);
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(out.matches("\t(arc ").count(), 0, "a stale arc is never written: {out}");
        assert_eq!(out.matches("\t(segment ").count(), 1 + 4);
    }

    #[test]
    fn a_tuning_pattern_is_written_as_a_generated_item_naming_its_tracks_and_read_back_with_them() {
        use eda_model::ir::Point;
        use eda_model::tuning::{PatternSide, TargetRange, TuningMode, TuningPattern};
        let (mut design, model) = fixture();
        // The meander: a polyline of three points (two segments) beside the board's plain track.
        design.routing.as_mut().unwrap().tracks.push(Track { id: String::new(), net: "VIN".into(), pins: vec![], layer: "F.Cu".into(), width: Nm::from_mm(0.2), pts: vec![Point { x: Nm::from_mm(10.0), y: Nm::from_mm(5.0) }, Point { x: Nm::from_mm(10.0), y: Nm::from_mm(6.0) }, Point { x: Nm::from_mm(11.0), y: Nm::from_mm(6.0) }], arc_mid_offset: None });
        design.assign_missing_ids();
        let ids: Vec<String> = design.routing.as_ref().unwrap().tracks.iter().map(|t| t.id.clone()).collect();
        let pattern = TuningPattern {
            id: String::new(),
            mode: TuningMode::Single,
            layer: "F.Cu".into(),
            origin: Point { x: Nm::from_mm(10.0), y: Nm::from_mm(5.0) },
            end: Point { x: Nm::from_mm(11.0), y: Nm::from_mm(6.0) },
            initial_side: PatternSide::Left,
            min_amplitude: Nm::from_mm(0.2),
            max_amplitude: Nm::from_mm(1.0),
            spacing: Nm::from_mm(0.6),
            corner_radius_percent: 80,
            rounded: true,
            single_sided: false,
            override_custom_rules: false,
            target_length: TargetRange { min: 99_900_000, opt: 100_000_000, max: 100_100_000 },
            target_skew: TargetRange { min: -100_000, opt: 0, max: 100_000 },
            is_time_domain: false,
            target_delay: TargetRange::delay_unconstrained(),
            target_skew_delay: TargetRange { min: -100_000, opt: 0, max: 100_000 },
            base_line: vec![Point { x: Nm::from_mm(10.0), y: Nm::from_mm(5.0) }, Point { x: Nm::from_mm(11.0), y: Nm::from_mm(6.0) }],
            base_line_coupled: vec![],
            last_track_width: Nm::from_mm(0.2),
            last_diff_pair_gap: Nm::ZERO,
            last_tuning_length: Nm::from_mm(1414.213),
            last_tuning_delay: 0,
            last_netname: "VIN".into(),
            last_status: "too_short".into(),
            member_ids: vec![ids[1].clone()],
        };
        design.drawings = Some(eda_model::ir::DrawingsSection { tuning_patterns: vec![pattern.clone()], ..Default::default() });
        design.assign_missing_ids();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(out.matches("\t(generated\n").count(), 1, "{out}");
        // Both segments of the polyline are members, by the uuids they were written with, and the plain track is not.
        let seg_uuids: Vec<&str> = out.lines().filter(|l| l.starts_with("\t(segment ")).filter_map(|l| l.split("(uuid \"").nth(1)).filter_map(|r| r.split('\"').next()).collect();
        assert_eq!(seg_uuids.len(), 3, "{out}");
        let members_line = out.lines().find(|l| l.trim_start().starts_with("(members") && l.contains("\"")).and_then(|l| (!l.contains("(group")).then_some(l)).unwrap_or_else(|| panic!("a members line: {out}"));
        assert_eq!(members_line.matches('"').count(), 4, "two quoted uuids: {members_line}");
        let in_members = seg_uuids.iter().filter(|u| members_line.contains(**u)).count();
        assert_eq!(in_members, 2, "the polyline's two segments, not the plain track: {members_line}");

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let drawings = back.drawings.as_ref().expect("the pattern comes back");
        assert_eq!(drawings.tuning_patterns.len(), 1);
        let got = &drawings.tuning_patterns[0];
        let tracks = &back.routing.as_ref().unwrap().tracks;
        assert_eq!(got.member_ids.len(), 2);
        for m in &got.member_ids {
            let t = tracks.iter().find(|t| &t.id == m).expect("a member is a track of the board");
            assert_eq!(t.pts.len(), 2);
            assert!(t.pts.iter().all(|p| p.x >= Nm::from_mm(10.0) && p.y >= Nm::from_mm(5.0)), "the polyline's segments, not the plain track: {t:?}");
        }
        let mut want = pattern;
        want.id = got.id.clone();
        want.member_ids = got.member_ids.clone();
        assert_eq!(*got, want);
        assert!(got.id.starts_with("tun_"), "{}", got.id);

        // r2-board-io F14: a group holding the pattern names it by its uuid, and the reader puts it back in the group (`resolveGroups` takes generators).
        let mut grouped = design.clone();
        let pattern_id = grouped.drawings.as_ref().unwrap().tuning_patterns[0].id.clone();
        grouped.drawings.as_mut().unwrap().groups = vec![eda_model::ir::Group { id: "grp_t".into(), name: "tuned".into(), member_ids: vec![ids[0].clone(), pattern_id], design_block: String::new() }];
        let (back, _, _) = crate::import_kicad_pcb(&export_kicad_pcb(&grouped, &model, &meta()).unwrap()).unwrap();
        let dr = back.drawings.as_ref().unwrap();
        assert_eq!(dr.groups.len(), 1);
        assert!(dr.groups[0].member_ids.contains(&dr.tuning_patterns[0].id), "the pattern is still a member: {:?}", dr.groups[0]);
        assert_eq!(dr.groups[0].member_ids.len(), 2, "{:?}", dr.groups[0]);

        // A pattern whose tracks are gone is a ghost and is not written.
        design.routing.as_mut().unwrap().tracks.retain(|t| t.id != ids[1]);
        design.drawings.as_mut().unwrap().tuning_patterns[0].member_ids = vec![ids[1].clone()];
        assert_eq!(export_kicad_pcb(&design, &model, &meta()).unwrap().matches("(generated").count(), 0);
    }

    /// KiCad's own boards with tuning patterns (`qa/data/pcbnew`): every pattern comes in with the tracks its `(members ..)` names. Runs when the KiCad
    /// source is at `KICAD_SRC_DIR`.
    #[test]
    fn the_tuning_patterns_of_kicads_own_boards_are_read_with_their_tracks() {
        let Ok(dir) = std::env::var("KICAD_SRC_DIR") else { return };
        for (file, patterns) in [("tuning_generators_load_save.kicad_pcb", 1usize), ("issue22536.kicad_pcb", 4)] {
            let Ok(text) = std::fs::read_to_string(std::path::Path::new(&dir).join("qa/data/pcbnew").join(file)) else { continue };
            let (design, model, _) = crate::import_kicad_pcb(&text).unwrap();
            let got = &design.drawings.as_ref().expect("patterns").tuning_patterns;
            assert_eq!(got.len(), patterns, "{file}");
            let n_tracks = design.routing.as_ref().unwrap().tracks.len();
            for p in got {
                assert!(p.member_ids.len() > 3 && p.member_ids.len() <= n_tracks, "{file}: {} members", p.member_ids.len());
                assert!(p.base_line.len() >= 2 && p.target_length.opt > 0, "{file}: {p:?}");
            }
            // Written again, the board has the same patterns, with the same members.
            let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
            assert_eq!(out.matches("\t(generated\n").count(), patterns, "{file}");
            let (again, _, _) = crate::import_kicad_pcb(&out).unwrap();
            let key = |p: &eda_model::tuning::TuningPattern| (p.layer.clone(), p.origin.x, p.origin.y);
            let mut again = again.drawings.as_ref().unwrap().tuning_patterns.clone();
            again.sort_by_key(key);
            let mut got: Vec<_> = got.clone();
            got.sort_by_key(key);
            assert_eq!(again.len(), patterns, "{file}");
            for (a, b) in got.iter().zip(&again) {
                let mut b = b.clone();
                assert_eq!(a.member_ids.len(), b.member_ids.len(), "{file}");
                b.id = a.id.clone();
                b.member_ids = a.member_ids.clone();
                assert_eq!(*a, b, "{file}");
            }
        }
    }

    #[test]
    fn locked_items_are_written_locked_and_come_back_locked() {
        use eda_model::ir::Point;
        let (mut design, model) = fixture();
        design.routing.as_mut().unwrap().vias.push(Via { id: String::new(), net: "VIN".into(), at: Point { x: Nm::from_mm(8.0), y: Nm::from_mm(8.0) }, drill: Nm::from_mm(0.3), diameter: Nm::from_mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        design.drawings = Some(eda_model::ir::DrawingsSection {
            shapes: vec![Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: Nm::from_mm(0.15), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: Nm::from_mm(1.0), y: Nm::ZERO } }],
            texts: vec![Text { id: String::new(), content: "REV A".into(), at: Point { x: Nm::from_mm(1.0), y: Nm::from_mm(2.0) }, angle: 0, layer: "F.SilkS".into(), size_nm: Nm::from_mm(1.0), stroke_width: Nm::from_mm(0.15), justify: TextJustify::Center, mirror: false }],
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
        assert!(out.contains("(footprint \"SOT-23\"\n\t\t(locked yes)\n\t\t(layer"), "{out}");

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

    /// `PCB_DIM_LEADER::updateGeometry` draws its line *to* `GetTextPos()` and never sets it, and so does `PCB_DIM_RADIAL::updateGeometry`
    /// (`updateText` only turns the text toward it, rounded to a whole degree); `text_position_mode 0` makes KiCad recompute aligned and
    /// orthogonal texts only. A leader's `(text_frame 1|2)` is its `DIM_TEXT_BORDER`. `issue6945`'s four leaders and `api_kitchen_sink`'s
    /// leader and radial came back with their text moved 5.3 mm away and the frame lost.
    #[test]
    fn a_leader_and_a_radial_dimension_keep_their_text_position() {
        let board = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (44 "Edge.Cuts" user) (49 "Dwgs.User" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (dimension (type leader) (layer "Dwgs.User") (uuid "3f749de1-cddc-4992-8135-8d1854ebbd09")
    (pts (xy 167.95 100.95) (xy 170.31 98.29))
    (format (prefix "") (suffix "") (units 0) (units_format 0) (precision 4) (override_value "PTH"))
    (style (thickness 0.2) (arrow_length 1.27) (text_position_mode 0) (text_frame 1) (extension_offset 0.5))
    (gr_text "PTH" (at 176.52 92.91 0) (layer "Dwgs.User") (effects (font (size 1.5 1.5) (thickness 0.3)))))
  (dimension (type radial) (layer "F.SilkS") (uuid "32a22a4a-ae09-4c8c-9323-a332622fcb7d")
    (pts (xy 115 93.5) (xy 113.25 94.75))
    (leader_length 3.81)
    (format (prefix "R ") (suffix "") (units 3) (units_format 1) (precision 4))
    (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_offset 0.5) (keep_text_aligned yes))
    (gr_text "R 2.1506 mm" (at 97.449675 96.964518 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.1)))))
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(text_frame 1)"), "the leader's frame is kept: {out}");
        assert!(out.contains("(gr_text \"PTH\" (at 176.52 92.91 0)"), "the leader's text is where the file has it: {out}");
        assert!(out.contains("(gr_text \"R 2.1506 mm\" (at 97.449675 96.964518 0)"), "the radial's text is where the file has it: {out}");
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
            text_size_nm: Nm::from_mm(1.2),
            stroke_width: Nm::from_mm(0.15),
            arrow_length: Nm::from_mm(1.27),
            extension_offset: Nm::from_mm(0.5),
            extension_height: Nm::from_mm(0.58),
            arrow_direction: ArrowDirection::Inward,
            text_pos: None,
            text_frame: Default::default(),
            text_thickness_nm: Some(Nm::from_mm(0.18)),
        };
        let mut dims = vec![
            dim(DimensionKind::Aligned { height: Nm::from_mm(2.0) }, Point { x: Nm::from_mm(2.0), y: Nm::from_mm(12.0) }, Point { x: Nm::from_mm(12.0), y: Nm::from_mm(12.0) }),
            dim(DimensionKind::Orthogonal { height: Nm::from_mm(1.5), horizontal: false }, Point { x: Nm::from_mm(2.0), y: Nm::from_mm(14.0) }, Point { x: Nm::from_mm(6.0), y: Nm::from_mm(18.0) }),
            dim(DimensionKind::Radial { leader_length: Nm::from_mm(3.0) }, Point { x: Nm::from_mm(15.0), y: Nm::from_mm(15.0) }, Point { x: Nm::from_mm(17.0), y: Nm::from_mm(15.0) }),
            dim(DimensionKind::Leader, Point { x: Nm::from_mm(10.0), y: Nm::from_mm(10.0) }, Point { x: Nm::from_mm(12.0), y: Nm::from_mm(8.0) }),
            dim(DimensionKind::Center, Point { x: Nm::from_mm(5.0), y: Nm::from_mm(5.0) }, Point { x: Nm::from_mm(6.0), y: Nm::from_mm(5.0) }),
        ];
        dims[0].override_text = Some("ten".into());
        dims[1].units = DimensionUnits::Mil;
        dims[1].keep_text_aligned = false;
        dims[1].text_position = DimensionTextPosition::Inline;
        // A unit-dependent `DIM_PRECISION` level (V_VV) is KiCad's own number in the file, both ways.
        dims[2].precision = 6;
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
        assert!(out.contains("(precision 6)"), "{out}");
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
                g.text_size_nm = want.text_size_nm;
                g.text_thickness_nm = want.text_thickness_nm;
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
        design.routing.as_mut().unwrap().vias.push(Via { id: String::new(), net: "VIN".into(), at: Point { x: Nm::from_mm(8.0), y: Nm::from_mm(8.0) }, drill: Nm::from_mm(0.3), diameter: Nm::from_mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        design.routing.as_mut().unwrap().tracks.push(Track { id: String::new(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: Nm::from_mm(0.2), pts: vec![Point { x: Nm::from_mm(1.0), y: Nm::from_mm(6.0) }, Point { x: Nm::from_mm(5.0), y: Nm::from_mm(6.0) }, Point { x: Nm::from_mm(5.0), y: Nm::from_mm(9.0) }], arc_mid_offset: None });
        design.assign_missing_ids();
        let rt = design.routing.as_ref().unwrap();
        let (via_id, long_track) = (rt.vias[0].id.clone(), rt.tracks.iter().find(|t| t.net == "GND").unwrap().id.clone());
        let mut dr = eda_model::ir::DrawingsSection::default();
        dr.groups = vec![Group { id: String::new(), name: "decoupling".into(), member_ids: vec!["U1".into(), "C1".into(), via_id.clone(), long_track.clone(), "no such item".into()], design_block: String::new() }];
        dr.assign_missing_ids();
        dr.locked_ids = vec![dr.groups[0].id.clone()];
        design.drawings = Some(dr);
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let group = out.split("\t(group ").nth(1).expect("a group block");
        assert!(group.starts_with("\"decoupling\" (uuid \""), "{group}");
        assert!(group.contains("(locked yes)"), "{group}");
        // Round 4 B-5: `BOARD_ITEM::IsLocked` counts the group above, and every `format()` writes it: the members are written locked too.
        let segments: Vec<&str> = out.split("\t(segment ").skip(1).map(|b| b.split('\n').next().unwrap()).filter(|b| b.starts_with("(start 1 6)") || b.starts_with("(start 5 6)")).collect();
        assert!(segments.len() == 2 && segments.iter().all(|b| b.contains("(locked yes)")), "the track's segments: {segments:?}");
        let other = out.split("\t(segment ").skip(1).map(|b| b.split('\n').next().unwrap()).find(|b| b.starts_with("(start 5 5)")).unwrap();
        assert!(!other.contains("(locked yes)"), "a track outside the group is not: {other}");
        let fp_u1 = out.split("\t(footprint ").skip(1).find(|b| b.contains("\"Reference\" \"U1\"")).expect("U1");
        assert!(fp_u1.lines().take(3).any(|l| l.contains("(locked yes)")), "U1 is written locked: {}", fp_u1.lines().take(3).collect::<Vec<_>>().join("|"));
        // U1, C1, the via, both segments of the two-segment track; the stale id names nothing.
        assert_eq!(group.split("(members").nth(1).unwrap().matches('"').count() / 2, 5, "{group}");
        assert!(out.rfind("\t(group ").unwrap() > out.rfind("\t(segment ").unwrap(), "groups come last, like KiCad's own writer");

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let groups = back.drawings.as_ref().unwrap().groups.clone();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "decoupling");
        assert!(back.drawings.as_ref().unwrap().locked_ids.contains(&groups[0].id), "r2-board-io F7: the group's lock is read back");
        let brt = back.routing.as_ref().unwrap();
        assert_eq!(groups[0].member_ids.len(), 5, "{:?}", groups[0].member_ids);
        assert!(groups[0].member_ids.contains(&"U1".to_string()) && groups[0].member_ids.contains(&"C1".to_string()));
        assert!(groups[0].member_ids.contains(&brt.vias[0].id));
        for t in brt.tracks.iter().filter(|t| t.net == "GND") {
            assert!(groups[0].member_ids.contains(&t.id), "both segments of the track are members");
        }
    }

    #[test]
    fn a_group_inside_a_group_is_written_by_its_uuid_and_read_back_nested() {
        use eda_model::ir::{DrawingsSection, Group, Point};
        let (mut design, model) = fixture();
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: String::new(), net: "VIN".into(), at: Point { x: Nm::from_mm(8.0), y: Nm::from_mm(8.0) }, drill: Nm::from_mm(0.3), diameter: Nm::from_mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.vias.push(Via { id: String::new(), net: "GND".into(), at: Point { x: Nm::from_mm(12.0), y: Nm::from_mm(12.0) }, drill: Nm::from_mm(0.3), diameter: Nm::from_mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        design.assign_missing_ids();
        let rt = design.routing.as_ref().unwrap();
        let (track_id, via_a, via_b) = (rt.tracks[0].id.clone(), rt.vias[0].id.clone(), rt.vias[1].id.clone());
        let group = |id: &str, name: &str, members: &[&str]| Group { id: id.into(), name: name.into(), member_ids: members.iter().map(|m| m.to_string()).collect(), design_block: String::new() };
        design.drawings = Some(DrawingsSection {
            groups: vec![
                group("grp_a_outer", "outer", &["U1", "grp_b_middle"]),
                group("grp_b_middle", "middle", &["C1", "grp_c_inner"]),
                group("grp_c_inner", "inner", &[via_a.as_str(), track_id.as_str()]),
                // nothing of it is on the board: not written, and not named by the group above it either
                group("grp_d_ghost", "ghost", &["no such item", "nor this one"]),
                group("grp_e_holder", "holder", &[via_b.as_str(), "grp_d_ghost"]),
            ],
            ..Default::default()
        });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(out.matches("\t(group ").count(), 4, "outer, middle, inner and the holder; the ghost is not written:\n{out}");
        let block = |name: &str| out.split("\t(group ").skip(1).find(|g| g.starts_with(&format!("\"{name}\""))).unwrap_or_else(|| panic!("no group {name}")).to_string();
        let uuids_in = |block: &str| block.split("(members").nth(1).unwrap().matches('"').count() / 2;
        assert_eq!(uuids_in(&block("outer")), 2, "U1 and the middle group: {}", block("outer"));
        assert_eq!(uuids_in(&block("holder")), 1, "the via alone: {}", block("holder"));

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let dr = back.drawings.as_ref().unwrap();
        let by_name = |n: &str| dr.groups.iter().find(|g| g.name == n).unwrap_or_else(|| panic!("no group {n} in {:?}", dr.groups));
        let (outer, middle, inner) = (by_name("outer"), by_name("middle"), by_name("inner"));
        let brt = back.routing.as_ref().unwrap();
        assert!(outer.member_ids.contains(&"U1".to_string()) && outer.member_ids.contains(&middle.id), "{outer:?}");
        assert!(middle.member_ids.contains(&"C1".to_string()) && middle.member_ids.contains(&inner.id), "{middle:?}");
        assert!(inner.member_ids.contains(&brt.tracks[0].id) && inner.member_ids.iter().any(|m| brt.vias.iter().any(|v| &v.id == m)), "{inner:?}");
        let holder = by_name("holder");
        assert_eq!(holder.member_ids, vec![brt.vias.iter().find(|v| holder.member_ids.contains(&v.id)).expect("the via").id.clone()], "a group of one item is read back (`resolveGroups` keeps every group)");
        assert_eq!(dr.group_leaves(&outer.id).len(), 4, "U1, C1, the track and a via: {:?}", dr.group_leaves(&outer.id));
    }

    /// r2-pcb-edit A2: the orientation the studio shows is the file's angle.
    #[test]
    fn a_back_footprints_kicad_orientation_is_the_angle_it_is_written_with() {
        let (mut design, model) = fixture();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let c1 = design.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "C1").unwrap().clone();
        let u1 = design.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "U1").unwrap().clone();
        // C1 is on the back at rot 90 (clockwise), made here: written half a turn round, -(90 + 180) = 90.
        assert_eq!(kicad_footprint_orientation(&design, &c1), 90_000);
        assert!(out.contains("(at 10 10 90)"), "{out}");
        assert_eq!(kicad_footprint_orientation(&design, &u1), 0);
        // Read in the file's frame: no half turn.
        design.drawings = Some(eda_model::ir::DrawingsSection { footprint_extras: vec![eda_model::ir::FootprintExtra { id: "C1".into(), ..Default::default() }], ..Default::default() });
        assert_eq!(kicad_footprint_orientation(&design, &c1), -90_000);
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
            outline: vec![eda_model::ir::Point { x: Nm::from_mm(4.0), y: Nm::from_mm(4.0) }, eda_model::ir::Point { x: Nm::from_mm(5.0), y: Nm::from_mm(4.3) }, eda_model::ir::Point { x: Nm::from_mm(5.0), y: Nm::from_mm(5.3) }],
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
    fn a_zone_a_footprint_owns_is_written_inside_that_footprint_and_read_back_as_its_own() {
        // A footprint's own rule area does not test the footprint that owns it (`intersectsArea`'s
        // `aArea->GetParent() == item`). Written at board level it reported that footprint as a keepout
        // violation, on every real board that carries such an area (found on four connectors of one).
        let (mut design, model) = fixture();
        let rule_area = |id: &str, parent: Option<&str>, at: Nm| Zone {
            id: id.into(),
            layer: "F.Cu".into(),
            outline: square(at, at, Nm::from_mm(2.0)),
            is_rule_area: true,
            keepout_pads: true,
            parent_footprint: parent.map(String::from),
            ..Default::default()
        };
        design.routing.as_mut().unwrap().zones = vec![rule_area("own", Some("U1"), Nm::from_mm(4.0)), rule_area("free", None, Nm::from_mm(12.0))];
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(out.matches("\t(zone").count(), 2, "both zones are written once");
        // Footprints come first (C1, then U1, which owns the zone at 4,4), then the tracks, then board-level zones.
        let (own_at, free_at, segment_at) = (out.find("(xy 4 4)").unwrap(), out.find("(xy 12 12)").unwrap(), out.find("\t(segment ").unwrap());
        let u1_at = out.find("(property \"Reference\" \"U1\"").unwrap();
        assert!(u1_at < own_at && own_at < segment_at, "the zone U1 owns sits inside U1's block, before the tracks");
        assert!(free_at > segment_at, "a zone nothing owns stays at board level, after the tracks");

        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let zones = &back.routing.as_ref().unwrap().zones;
        assert_eq!(zones.iter().filter(|z| z.parent_footprint.as_deref() == Some("U1")).count(), 1);
        assert_eq!(zones.iter().filter(|z| z.parent_footprint.is_none()).count(), 1);
    }

    #[test]
    fn a_board_with_stated_constraints_writes_them_exactly_and_one_without_keeps_its_lowered_floors() {
        let (_, mut model) = fixture();
        model.board.net_classes.push(eda_model::NetClass { name: "escape".into(), nets: vec!["VIN".into()], track_width: Some(Nm::from_mm(0.15)), clearance: Some(Nm::from_mm(0.15)), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
        // An intent states no minimums: the floors follow the narrowest class, so the escape routing is not a violation of rules nobody wrote.
        let legacy: serde_json::Value = serde_json::from_str(&export_kicad_pro(&model)).unwrap();
        let rules = &legacy["board"]["design_settings"]["rules"];
        assert_eq!((rules["min_track_width"].as_f64(), rules["min_clearance"].as_f64()), (Some(0.15), Some(0.15)));
        assert!(rules["min_copper_edge_clearance"].is_null(), "KiCad's own default applies, as it always did");

        // Stated minimums (Board Setup's Constraints, or the project the board came from) are written as they are.
        model.board.constraints_explicit = true;
        model.board.track_width_min_nm = Nm::from_mm(0.25);
        model.board.min_clearance_nm = Nm::from_mm(0.3);
        model.board.copper_edge_clearance_nm = Some(Nm::from_mm(0.35));
        model.board.annular_width_min_nm = Nm::from_mm(0.13);
        model.board.min_silk_text_height_nm = Nm::from_mm(1.0);
        model.board.solder_mask.to_copper_clearance_nm = Nm::from_mm(0.04);
        let exact: serde_json::Value = serde_json::from_str(&export_kicad_pro(&model)).unwrap();
        let rules = &exact["board"]["design_settings"]["rules"];
        assert_eq!(rules["min_track_width"], 0.25, "the stated floor, not the narrowest class");
        assert_eq!(rules["min_clearance"], 0.3);
        assert_eq!(rules["min_copper_edge_clearance"], 0.35);
        assert_eq!(rules["min_via_annular_width"], 0.13);
        assert_eq!(rules["min_text_height"], 1.0);
        assert_eq!(rules["solder_mask_to_copper_clearance"], 0.04);
        assert_eq!(rules["max_error"], 0.005);
        assert_eq!(rules["min_resolved_spokes"], 2);
        assert_eq!(exact["board"]["design_settings"]["defaults"]["silk_text_size_h"], 1.0, "the Text & Graphics grid rides along");

        // And the importer reads every one back from that project.
        let mut back = ConstraintModel::default();
        crate::merge_project_design_rules(&mut back, &export_kicad_pro(&model));
        assert!(back.board.constraints_explicit);
        assert_eq!((back.board.track_width_min_nm, back.board.min_clearance_nm, back.board.copper_edge_clearance_nm, back.board.annular_width_min_nm), (Nm::from_mm(0.25), Nm::from_mm(0.3), Some(Nm::from_mm(0.35)), Nm::from_mm(0.13)));
        assert_eq!((back.board.max_error_nm, back.board.min_resolved_spokes, back.board.min_silk_text_height_nm), (Nm::new(5000), 2, Nm::from_mm(1.0)));
    }

    #[test]
    fn solder_mask_paste_the_stackup_and_the_board_thickness_are_written_and_read_back() {
        let (design, mut model) = fixture();
        model.board.solder_mask = eda_model::SolderMaskRules { expansion_nm: Nm::from_mm(0.05), min_width_nm: Nm::from_mm(0.12), to_copper_clearance_nm: Nm::ZERO, allow_bridges_in_footprints: true, tent_vias_front: false, tent_vias_back: true, paste_margin_nm: Nm::from_mm(-0.04), paste_margin_ratio: -0.05 };
        let stackup = eda_model::rules::default_stackup(4000, Nm::from_mm(1.6));
        model.board.board_thickness_nm = eda_model::rules::stackup_thickness_nm(&stackup);
        model.stackup = Some(stackup.clone());
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        for want in [
            "(pad_to_mask_clearance 0.05)",
            "(solder_mask_min_width 0.12)",
            "(pad_to_paste_clearance -0.04)",
            "(pad_to_paste_clearance_ratio -0.05)",
            "(allow_soldermask_bridges_in_footprints yes)",
            "(tenting (front no) (back yes))",
            "(stackup",
            "(layer \"In2.Cu\" (type \"copper\") (thickness 0.035))",
            "(layer \"dielectric 1\" (type \"core\") (thickness",
            "(material \"FR4\") (epsilon_r 4.5) (loss_tangent 0.02)",
            "(dielectric_constraints no)",
        ] {
            assert!(out.contains(want), "missing {want}:\n{}", &out[..out.len().min(2500)]);
        }
        assert!(out.contains(&format!("(thickness {})", mm(model.board.board_thickness_nm))), "the board's thickness is the stackup's sum");

        let (_, back, _) = crate::import_kicad_pcb(&out).unwrap();
        assert_eq!(back.board.solder_mask, model.board.solder_mask);
        assert_eq!(back.board.board_thickness_nm, model.board.board_thickness_nm);
        let got = back.stackup.expect("the stackup comes back");
        assert_eq!(got.layers.len(), stackup.layers.len());
        assert_eq!(got.layers.iter().map(|l| (l.name.as_str(), l.kind.as_deref())).collect::<Vec<_>>(), stackup.layers.iter().map(|l| (l.name.as_str(), l.kind.as_deref())).collect::<Vec<_>>());
        let near = |a: Option<f64>, b: Option<f64>| a.zip(b).map_or(a == b, |(x, y)| (x - y).abs() < 1e-4);
        assert!(got.layers.iter().zip(&stackup.layers).all(|(g, w)| near(g.thickness_mm, w.thickness_mm) && g.material == w.material));
    }

    /// `(color "Green")` of a mask, a silkscreen and a dielectric is written before the thickness (`BOARD_STACKUP::FormatBoardStackup`) and read back; a
    /// colour that is not specified is not written, and nothing is written for a layer whose colour is not editable (copper, paste).
    #[test]
    fn the_colours_of_the_stackup_are_written_and_read_back() {
        let (design, mut model) = fixture();
        let mut stackup = eda_model::rules::default_stackup(2000, Nm::from_mm(1.6));
        for (name, color) in [("F.Mask", "Red"), ("F.SilkS", "#FFEE00"), ("dielectric 1", "FR4 natural"), ("F.Cu", "Green"), ("F.Paste", "Silver"), ("B.Mask", "Not specified")] {
            stackup.layers.iter_mut().find(|l| l.name == name).unwrap().color = Some(color.into());
        }
        model.board.board_thickness_nm = eda_model::rules::stackup_thickness_nm(&stackup);
        model.stackup = Some(stackup);
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        for want in ["(layer \"F.Mask\" (type \"Top Solder Mask\") (color \"Red\") (thickness", "(layer \"F.SilkS\" (type \"Top Silk Screen\") (color \"#FFEE00\")", "(layer \"dielectric 1\" (type \"core\") (color \"FR4 natural\") (thickness"] {
            assert!(out.contains(want), "missing {want}:\n{}", &out[..out.len().min(3000)]);
        }
        assert!(!out.contains("(layer \"F.Cu\" (type \"copper\") (color") && !out.contains("(layer \"F.Paste\" (type \"Top Solder Paste\") (color"), "copper and paste have no colour");
        assert!(!out.contains("Not specified"), "a colour that is not specified is not written");
        let (_, back, _) = crate::import_kicad_pcb(&out).unwrap();
        let colour = |name: &str| back.stackup.as_ref().unwrap().layers.iter().find(|l| l.name == name).unwrap().color.clone();
        assert_eq!((colour("F.Mask"), colour("F.SilkS"), colour("dielectric 1")), (Some("Red".into()), Some("#FFEE00".into()), Some("FR4 natural".into())));
        assert_eq!((colour("F.Cu"), colour("B.Mask")), (None, None));
    }

    #[test]
    fn a_stackup_the_intent_listed_by_name_alone_is_not_written() {
        // `StackupLayer`s without a type (the older shape: name, material, thickness) do not describe the board to KiCad.
        let (design, mut model) = fixture();
        model.stackup = Some(eda_model::Stackup { layers: vec![eda_model::StackupLayer { name: "F.Cu".into(), material: Some("copper".into()), thickness_mm: Some(0.035), kind: None, epsilon_r: None, loss_tangent: None, color: None }], copper_finish: None, dielectric_constraints: false, edge_connector: 0, edge_plating: false });
        assert!(!export_kicad_pcb(&design, &model, &meta()).unwrap().contains("(stackup"));
    }

    #[test]
    fn a_net_class_writes_its_microvia_and_pair_sizes_and_they_read_back() {
        let (design, mut model) = fixture();
        model.board.net_classes.push(eda_model::NetClass { name: "usb".into(), nets: vec!["VIN".into()], track_width: Some(Nm::from_mm(0.2)), clearance: Some(Nm::from_mm(0.2)), via_diameter: None, via_drill: None, microvia_diameter: Some(Nm::from_mm(0.25)), microvia_drill: Some(Nm::from_mm(0.11)), diff_pair_width: Some(Nm::from_mm(0.12)), diff_pair_gap: Some(Nm::from_mm(0.15)), diff_pair_via_gap: None, priority: 0 });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(uvia_dia 0.25)") && out.contains("(uvia_drill 0.11)") && out.contains("(diff_pair_width 0.12)") && out.contains("(diff_pair_gap 0.15)"), "{out}");
        assert_eq!(out.matches("(uvia_dia 0.3)").count(), 1, "the Default class keeps KiCad's own microvia");
        let (_, back, _) = crate::import_kicad_pcb(&out).unwrap();
        let usb = back.board.net_classes.iter().find(|c| c.name == "usb").expect("the class comes back");
        assert_eq!((usb.microvia_diameter, usb.microvia_drill, usb.diff_pair_width, usb.diff_pair_gap), (Some(Nm::from_mm(0.25)), Some(Nm::from_mm(0.11)), Some(Nm::from_mm(0.12)), Some(Nm::from_mm(0.15))));
    }

    #[test]
    fn copper_with_no_net_exports_as_net_zero_and_a_missing_net_still_fails() {
        // An imported board can carry copper that has no net (KiCad's net 0); that is not the silent default
        // the net check exists to catch, so it must not fail the export of the whole board.
        let (mut design, model) = fixture();
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: String::new(), net: String::new(), pins: vec![], layer: "F.Cu".into(), width: Nm::from_mm(0.2), pts: vec![eda_model::ir::Point { x: Nm::from_mm(1.0), y: Nm::from_mm(1.0) }, eda_model::ir::Point { x: Nm::from_mm(2.0), y: Nm::from_mm(1.0) }], arc_mid_offset: None });
        rt.zones = vec![Zone { id: "z0".into(), net: String::new(), layer: "F.Cu".into(), outline: square(Nm::from_mm(10.0), Nm::from_mm(10.0), Nm::from_mm(3.0)), ..Default::default() }];
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
            at: Point { x: Nm::from_mm(15.0), y: Nm::from_mm(5.0) },
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
                let (ex, ey) = (pad.center.x.mm(), pad.center.y.mm());
                assert!(
                    (x - ex).abs() < 1e-6 && (y - ey).abs() < 1e-6,
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

    /// What kicad-cli draws a footprint's 3D model at is the footprint's `(model ...)` placement, and KiCad turns a flipped footprint's model with
    /// `Ry(pi) Rz(pi)` where this studio mirrors x. A bottom footprint made here is written in KiCad's frame (mirrored in y, the half turn in its
    /// orientation, `write_footprint`), so its model is written as it is held, as KiCad's Flip leaves a model; the reader reads it back in the library's frame
    /// (`FootprintExtra::library_frame`), the model as it was.
    #[test]
    fn a_footprints_models_are_written_with_their_placement_and_a_bottom_one_in_kicads_frame() {
        use eda_model::footprint::Model3d;
        let (design, mut model) = fixture();
        let placed = Model3d { path: "${KICAD10_3DMODEL_DIR}/X.3dshapes/X.step".into(), offset: [1.0, 2.0, 0.5], scale: [1.0, 1.0, 1.0], rotate: [90.0, 0.0, 30.0], opacity: 1.0, show: true };
        for reference in ["U1", "C1"] {
            let mut fp = model.footprint_of(model.part(reference).unwrap()).unwrap();
            fp.name = format!("{reference}_FP");
            fp.models3d = vec![placed.clone()];
            model.footprints.push(fp);
            model.parts.iter_mut().find(|p| p.reference == reference).unwrap().footprint = Some(format!("{reference}_FP"));
        }
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let block = |reference: &str| -> String {
            let start = out.find(&format!("(footprint \"{reference}_FP\"")).unwrap_or_else(|| panic!("{reference}_FP is written:\n{out}"));
            let rest = &out[start..];
            let fp = &rest[..rest[1..].find("\n\t(footprint ").map_or(rest.len(), |i| i + 1)];
            fp[fp.find("\t\t(model ").expect("a model is written")..].to_string()
        };
        let top = block("U1");
        assert!(top.contains("(offset\n\t\t\t\t(xyz 1 2 0.5)") && top.contains("(rotate\n\t\t\t\t(xyz 90 0 30)"), "a top footprint's model is written as it is held: {top}");
        let bottom = block("C1");
        assert!(bottom.contains("(offset\n\t\t\t\t(xyz 1 2 0.5)") && bottom.contains("(rotate\n\t\t\t\t(xyz 90 0 30)"), "a bottom footprint's model is written as it is held, in KiCad's frame: {bottom}");

        let (_, back, _) = crate::import_kicad_pcb(&out).unwrap();
        let held = |reference: &str| -> Vec<Model3d> {
            let part = back.parts.iter().find(|p| p.reference == reference).unwrap();
            back.footprint_of(part).unwrap().models3d
        };
        assert_eq!(held("U1"), vec![placed.clone()]);
        assert_eq!(held("C1"), vec![placed], "read back, the bottom footprint's model is the one it was written from (the reader undoes the frame)");
    }

    /// Two pads of one footprint on the same point with the same number (`complex_hierarchy`'s MIRE: crossed bars, every pad numbered "", two of them on one
    /// point) are two items with two uuids. With one uuid for both, kicad-cli reported the pair as a copper clearance violation (8 of them on that board).
    #[test]
    fn two_pads_on_one_point_with_one_number_are_written_with_two_uuids() {
        let board = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (footprint "MIRE" (layer "F.Cu") (at 10 10)
    (property "Reference" "MIRE" (at 0 0 0) (layer "F.SilkS"))
    (property "Value" "*" (at 0 0 0) (layer "F.Fab"))
    (attr through_hole)
    (pad "" thru_hole rect (at -1.5 0) (size 2.3 0.25) (drill 0.76) (layers "*.Cu" "*.Mask"))
    (pad "" thru_hole rect (at -1.5 0) (size 2.8 0.25) (drill 0.76) (layers "*.Cu" "*.Mask"))
    (pad "" thru_hole rect (at 1.5 0) (size 2.8 0.25) (drill 0.76) (layers "*.Cu" "*.Mask"))
  )
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let uuids: Vec<&str> = out.lines().filter(|l| l.trim_start().starts_with("(pad ")).filter_map(|l| l.split("(uuid \"").nth(1)).filter_map(|r| r.split('"').next()).collect();
        assert_eq!(uuids.len(), 3, "{out}");
        let distinct: std::collections::BTreeSet<&&str> = uuids.iter().collect();
        assert_eq!(distinct.len(), 3, "each pad has a uuid of its own: {uuids:?}");
        // The uuids are still a function of the board alone (a saved DRC exclusion names its items by uuid).
        let again = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(out, again);
    }

    /// A text of several lines is written with KiCad's `\n` escape (a raw newline inside the quotes is a parse error for it) and read back with its lines; a quote, a
    /// backslash and a carriage return come back as they went. `complex_hierarchy`'s "Actionneur\nPiezo New Amp\nV02" came back as one line with backslashes in it, 60 mm
    /// wide, and kicad-cli reported it shorting the nets under it.
    #[test]
    fn a_texts_lines_quotes_and_backslashes_are_written_as_kicad_writes_them_and_read_back() {
        let (mut design, model) = fixture();
        let content = "Actionneur\nPiezo \"New\" Amp\\V02\r\nline 3";
        design.drawings = Some(eda_model::ir::DrawingsSection {
            texts: vec![Text { id: "t1".into(), content: content.into(), at: Point { x: Nm::from_mm(1.0), y: Nm::from_mm(2.0) }, angle: 0, layer: "F.SilkS".into(), size_nm: Nm::from_mm(1.0), stroke_width: Nm::from_mm(0.15), justify: TextJustify::Center, mirror: false }],
            ..Default::default()
        });
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let line = out.lines().find(|l| l.contains("(gr_text ")).expect("the text is written");
        assert!(line.starts_with("\t(gr_text \"Actionneur\\nPiezo \\\"New\\\" Amp\\\\V02\\r\\nline 3\" (at 1 2 0)"), "{line}");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let texts = &back.drawings.as_ref().expect("drawings").texts;
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].content, content);
    }

    /// A board text keeps its whole `(effects ..)`: `(size HEIGHT WIDTH)` with a width that is not the height (`complex_hierarchy`'s "Actionneur" text is 2.032 high and 1.524
    /// wide), `justify left bottom` (`connection_width_rules`' labels sit above their anchors), bold, italic, a knockout, the line spacing, a font face, and no
    /// `(thickness ..)` when the file had none. kicad-cli measured those texts against their neighbours: written as square, centred, regular lines they overlapped tracks the
    /// originals do not.
    #[test]
    fn a_board_texts_size_justification_weight_and_looks_are_written_back_as_they_were_read() {
        let board = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (gr_text "High_current netclass:" (at 106.147 73.939) (layer "F.Cu") (tstamp dd7b0757-fb61-40bd-84e1-598677c91625)
    (effects (font (size 0.2 0.2) (thickness 0.04) bold) (justify left bottom))
  )
  (gr_text "Actionneur" (at 176.149 64.643 0) (layer "B.Cu" knockout) (tstamp ef157bdf-7633-496c-93fa-91119bb1ecd2)
    (effects (font (face "Arial") (size 2.032 1.524) (line_spacing 1.5) (thickness 0.3048) italic) (justify right top mirror))
  )
  (gr_text "plain" (at 1 2 0) (layer "F.SilkS") (tstamp 00000000-0000-0000-0000-000000000001)
    (effects (font (size 1 1) (thickness 0.15)))
  )
  (gr_text "auto" (at 3 4 0) (layer "F.SilkS") (tstamp 00000000-0000-0000-0000-000000000002)
    (effects (font (size 1.2 1.6)) (justify bottom))
  )
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        let dr = design.drawings.as_ref().expect("drawings");
        let by_text = |content: &str| dr.texts.iter().find(|t| t.content == content).unwrap_or_else(|| panic!("{content}"));
        let extra = |content: &str| dr.text_extra(&by_text(content).id).cloned();
        // The square, centred, regular text with a stroke width is the model's own default and keeps no extras.
        assert_eq!(extra("plain"), None);
        assert_eq!((by_text("plain").size_nm, by_text("plain").stroke_width), (Nm::from_mm(1.0), Nm::from_mm(0.15)));

        let labels = extra("High_current netclass:").expect("bold, bottom");
        assert!(labels.bold && !labels.italic && labels.valign == eda_model::ir::TextVAlign::Bottom && labels.width_nm.is_none() && !labels.auto_thickness, "{labels:?}");
        assert_eq!((by_text("High_current netclass:").justify, by_text("High_current netclass:").stroke_width), (TextJustify::Left, Nm::from_mm(0.04)));

        let wide = extra("Actionneur").expect("italic, narrow");
        assert_eq!((wide.width_nm, wide.italic, wide.knockout, wide.line_spacing_permille, wide.face.as_deref(), wide.valign), (Some(Nm::from_mm(1.524)), true, true, 1500, Some("Arial"), eda_model::ir::TextVAlign::Top));
        assert_eq!((by_text("Actionneur").size_nm, by_text("Actionneur").stroke_width, by_text("Actionneur").justify, by_text("Actionneur").mirror), (Nm::from_mm(2.032), Nm::new(304_800), TextJustify::Right, true));

        // No thickness in the file: KiCad's automatic stroke width (an eighth of the glyph width), and none is written back.
        let auto = extra("auto").expect("auto thickness");
        assert_eq!((auto.auto_thickness, auto.width_nm, by_text("auto").size_nm, by_text("auto").stroke_width), (true, Some(Nm::from_mm(1.6)), Nm::from_mm(1.2), Nm::from_mm(0.2)));

        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let block = |content: &str| -> String {
            let start = out.find(&format!("(gr_text \"{content}\"")).unwrap_or_else(|| panic!("{content} in {out}"));
            out[start..].lines().take(2).collect::<Vec<_>>().join("\n")
        };
        let labels = block("High_current netclass:");
        assert!(labels.contains("(layer \"F.Cu\")") && labels.contains("(effects (font (size 0.2 0.2) (thickness 0.04) bold) (justify left bottom))"), "{labels}");
        let wide = block("Actionneur");
        assert!(wide.contains("(layer \"B.Cu\" knockout)"), "{wide}");
        assert!(wide.contains("(effects (font (face \"Arial\") (size 2.032 1.524) (line_spacing 1.5) (thickness 0.3048) italic) (justify right top mirror))"), "{wide}");
        assert!(block("plain").contains("(effects (font (size 1 1) (thickness 0.15)))"), "{}", block("plain"));
        let auto = block("auto");
        assert!(auto.contains("(effects (font (size 1.2 1.6)) (justify bottom))"), "{auto}");

        // Read again, the same extras and the same texts.
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let (a, b) = (design.drawings.as_ref().unwrap(), back.drawings.as_ref().unwrap());
        assert_eq!(a.texts.len(), b.texts.len());
        for t in &a.texts {
            let u = b.texts.iter().find(|u| u.content == t.content).unwrap();
            assert_eq!((t.size_nm, t.stroke_width, t.justify, t.mirror, t.angle, &t.layer), (u.size_nm, u.stroke_width, u.justify, u.mirror, u.angle, &u.layer));
            assert_eq!(a.text_extra(&t.id).map(|x| (x.width_nm, x.valign, x.bold, x.italic, x.knockout, x.auto_thickness)), b.text_extra(&u.id).map(|x| (x.width_nm, x.valign, x.bold, x.italic, x.knockout, x.auto_thickness)));
        }
    }

    /// A board with a few footprints, for the tests of what a footprint of a KiCad file keeps: `D5` has no library link and no courtyard, `R1` has both, and the
    /// courtyard of `Q1` is a pair of `fp_line`s that close nothing.
    const FOOTPRINT_BOARD: &str = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (footprint "D5" (layer "F.Cu") (at 10 10)
    (property "Reference" "D1" (at 0 0 0) (layer "F.SilkS"))
    (property "Value" "D" (at 0 0 0) (layer "F.Fab"))
    (attr through_hole)
    (pad "1" thru_hole rect (at -2.54 0) (size 1.8 1.8) (drill 0.9) (layers "*.Cu" "*.Mask"))
    (pad "2" thru_hole circle (at 2.54 0) (size 1.8 1.8) (drill 0.9) (layers "*.Cu" "*.Mask"))
  )
  (footprint "Resistor_SMD:R_0603_1608Metric" (layer "F.Cu") (at 20 10)
    (property "Reference" "R1" (at 0 0 0) (layer "F.SilkS"))
    (property "Value" "R" (at 0 0 0) (layer "F.Fab"))
    (attr smd)
    (fp_rect (start -1.5 -0.7) (end 1.5 0.7) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
    (pad "1" smd roundrect (at -0.8 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25))
    (pad "2" smd roundrect (at 0.8 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25))
  )
)"#;

    /// A footprint the file gave no library link (`(footprint "D5" ..)`, `LIB_ID::GetLibNickname()` empty) is written with none, as `LIB_ID::Format` writes it: the bare name.
    /// Written as `eda:D5` it linked the footprint to a library that does not exist, and a project that checks library links reported it (72 times on `complex_hierarchy`,
    /// `lib_footprint_issues`). A footprint built here is named as its symbol names it (`footprint_lib_id`), never `eda:`.
    #[test]
    fn a_footprint_with_no_library_link_is_written_with_none() {
        let (design, model, _) = crate::import_kicad_pcb(FOOTPRINT_BOARD).unwrap();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(footprint \"D5\"\n"), "{out}");
        assert!(out.contains("(footprint \"Resistor_SMD:R_0603_1608Metric\"\n"), "{out}");
        assert!(!out.contains("\"eda:"), "{out}");
        // Read again: the same, and the board is stable.
        let (back, back_model, _) = crate::import_kicad_pcb(&out).unwrap();
        let again = export_kicad_pcb(&back, &back_model, &meta()).unwrap();
        assert!(again.contains("(footprint \"D5\"\n") && !again.contains("\"eda:"), "{again}");
        // A part built here is on the footprint it names, with no library when the name has none (`footprint_lib_id`), as its symbol's Footprint
        // field names it.
        let (made, made_model) = fixture();
        assert!(export_kicad_pcb(&made, &made_model, &meta()).unwrap().contains("(footprint \"SOT-23\""));
        // The footprint exchanged for another one is the one it is named: a bare name with no library.
        let (swapped, mut swapped_model, _) = crate::import_kicad_pcb(FOOTPRINT_BOARD).unwrap();
        swapped_model.parts.iter_mut().find(|p| p.reference == "D1").unwrap().footprint = Some("SOT-23".into());
        let out = export_kicad_pcb(&swapped, &swapped_model, &meta()).unwrap();
        assert!(out.contains("(footprint \"SOT-23\""), "{out}");
    }

    /// A footprint with nothing on its courtyard layers gets none in the file; before, it got the box its pads derive, and two such boxes overlapped
    /// (`courtyards_overlap` on `complex_hierarchy`'s R20 and Q8, which the original does not report).
    #[test]
    fn a_footprint_that_had_no_courtyard_is_written_without_one() {
        let (design, model, _) = crate::import_kicad_pcb(FOOTPRINT_BOARD).unwrap();
        let extras = &design.drawings.as_ref().unwrap().footprint_extras;
        assert_eq!((extras.iter().find(|e| e.id == "D1").unwrap().no_courtyard, extras.iter().find(|e| e.id == "R1").unwrap().no_courtyard), (true, false));
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let block = |reference: &str| -> String {
            let start = out.find(&format!("(property \"Reference\" \"{reference}\"")).expect("footprint");
            let begin = out[..start].rfind("\t(footprint ").unwrap();
            let end = out[start..].find("\n\t(footprint ").map_or(out.len(), |i| start + i);
            out[begin..end].to_string()
        };
        assert!(!block("D1").contains("CrtYd"), "{}", block("D1"));
        assert!(block("R1").contains("(layer \"F.CrtYd\")"), "{}", block("R1"));
        // The footprints built here keep the box the placer keeps clear.
        let (made, made_model) = fixture();
        assert!(export_kicad_pcb(&made, &made_model, &meta()).unwrap().contains("CrtYd"));
    }

    /// The width of the lines of an imported outline is kept (`complex_hierarchy`'s Edge.Cuts are 0.2032 mm wide, written at the 0.1 KiCad gives a line that says none, and
    /// kicad-cli measures edge clearance from the drawn line and plots it at its width). Lines of different widths are not one loop of lines here: they stay shapes, each
    /// at its own width.
    #[test]
    fn the_width_of_an_imported_outline_is_written_back() {
        let outline = |widths: [&str; 4]| -> String {
            format!(
                r#"(kicad_pcb (version 20240108) (generator "pcbnew") (general (thickness 1.6)) (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user)) (setup (pad_to_mask_clearance 0)) (net 0 "")
  (gr_line (start 0 0) (end 20 0) (stroke (width {}) (type solid)) (layer "Edge.Cuts"))
  (gr_line (start 20 0) (end 20 10) (stroke (width {}) (type solid)) (layer "Edge.Cuts"))
  (gr_line (start 20 10) (end 0 10) (stroke (width {}) (type solid)) (layer "Edge.Cuts"))
  (gr_line (start 0 10) (end 0 0) (stroke (width {}) (type solid)) (layer "Edge.Cuts"))
)"#,
                widths[0], widths[1], widths[2], widths[3]
            )
        };
        let (design, model, notes) = crate::import_kicad_pcb(&outline(["0.2032"; 4])).unwrap();
        assert_eq!(notes.outline_source, "lines");
        assert_eq!(design.drawings.as_ref().and_then(|d| d.outline_width_nm), Some(Nm::new(203_200)));
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let lines: Vec<&str> = out.lines().filter(|l| l.contains("\"Edge.Cuts\")") && l.contains("(gr_line")).collect();
        assert_eq!(lines.len(), 4, "{out}");
        assert!(lines.iter().all(|l| l.contains("(stroke (width 0.2032) (type default))")), "{lines:?}");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        assert_eq!(back.drawings.as_ref().and_then(|d| d.outline_width_nm), Some(Nm::new(203_200)));

        // An outline made here says no width, and is written as it always was.
        let (made, made_model) = fixture();
        let plain = export_kicad_pcb(&made, &made_model, &meta()).unwrap();
        assert!(plain.lines().filter(|l| l.contains("\"Edge.Cuts\")")).all(|l| !l.contains("stroke")), "{plain}");

        // Four widths: shapes, each as it was.
        let (design, model, notes) = crate::import_kicad_pcb(&outline(["0.1", "0.2", "0.1", "0.2"])).unwrap();
        assert_eq!(notes.outline_source, "shapes");
        assert_eq!(design.drawings.as_ref().and_then(|d| d.outline_width_nm), None);
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(out.matches("(stroke (width 0.2)").count(), 2, "{out}");
    }

    /// The classes a board file gives its footprints (`(component_classes (class "CLASS2") (class "CLASS3"))`) and the project's assignment rules are written back, so a DRC rule
    /// that reads `A.Component_Class` judges the re-export as it judged the original (`component_classes_drc`: two assertion failures became four, because no footprint
    /// had a class). The sheet a footprint is on comes with them, for a project that makes a class of every sheet.
    #[test]
    fn component_classes_and_the_sheet_of_a_footprint_are_written_back_and_the_projects_rules_with_them() {
        let board = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (footprint "Resistor_SMD:R_0603_1608Metric" (layer "F.Cu") (at 10 10)
    (property "Reference" "R1" (at 0 0 0) (layer "F.SilkS"))
    (property "Value" "R" (at 0 0 0) (layer "F.Fab"))
    (component_classes (class "CLASS2") (class "CLASS3") (class "CLASS2"))
    (path "/0acad4a2-8e97-4c0c-9dc3-627d0a5dadd5")
    (sheetname "/sub/")
    (sheetfile "sub.kicad_sch")
    (attr smd)
    (fp_rect (start -1.5 -0.7) (end 1.5 0.7) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
    (pad "1" smd roundrect (at -0.8 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25))
  )
  (footprint "Resistor_SMD:R_0603_1608Metric" (layer "F.Cu") (at 20 10)
    (property "Reference" "R2" (at 0 0 0) (layer "F.SilkS"))
    (property "Value" "R" (at 0 0 0) (layer "F.Fab"))
    (attr smd)
    (fp_rect (start -1.5 -0.7) (end 1.5 0.7) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
    (pad "1" smd roundrect (at -0.8 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25))
  )
)"#;
        let (design, mut model, _) = crate::import_kicad_pcb(board).unwrap();
        let extras = &design.drawings.as_ref().unwrap().footprint_extras;
        let r1 = extras.iter().find(|e| e.id == "R1").unwrap();
        assert_eq!(r1.component_classes, vec!["CLASS2".to_string(), "CLASS3".to_string()], "each class once, in the file's order");
        assert_eq!((r1.sheetname.as_deref(), r1.sheetfile.as_deref()), (Some("/sub/"), Some("sub.kicad_sch")));
        assert!(extras.iter().find(|e| e.id == "R2").unwrap().component_classes.is_empty());

        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let r1_block = &out[out.find("(property \"Reference\" \"R1\"").unwrap()..out.find("(property \"Reference\" \"R2\"").unwrap()];
        assert!(r1_block.contains("(component_classes (class \"CLASS2\") (class \"CLASS3\"))"), "{r1_block}");
        assert!(r1_block.contains("(sheetname \"/sub/\")") && r1_block.contains("(sheetfile \"sub.kicad_sch\")"), "{r1_block}");
        assert!(!r1_block.contains("(path "), "the link to the symbol is not written: the derived schematic's symbols have uuids of their own");
        let r2_block = &out[out.find("(property \"Reference\" \"R2\"").unwrap()..];
        assert!(!r2_block.contains("component_classes") && !r2_block.contains("sheetname"), "{r2_block}");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let back_r1 = back.drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == "R1").unwrap().clone();
        assert_eq!((back_r1.component_classes, back_r1.sheetname), (r1.component_classes.clone(), r1.sheetname.clone()));

        // The project's rules reach the derived project, and a project without any has no `component_class_settings` at all.
        assert!(serde_json::from_str::<serde_json::Value>(&export_kicad_pro(&model)).unwrap().get("component_class_settings").is_none());
        let pro = serde_json::json!({ "component_class_settings": { "assignments": [{ "component_class": "RES", "conditions": { "REFERENCE": { "primary": "R*" } }, "conditions_operator": "ALL" }], "meta": { "version": 0 }, "sheet_component_classes": { "enabled": true } } });
        crate::merge_project_setup_pages(&mut model, &pro.to_string());
        assert!(model.board.component_classes.sheet_classes && model.board.component_classes.assignments.len() == 1);
        let derived: serde_json::Value = serde_json::from_str(&export_kicad_pro(&model)).unwrap();
        assert_eq!(derived["component_class_settings"]["assignments"][0]["conditions"]["REFERENCE"]["primary"], "R*");
        assert_eq!(derived["component_class_settings"]["sheet_component_classes"]["enabled"], true);
    }

    /// A board's text boxes (`PCB_TEXTBOX`) are read and written back as `format( const PCB_TEXTBOX* )` writes them: the corners (or the polygon of a turned box), the margins, the
    /// text angle, the layer, the looks of the text, the border with its style, the knockout and the lock; a group may hold one.
    #[test]
    fn a_boards_text_boxes_are_written_back_as_kicad_writes_them() {
        let board = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (36 "B.SilkS" user) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (gr_text_box "Rev A\nnotes" (start 20 20) (end 40 30) (margins 1 1.5 2 2.5) (layer "F.SilkS") (uuid "aaaaaaaa-1111-4111-8111-111111111111")
    (effects (font (size 1 1.2) (thickness 0.15) bold) (justify left top))
    (border yes) (stroke (width 0.12) (type dash)) (knockout yes))
  (gr_text_box "turned" (locked yes) (pts (xy 50 20) (xy 55 25) (xy 50 30) (xy 45 25)) (margins 1 1 1 1) (angle 45) (layer "B.SilkS") (uuid "bbbbbbbb-2222-4222-8222-222222222222")
    (effects (font (size 1 1) (thickness 0.1)) (justify right bottom mirror))
    (border no) (stroke (width 0.1) (type solid)) (knockout no))
  (gr_line (start 0 0) (end 5 0) (stroke (width 0.1) (type default)) (layer "Edge.Cuts") (uuid "cccccccc-3333-4333-8333-333333333333"))
  (group "keep" (uuid "dddddddd-4444-4444-8444-444444444444") (members "aaaaaaaa-1111-4111-8111-111111111111" "cccccccc-3333-4333-8333-333333333333"))
)"#;
        let (design, model, notes) = crate::import_kicad_pcb(board).unwrap();
        assert!(notes.unread_items.is_empty(), "{:?}", notes.unread_items);
        let dr = design.drawings.as_ref().expect("drawings");
        assert_eq!(dr.board_items.text_boxes.len(), 2);
        assert!(dr.board_items.text_boxes.iter().all(|t| t.id.starts_with("tbx_")), "ids of ours");
        let rev = dr.board_items.text_boxes.iter().find(|t| t.text.starts_with("Rev A")).unwrap();
        assert_eq!((rev.start, rev.end, rev.layer.as_str()), (Point { x: Nm::from_mm(20.0), y: Nm::from_mm(20.0) }, Point { x: Nm::from_mm(40.0), y: Nm::from_mm(30.0) }, "F.SilkS"));
        assert_eq!((rev.margin_left_nm, rev.margin_top_nm, rev.margin_right_nm, rev.margin_bottom_nm), (Nm::from_mm(1.0), Nm::from_mm(1.5), Nm::from_mm(2.0), Nm::from_mm(2.5)));
        assert_eq!((rev.size_nm, rev.width_nm, rev.bold, rev.knockout, rev.border), (Nm::from_mm(1.0), Some(Nm::from_mm(1.2)), true, true, true));
        let turned = dr.board_items.text_boxes.iter().find(|t| t.text == "turned").unwrap();
        assert_eq!((turned.pts.len(), turned.angle, turned.border, turned.mirror), (4, 45_000, false, true));
        assert!(dr.locked_ids.contains(&turned.id), "a locked text box comes back locked");
        assert_eq!(dr.groups.len(), 1, "a group holds the first text box and the line");
        assert!(dr.groups[0].member_ids.contains(&rev.id));

        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(gr_text_box \"Rev A\\nnotes\""), "{out}");
        assert!(out.contains("(gr_text_box \"turned\" (locked yes)"), "{out}");
        assert_eq!(out.matches("(gr_text_box").count(), 2);
        let (back, _, back_notes) = crate::import_kicad_pcb(&out).unwrap();
        assert!(back_notes.unread_items.is_empty());
        let strip = |d: &Design| -> Vec<eda_model::pcb_items::TextBox> {
            let mut v: Vec<_> = d.drawings.as_ref().unwrap().board_items.text_boxes.iter().cloned().map(|mut b| { b.id.clear(); b }).collect();
            v.sort_by(|a, b| a.text.cmp(&b.text));
            v
        };
        assert_eq!(strip(&design), strip(&back), "read again, the same text boxes");
        let back_dr = back.drawings.as_ref().unwrap();
        assert_eq!(back_dr.groups.len(), 1, "the group survives with its text box");
        assert_eq!(back_dr.locked_ids.len(), 1);
    }

    /// A board's barcodes (`api_kitchen_sink` has a QR code and a Data Matrix) are read and written back as `format( const PCB_BARCODE* )` writes them: the layer, the size,
    /// the text and its height, the kind, a QR code's error correction level, the margins and the flags. kicad-cli keeps a barcode a visual separation from the board edge
    /// (`clearance`); a re-export without them lost that finding.
    #[test]
    fn a_boards_barcodes_are_written_back_as_kicad_writes_them() {
        let board = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (5 "F.SilkS" user "F.Silkscreen") (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (barcode (at 258.5 141.5 90) (layer "F.Cu") (size 42 44) (text "acode") (text_height 3) (type qr) (ecc_level M) (hide no) (knockout no) (uuid "705675f1-5a54-4ef0-bc6c-93b7d057008e"))
  (barcode (locked yes) (at 255 44.5 180) (layer "F.SilkS") (size 42 44) (text "differentcode") (text_height 2) (type datamatrix) (hide yes) (knockout yes) (margins 1 2) (uuid "11111111-5a54-4ef0-bc6c-93b7d057008e"))
  (barcode (at 1 2) (layer "F.SilkS") (type code128) (text "123") (uuid "22222222-5a54-4ef0-bc6c-93b7d057008e"))
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        let dr = design.drawings.as_ref().expect("drawings");
        assert_eq!(dr.barcodes.len(), 3);
        assert!(dr.barcodes.iter().all(|b| b.id.starts_with("bar_")), "{:?}", dr.barcodes.iter().map(|b| &b.id).collect::<Vec<_>>());
        let qr = dr.barcodes.iter().find(|b| b.text == "acode").unwrap();
        assert_eq!((qr.at, qr.angle, qr.layer.as_str(), qr.width_nm, qr.height_nm, qr.text_height_nm), (Point { x: Nm::from_mm(258.5), y: Nm::from_mm(141.5) }, 90_000, "F.Cu", Nm::from_mm(42.0), Nm::from_mm(44.0), Nm::from_mm(3.0)));
        assert_eq!((qr.kind, qr.ecc, qr.show_text, qr.knockout), (eda_model::barcode::BarcodeKind::Qr, eda_model::barcode::EccLevel::M, true, false));
        let dm = dr.barcodes.iter().find(|b| b.text == "differentcode").unwrap();
        assert_eq!((dm.kind, dm.show_text, dm.knockout, dm.margin_x_nm, dm.margin_y_nm), (eda_model::barcode::BarcodeKind::DataMatrix, false, true, Nm::from_mm(1.0), Nm::from_mm(2.0)));
        assert!(dr.locked_ids.contains(&dm.id), "a locked barcode comes back locked");
        // What the file left out is PCB_BARCODE's own: 40 mm square, the text printed.
        let plain = dr.barcodes.iter().find(|b| b.text == "123").unwrap();
        assert_eq!((plain.width_nm, plain.height_nm, plain.show_text, plain.kind), (Nm::from_mm(40.0), Nm::from_mm(40.0), true, eda_model::barcode::BarcodeKind::Code128));

        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let line = |text: &str| out.lines().find(|l| l.contains("(barcode") && l.contains(&format!("(text \"{text}\")"))).unwrap_or_else(|| panic!("{text} in {out}")).trim().to_string();
        let l = line("acode");
        assert!(l.starts_with("(barcode (at 258.5 141.5 90) (layer \"F.Cu\") (size 42 44) (text \"acode\") (text_height 3) (type qr) (ecc_level M) (hide no) (knockout no) (uuid \""), "{l}");
        let l = line("differentcode");
        assert!(l.starts_with("(barcode (locked yes) (at 255 44.5 180) (layer \"F.SilkS\") (size 42 44) (text \"differentcode\") (text_height 2) (type datamatrix) (hide yes) (knockout yes) (margins 1 2) (uuid \""), "{l}");
        assert!(!l.contains("ecc_level"), "only the QR kinds have an error correction level: {l}");
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let strip = |d: &Design| -> Vec<eda_model::barcode::Barcode> { d.drawings.as_ref().unwrap().barcodes.iter().cloned().map(|mut b| { b.id.clear(); b }).collect() };
        let (mut a, mut b) = (strip(&design), strip(&back));
        a.sort_by(|x, y| x.text.cmp(&y.text));
        b.sort_by(|x, y| x.text.cmp(&y.text));
        assert_eq!(a, b, "read again, the same barcodes");
    }

    /// KiCad 10's files carry no table of the nets and name a net on every item that is on one (`(net "net_1")`): a net that only a track, a via or a zone is on is one of the
    /// board's nets, and the board exports (it failed with `kicad.unknown_net`; `connection_width_rules` has copper on four nets and no pad on any).
    #[test]
    fn a_net_only_copper_names_is_a_net_of_the_board() {
        let board = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (segment (start 0 0) (end 5 0) (width 0.25) (layer "F.Cu") (net "net_1") (uuid "00000000-0000-0000-0000-000000000001"))
  (segment (start 0 1) (end 5 1) (width 0.25) (layer "F.Cu") (net "net_HC") (uuid "00000000-0000-0000-0000-000000000002"))
  (via (at 5 0) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net "net_via") (uuid "00000000-0000-0000-0000-000000000003"))
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        assert_eq!(model.nets.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["net_1", "net_HC", "net_via"]);
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(out.contains("(net 1 \"net_1\")") && out.contains("(net 2 \"net_HC\")") && out.contains("(net 3 \"net_via\")"), "{out}");
    }

    /// A pad's pin type is written back (`(pintype "free")`): KiCad's copper clearance and solder mask checks and its connectivity leave a free pad out
    /// (`PAD::IsFreePad`), so a re-export that dropped the type judged the pad as the copper it is not.
    #[test]
    fn a_pads_pin_type_is_written_back() {
        let board = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (net 1 "GND")
  (footprint "Resistor_SMD:R_0603_1608Metric" (layer "F.Cu") (at 10 10)
    (property "Reference" "R1" (at 0 0 0) (layer "F.SilkS"))
    (property "Value" "R" (at 0 0 0) (layer "F.Fab"))
    (attr smd)
    (fp_rect (start -1.5 -0.7) (end 1.5 0.7) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
    (pad "1" smd roundrect (at -0.8 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25) (net 1 "GND") (pinfunction "A") (pintype "passive"))
    (pad "2" smd circle (at 0.8 0) (size 0.8 0.8) (layers "F.Cu" "F.Mask") (pintype "free"))
    (pad "3" smd circle (at 0 1) (size 0.8 0.8) (layers "F.Cu" "F.Mask"))
  )
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let pad = |number: &str| out.lines().find(|l| l.contains(&format!("(pad \"{number}\""))).unwrap_or_else(|| panic!("pad {number} in {out}")).to_string();
        assert!(pad("1").contains("(net 1 \"GND\") (pinfunction \"A\") (pintype \"passive\")"), "{}", pad("1"));
        assert!(pad("2").contains("(pintype \"free\")") && !pad("2").contains("(net "), "{}", pad("2"));
        assert!(!pad("3").contains("pintype"), "{}", pad("3"));
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let types: Vec<String> = back.drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == "R1").unwrap().pads.iter().map(|p| p.pin_type.clone()).collect();
        assert_eq!(types, ["passive", "free", ""]);
        let functions: Vec<String> = back.drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == "R1").unwrap().pads.iter().map(|p| p.pin_function.clone()).collect();
        assert_eq!(functions, ["A", "", ""]);
    }

    /// The layers a pad was on are the layers it is written on: an SMD pad with no paste layer (a fiducial) gets none, a pad with no mask layer gets none, and the pads that have
    /// both (nearly all) are written as they always were. Written with the paste of every SMD pad, a fiducial had a stencil aperture the original does not.
    #[test]
    fn the_layers_of_a_pad_are_written_as_they_were_read_with_or_without_paste_and_mask() {
        let board = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (footprint "Test:Pads" (layer "F.Cu") (at 10 10)
    (property "Reference" "U1" (at 0 0 0) (layer "F.SilkS"))
    (property "Value" "U" (at 0 0 0) (layer "F.Fab"))
    (attr smd)
    (fp_rect (start -5 -5) (end 5 5) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
    (pad "1" smd rect (at -3 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
    (pad "2" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask"))
    (pad "3" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Paste"))
    (pad "4" smd rect (at 3 0) (size 1 1) (layers "F.Cu"))
    (pad "5" thru_hole circle (at -3 2) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "*.Mask"))
    (pad "6" thru_hole circle (at -1 2) (size 1.6 1.6) (drill 0.8) (layers "*.Cu"))
    (pad "7" thru_hole circle (at 1 2) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "*.Mask" "F.SilkS"))
    (pad "8" thru_hole circle (at 3 2) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "F.SilkS"))
    (pad "9" connect roundrect (at -3 4) (size 2.5 8) (layers "*.Mask" "F.Cu") (roundrect_rratio 0.098))
    (pad "10" connect roundrect (at -1 4) (size 2.5 8) (layers "*.Mask" "B.Cu") (roundrect_rratio 0.098))
  )
  (footprint "Test:Bottom" (layer "B.Cu") (at 30 10)
    (property "Reference" "U2" (at 0 0 0) (layer "B.SilkS"))
    (property "Value" "U" (at 0 0 0) (layer "B.Fab"))
    (attr smd)
    (fp_rect (start -5 -5) (end 5 5) (stroke (width 0.05) (type solid)) (fill none) (layer "B.CrtYd"))
    (pad "1" smd rect (at -1 0) (size 1 1) (layers "B.Cu" "B.Paste" "B.Mask"))
    (pad "2" smd rect (at 1 0) (size 1 1) (layers "B.Cu" "B.Mask"))
  )
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let layers_of = |reference: &str, number: &str| -> String {
            let start = out.find(&format!("(property \"Reference\" \"{reference}\"")).unwrap_or_else(|| panic!("{reference}"));
            let end = out[start..].find("\n\t(footprint ").map_or(out.len(), |i| start + i);
            let pad = out[start..end].lines().find(|l| l.contains(&format!("(pad \"{number}\""))).unwrap_or_else(|| panic!("pad {number} of {reference}")).to_string();
            pad[pad.find("(layers").expect("layers")..].split(')').next().unwrap().to_string() + ")"
        };
        assert_eq!(layers_of("U1", "1"), "(layers \"F.Cu\" \"F.Paste\" \"F.Mask\")");
        assert_eq!(layers_of("U1", "2"), "(layers \"F.Cu\" \"F.Mask\")");
        assert_eq!(layers_of("U1", "3"), "(layers \"F.Cu\" \"F.Paste\")");
        assert_eq!(layers_of("U1", "4"), "(layers \"F.Cu\")");
        assert_eq!(layers_of("U1", "5"), "(layers \"*.Cu\" \"*.Mask\")");
        assert_eq!(layers_of("U1", "6"), "(layers \"*.Cu\")");
        assert_eq!(layers_of("U1", "7"), "(layers \"*.Cu\" \"*.Mask\" \"F.SilkS\")");
        assert_eq!(layers_of("U1", "8"), "(layers \"*.Cu\" \"F.SilkS\")");
        // An edge connector pad stays a connector pad: copper on its side, mask on both, no paste.
        assert_eq!(layers_of("U1", "9"), "(layers \"F.Cu\" \"F.Mask\" \"B.Mask\")");
        assert_eq!(layers_of("U1", "10"), "(layers \"B.Cu\" \"F.Mask\" \"B.Mask\")");
        let kind_of = |number: &str| -> String { let l = out.lines().find(|l| l.contains(&format!("(pad \"{number}\" "))).unwrap(); l.split_whitespace().nth(2).unwrap().to_string() };
        assert_eq!((kind_of("9"), kind_of("10"), kind_of("1")), ("connect".to_string(), "connect".to_string(), "smd".to_string()));
        // A back-side footprint's pads are on its side.
        assert_eq!(layers_of("U2", "1"), "(layers \"B.Cu\" \"B.Paste\" \"B.Mask\")");
        assert_eq!(layers_of("U2", "2"), "(layers \"B.Cu\" \"B.Mask\")");
        // Read again, the same flags; and the pads of a footprint built here keep the paste and mask they always had.
        let (back, back_model, _) = crate::import_kicad_pcb(&out).unwrap();
        // (The writer lists the pads by number, so the pads are compared by number.)
        let flags = |d: &Design, m: &ConstraintModel, reference: &str| -> BTreeMap<String, (bool, bool)> {
            let part = m.parts.iter().find(|p| p.reference == reference).unwrap();
            let footprint = m.footprint_of(part).unwrap();
            let infos = &d.drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == reference).unwrap().pads;
            footprint.pads.iter().zip(infos).map(|(pad, i)| (pad.number.clone(), (i.no_paste, i.no_mask))).collect()
        };
        assert_eq!(flags(&design, &model, "U1"), flags(&back, &back_model, "U1"));
        let expected: BTreeMap<String, (bool, bool)> = [("1", (false, false)), ("2", (true, false)), ("3", (false, true)), ("4", (true, true)), ("5", (false, false)), ("6", (false, true)), ("7", (false, false)), ("8", (false, true)), ("9", (true, false)), ("10", (true, false))]
            .into_iter()
            .map(|(n, f)| (n.to_string(), f))
            .collect();
        assert_eq!(flags(&design, &model, "U1"), expected);
        let (made, made_model) = fixture();
        let plain = export_kicad_pcb(&made, &made_model, &meta()).unwrap();
        assert!(plain.contains("(layers \"F.Cu\" \"F.Paste\" \"F.Mask\")") && plain.contains("(layers \"B.Cu\" \"B.Paste\" \"B.Mask\")"), "{plain}");
    }

    /// A microvia stack (`PCB_VIA_STACK`) puts two vias of one net at one point: each is written with a uuid of its own (KiCad's `via_generators` QA board came back
    /// with one uuid for both, and the stack's members named the wrong via).
    #[test]
    fn two_vias_of_a_net_at_one_point_get_two_uuids() {
        let board = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (via micro (at 20 10) (size 0.3) (drill 0.15) (layers "F.Cu" "In1.Cu") (net "GND") (uuid "00000000-0000-0000-0000-000000000001"))
  (via micro (at 20 10) (size 0.3) (drill 0.15) (layers "In1.Cu" "In2.Cu") (net "GND") (uuid "00000000-0000-0000-0000-000000000002"))
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let uuids: Vec<&str> = out.lines().filter(|l| l.contains("(via micro")).filter_map(|l| l.split("(uuid \"").nth(1)).collect();
        assert_eq!(uuids.len(), 2, "{out}");
        assert_ne!(uuids[0], uuids[1], "{out}");
    }

    /// A via keeps what its tokens say beyond its position, size, drill, layers and net: its type, the mode of the layers it does not connect, `free`, the zone layer
    /// connections, its tenting, capping and covering, its own padstack and teardrop settings, whatever a newer KiCad adds. The file had them on 10370 of the QA corpus's vias
    /// for the tenting alone, and the writer dropped every one: `api_kitchen_sink`'s two untented vias came back tented, with no opening in the solder mask plot.
    #[test]
    fn a_vias_type_and_tokens_are_written_back_as_the_file_had_them() {
        let board = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (via blind (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "In2.Cu") (remove_unused_layers yes) (keep_end_layers yes) (free yes) (zone_layer_connections "F.Cu" "In1.Cu") (tenting (front no) (back no)) (capping yes) (covering (front no) (back yes)) (net "A") (uuid "00000000-0000-0000-0000-000000000001"))
  (via micro (at 20 10) (size 0.3) (drill 0.1) (layers "F.Cu" "In1.Cu") (padstack (mode front_inner_back) (layer "Inner" (size 0.35)) (layer "B.Cu" (size 0.4))) (teardrops (best_length_ratio 0.5) (max_length 1) (curved_edges no)) (net "B") (uuid "00000000-0000-0000-0000-000000000002"))
  (via (at 30 10) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net "C") (uuid "00000000-0000-0000-0000-000000000003"))
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        let extras = &design.drawings.as_ref().expect("drawings").via_extras;
        assert_eq!(extras.len(), 2, "the plain via keeps no entry: {extras:?}");
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        // The nets are numbered by name: A is 1, B is 2, C is 3.
        let line = |net: usize| out.lines().find(|l| l.contains("(via") && l.contains(&format!("(net {net}) "))).unwrap_or_else(|| panic!("via on net {net} in {out}")).trim().to_string();
        let blind = line(1);
        assert!(blind.starts_with("(via blind (at 10 10) (size 0.6) (drill 0.3) (layers \"F.Cu\" \"In2.Cu\") (remove_unused_layers yes) (keep_end_layers yes) (free yes) (zone_layer_connections \"F.Cu\" \"In1.Cu\") (tenting (front no) (back no)) (capping yes) (covering (front no) (back yes)) (net "), "{blind}");
        let micro = line(2);
        assert!(micro.starts_with("(via micro (at 20 10) (size 0.3) (drill 0.1) (layers \"F.Cu\" \"In1.Cu\") (padstack (mode front_inner_back) (layer \"Inner\" (size 0.35)) (layer \"B.Cu\" (size 0.4))) (teardrops (best_length_ratio 0.5) (max_length 1) (curved_edges no)) (net "), "{micro}");
        let plain = line(3);
        assert!(plain.starts_with("(via (at 30 10) (size 0.8) (drill 0.4) (layers \"F.Cu\" \"B.Cu\") (net "), "{plain}");
        // Read again, the same.
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let strip = |d: &Design| -> Vec<(eda_model::via_extra::ViaKind, Vec<String>)> { d.drawings.as_ref().unwrap().via_extras.iter().map(|e| (e.kind, e.tokens.clone())).collect() };
        assert_eq!(strip(&design), strip(&back));
        // The tenting is still what the in-house checks read.
        assert!(design.drawings.as_ref().unwrap().via_tenting.iter().any(|t| (t.front, t.back) == (Some(false), Some(false))));
    }

    /// A pad keeps the tokens its fields do not stand for: the unconnected-layer mode (`remove_unused_layers`, `keep_end_layers`), `zone_layer_connections`, its teardrop
    /// settings, its tenting, a `(property pad_prop_heatsink)` (a heatsink pad, a castellated one ..), `die_length` (the pad-to-die length a length check adds), whatever a newer
    /// KiCad adds. The writer dropped all of them: the corpus has them on thousands of pads of two dozen boards.
    #[test]
    fn a_pads_other_tokens_are_written_back_as_the_file_had_them() {
        let board = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (footprint "Test:Heatsink" (layer "F.Cu") (at 10 10)
    (property "Reference" "U1" (at 0 0 0) (layer "F.SilkS"))
    (property "Value" "U" (at 0 0 0) (layer "F.Fab"))
    (attr smd)
    (fp_rect (start -5 -5) (end 5 5) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
    (pad "1" thru_hole circle (at -2 0) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "*.Mask") (remove_unused_layers yes) (keep_end_layers no) (zone_layer_connections "F.Cu") (tenting (front no) (back yes)) (teardrops (best_length_ratio 0.5) (curved_edges no)) (die_length 0.25) (property pad_prop_heatsink))
    (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (clearance 0.2) (locked yes))
    (pad "3" smd rect (at 4 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
  )
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        let tokens: Vec<Vec<String>> = design.drawings.as_ref().unwrap().footprint_extras[0].pads.iter().map(|p| p.tokens.clone()).collect();
        assert_eq!(
            tokens,
            vec![
                // The unconnected-layer mode is the pad's own now (`Pad::ext.stack`), not a token.
                vec!["(zone_layer_connections \"F.Cu\")", "(tenting (front no) (back yes))", "(teardrops (best_length_ratio 0.5) (curved_edges no))", "(die_length 0.25)", "(property pad_prop_heatsink)"],
                vec!["(locked yes)"],
                vec![],
            ]
        );
        let first = &model.footprints.first().expect("the footprint").pads;
        assert_eq!(first[0].ext.stack.unconnected, eda_model::padstack::UnconnectedLayers::RemoveAll, "remove_unused_layers yes, keep_end_layers no");
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let pad = |number: &str| out.lines().find(|l| l.contains(&format!("(pad \"{number}\" "))).unwrap_or_else(|| panic!("pad {number}")).trim().to_string();
        assert!(pad("1").contains("(layers \"*.Cu\" \"*.Mask\") (remove_unused_layers yes) (keep_end_layers no) (zone_layer_connections \"F.Cu\") (tenting (front no) (back yes)) (teardrops (best_length_ratio 0.5) (curved_edges no)) (die_length 0.25) (property pad_prop_heatsink)"), "{}", pad("1"));
        assert!(pad("2").contains("(locked yes)") && pad("2").contains("(clearance 0.2)"), "{}", pad("2"));
        assert!(pad("3").ends_with("))") && !pad("3").contains("locked"), "{}", pad("3"));
        // Written once: the fields the pad stands for are not among the tokens.
        assert_eq!(pad("1").matches("(layers").count(), 1);
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let again: Vec<Vec<String>> = back.drawings.as_ref().unwrap().footprint_extras[0].pads.iter().map(|p| p.tokens.clone()).collect();
        assert_eq!(again.iter().map(|t| t.len()).collect::<Vec<_>>(), tokens.iter().map(|t| t.len()).collect::<Vec<_>>());
    }

    /// A footprint keeps its description, tags, jumper pad groups and its Datasheet and Description fields -- `duplicate_pad_numbers_are_jumpers` makes the pads that share a number
    /// one pad to the connectivity -- and the board keeps the items nothing reads (a text box, a reference image, a target), as the file had them. The footprint's link to its symbol
    /// is not written, and the footprint's text goes when the part is exchanged for another footprint.
    #[test]
    fn a_footprints_other_tokens_and_the_boards_unread_items_are_written_back_as_the_file_had_them() {
        let board = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (36 "B.SilkS" user) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0) (covering (front yes) (back no)) (capping yes))
  (net 0 "")
  (footprint "Jumper:Jumper_2" (layer "F.Cu") (at 10 10)
    (descr "Jumper, 2 pads \"closed\"")
    (tags "jumper solder")
    (property "Reference" "JP1" (at 0 0 0) (layer "F.SilkS"))
    (property "Value" "J" (at 0 0 0) (layer "F.Fab"))
    (property "Datasheet" "~" (at 0 0 0) (unlocked yes) (layer "F.Fab") (hide yes) (effects (font (size 1.27 1.27))))
    (property "Description" "Jumper" (at 0 0 0) (unlocked yes) (layer "F.Fab") (hide yes) (effects (font (size 1.27 1.27))))
    (path "/00000000-0000-0000-0000-000000000042")
    (attr smd)
    (duplicate_pad_numbers_are_jumpers yes)
    (jumper_pad_groups ("1" "2"))
    (solder_mask_margin 0.02)
    (solder_paste_margin -0.01)
    (fp_rect (start -2 -1) (end 2 1) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
    (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
    (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))
  (gr_text_box "Box\nof text" (start 20 20) (end 30 25) (margins 1 1 1 1) (layer "F.SilkS") (border yes) (stroke (width 0.12) (type dot)) (effects (font (size 1 1)) (justify top)))
  (target x (at 40 40) (size 5) (width 0.15) (layer "Edge.Cuts") (tstamp 1234))
)"#;
        let (design, mut model, notes) = crate::import_kicad_pcb(board).unwrap();
        // The text box has a reader now (`pcb_textbox.rs`); the target is still kept as text.
        assert_eq!(notes.unread_items.keys().map(String::as_str).collect::<Vec<_>>(), ["target"]);
        assert_eq!(design.drawings.as_ref().unwrap().board_items.text_boxes.len(), 1);
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        for want in [
            "(descr \"Jumper, 2 pads \\\"closed\\\"\")",
            "(tags \"jumper solder\")",
            "(covering (front yes) (back no))",
            "(capping yes)",
            "(duplicate_pad_numbers_are_jumpers yes)",
            "(jumper_pad_groups (\"1\" \"2\"))",
            // The footprint's own margins reach the pads that have none of their own (`PAD::GetSolderMaskExpansion`): the mask margin is written from `FootprintExtra`, the paste
            // margin as the file had it, and the mask and paste checks of kicad-cli judge the pads by them.
            "(solder_mask_margin 0.02)",
            "(solder_paste_margin -0.01)",
            "(property \"Datasheet\" \"~\" (at 0 0 0) (unlocked yes) (layer \"F.Fab\") (hide yes) (effects (font (size 1.27 1.27))))",
            "(property \"Description\" \"Jumper\"",
            "(gr_text_box \"Box\\nof text\"",
            "(start 20 20) (end 30 25) (margins 1 1 1 1) (layer \"F.SilkS\")",
            "(border yes) (stroke (width 0.12) (type dot)) (knockout no)",
            "(target x (at 40 40) (size 5) (width 0.15) (layer \"Edge.Cuts\") (tstamp 1234))",
        ] {
            assert!(out.contains(want), "{want} in {out}");
        }
        assert!(!out.contains("(path "), "{out}");
        assert!(out.trim_end().ends_with("(target x (at 40 40) (size 5) (width 0.15) (layer \"Edge.Cuts\") (tstamp 1234))\n)"), "the unread items close the board: {}", &out[out.len().saturating_sub(300)..]);
        // Read again, the same.
        let (back, _, back_notes) = crate::import_kicad_pcb(&out).unwrap();
        assert_eq!(back_notes.unread_items.keys().map(String::as_str).collect::<Vec<_>>(), ["target"]);
        assert_eq!(back.drawings.as_ref().unwrap().board_items.text_boxes.len(), 1, "the text box is read again");
        let tokens = |d: &Design| d.drawings.as_ref().unwrap().footprint_extras[0].tokens.clone();
        assert_eq!(tokens(&design), tokens(&back));
        // The part exchanged for another footprint is not described by the old one's text.
        model.parts.iter_mut().find(|p| p.reference == "JP1").unwrap().footprint = Some("SOT-23".into());
        let swapped = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(!swapped.contains("(tags \"jumper solder\")") && !swapped.contains("duplicate_pad_numbers_are_jumpers"), "{swapped}");
        assert!(swapped.contains("(gr_text_box"), "the board's items stay");
    }

    /// A footprint's courtyard is written as the file had it, so a courtyard of four lines or a circle is still that and the footprint matches the library's copy (a courtyard
    /// polygon made of the lines made `TestPoint_Pad_1.0x1.0mm` and a mounting hole's circle `lib_footprint_mismatch` on every board that has one). On the other side, or on
    /// another footprint, it is the polygon outlines as before.
    #[test]
    fn a_footprints_courtyard_graphics_are_written_as_the_file_had_them() {
        let board = r#"(kicad_pcb (version 20241229) (generator "pcbnew")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (46 "B.CrtYd" user "B.Courtyard") (47 "F.CrtYd" user "F.Courtyard") (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (footprint "L:Lines" (layer "F.Cu") (uuid "a0000000-0000-0000-0000-000000000001") (at 10 10)
    (property "Reference" "U1" (at 0 0 0) (layer "F.SilkS")) (property "Value" "U" (at 0 0 0) (layer "F.Fab"))
    (fp_line (start -1 -1) (end -1 1) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "b0000000-0000-0000-0000-000000000001"))
    (fp_line (start -1 -1) (end 1 -1) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "b0000000-0000-0000-0000-000000000002"))
    (fp_line (start 1 1) (end -1 1) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "b0000000-0000-0000-0000-000000000003"))
    (fp_line (start 1 1) (end 1 -1) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "b0000000-0000-0000-0000-000000000004"))
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))
  (footprint "L:Round" (layer "F.Cu") (uuid "a0000000-0000-0000-0000-000000000002") (at 20 10)
    (property "Reference" "U2" (at 0 0 0) (layer "F.SilkS")) (property "Value" "U" (at 0 0 0) (layer "F.Fab"))
    (fp_circle (center 0 0) (end 2.6 0) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd") (uuid "b0000000-0000-0000-0000-000000000005"))
    (pad "1" smd circle (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))
  (footprint "L:Back" (layer "B.Cu") (uuid "a0000000-0000-0000-0000-000000000003") (at 30 10)
    (property "Reference" "U3" (at 0 0 0) (layer "B.SilkS")) (property "Value" "U" (at 0 0 0) (layer "B.Fab"))
    (fp_arc (start 1 0) (mid 0 1) (end -1 0) (stroke (width 0.05) (type solid)) (layer "B.CrtYd") (uuid "b0000000-0000-0000-0000-000000000006"))
    (fp_line (start -1 0) (end 1 0) (stroke (width 0.05) (type solid)) (layer "B.CrtYd") (uuid "b0000000-0000-0000-0000-000000000007"))
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "B.Cu" "B.Paste" "B.Mask")))
)"#;
        let (design, mut model, _) = crate::import_kicad_pcb(board).unwrap();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        for want in [
            "(fp_line (start -1 -1) (end -1 1) (stroke (width 0.05) (type solid)) (layer \"F.CrtYd\") (uuid \"b0000000-0000-0000-0000-000000000001\"))",
            "(fp_line (start 1 1) (end 1 -1) (stroke (width 0.05) (type solid)) (layer \"F.CrtYd\") (uuid \"b0000000-0000-0000-0000-000000000004\"))",
            "(fp_circle (center 0 0) (end 2.6 0) (stroke (width 0.05) (type solid)) (fill no) (layer \"F.CrtYd\") (uuid \"b0000000-0000-0000-0000-000000000005\"))",
            "(fp_arc (start 1 0) (mid 0 1) (end -1 0) (stroke (width 0.05) (type solid)) (layer \"B.CrtYd\") (uuid \"b0000000-0000-0000-0000-000000000006\"))",
        ] {
            assert!(out.contains(want), "{want} in {out}");
        }
        assert!(!out.contains("(fp_poly") && !out.contains("(fp_rect"), "no courtyard polygon beside the graphics: {out}");
        // Read again, the same.
        let (back, _, _) = crate::import_kicad_pcb(&out).unwrap();
        let items = |d: &Design| -> Vec<Vec<String>> { d.drawings.as_ref().unwrap().footprint_extras.iter().map(|e| e.courtyard_items.clone()).collect() };
        assert_eq!(items(&design), items(&back));
        // A part moved to the other side is not described by the courtyard of this one.
        let mut flipped = design.clone();
        flipped.placement.as_mut().unwrap().footprints.iter_mut().find(|f| f.id == "U2").unwrap().side = Side::Bottom;
        let on_back = export_kicad_pcb(&flipped, &model, &meta()).unwrap();
        assert!(!on_back.contains("b0000000-0000-0000-0000-000000000005") && on_back.contains("(layer \"B.CrtYd\")"), "{on_back}");
        // And so is a part exchanged for another footprint.
        model.parts.iter_mut().find(|p| p.reference == "U1").unwrap().footprint = Some("SOT-23".into());
        let swapped = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert!(!swapped.contains("b0000000-0000-0000-0000-000000000001"), "{swapped}");
    }

    /// A through-hole pad the board's file gave no drill (KiCad's Allegro importer makes them: 155 on one of the QA boards) is written with none, as `format( PAD )` does for a
    /// hole of no size; the export used to panic on it ("Footprint::validate requires a drill").
    #[test]
    fn a_through_hole_pad_with_no_drill_is_written_with_none() {
        let board = r#"(kicad_pcb (version 20241229) (generator "pcbnew")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (footprint "Test:NoDrill" (layer "F.Cu") (at 10 10)
    (property "Reference" "U1" (at 0 0 0) (layer "F.SilkS"))
    (property "Value" "U" (at 0 0 0) (layer "F.Fab"))
    (fp_rect (start -2 -2) (end 2 2) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
    (pad "1" thru_hole rect (at -1 0) (size 1.5 1.5) (layers "*.Cu" "*.Mask"))
    (pad "2" thru_hole circle (at 1 0) (size 1.5 1.5) (drill 0.8) (layers "*.Cu" "*.Mask"))
  )
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        let pad = |number: &str| out.lines().find(|l| l.contains(&format!("(pad \"{number}\" "))).unwrap_or_else(|| panic!("pad {number} in {out}")).to_string();
        assert!(pad("1").contains("thru_hole") && !pad("1").contains("(drill"), "{}", pad("1"));
        assert!(pad("2").contains("(drill 0.8)"), "{}", pad("2"));
        assert!(crate::import_kicad_pcb(&out).is_ok());
    }

    /// A footprint keeps the uuid its board file gave it, so a text that names a footprint by uuid (`${uuid:REFERENCE}`, KiCad's cross-reference) still finds it: re-exported with a
    /// uuid of its own, `issue4257`'s text read `<Unknown reference: ..>`, was longer than the original's and touched the pads beside it. Two footprints that claim one uuid
    /// are not both written with it, and a footprint made here has the uuid derived from its reference as always.
    #[test]
    fn a_footprint_keeps_the_uuid_its_file_gave_it() {
        let board = r#"(kicad_pcb (version 20241229) (generator "pcbnew")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (gr_text "Text: ${LAYER} ${00000000-0000-0000-0000-00005e384273:REFERENCE}" (at 5 5) (layer "F.SilkS") (uuid "a1000000-0000-0000-0000-000000000001") (effects (font (size 1 1))))
  (footprint "L:A" (layer "F.Cu") (uuid "00000000-0000-0000-0000-00005e384273") (at 10 10)
    (property "Reference" "U1" (at 0 0 0) (layer "F.SilkS")) (property "Value" "U" (at 0 0 0) (layer "F.Fab"))
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))
  (footprint "L:A" (layer "F.Cu") (tstamp 11111111-1111-1111-1111-111111111111) (at 20 10)
    (property "Reference" "U2" (at 0 0 0) (layer "F.SilkS")) (property "Value" "U" (at 0 0 0) (layer "F.Fab"))
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))
  (footprint "L:A" (layer "F.Cu") (uuid "11111111-1111-1111-1111-111111111111") (at 30 10)
    (property "Reference" "U3" (at 0 0 0) (layer "F.SilkS")) (property "Value" "U" (at 0 0 0) (layer "F.Fab"))
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))
)"#;
        let (design, model, _) = crate::import_kicad_pcb(board).unwrap();
        let uuid_of = |out: &str, reference: &str| -> String {
            let start = out.find(&format!("(property \"Reference\" \"{reference}\"")).unwrap_or_else(|| panic!("{reference}"));
            let head = out[..start].rfind("\t(footprint ").expect("footprint start");
            let line = out[head..start].lines().find(|l| l.trim_start().starts_with("(uuid ")).expect("uuid line");
            line.trim().trim_start_matches("(uuid \"").trim_end_matches("\")").to_string()
        };
        let out = export_kicad_pcb(&design, &model, &meta()).unwrap();
        assert_eq!(uuid_of(&out, "U1"), "00000000-0000-0000-0000-00005e384273");
        assert_eq!(uuid_of(&out, "U2"), "11111111-1111-1111-1111-111111111111", "a tstamp is the uuid of an older file");
        assert_ne!(uuid_of(&out, "U3"), "11111111-1111-1111-1111-111111111111", "the second claimant gets a uuid of its own");
        assert_eq!(out.matches("(uuid \"11111111-1111-1111-1111-111111111111\")").count(), 1);
        // Written again from what was read again, the same.
        let (again, again_model, _) = crate::import_kicad_pcb(&out).unwrap();
        assert_eq!(uuid_of(&export_kicad_pcb(&again, &again_model, &meta()).unwrap(), "U1"), "00000000-0000-0000-0000-00005e384273");
    }
}

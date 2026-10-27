//! `import_kicad_pcb` -- the inverse of [`crate::pcb::export_kicad_pcb`]:
//! reads a real `.kicad_pcb` file (ours or KiCad's own) into
//! `Design`+`ConstraintModel`.
//!
//! This is a port of the fields `pcbnew/pcb_io/kicad_sexpr/
//! pcb_io_kicad_sexpr_parser.cpp` extracts that our model can hold: board
//! outline (Edge.Cuts), footprints (reference, position, rotation, side)
//! and their pads (number, position, size, shape, drill, net), nets,
//! tracks (segments and arcs), vias, the copper layer stackup and the
//! board's default design rules (from its `"Default"` net class, which is
//! where KiCad 6+ actually stores track width/clearance/via size -- see
//! `parseNETCLASS`, not `parseSetup`).
//!
//! Unlike the real parser, this one does not enumerate every token KiCad's
//! grammar accepts: [`crate::sexpr`] gives a generic tree, and `find`/
//! `find_all` only look for the tags we know what to do with. A field we
//! have not ported (padstacks, teardrops, plot params, embedded fonts, ...)
//! is simply invisible rather than a parse error, which is what a reader
//! for a format we do not fully own should do -- a newer KiCad adding a
//! field must not break every board written since.
//!
//! Pads carry every feature `Pad` models: number (not necessarily unique --
//! a connector's shield tab is commonly several physical pads on one pin),
//! plated/non-plated/smd kind, round or slot (oval) drill, a rotation of
//! their own relative to their footprint, and a roundrect ratio. A
//! non-plated hole (`np_thru_hole`) is kept as a pad (mechanical, no net,
//! no schematic pin) rather than dropped, so it still counts as a hole for
//! clearance.
//!
//! What real boards carry that this does not import, and why (see the
//! task's report for proposed model shapes):
//! - **Footprint-level zones**: imported as zones tagged with `parent_footprint`.
//!   Board-level zones (pours, teardrops, rule areas) are imported by
//!   [`import_zones`]; their stored fills are not read, fills are derived.
//! - **The board outline** (Edge.Cuts) is built the way KiCad builds it (`eda_drc::outline`:
//!   `ConvertOutlineToPolygon`). A board whose Edge.Cuts are lines that chain into one closed loop gets
//!   that loop as `placement.outline`, as before. Anything else -- arcs, circles, rectangles, polygons,
//!   curves, cutouts, several loops, an outline that does not close -- stays what it is: every item is a
//!   [`eda_model::ir::Shape`] on layer `Edge.Cuts` ([`eda_model::ir::DrawingsSection::outline_is_shapes`]),
//!   the writer emits the same shapes, and `placement.outline` is only the summary of the outline they
//!   make. A footprint's own Edge.Cuts graphics stay in the footprint (its art). Nothing is closed that was not
//!   closed: what KiCad reports as a malformed outline is in [`ImportNotes::outline_errors`]. A track
//!   `(arc ...)` is kept as an arc ([`eda_model::ir::Track::new_arc`], counted in
//!   [`ImportNotes::track_arcs_kept`], and written back as an arc), and a board-level `gr_arc` is a
//!   [`eda_model::ir::Shape::Arc`], exactly (KiCad's own three-point arc storage is our `Arc`'s storage
//!   too).
//! - **Pad shapes** are all kept (`crate::pad_shape`): a trapezoid with its delta, a chamfered rectangle with its ratio and corners,
//!   a custom pad with its anchor, clearance mode and primitives, and the copper's offset from the pad's position. A padstack that
//!   differs between layers (`(padstack (mode front_inner_back) ..)`) is read as its front shape.
//! - **Board-level graphics and text** (`gr_line`/`gr_rect`/`gr_circle`/
//!   `gr_poly`/`gr_arc`/`gr_curve`/`gr_text`): imported into `design.drawings` as
//!   [`eda_model::ir::Shape`]/[`eda_model::ir::Text`] (see
//!   [`import_drawings`]). Those on Edge.Cuts are the board outline: see above.
//! - **Not imported at all**: barcodes, generator objects, and the
//!   project-level component classes and tuning profiles (a board that
//!   depends on them is judged differently by kicad-cli once re-exported;
//!   `docs/parity/REPORT.md` lists the boards). Groups, dimensions, locks,
//!   the stackup, net ties, 3D model references and footprint-local
//!   graphics and text are read.

use eda_model::units::mm;
use std::collections::BTreeMap;

use eda_model::units::Nm;
use eda_model::ir::{Design, DrawingsSection, FillMode, FootprintExtra, FootprintGraphic, FootprintInstance, FootprintText, PadMaskInfo, ViaTenting, IslandRemovalMode, PadConnection, Point, PlacementSection, Provenance, RoutingSection, Shape, Side, Text, Track, Via, Zone};
use eda_model::layers::{LayerSetup, LayerType};
use eda_model::outline as edge;
use eda_model::{BoardRules, CheckResult, ConstraintModel, Footprint, Net, NetClass, Pad, PadKind, PadShape, Part, Pin, PinKind};

use crate::import_items::{self, ItemRef, Refs};
use crate::sexpr::{self, Sexpr};

/// Counts of things the importer saw but could not carry into our model
/// exactly, or at all -- meant to be printed, never silently absorbed.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportNotes {
    pub zones_skipped: usize,
    /// Footprints whose `(transform ... (scale sx sy))` is not 1:1 (KiCad 10, [`crate::fp_transform`]): the model has no footprint scale, so they are
    /// read at 1:1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footprints_scaled: Vec<String>,
    /// Always 0 now: an Edge.Cuts arc is kept as an arc ([`Self::outline_shapes`]), not tessellated into the outline's straight segments.
    /// (Kept so an `ImportNotes` written by an older build still reads.)
    pub track_arcs_approximated: usize,
    /// Track `(arc ...)`s kept as arcs (`Track::arc_mid_offset`); informational, nothing was lost.
    #[serde(default)]
    pub track_arcs_kept: usize,
    /// Always 0 now: a trapezoid and a custom pad are kept as they are ([`crate::pad_shape`]). (Kept so an `ImportNotes` written by an
    /// older build still reads.)
    pub non_rect_pad_shapes_approximated: usize,
    /// A `(pad ...)` whose own `(layers ...)` list names no copper layer at
    /// all (no `F.Cu`/`B.Cu`/`*.Cu`/inner `.Cu`) -- not a pad of the model's footprint rather
    /// than a phantom copper pad; it is kept as the file wrote it (`FootprintExtra::aux_pads`, and a mask
    /// aperture also as a mask graphic for the DRC) and written back, so the stencil keeps its openings. Real boards use this for a
    /// paste-stencil-only or mask-only auxiliary "pad" (a generator's way to
    /// carry extra paste/mask geometry, e.g. a thermal pad split into
    /// solder-paste quadrants for a QFN/SON "PullBack" footprint): it has no
    /// electrical role and, critically, no copper to collide with anything,
    /// so flashing it on the footprint's copper layer anyway (this crate's
    /// old, kind-only convention: every `smd` pad flashes on `F.Cu`/`B.Cu`)
    /// fabricated foreign-looking copper that doesn't exist, which was
    /// GAPS.md #3's third over-firing source (confirmed on `issue11814`'s
    /// U4, a WSON-8-1EP: its thermal pad "9" is real `F.Cu`, but the four
    /// unnumbered `F.Paste`-only quadrant pads sitting on top of it are not,
    /// and were being tested for clearance/shorting against pad 9 and a
    /// nearby via as if they were).
    #[serde(default)]
    pub non_copper_pads_skipped: usize,
    /// How the board outline was reconstructed: "lines" (`gr_line`s chained into one closed loop, which is `placement.outline`),
    /// "shapes" (every Edge.Cuts item kept as a shape: [`eda_model::ir::DrawingsSection::outline_is_shapes`]), or "none" (no Edge.Cuts).
    #[serde(default)]
    pub outline_source: &'static str,
    /// The Edge.Cuts did not chain into closed outlines (a gap wider than the chaining epsilon, a stray segment): the outline is
    /// malformed, the items are kept as they are, and `placement.outline` is the rectangle round them.
    #[serde(default)]
    pub outline_open: bool,
    /// How many Edge.Cuts items were kept as shapes (`outline_source == "shapes"`).
    #[serde(default)]
    pub outline_shapes: usize,
    /// What KiCad would report as `invalid_outline` ("Board has malformed outline") for this board, one line per finding with the
    /// place it is at, mm (`eda_drc::outline::check_edge_cuts`).
    #[serde(default)]
    pub outline_errors: Vec<String>,
    /// The kinds of board item the file has and this importer has no reader for (`gr_text_box`, `image`, `table`, a board-level `property`, `target`, ..), and how many of
    /// each: they are not in the design and not in what is exported from it. Nothing is listed for a kind that is read.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unread_items: BTreeMap<String, usize>,
    /// What was wrong in the `(embedded_files ..)` of the board (a file with no name, a type KiCad does not know): the files are read as far as they go, and these are what
    /// KiCad would have stopped at. A checksum that does not match is not found here: that is told when the file is looked at (`crate::embedded::decode`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub embedded_problems: Vec<String>,
    /// The `(filled_polygon ..)` the file stored for its zones (KiCad's own fill, as saved): not part of the design. The caller that keeps the board files
    /// (`eda_kicad::zone_fills::entries_of_imported_file`) files them beside `design.json`, so an imported board shows the fills it was saved with.
    #[serde(skip)]
    pub zone_fills: Vec<crate::zone_fills::ScannedFill>,
    /// The id each item of the file has in the design, by the uuid the file gave it (`(uuid ..)`, `(tstamp ..)`): footprints, pads (`REF.NUM`), tracks, vias, zones,
    /// shapes, texts and the other items. What a project's DRC exclusions name their items by (`eda_kicad::drc_exclusions::from_project`).
    #[serde(skip)]
    pub item_ids: BTreeMap<String, String>,
}

/// Parse a `.kicad_pcb` file's text into our own design + constraint
/// model. See the module docs for exactly what is ported vs. approximated
/// vs. dropped (reported in the returned [`ImportNotes`]).
pub fn import_kicad_pcb(text: &str) -> Result<(Design, ConstraintModel, ImportNotes), Vec<CheckResult>> {
    let mut tree = sexpr::parse(text)
        .map_err(|e| vec![CheckResult::fail("kicad_import.parse", "file", format!("not a valid s-expression file: {e}"))])?;
    // KiCad 10's footprint `(transform ...)` and library-frame footprint zones, rewritten into the layout the readers below take (`fp_transform`).
    let footprints_scaled = crate::fp_transform::normalize(&mut tree);
    // KiCad 10's bold text, whose thickness is the width before the bold factor, in the older meaning every reader here has (`bold_text`).
    crate::bold_text::normalize(&mut tree);
    let root = tree
        .as_list()
        .filter(|l| sexpr::tag(l) == Some("kicad_pcb"))
        .ok_or_else(|| vec![CheckResult::fail("kicad_import.not_a_board", "file", "top-level form is not (kicad_pcb ...); not a KiCad PCB file")])?;

    let mut notes = ImportNotes { footprints_scaled, unread_items: unread_items(root), ..ImportNotes::default() };
    // The items nothing here reads are kept as text and written back (`DrawingsSection::raw_items`).
    let mut raw_items: Vec<String> = root.iter().skip(1).filter_map(Sexpr::as_list).filter(|l| sexpr::tag(l).is_some_and(|t| !is_read(l, t))).map(|l| sexpr::to_text(&Sexpr::List(l.to_vec()))).collect();

    let layers = import_layers(root);
    let net_names = import_net_names(root);
    let mut board = import_board_rules(root, &layers);

    // Each item parser records the uuid and lock of what it emits (`Refs`), so the groups and locks of the
    // file can be turned into ids once the items have them.
    let mut refs = Refs::default();
    let (footprints_ir, parts, explicit_footprints, pin_nets, footprint_extras, footprint_edits) = import_footprints(root, &net_names, &layers, &mut notes, &mut refs)?;
    let (tracks, track_masks, vias, via_tenting, via_extras) = import_routing(root, &net_names, &mut notes, &mut refs);
    let (mut shapes, texts, text_extras) = import_drawings(root, &net_names, &mut refs);
    // The Edge.Cuts shapes just read are the outline's items, or, when they are a plain closed loop of lines, that loop.
    let (outline, outline_is_shapes, outline_width_nm) = import_outline(&mut shapes, &mut refs, &mut notes);
    let dimensions = import_items::import_dimensions(root, &mut refs);
    let barcodes = crate::barcode_io::read_all(root, &mut refs.barcodes);
    let board_items = crate::pcb_items_io::read_all(root, &mut refs.board_items);
    // Tables (`PCB_TABLE`): the ones this model can hold as tables, and the text of the others, kept as they were.
    let (mut tables, raw_tables) = crate::pcb_table::read_all(root, &mut refs.tables);
    if !raw_tables.is_empty() {
        *notes.unread_items.entry("table".to_string()).or_insert(0) += raw_tables.len();
        raw_items.extend(raw_tables);
    }
    // KiCad 10's drill charts: tables of their own, after the board's other tables, and what makes each a chart (named by its table's id once the ids are given).
    let (charts, raw_charts) = crate::drill_chart_io::read_charts(root, &mut refs.tables);
    if !raw_charts.is_empty() {
        *notes.unread_items.entry("drill_chart".to_string()).or_insert(0) += raw_charts.len();
        raw_items.extend(raw_charts);
    }
    let first_chart_table = tables.len();
    let mut chart_overlays = Vec::new();
    for (t, c) in charts {
        tables.push(t);
        chart_overlays.push(c);
    }
    let silk_texts = import_board_silk_texts(root);
    let copper_texts = import_board_copper_texts(root);
    let zones = import_zones(root, &net_names, &layers, &mut notes, &mut refs);
    notes.zone_fills = crate::zone_fills::scan_zone_fills(text);
    // The board's nets: those the file lists and every net an item names -- a pad, but also a track, a via or a zone (`PCB_IO_KICAD_SEXPR_PARSER::parseNet` makes a net of any
    // `(net "name")` it meets, and a KiCad 10 file lists none: a net that only copper uses, a pour with no pad on it, is a net of the board all the same).
    let copper_nets: Vec<&str> = tracks.iter().map(|t| t.net.as_str()).chain(vias.iter().map(|v| v.net.as_str())).chain(zones.iter().map(|z| z.net.as_str())).chain(refs.shapes.iter().filter_map(|r| r.net.as_deref())).collect();
    let nets = build_nets(&net_names, &pin_nets, &copper_nets);

    add_layers_in_use(
        &mut board.layer_setup,
        shapes.iter().map(|s| s.layer().to_string()).chain(texts.iter().map(|t| t.layer.clone())).chain(dimensions.iter().map(|d| d.layer.clone())).chain(silk_texts.iter().map(|t| t.layer.clone())).chain(board_items.text_boxes.iter().map(|t| t.layer.clone())).chain(board_items.images.iter().map(|i| i.layer.clone())).chain(board_items.points.iter().map(|p| p.layer.clone())),
    );

    // The outline override lives on `board` too (used when a downstream
    // tool re-derives placement); keep it in step with what we actually
    // found on Edge.Cuts.
    if outline.len() >= 3 {
        board.outline = Some(outline.clone());
    }
    // `BoardRules::outline_closed`: whether the Edge.Cuts chained into closed outlines (`ImportNotes::outline_open`, negated); no
    // Edge.Cuts at all leaves this `None` (nothing to assert either way).
    board.outline_closed = match notes.outline_source {
        "lines" | "shapes" => Some(!notes.outline_open),
        _ => None,
    };

    let mut design = Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            intent_hash: blake3::hash(text.as_bytes()).to_hex().to_string(),
            seed: 0,
            stage_hashes: vec![],
        },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline, footprints: footprints_ir, modules: vec![] }),
        routing: if tracks.is_empty() && vias.is_empty() && zones.is_empty() { None } else { Some(RoutingSection { tracks, vias, zones, track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }) },
        drawings: if shapes.is_empty() && texts.is_empty() && dimensions.is_empty() && footprint_extras.is_empty() && via_tenting.is_empty() && copper_texts.is_empty() && footprint_edits.is_empty() && outline_width_nm.is_none() && barcodes.is_empty() && board_items.is_empty() { None } else { Some(DrawingsSection { shapes, texts, dimensions, footprint_extras, via_tenting, silk_texts, copper_texts, footprint_edits, outline_is_shapes, outline_width_nm, barcodes, board_items, ..Default::default() }) },
        footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
    };
    if !raw_items.is_empty() {
        design.drawings.get_or_insert_with(Default::default).raw_items = raw_items;
    }
    if !tables.is_empty() {
        design.drawings.get_or_insert_with(Default::default).tables = tables;
    }
    // The files embedded in the board, with their data as read: `(embedded_fonts ..)` and `(embedded_files ..)` (`PCB_IO_KICAD_SEXPR_PARSER`, `EMBEDDED_FILES_PARSER::ParseEmbedded`).
    let embedded = crate::embedded::read_owner(root, &mut notes.embedded_problems);
    if !embedded.is_empty() {
        design.drawings.get_or_insert_with(Default::default).embedded = embedded;
    }
    // The vias' protection defaults of KiCad 10's `(setup ..)`.
    let setup_tokens: Vec<String> = sexpr::find(root, "setup").map(|s| s.iter().skip(1).filter_map(Sexpr::as_list).filter(|l| matches!(sexpr::tag(l), Some("covering") | Some("plugging") | Some("capping") | Some("filling"))).map(|l| sexpr::to_text(&Sexpr::List(l.to_vec()))).collect()).unwrap_or_default();
    if !setup_tokens.is_empty() {
        design.drawings.get_or_insert_with(Default::default).setup_tokens = setup_tokens;
    }
    // `(setup (aux_axis_origin x y))`: the drill/place file origin the board was saved with.
    if let Some(aux) = sexpr::find(root, "setup").and_then(|s| sexpr::find(s, "aux_axis_origin")) {
        if let (Some(x), Some(y)) = (sexpr::num(aux, 1), sexpr::num(aux, 2)) {
            let at = Point { x: mm_to_nm(x), y: mm_to_nm(y) };
            if at.x != Nm::ZERO || at.y != Nm::ZERO {
                design.drawings.get_or_insert_with(Default::default).aux_origin = Some(at);
            }
        }
    }
    // `(setup (grid_origin x y))`: the point the editing grid was anchored at.
    if let Some(grid) = sexpr::find(root, "setup").and_then(|s| sexpr::find(s, "grid_origin")) {
        if let (Some(x), Some(y)) = (sexpr::num(grid, 1), sexpr::num(grid, 2)) {
            let at = Point { x: mm_to_nm(x), y: mm_to_nm(y) };
            if at.x != Nm::ZERO || at.y != Nm::ZERO {
                design.drawings.get_or_insert_with(Default::default).grid_origin = Some(at);
            }
        }
    }
    // `(paper ...)` and `(title_block ...)`: the board's Page Settings. A4 landscape and an empty title block are the defaults and are not kept.
    if let Some(page) = crate::page::import_page(root).filter(|p| !p.is_default()) {
        design.drawings.get_or_insert_with(Default::default).page = Some(page);
    }
    // (The schematic's title block carries its sheet's paper name too; the board's paper is `page`, so the name is dropped here.)
    if let Some(tb) = crate::sch_import::import_title_block(root).map(|tb| eda_model::ir::TitleBlock { paper: String::new(), ..tb }).filter(|tb| *tb != eda_model::ir::TitleBlock::default()) {
        design.drawings.get_or_insert_with(Default::default).title_block = Some(tb);
    }
    // Every track/via this parse just built, and every shape/text, has no
    // id yet (the file does not carry ours) -- assign the same
    // deterministic ids a fresh route or a hand-add would get, so an
    // imported board is addressable from the moment it lands.
    design.assign_missing_ids();
    // The shapes' hatch (`(fill hatch)` and the others), line endings and nets, named by the ids the shapes have just been given.
    if let Some(dr) = design.drawings.as_mut() {
        let styles: Vec<eda_model::pcb_items::ShapeStyle> = dr.shapes.iter().zip(&refs.shapes).filter_map(|(s, r)| r.shape_style(s.id())).collect();
        dr.board_items.shape_styles = styles;
    }
    // The tracks' solder-mask openings, named by the ids the tracks have just been given.
    let track_masks: Vec<eda_model::track_mask::TrackMask> = design.routing.iter().flat_map(|r| r.tracks.iter()).zip(track_masks).filter_map(|(t, m)| m.map(|m| eda_model::track_mask::TrackMask { id: t.id.clone(), ..m })).collect();
    if !track_masks.is_empty() {
        design.drawings.get_or_insert_with(Default::default).track_masks = track_masks;
    }
    // The vias' tokens beyond `Via`'s fields, named by the ids the vias have just been given.
    let via_extras: Vec<eda_model::via_extra::ViaExtra> = design.routing.iter().flat_map(|r| r.vias.iter()).zip(via_extras).filter(|(_, x)| !x.is_default()).map(|(v, mut x)| { x.id = v.id.clone(); x }).collect();
    if !via_extras.is_empty() {
        design.drawings.get_or_insert_with(Default::default).via_extras = via_extras;
    }
    // The texts' looks beyond `Text`'s fields, named by the ids the texts have just been given.
    if let Some(dr) = design.drawings.as_mut() {
        dr.text_extras = dr.texts.iter().zip(text_extras).filter(|(_, x)| !x.is_default()).map(|(t, mut x)| { x.id = t.id.clone(); x }).collect();
    }
    // The drill charts, named by the ids their tables have just been given.
    if !chart_overlays.is_empty() {
        let dr = design.drawings.get_or_insert_with(Default::default);
        for (i, mut c) in chart_overlays.into_iter().enumerate() {
            c.table_id = dr.tables[first_chart_table + i].id.clone();
            dr.board_items.drill_charts.push(c);
        }
    }
    // KiCad 10's via stitchings (`(generated (type via_stitch) ..)`), their members named by the ids the vias have just been given.
    crate::via_stitch_io::attach(&mut design, root, &mut refs);
    // The file's groups and locks, now that every item has its id: a `(group ..)` names its members by file
    // uuid, and `locked` rides on each item (`BOARD_ITEM::IsLocked()`).
    let (groups, locked_ids) = refs.resolve(&design, &import_items::import_groups(root));
    for (uuid, id) in refs.item_ids(&design) {
        notes.item_ids.entry(uuid).or_insert(id);
    }
    if !groups.is_empty() || !locked_ids.is_empty() {
        let dr = design.drawings.get_or_insert_with(Default::default);
        dr.groups = groups;
        dr.locked_ids = locked_ids;
        dr.assign_missing_ids();
    }
    // `(generated ..)`: the tuning patterns, named by the same file uuids.
    crate::pcb_generated::attach_patterns(&mut design, root, &refs);
    let mut model = ConstraintModel { schema: eda_model::INTENT_SCHEMA_VERSION,
        parts,
        nets,
        intent_net_names: Default::default(),
        symbol_links: Default::default(),
        clusters: vec![],
        placement_rules: vec![],
        stackup: import_stackup(root),
        impedance_targets: vec![],
        footprints: explicit_footprints.into_values().collect(),
        instance_footprints: Default::default(),
        edited_models: Default::default(),
        installed_art: Default::default(),
        installed_footprints: Default::default(),
        symbols: vec![],
        symbol_sim_fields: Default::default(),
        board,
        allow: Default::default(),
        solver: Default::default(),
    };
    // The summary of an outline made of shapes, now that the shapes have their styles (a rectangle's corner radius) and the model holds the footprints' own Edge.Cuts
    // (a card-edge tab a footprint closes): `BuildBoardPolygonOutlines` with its footprint pass.
    eda_drc::outline::refresh_outline_summary(&mut design, Some(&model));
    // The footprints' own Edge.Cuts (their art) take part in the outline, through `BuildBoardPolygonOutlines`' footprint pass: what the notes say of the outline is
    // then the check of the whole, as kicad-cli's `testOutline` makes it.
    if !eda_drc::outline::footprint_edge_cuts_shapes(&design, &model).is_empty() {
        let built = eda_drc::outline::board_outline_with(&design, Some(&model), true);
        notes.outline_open = !built.valid;
        notes.outline_errors = eda_drc::outline::check_board_outline_with(&design, Some(&model))
            .iter()
            .map(|e| format!("{} at ({:.3}, {:.3}) mm", e.describe(), eda_model::units::to_mm(e.at.x), eda_model::units::to_mm(e.at.y)))
            .collect();
        if notes.outline_source != "none" {
            model.board.outline_closed = Some(!notes.outline_open);
        }
    }
    Ok((design, model, notes))
}

/// The kinds of top-level item a reader here takes: everything `import_kicad_pcb` reads below, the header (`version`, `generator`, `general`, `paper`, `title_block`, `layers`,
/// `setup`, nets and net classes), `embedded_fonts` (a flag) and `embedded_files` (`DrawingsSection::embedded`). The other kinds are counted ([`unread_items`]) and kept as text (`DrawingsSection::raw_items`).
const READ_ITEMS: &[&str] = &[
    "version", "generator", "generator_version", "general", "paper", "page", "title_block", "layers", "setup", "host", "net", "net_class", "footprint", "module", "segment", "arc", "via", "zone", "gr_line", "gr_arc",
    "gr_rect", "gr_circle", "gr_poly", "gr_curve", "gr_text", "gr_text_box", "image", "point", "dimension", "group", "generated", "barcode", "embedded_fonts",
    "table", "embedded_files", "grid_item", "drill_chart", "drill_map",
];

/// Whether a reader here takes the top-level `item` of kind `tag`: its kind is in [`READ_ITEMS`], a reference image is one whose data is a file the reader understands
/// (an image that is not goes on as text, as KiCad would not have loaded the board at all), and a generator is a tuning pattern or a via stitching.
fn is_read(item: &[Sexpr], tag: &str) -> bool {
    READ_ITEMS.contains(&tag)
        && (tag != "image" || crate::pcb_image::read(item).is_some())
        // A generator of a kind nothing here reads (KiCad 10's microvia stack, `via_stack`) goes on as text, as an unknown item does.
        && (tag != "generated" || matches!(sexpr::find(item, "type").and_then(|t| sexpr::txt(t, 1)), Some("tuning_pattern") | Some("via_stitch")))
}

/// The top-level items of the board whose kind no reader here takes ([`READ_ITEMS`]), counted by kind.
fn unread_items(root: &[Sexpr]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for item in root.iter().skip(1).filter_map(Sexpr::as_list) {
        if let Some(tag) = sexpr::tag(item).filter(|t| !is_read(item, t)) {
            *out.entry(tag.to_string()).or_insert(0) += 1;
        }
    }
    out
}

/// A board length in the file's millimetres to nanometres, as KiCad reads it (`PCB_IO_KICAD_SEXPR_PARSER::parseBoardUnits`: [`eda_model::units::from_mm`]).
pub fn mm_to_nm(mm: f64) -> eda_model::ir::Nm {
    eda_model::units::from_mm(mm)
}

/// A schematic length in the file's millimetres to nanometres on KiCad's 100 nm schematic unit (`SCH_IO_KICAD_SEXPR_PARSER::parseInternalUnits`:
/// [`eda_model::units::sch_from_mm`]).
pub fn sch_mm_to_nm(mm: f64) -> eda_model::ir::Nm {
    eda_model::units::sch_from_mm(mm)
}

/// KiCad's `(at x y angle)` rotates footprints clockwise for positive
/// angle (screen convention, y already pointing down) -- the opposite
/// sense from `eda_model::footprint::to_board`'s rotation matrix, exactly
/// as noted where the exporter negates it (see `pcb.rs::write_footprint`).
/// Importing is the same negation run backwards.
pub(crate) fn import_rot_millideg(file_deg: f64) -> u32 {
    let md = (-file_deg * 1000.0).round() as i64;
    md.rem_euclid(360_000) as u32
}

pub(crate) fn xy_point(list: &[Sexpr]) -> Option<Point> {
    let (x, y) = (sexpr::num(list, 1)?, sexpr::num(list, 2)?);
    Some(Point { x: mm_to_nm(x), y: mm_to_nm(y) })
}

// ---------------------------------------------------------------- layers

/// Copper layer names, outer first. KiCad's own parser collects layer
/// entries until the first one whose type is not a copper type (`signal`,
/// `power`, `mixed`, `jumper`) and ignores the layer *numbers* in the
/// file entirely -- position in the list is the stackup order (see
/// `PCB_IO_KICAD_SEXPR_PARSER::parseLayers`). We do the same.
fn import_layers(root: &[Sexpr]) -> Vec<String> {
    let Some(layers) = sexpr::find(root, "layers") else { return default_layers() };
    let mut out = Vec::new();
    for item in &layers[1..] {
        let Some(l) = item.as_list() else { continue };
        let Some(name) = sexpr::txt(l, 1) else { continue };
        let ltype = sexpr::txt(l, 2).unwrap_or("");
        if matches!(ltype, "signal" | "power" | "mixed" | "jumper") {
            out.push(name.to_string());
        } else {
            break;
        }
    }
    if out.len() >= 2 {
        out
    } else {
        default_layers()
    }
}

fn default_layers() -> Vec<String> {
    vec!["F.Cu".into(), "B.Cu".into()]
}

/// The layers every board this studio wrote had listed, whatever the file says: its adhesive, paste, silkscreen, mask and fabrication layers, the courtyards and the outline.
const ALWAYS_ENABLED: [&str; 13] = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Edge.Cuts", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab"];

/// `(layers ..)` as the Board Editor Layers page holds it (`PCB_IO_KICAD_SEXPR_PARSER::parseLayers` / `parseLayer`): the non-copper layers the file lists, the name a layer is
/// shown by (the entry's last word) and the type of the copper layers and of the user layers off-board at the front or the back. The page's mandatory layers (the courtyards, the
/// outline, the margin) and the ones this studio has always drawn are on whatever the file lists, so a board written before there was a page keeps its silkscreen and fabrication;
/// a layer name the file has that is not one of KiCad's (`Rescue`, a name from before KiCad 5) is skipped. [`add_layers_in_use`] adds the layers the imported items sit on.
fn import_layer_setup(root: &[Sexpr]) -> LayerSetup {
    let mut setup = LayerSetup { enabled: Vec::new(), names: BTreeMap::new(), types: BTreeMap::new() };
    for item in sexpr::find(root, "layers").map_or(&[][..], |l| &l[1..]) {
        let Some(l) = item.as_list() else { continue };
        let (Some(name), Some(word)) = (sexpr::txt(l, 1), sexpr::txt(l, 2)) else { continue };
        let ty = LayerType::parse(word);
        if eda_model::layers::is_copper_layer_name(name) {
            if let Some(t) = ty.filter(|t| t.is_copper_type()) {
                setup.types.insert(name.to_string(), t);
            }
        } else if eda_model::layers::is_optional_layer(name) {
            setup.enabled.push(name.to_string());
            if let Some(t) = ty.filter(|t| !t.is_copper_type() && eda_model::layers::user_layer_number(name).is_some()) {
                setup.types.insert(name.to_string(), t);
            }
        } else {
            continue;
        }
        if let Some(shown) = sexpr::txt(l, 3).filter(|s| *s != "hide" && !s.is_empty() && *s != eda_model::layers::standard_layer_name(name)) {
            setup.names.insert(name.to_string(), shown.to_string());
        }
    }
    setup.enabled.extend(ALWAYS_ENABLED.iter().chain(eda_model::layers::MANDATORY_LAYERS.iter()).map(|l| l.to_string()));
    setup.canonical()
}

/// A layer an imported item is on is a layer the board has: a file that lists fewer layers than its items use (a minimal fixture, a board from another tool) does not lose them.
fn add_layers_in_use(setup: &mut LayerSetup, layers: impl Iterator<Item = String>) {
    let before = setup.enabled.len();
    setup.enabled.extend(layers.filter(|l| eda_model::layers::is_optional_layer(l)));
    if setup.enabled.len() != before {
        *setup = setup.canonical();
    }
}

// ------------------------------------------------------------------ nets

fn import_net_names(root: &[Sexpr]) -> BTreeMap<i64, String> {
    let mut map = BTreeMap::new();
    for n in sexpr::find_all(root, "net") {
        if let (Some(code), Some(name)) = (sexpr::num(n, 1), sexpr::txt(n, 2)) {
            map.insert(code as i64, name.to_string());
        }
    }
    map
}

/// An item's `(net ...)`: legacy `(net N)`/`(net N "name")` (the code is
/// authoritative) or KiCad 10's `(net "name")` -- `parsePAD`'s / `parseTRACK`'s
/// `T_net` handling.
fn net_ref(net: Option<&[Sexpr]>, net_names: &BTreeMap<i64, String>) -> String {
    let Some(n) = net else { return String::new() };
    net_of(n, net_names)
}

/// The net a `(net ..)` form names. The reader of s-expressions does not keep whether an argument was quoted, and `PCB_IO_KICAD_SEXPR_PARSER::parseNet` goes by that: a number
/// is a net code, a string is a name. What tells them apart here is the file: a legacy file lists its nets at the top (`(net 0 "")` at least, always), so a number is a
/// code; a KiCad 10 file lists none, and its one argument is a name even when it is made of digits -- `(net "0")` is the net a SPICE netlist calls ground.
fn net_of(n: &[Sexpr], net_names: &BTreeMap<i64, String>) -> String {
    if net_names.is_empty() && n.len() == 2 {
        return sexpr::txt(n, 1).unwrap_or("").to_string();
    }
    match sexpr::num(n, 1) {
        Some(code) => net_names.get(&(code as i64)).cloned().or_else(|| sexpr::txt(n, 2).map(str::to_string)).unwrap_or_default(),
        None => sexpr::txt(n, 1).unwrap_or("").to_string(),
    }
}

fn build_nets(net_names: &BTreeMap<i64, String>, pin_nets: &[(String, String)], copper_nets: &[&str]) -> Vec<Net> {
    let mut pins_by_name: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (pin, net) in pin_nets {
        pins_by_name.entry(net.as_str()).or_default().push(pin.clone());
    }
    // KiCad 10 files carry no top-level net table: every name used by a pad
    // is a net too.
    let mut names: Vec<&str> = net_names.values().map(String::as_str).collect();
    names.extend(pin_nets.iter().map(|(_, n)| n.as_str()));
    names.extend(copper_nets.iter().copied());
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .filter(|n| !n.is_empty())
        .map(|n| Net { name: n.to_string(), pins: pins_by_name.get(n).cloned().unwrap_or_default() })
        .collect()
}

// -------------------------------------------------------------- board rules

/// Track width / clearance / via size, from the board's `"Default"` net
/// class (or its only one) -- see the module doc for why that is where
/// KiCad 6+ actually keeps these, not `(setup ...)`. Any other declared
/// net class becomes one of our `NetClass`es, matched by its literal
/// member list rather than a glob (an exact list is what KiCad wrote).
fn import_board_rules(root: &[Sexpr], layers: &[String]) -> BoardRules {
    // A board file is a board somebody judged with KiCad's rules: its minimums are KiCad's (the project's, or the factory
    // ones when it has none), stated -- not the unstated ones of an intent (`BoardRules::constraints_explicit`).
    let mut board = BoardRules { layers: layers.to_vec(), constraints_explicit: true, layer_setup: import_layer_setup(root), ..BoardRules::default() };
    // Legacy boards keep the copper-to-edge clearance in `(setup (edge_clearance ..))`.
    if let Some(v) = sexpr::find(root, "setup").and_then(|st| sexpr::find(st, "edge_clearance")).and_then(|e| sexpr::num(e, 1)) {
        board.copper_edge_clearance_nm = Some(mm_to_nm(v));
    }
    let classes: Vec<&[Sexpr]> = sexpr::find_all(root, "net_class").collect();
    let default_idx = classes.iter().position(|nc| sexpr::txt(nc, 1) == Some("Default")).or(if classes.len() == 1 { Some(0) } else { None });

    if let Some(i) = default_idx {
        let nc = classes[i];
        if let Some(v) = sexpr::find(nc, "trace_width").and_then(|f| sexpr::num(f, 1)) {
            board.track_width = mm_to_nm(v);
        }
        if let Some(v) = sexpr::find(nc, "clearance").and_then(|f| sexpr::num(f, 1)) {
            board.clearance = mm_to_nm(v);
        }
        if let Some(v) = sexpr::find(nc, "via_dia").and_then(|f| sexpr::num(f, 1)) {
            board.via_diameter = mm_to_nm(v);
        }
        if let Some(v) = sexpr::find(nc, "via_drill").and_then(|f| sexpr::num(f, 1)) {
            board.via_drill = mm_to_nm(v);
        }
        board.default_class = default_class_extras(nc);
    }

    board.net_classes = classes
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != default_idx)
        .filter_map(|(_, nc)| {
            let name = sexpr::txt(nc, 1)?.to_string();
            let nets: Vec<String> = sexpr::find_all(nc, "add_net").filter_map(|a| sexpr::txt(a, 1)).map(String::from).collect();
            if nets.is_empty() {
                return None; // an unused class matches nothing; not worth carrying
            }
            let track_width = sexpr::find(nc, "trace_width").and_then(|f| sexpr::num(f, 1)).map(mm_to_nm);
            let clearance = sexpr::find(nc, "clearance").and_then(|f| sexpr::num(f, 1)).map(mm_to_nm);
            let via_diameter = sexpr::find(nc, "via_dia").and_then(|f| sexpr::num(f, 1)).map(mm_to_nm);
            let via_drill = sexpr::find(nc, "via_drill").and_then(|f| sexpr::num(f, 1)).map(mm_to_nm);
            let extras = default_class_extras(nc);
            let (microvia_diameter, microvia_drill, diff_pair_width, diff_pair_gap) = extras.map_or((None, None, None, None), |e| (e.microvia_diameter, e.microvia_drill, e.diff_pair_width, e.diff_pair_gap));
            Some(NetClass { name, nets, track_width, clearance, via_diameter, via_drill, microvia_diameter, microvia_drill, diff_pair_width, diff_pair_gap, diff_pair_via_gap: None, priority: 0 })
        })
        .collect();
    import_legacy_setup_minimums(root, &mut board);
    import_solder_mask_setup(root, &mut board);
    import_formatting_and_hatch_offsets(root, &mut board);
    board
}

/// `(setup (pcbplotparams (dashed_line_dash_ratio ..) (dashed_line_gap_ratio ..)))` (Text & Graphics > Formatting) and `(setup (zone_defaults (property (layer ..) (hatch_position (xy ..)))))`
/// (Board Stackup > Zone Hatch Offsets): `parsePCB_PLOT_PARAMS`, `parseZoneLayerProperty`. A ratio that is not a positive number is left at the default; an offset of a layer the board has no copper on is dropped.
fn import_formatting_and_hatch_offsets(root: &[Sexpr], board: &mut BoardRules) {
    let Some(setup) = sexpr::find(root, "setup") else { return };
    let ratio = |p: &[Sexpr], key: &str| sexpr::find(p, key).and_then(|f| sexpr::num(f, 1)).filter(|v| v.is_finite() && *v > 0.0 && *v <= eda_model::setup_pages::MAX_DASH_RATIO);
    if let Some(plot) = sexpr::find(setup, "pcbplotparams") {
        if let Some(v) = ratio(plot, "dashed_line_dash_ratio") {
            board.formatting.dash_length_ratio = v;
        }
        if let Some(v) = ratio(plot, "dashed_line_gap_ratio") {
            board.formatting.gap_length_ratio = v;
        }
    }
    if let Some(defaults) = sexpr::find(setup, "zone_defaults") {
        for prop in sexpr::find_all(defaults, "property") {
            let (Some(layer), Some(xy)) = (sexpr::find(prop, "layer").and_then(|l| sexpr::txt(l, 1)), sexpr::find(prop, "hatch_position").and_then(|h| sexpr::find(h, "xy"))) else { continue };
            let (Some(x), Some(y)) = (sexpr::num(xy, 1), sexpr::num(xy, 2)) else { continue };
            let offset = eda_model::setup_pages::HatchOffset { x_nm: mm_to_nm(x), y_nm: mm_to_nm(y) };
            if !offset.is_zero() && board.layers.iter().any(|l| l == layer) && offset.x_nm.abs() <= eda_model::setup_pages::MAX_HATCH_OFFSET_NM && offset.y_nm.abs() <= eda_model::setup_pages::MAX_HATCH_OFFSET_NM {
                board.hatch_offsets.insert(layer.to_string(), offset);
            }
        }
    }
}

/// A `.kicad_pro`'s Board Setup pages that hold a handful of settings (`eda_model::setup_pages`): `board.design_settings.defaults.apply_defaults_to_fp_*` (Formatting),
/// `board.design_settings.tuning_pattern_settings` (Length-tuning Patterns), the top-level `tuning_profiles` and each net class's `tuning_profile` (Tuning Profiles). Absent
/// or unrecognised JSON is a no-op, as in the project's other readers; an entry the pages would refuse (a negative delay, a name with a control character) is left out.
pub fn merge_project_setup_pages(model: &mut eda_model::ConstraintModel, project_json: &str) {
    use eda_model::setup_pages::{CornerStyle, MeanderSettings, ProfileLayerEntry, ProfileType, TuningProfile, ViaOverride};
    let Ok(root) = serde_json::from_str::<serde_json::Value>(project_json) else { return };
    let b = &mut model.board;
    let settings = root.get("board").and_then(|b| b.get("design_settings"));
    if let Some(def) = settings.and_then(|s| s.get("defaults")) {
        let flag = |key: &str| def.get(key).and_then(|v| v.as_bool());
        let f = &mut b.formatting;
        if let Some(v) = flag("apply_defaults_to_fp_fields") {
            f.apply_to_fp_fields = v;
        }
        if let Some(v) = flag("apply_defaults_to_fp_text") {
            f.apply_to_fp_text = v;
        }
        if let Some(v) = flag("apply_defaults_to_fp_shapes") {
            f.apply_to_fp_shapes = v;
        }
        if let Some(v) = flag("apply_defaults_to_fp_dimensions") {
            f.apply_to_fp_dimensions = v;
        }
        if let Some(v) = flag("apply_defaults_to_fp_barcodes") {
            f.apply_to_fp_barcodes = v;
        }
    }
    if let Some(patterns) = settings.and_then(|s| s.get("tuning_pattern_settings")) {
        // `read_settings` starts from `PNS::MEANDER_SETTINGS()` whatever the group, so a key the file lacks is that default's.
        let read = |entry: Option<&serde_json::Value>, current: &MeanderSettings| -> MeanderSettings {
            let Some(e) = entry else { return current.clone() };
            let mut m = MeanderSettings::single_track();
            let um = |key: &str| e.get(key).and_then(|v| v.as_f64()).map(mm_to_nm);
            if let Some(v) = um("min_amplitude") {
                m.min_amplitude_nm = v;
            }
            if let Some(v) = um("max_amplitude") {
                m.max_amplitude_nm = v;
            }
            if let Some(v) = um("spacing") {
                m.spacing_nm = v;
            }
            if let Some(v) = e.get("corner_style").and_then(|v| v.as_i64()) {
                m.corner_style = CornerStyle::from_code(v);
            }
            if let Some(v) = e.get("corner_radius_percentage").and_then(|v| v.as_i64()) {
                m.corner_radius_percentage = v.clamp(0, 100) as u32;
            }
            if let Some(v) = e.get("single_sided").and_then(|v| v.as_bool()) {
                m.single_sided = v;
            }
            m
        };
        let next = eda_model::setup_pages::TuningPatterns {
            single_track: read(patterns.get("single_track_defaults"), &b.tuning_patterns.single_track),
            diff_pair: read(patterns.get("diff_pair_defaults"), &b.tuning_patterns.diff_pair),
            diff_pair_skew: read(patterns.get("diff_pair_skew_defaults"), &b.tuning_patterns.diff_pair_skew),
        };
        if next.validate().is_ok() {
            b.tuning_patterns = next;
        }
    }
    let layer_of = |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_str()).filter(|s| !s.is_empty() && *s != "UNDEFINED").map(String::from);
    if let Some(list) = root.get("tuning_profiles").and_then(|t| t.get("tuning_profiles_impedance_geometric")).and_then(|v| v.as_array()) {
        let mut profiles = Vec::new();
        for entry in list.iter().filter(|e| e.is_object() && e.get("profile_name").is_some()) {
            let int = |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_i64()).unwrap_or(0);
            let layer_entries = entry
                .get("layer_entries")
                .and_then(|v| v.as_array())
                .map(|rows| {
                    rows.iter()
                        .filter(|r| r.is_object())
                        .filter_map(|r| {
                            Some(ProfileLayerEntry {
                                signal_layer: layer_of(r, "signal_layer")?,
                                top_reference_layer: layer_of(r, "top_reference_layer"),
                                bottom_reference_layer: layer_of(r, "bottom_reference_layer"),
                                width_nm: Nm::new(int(r, "width")),
                                diff_pair_gap_nm: Nm::new(int(r, "diff_pair_gap")),
                                delay_as_per_mm: int(r, "delay"),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let via_overrides = entry
                .get("via_overrides")
                .and_then(|v| v.as_array())
                .map(|rows| {
                    rows.iter()
                        .filter(|r| r.is_object())
                        .filter_map(|r| Some(ViaOverride { signal_layer_from: layer_of(r, "signal_layer_from")?, signal_layer_to: layer_of(r, "signal_layer_to")?, via_layer_from: layer_of(r, "via_layer_from")?, via_layer_to: layer_of(r, "via_layer_to")?, delay_as: int(r, "delay") }))
                        .collect()
                })
                .unwrap_or_default();
            profiles.push(TuningProfile {
                name: entry.get("profile_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                profile_type: ProfileType::from_code(int(entry, "type")),
                target_impedance: entry.get("target_impedance").and_then(|v| v.as_f64()).unwrap_or(0.0),
                enable_time_domain_tuning: entry.get("enable_time_domain_tuning").and_then(|v| v.as_bool()).unwrap_or(false),
                layer_entries,
                via_propagation_delay_as: int(entry, "via_prop_delay"),
                via_overrides,
            });
        }
        // A profile the Tuning Profiles page would refuse is not carried (KiCad takes it; the studio keeps its page valid).
        let copper = b.layers.clone();
        let candidate = eda_model::setup_pages::TuningProfiles { profiles: profiles.clone(), class_profiles: b.tuning_profiles.class_profiles.clone() };
        if candidate.validate(&copper).is_ok() {
            b.tuning_profiles.profiles = profiles;
        } else {
            b.tuning_profiles.profiles = profiles.into_iter().filter(|p| eda_model::setup_pages::TuningProfiles { profiles: vec![p.clone()], class_profiles: Default::default() }.validate(&copper).is_ok()).collect();
        }
    }
    // Component Classes: the project's assignment rules and the sheet classes switch.
    if let Some(settings) = root.get("component_class_settings").filter(|s| s.is_object()) {
        b.component_classes = eda_model::component_classes::ComponentClassSettings::from_project_json(settings);
    }
    if let Some(classes) = root.get("net_settings").and_then(|n| n.get("classes")).and_then(|v| v.as_array()) {
        for class in classes {
            let (Some(name), Some(profile)) = (class.get("name").and_then(|v| v.as_str()), class.get("tuning_profile").and_then(|v| v.as_str())) else { continue };
            if !name.is_empty() && !profile.is_empty() && !profile.chars().any(|c| c.is_control()) && profile.len() <= eda_model::setup_pages::MAX_PROFILE_NAME {
                b.tuning_profiles.class_profiles.insert(name.to_string(), profile.to_string());
            }
        }
    }
}

/// A `(net_class ..)` block's microvia size and differential-pair sizes (`uvia_dia`, `uvia_drill`, `diff_pair_width`,
/// `diff_pair_gap`), as a class with only those set -- `None` when the class has none. KiCad's own 0.3 / 0.1 mm microvia
/// (what the writer gives a class that sets none) reads back as unset, so a board that never set one round-trips unchanged.
fn default_class_extras(nc: &[Sexpr]) -> Option<NetClass> {
    let nm = |key: &str| sexpr::find(nc, key).and_then(|f| sexpr::num(f, 1)).map(mm_to_nm);
    let microvia_diameter = nm("uvia_dia").filter(|v| *v != mm(0.3));
    let microvia_drill = nm("uvia_drill").filter(|v| *v != mm(0.1));
    let (diff_pair_width, diff_pair_gap) = (nm("diff_pair_width"), nm("diff_pair_gap"));
    if microvia_diameter.is_none() && microvia_drill.is_none() && diff_pair_width.is_none() && diff_pair_gap.is_none() {
        return None;
    }
    Some(NetClass { name: "Default".into(), nets: vec![], track_width: None, clearance: None, via_diameter: None, via_drill: None, microvia_diameter, microvia_drill, diff_pair_width, diff_pair_gap, diff_pair_via_gap: None, priority: 0 })
}

/// `(front yes) (back no)` / legacy bare `front back none` tenting list, as
/// `PCB_IO_KICAD_SEXPR_PARSER::parseFrontBackOptBool( true )` reads it:
/// `None` for "none"/absent.
fn front_back_opt_bool(list: &[Sexpr]) -> (Option<bool>, Option<bool>) {
    let (mut front, mut back) = (None, None);
    for it in list.iter().skip(1) {
        match it {
            Sexpr::List(l) => {
                let v = match sexpr::txt(l, 1) {
                    Some("yes") => Some(true),
                    Some("no") => Some(false),
                    _ => None,
                };
                match sexpr::tag(l) {
                    Some("front") => front = v,
                    Some("back") => back = v,
                    _ => {}
                }
            }
            Sexpr::Atom(a) => match a.as_str() {
                "front" => front = Some(true),
                "back" => back = Some(true),
                "none" => {
                    front = None;
                    back = None;
                }
                _ => {}
            },
        }
    }
    (front, back)
}

/// `(setup (pad_to_mask_clearance ..) (solder_mask_min_width ..)
/// (allow_soldermask_bridges_in_footprints ..) (tenting ..))` --
/// `PCB_IO_KICAD_SEXPR_PARSER::parseSetup`'s `T_pad_to_mask_clearance`/
/// `T_solder_mask_min_width`/`T_allow_soldermask_bridges_in_footprints`/
/// `T_tenting` cases. Tenting defaults to "tented both sides"
/// (`BOARD_DESIGN_SETTINGS`'s constructor) and, when a `(tenting ..)` form
/// is present, to `false` for a side it does not name (`front.value_or( false )`).
fn import_solder_mask_setup(root: &[Sexpr], board: &mut BoardRules) {
    let Some(setup) = sexpr::find(root, "setup") else { return };
    if let Some(v) = sexpr::find(setup, "pad_to_mask_clearance").and_then(|f| sexpr::num(f, 1)) {
        board.solder_mask.expansion_nm = mm_to_nm(v);
    }
    if let Some(v) = sexpr::find(setup, "solder_mask_min_width").and_then(|f| sexpr::num(f, 1)) {
        board.solder_mask.min_width_nm = mm_to_nm(v);
    }
    if let Some(v) = sexpr::find(setup, "allow_soldermask_bridges_in_footprints").and_then(|f| sexpr::txt(f, 1)) {
        board.solder_mask.allow_bridges_in_footprints = v == "yes";
    }
    if let Some(t) = sexpr::find(setup, "tenting") {
        let (front, back) = front_back_opt_bool(t);
        board.solder_mask.tent_vias_front = front.unwrap_or(false);
        board.solder_mask.tent_vias_back = back.unwrap_or(false);
    }
    // `m_SolderPasteMargin` / `m_SolderPasteMarginRatio`.
    if let Some(v) = sexpr::find(setup, "pad_to_paste_clearance").and_then(|f| sexpr::num(f, 1)) {
        board.solder_mask.paste_margin_nm = mm_to_nm(v);
    }
    if let Some(v) = sexpr::find(setup, "pad_to_paste_clearance_ratio").and_then(|f| sexpr::num(f, 1)) {
        board.solder_mask.paste_margin_ratio = v;
    }
    // `(general (thickness ..))`: `BOARD_DESIGN_SETTINGS::SetBoardThickness`.
    if let Some(v) = sexpr::find(root, "general").and_then(|g| sexpr::find(g, "thickness")).and_then(|f| sexpr::num(f, 1)) {
        board.board_thickness_nm = mm_to_nm(v);
    }
}

/// `PCB_IO_KICAD_SEXPR_PARSER::parseBoardStackup`: `(setup (stackup (layer "F.Cu" (type "copper") (thickness 0.035)) ..
/// (copper_finish "ENIG") (dielectric_constraints yes) ..))`. A layer with several sublayers (`addsublayer`) is read as its
/// first. `None` when the board has no stackup block.
fn import_stackup(root: &[Sexpr]) -> Option<eda_model::Stackup> {
    let st = sexpr::find(root, "setup").and_then(|setup| sexpr::find(setup, "stackup"))?;
    let layers: Vec<eda_model::StackupLayer> = sexpr::find_all(st, "layer")
        .filter_map(|l| {
            let name = sexpr::txt(l, 1)?.to_string();
            let kind = sexpr::find(l, "type").and_then(|t| sexpr::txt(t, 1)).map(String::from);
            Some(eda_model::StackupLayer {
                name,
                material: sexpr::find(l, "material").and_then(|m| sexpr::txt(m, 1)).map(String::from),
                thickness_mm: sexpr::find(l, "thickness").and_then(|t| sexpr::num(t, 1)),
                kind,
                epsilon_r: sexpr::find(l, "epsilon_r").and_then(|e| sexpr::num(e, 1)),
                loss_tangent: sexpr::find(l, "loss_tangent").and_then(|e| sexpr::num(e, 1)),
                color: sexpr::find(l, "color").and_then(|c| sexpr::txt(c, 1)).filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("not specified")).map(String::from),
            })
        })
        .collect();
    if layers.is_empty() {
        return None;
    }
    Some(eda_model::Stackup {
        layers,
        copper_finish: sexpr::find(st, "copper_finish").and_then(|f| sexpr::txt(f, 1)).map(String::from),
        dielectric_constraints: sexpr::find(st, "dielectric_constraints").and_then(|f| sexpr::txt(f, 1)) == Some("yes"),
        edge_connector: match sexpr::find(st, "edge_connector").and_then(|f| sexpr::txt(f, 1)) {
            Some("yes") => 1,
            Some("bevelled") => 2,
            _ => 0,
        },
        edge_plating: sexpr::find(st, "edge_plating").and_then(|f| sexpr::txt(f, 1)) == Some("yes"),
    })
}

/// Board-wide *minimum* constraints (`BOARD_DESIGN_SETTINGS`'s `rules.min_*`
/// -- see [`merge_project_design_rules`]'s doc comment for the nominal-vs-
/// minimum distinction this is one half of) from a bare `.kicad_pcb`'s own
/// legacy `(setup ...)` block -- the pre-KiCad-7 tokens
/// `PCB_IO_KICAD_SEXPR_PARSER::parseSetup` still reads for a file saved
/// before board design settings moved into the sidecar `.kicad_pro`
/// (`pcb_io_kicad_sexpr_parser.cpp`'s `T_trace_min`/`T_clearance_min`/
/// `T_via_min_size`/`T_through_hole_min`/`T_via_min_drill`/
/// `T_hole_to_hole_min`/`T_via_min_annulus` cases, read directly from the
/// source). A modern multi-file project typically has none of these at all
/// (confirmed empirically against the QA corpus: a current `.kicad_pcb`'s
/// `(setup ...)` carries stackup/mask/zone defaults, never these) -- in
/// that case [`merge_project_design_rules`] is the real source and should
/// run after this, same ordering as net classes
/// ([`merge_project_net_classes`]'s own doc comment).
fn import_legacy_setup_minimums(root: &[Sexpr], board: &mut BoardRules) {
    let Some(setup) = sexpr::find(root, "setup") else { return };
    if let Some(v) = sexpr::find(setup, "trace_min").and_then(|f| sexpr::num(f, 1)) {
        board.track_width_min_nm = mm_to_nm(v);
    }
    if let Some(v) = sexpr::find(setup, "clearance_min").and_then(|f| sexpr::num(f, 1)) {
        board.min_clearance_nm = mm_to_nm(v);
    }
    if let Some(v) = sexpr::find(setup, "via_min_size").and_then(|f| sexpr::num(f, 1)) {
        board.via_diameter_min_nm = mm_to_nm(v);
    }
    let through_hole_min = sexpr::find(setup, "through_hole_min").or_else(|| sexpr::find(setup, "via_min_drill"));
    if let Some(v) = through_hole_min.and_then(|f| sexpr::num(f, 1)) {
        board.via_drill_min_nm = mm_to_nm(v);
    }
    if let Some(v) = sexpr::find(setup, "hole_to_hole_min").and_then(|f| sexpr::num(f, 1)) {
        board.hole_to_hole_min_nm = mm_to_nm(v);
    }
    if let Some(v) = sexpr::find(setup, "via_min_annulus").and_then(|f| sexpr::num(f, 1)) {
        board.annular_width_min_nm = mm_to_nm(v);
    }
}

/// A KiCad 7+ project's real net-class source: `net_settings.classes[]` +
/// `net_settings.netclass_patterns[]` in the sidecar `.kicad_pro` JSON file
/// -- *not* the `(net_class ...)` s-expression [`import_board_rules`] reads
/// from the `.kicad_pcb` itself, which current KiCad only still emits for
/// the implicit `"Default"` class (read there for track width/clearance/via
/// size) and leaves empty of any custom class a real project defines. A
/// `.kicad_pcb` with no sidecar project (common for a single-file QA
/// fixture) legitimately has no custom classes to find; this returns an
/// empty `Vec` rather than an error for any JSON this doesn't recognize, matching
/// the rest of this module's "unknown field is invisible, not a parse
/// error" convention.
///
/// `nets: Vec<String>` is populated straight from each matching pattern
/// string (not resolved against the board's actual net list): `NetClass::
/// matches` already glob-matches a net name against these patterns the
/// same way `BoardRules::class_of`/`clearance_of` resolve every other
/// class, so no extra resolution step is needed here. Declaration order in
/// `classes[]` is preserved (skipping `"Default"`, which this crate's
/// `BoardRules` board-wide defaults already stand in for) -- `class_of`'s
/// "first match wins" is exactly KiCad's own "earliest-declared non-default
/// netclass wins" precedence (`net_settings.cpp`'s `makeEffectiveNetclass`).
pub fn parse_project_net_classes(project_json: &str) -> Vec<NetClass> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(project_json) else {
        return Vec::new();
    };
    let Some(net_settings) = root.get("net_settings") else {
        return Vec::new();
    };
    let mut classes = net_settings.get("classes").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    // KiCad writes the classes in the order of their names; a class's precedence is its `priority` (the Net Classes panel gives each its row, the lowest number winning, a class that
    // says none is -1), and the studio's list is in precedence order (`BoardRules::matching_classes`), so the list is sorted the way `makeEffectiveNetclass` sorts the classes.
    classes.sort_by(|a, b| {
        let key = |c: &serde_json::Value| (c.get("priority").and_then(|p| p.as_i64()).unwrap_or(-1), c.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string());
        key(a).cmp(&key(b))
    });
    let patterns = net_settings.get("netclass_patterns").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    // `netclass_assignments`: the nets the schematic's net class labels and directives put in classes (`NET_SETTINGS::m_netClassLabelAssignments`, net name -> class names), by exact
    // name. `GetEffectiveNetClass` collects them with the pattern assignments, so a net is in every class either names; each is written as the pattern that matches that net and no
    // other. A class they name that `classes` does not have is an implicit class: no parameters of its own, but a rule reads it (`A.hasNetclass('X')`).
    let assigned: Vec<(String, String)> = net_settings
        .get("netclass_assignments")
        .and_then(|v| v.as_object())
        .map(|m| m.iter().flat_map(|(net, cs)| cs.as_array().into_iter().flatten().filter_map(|c| c.as_str()).map(move |c| (c.to_string(), crate::pcb::exact_pattern(net)))).collect())
        .unwrap_or_default();

    let num_mm = |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_f64()).map(mm_to_nm);

    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for class in &classes {
        let Some(name) = class.get("name").and_then(|v| v.as_str()) else { continue };
        seen.push(name.to_string());
        if name == "Default" {
            continue; // the board-default fields already cover this; see the doc comment above
        }
        let nets: Vec<String> = patterns
            .iter()
            .filter(|p| p.get("netclass").and_then(|v| v.as_str()) == Some(name))
            .filter_map(|p| p.get("pattern").and_then(|v| v.as_str()).map(String::from))
            .chain(assigned.iter().filter(|(class, _)| class == name).map(|(_, pattern)| pattern.clone()))
            .collect();
        if nets.is_empty() {
            continue; // an unused class matches nothing; not worth carrying (same rule import_board_rules uses)
        }
        out.push(NetClass {
            name: name.to_string(),
            nets,
            track_width: num_mm(class, "track_width"),
            clearance: num_mm(class, "clearance"),
            via_diameter: num_mm(class, "via_diameter"),
            via_drill: num_mm(class, "via_drill"),
            microvia_diameter: num_mm(class, "microvia_diameter"),
            microvia_drill: num_mm(class, "microvia_drill"),
            diff_pair_width: num_mm(class, "diff_pair_width"),
            diff_pair_gap: num_mm(class, "diff_pair_gap"),
            diff_pair_via_gap: num_mm(class, "diff_pair_via_gap"),
            priority: out.len() as i32,
        });
    }
    // The implicit classes, after the defined ones (`getOrAddImplicitNetcless`: a priority past every defined class).
    for (class, _) in &assigned {
        if class == "Default" || seen.contains(class) {
            continue;
        }
        seen.push(class.clone());
        out.push(NetClass {
            name: class.clone(),
            nets: assigned.iter().filter(|(c, _)| c == class).map(|(_, pattern)| pattern.clone()).collect(),
            track_width: None,
            clearance: None,
            via_diameter: None,
            via_drill: None,
            microvia_diameter: None,
            microvia_drill: None,
            diff_pair_width: None,
            diff_pair_gap: None,
            diff_pair_via_gap: None,
            priority: out.len() as i32,
        });
    }
    out
}

/// Merge a `.kicad_pro`'s net classes into an already-imported board:
/// project classes are inserted *before* whatever [`import_board_rules`]
/// found in the `.kicad_pcb` itself (first-match-wins in `class_of`, and a
/// bare `.kicad_pcb`-level class is the rarer, more conservative case worth
/// keeping as a fallback rather than letting it shadow the project's own).
/// A caller that has both files -- any real project directory, as opposed
/// to a bare single-file `.kicad_pcb` -- should call this after
/// [`import_kicad_pcb`] with the sidecar `.kicad_pro`'s contents.
///
/// Also applies the project's own `"Default"` class to `BoardRules`'s own
/// scalar fields (`clearance`/`track_width`/`via_diameter`/`via_drill`).
/// `import_board_rules` already reads a `"Default"` class for these -- but
/// only from a legacy `(net_class ...)` s-expression in the `.kicad_pcb`
/// itself, which a modern (KiCad 7+) project typically does not write at
/// all once it has a `.kicad_pro` (confirmed empirically: zero such blocks
/// in several real multi-class QA-corpus boards sampled while building
/// this). Without this, a board imported alongside its real project would
/// silently keep this crate's own placeholder defaults (`BoardRules::
/// default()`) instead of that project's actual default clearance/width/
/// via size -- wrong for every net that has no more specific class, not
/// just the ones a custom class names.
pub fn merge_project_net_classes(model: &mut eda_model::ConstraintModel, project_json: &str) {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(project_json) else {
        return;
    };
    let classes = root.get("net_settings").and_then(|ns| ns.get("classes")).and_then(|v| v.as_array());
    if let Some(default) = classes.and_then(|cs| cs.iter().find(|c| c.get("name").and_then(|v| v.as_str()) == Some("Default"))) {
        let mm = |key: &str| default.get(key).and_then(|v| v.as_f64()).map(mm_to_nm);
        if let Some(v) = mm("clearance") {
            model.board.clearance = v;
        }
        if let Some(v) = mm("track_width") {
            model.board.track_width = v;
        }
        if let Some(v) = mm("via_diameter") {
            model.board.via_diameter = v;
        }
        if let Some(v) = mm("via_drill") {
            model.board.via_drill = v;
        }
    }

    let mut project_classes = parse_project_net_classes(project_json);
    if !project_classes.is_empty() {
        project_classes.append(&mut model.board.net_classes);
        model.board.net_classes = project_classes;
    }
}

/// Merge a `.kicad_pro`'s `board.design_settings.rules` object -- the
/// absolute board-wide *minimums* `DRC_ENGINE::loadImplicitRules`'s "board
/// setup constraints" implicit rule reads straight from
/// `BOARD_DESIGN_SETTINGS` (`pcbnew/board_design_settings.cpp`'s
/// `rules.min_*` `PARAM_SCALED` entries, confirmed directly against the
/// source) -- entirely distinct from a net class's own nominal width/
/// clearance/via size ([`parse_project_net_classes`]/[`merge_project_net_classes`]),
/// which only ever sets the *default*/`Opt` value a new route or via is
/// drawn at, never the `Min` a DRC check enforces. Conflating the two was
/// GAPS.md #10: this crate used to have no field at all for
/// `rules.min_track_width` and checked every track against its net
/// class's (often much larger) nominal width instead, which alone produced
/// ~2900 false `track_width` positives on one QA-corpus board
/// (`issue11814`). Same "absent/unrecognized JSON is a no-op, never an
/// error" convention as this module's other `.kicad_pro` readers; a
/// caller with both files should call this after [`import_kicad_pcb`],
/// same ordering as [`merge_project_net_classes`]/[`merge_project_rule_severities`]
/// (and after those two, in practice -- order does not matter between
/// these three, since each only ever touches its own fields).
pub fn merge_project_design_rules(model: &mut eda_model::ConstraintModel, project_json: &str) {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(project_json) else {
        return;
    };
    let Some(design_settings) = root.get("board").and_then(|b| b.get("design_settings")) else {
        return;
    };
    let Some(rules) = design_settings.get("rules") else {
        return;
    };
    let mm = |key: &str| rules.get(key).and_then(|v| v.as_f64()).map(mm_to_nm);
    let b = &mut model.board;
    // The project stated its minimums: they are exact, and written back exactly (`BoardRules::constraints_explicit`).
    b.constraints_explicit = true;
    if let Some(v) = mm("min_track_width") {
        b.track_width_min_nm = v;
    }
    if let Some(v) = mm("min_clearance") {
        b.min_clearance_nm = v;
    }
    if let Some(v) = mm("min_copper_edge_clearance") {
        b.copper_edge_clearance_nm = Some(v);
    }
    if let Some(v) = mm("min_via_diameter") {
        b.via_diameter_min_nm = v;
    }
    if let Some(v) = mm("min_through_hole_diameter") {
        b.via_drill_min_nm = v;
    }
    if let Some(v) = mm("min_hole_to_hole") {
        b.hole_to_hole_min_nm = v;
    }
    if let Some(v) = mm("min_hole_clearance") {
        b.hole_clearance_nm = v;
    }
    if let Some(v) = mm("min_via_annular_width") {
        b.annular_width_min_nm = v;
    }
    if let Some(v) = mm("min_silk_clearance") {
        b.silk_clearance_nm = v;
    }
    if let Some(v) = mm("min_text_height") {
        b.min_silk_text_height_nm = v;
    }
    if let Some(v) = mm("min_text_thickness") {
        b.min_silk_text_thickness_nm = v;
    }
    // `m_SolderMaskToCopperClearance` lives only in the project file.
    if let Some(v) = mm("solder_mask_to_copper_clearance") {
        b.solder_mask.to_copper_clearance_nm = v;
    }
    // The rest of Board Setup > Constraints.
    if let Some(v) = mm("min_connection") {
        b.min_connection_nm = v;
    }
    if let Some(v) = mm("min_microvia_diameter") {
        b.microvia_diameter_min_nm = v;
    }
    if let Some(v) = mm("min_microvia_drill") {
        b.microvia_drill_min_nm = v;
    }
    if let Some(v) = mm("min_groove_width") {
        b.min_groove_width_nm = v;
    }
    if let Some(v) = mm("max_error") {
        b.max_error_nm = v.max(Nm::new(1));
    }
    if let Some(v) = rules.get("min_resolved_spokes").and_then(|v| v.as_u64()) {
        b.min_resolved_spokes = v.min(99) as u32;
    }
    if let Some(v) = rules.get("use_height_for_length_calcs").and_then(|v| v.as_bool()) {
        b.use_height_for_length_calcs = v;
    }
    if let Some(v) = design_settings.get("zones_allow_external_fillets").and_then(|v| v.as_bool()) {
        b.zones_allow_external_fillets = v;
    }
    // Text & Graphics > Defaults (`defaults.<class>_line_width`, `_text_size_h/_v`, `_text_thickness`, `_text_italic`, `_text_upright`).
    if let Some(def) = design_settings.get("defaults") {
        let num = |key: &str| def.get(key).and_then(|v| v.as_f64()).map(mm_to_nm);
        let flag = |key: &str| def.get(key).and_then(|v| v.as_bool());
        let class = |prefix: &str, c: &mut eda_model::rules::LayerClassDefaults| {
            if let Some(v) = num(&format!("{prefix}_line_width")) {
                c.line_width_nm = v;
            }
            if let Some(v) = num(&format!("{prefix}_text_size_h")) {
                c.text_width_nm = v;
            }
            if let Some(v) = num(&format!("{prefix}_text_size_v")) {
                c.text_height_nm = v;
            }
            if let Some(v) = num(&format!("{prefix}_text_thickness")) {
                c.text_thickness_nm = v;
            }
            if let Some(v) = flag(&format!("{prefix}_text_italic")) {
                c.italic = v;
            }
            if let Some(v) = flag(&format!("{prefix}_text_upright")) {
                c.upright = v;
            }
        };
        class("silk", &mut b.text_graphics.silk);
        class("copper", &mut b.text_graphics.copper);
        class("fab", &mut b.text_graphics.fab);
        class("other", &mut b.text_graphics.others);
        if let Some(v) = num("board_outline_line_width") {
            b.text_graphics.edge_cuts_line_width_nm = v;
        }
        if let Some(v) = num("courtyard_line_width") {
            b.text_graphics.courtyard_line_width_nm = v;
        }
    }
}

/// A `.kicad_pro`'s `board.design_settings.rule_severities` -- KiCad 7+'s
/// per-DRC-type severity table (`BOARD_DESIGN_SETTINGS`'s `rule_severities`
/// `PARAM_LAMBDA`, `pcbnew/board_design_settings.cpp`: `ret[settingsKey] =
/// SeverityToString(m_DRCSeverities[code])` for every type it knows about).
/// KiCad always writes the *complete* resolved table on save (every known
/// type, including ones the user never touched), not just overrides, so
/// this returns the whole thing verbatim -- same "unrecognized/absent JSON
/// is an empty no-op, never an error" convention as
/// [`parse_project_net_classes`].
///
/// The one wrinkle ported deliberately: a V8-era project may still carry
/// the legacy `"hole_near_hole"` key instead of (or alongside) the current
/// `"hole_to_hole"` one. KiCad's own loader reads `hole_near_hole` first,
/// then lets any `hole_to_hole` key overwrite it (`board_design_settings.
/// cpp`'s migration comment, read directly from the source); reproduced
/// here by seeding `"hole_to_hole"` from the legacy key only when the
/// modern one is not already present.
pub fn parse_rule_severities(project_json: &str) -> std::collections::BTreeMap<String, String> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(project_json) else {
        return Default::default();
    };
    let Some(obj) = root.get("board").and_then(|b| b.get("design_settings")).and_then(|d| d.get("rule_severities")).and_then(|v| v.as_object()) else {
        return Default::default();
    };
    let mut out: std::collections::BTreeMap<String, String> = obj.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect();
    if let Some(legacy) = obj.get("hole_near_hole").and_then(|v| v.as_str()) {
        out.entry("hole_to_hole".to_string()).or_insert_with(|| legacy.to_string());
    }
    out
}

/// Merge a `.kicad_pro`'s `rule_severities` into an already-imported board
/// (task item 2: "import `rule_severities` ... into the IR, additively").
/// Wholesale replacement, not a key-by-key merge, because KiCad's own file
/// always holds the complete table already (see [`parse_rule_severities`]'s
/// doc comment) -- there is nothing to merge *with* on the `.kicad_pcb`
/// side, unlike net classes, which a legacy board file can carry its own
/// (partial) copy of. A caller with both files should call this after
/// [`import_kicad_pcb`], same pattern as [`merge_project_net_classes`].
pub fn merge_project_rule_severities(model: &mut eda_model::ConstraintModel, project_json: &str) {
    let severities = parse_rule_severities(project_json);
    if !severities.is_empty() {
        model.board.rule_severities = severities;
    }
}

// -------------------------------------------------------------- footprints

/// `(property "Reference"/"Value" "text" ...)` (KiCad 8+) or the older
/// `(fp_text reference/value "text" ...)` -- whichever the file has.
fn footprint_field(fp: &[Sexpr], field: &str) -> Option<String> {
    for p in sexpr::find_all(fp, "property") {
        if sexpr::txt(p, 1) == Some(field) {
            return sexpr::txt(p, 2).map(String::from);
        }
    }
    let legacy_tok = match field {
        "Reference" => "reference",
        "Value" => "value",
        _ => return None,
    };
    for t in sexpr::find_all(fp, "fp_text") {
        if sexpr::txt(t, 1) == Some(legacy_tok) {
            return sexpr::txt(t, 2).map(String::from);
        }
    }
    None
}

/// Parse one `(pad ...)` node's geometry -- number, kind, shape, position,
/// size, drill (round or slot), rotation relative to its footprint, and
/// roundrect ratio. No net: a bare `.kicad_mod` (the library loader) has
/// none, and the whole-board importer, which does, reads a `(net ...)`
/// child from the same node itself (see its call site) rather than this
/// shared function knowing about board-level net codes.
///
/// `fp_side`/`fp_rot` are the *footprint's own*, already resolved --
/// needed to recover this pad's own rotation from its absolute file angle
/// (see `pad_rot_from_file`). `None` for a pad this reader cannot place at
/// all (no `at`/`size`).
pub(crate) fn parse_pad_geometry(pad: &[Sexpr], fp_side: Side, fp_rot: u32, notes: &mut ImportNotes) -> Option<Pad> {
    parse_pad_geometry_opt(pad, fp_side, fp_rot, notes, false)
}

/// [`parse_pad_geometry`], optionally keeping a pad whose `(layers ..)` name
/// no copper layer (a mask-only aperture pad) instead of dropping it -- the
/// solder-mask DRC needs those as `PCB_PAD_T` mask apertures.
pub(crate) fn parse_pad_geometry_opt(pad: &[Sexpr], fp_side: Side, fp_rot: u32, notes: &mut ImportNotes, keep_non_copper: bool) -> Option<Pad> {
    parse_pad_geometry_in(pad, fp_side, fp_rot, notes, keep_non_copper, &[])
}

/// [`parse_pad_geometry_opt`] on a board whose copper layers are `copper_layers`, front to back (a bottom-side pad's custom padstack needs to know which
/// layer each of them has become; empty is a two-layer board).
pub(crate) fn parse_pad_geometry_in(pad: &[Sexpr], fp_side: Side, fp_rot: u32, notes: &mut ImportNotes, keep_non_copper: bool, copper_layers: &[String]) -> Option<Pad> {
    let number = sexpr::txt(pad, 1).unwrap_or("").to_string();
    let kind_tok = sexpr::txt(pad, 2).unwrap_or("");
    let pad_kind = match kind_tok {
        "np_thru_hole" => PadKind::NonPlatedHole,
        "smd" | "connect" => PadKind::Smd,
        _ => PadKind::ThroughHole, // "thru_hole"
    };
    // The shape and everything that goes with it -- a trapezoid's delta, a chamfered rectangle's corners, a custom pad's primitives,
    // the copper's offset (`parsePAD`).
    let read = crate::pad_shape::read_pad_shape(pad, fp_side);

    // A pad with a `(layers ...)` list naming no copper layer at all has no
    // copper to flash anywhere -- see `ImportNotes::non_copper_pads_skipped`.
    // An absent `(layers ...)` (never real on a board pad, but harmless to
    // tolerate) is not treated as "no copper": only an explicit, entirely
    // non-copper list skips the pad.
    let mut opposite_side = false;
    if let Some(layers) = sexpr::find(pad, "layers") {
        let names: Vec<&str> = layers.iter().skip(1).filter_map(Sexpr::text).collect();
        if !names.is_empty() && !names.iter().any(|l| *l == "*.Cu" || l.ends_with(".Cu")) && !keep_non_copper {
            notes.non_copper_pads_skipped += 1;
            return None;
        }
        // An SMD pad on the other outer layer than its footprint (layers
        // on file are already flipped for a back-side footprint). A pad on several copper layers is on the front-most of them as far as connectivity goes
        // (`CN_LIST::Add( PAD* )`: `LSEQ lmsk = pad->GetLayerSet().CuStack(); item->SetLayer( lmsk.front() )`), which is `F.Cu` whenever the set has it.
        if pad_kind == PadKind::Smd {
            let f = names.iter().any(|l| matches!(*l, "F.Cu" | "*.Cu" | "F&B.Cu"));
            let b = names.iter().any(|l| matches!(*l, "B.Cu" | "*.Cu" | "F&B.Cu"));
            opposite_side = if fp_side == Side::Bottom { f } else { b && !f };
        }
    }

    let pad_at = sexpr::find(pad, "at")?;
    let (px, py) = (sexpr::num(pad_at, 1)?, sexpr::num(pad_at, 2)?);
    // A pad's `(at x y)` in a real `.kicad_pcb` is *already* mirrored when
    // its footprint is on the back layer -- KiCad's own writer and reader
    // apply no separate mirror step at all; it just rotates+translates
    // whatever `x` is on file (confirmed directly against `kicad-cli pcb
    // drc`'s reported pad positions on a back-side, rotated footprint: a
    // pad's true board position is `fp.at + Rotate(-file_angle)(x, y)`,
    // identical in form for front and back). Our own `Pad::at`/`to_board`
    // convention is the opposite on purpose (a library footprint is always
    // authored once, as seen from the top, and `to_board` mirrors `x` at
    // placement time for a bottom-side instance -- see `eda_model::
    // footprint`'s module doc) -- `write_footprint` already pre-mirrors `x`
    // for exactly this reason (`let px = ... if Bottom { -pad.at.0 } ...`).
    // This importer has to undo that same mirror on the way in, or a real
    // KiCad-authored back-side pad's *position* re-mirrors a second time at
    // placement and lands on its mirror image -- this was GAPS.md #3's
    // "clearance" over-firing root cause (confirmed on `issue11814`'s R34:
    // a 90°-rotated back-side resistor whose two pads came out swapped by
    // exactly their pitch, putting one pad's copper on top of a foreign
    // net's track). A pure export/import round trip never caught this: our
    // own writer's mirror and this missing un-mirror canceled out for a
    // file we wrote ourselves, which is exactly why real-file import needed
    // its own regression test (see this module's tests).
    let px = if fp_side == Side::Bottom { -px } else { px };
    let pad_file_rot = sexpr::num(pad_at, 3).unwrap_or(0.0);
    let rot = crate::pad_rot_from_file(fp_side, fp_rot, pad_file_rot);

    let size = sexpr::find(pad, "size")?;
    let (w, h) = (sexpr::num(size, 1)?, sexpr::num(size, 2)?);

    // `(drill W)` is a round hole; `(drill oval W H)` a slot; an absent
    // token (an smd pad) is neither.
    let (drill, drill_slot) = match sexpr::find(pad, "drill") {
        Some(d) => {
            let toks: Vec<&str> = d.iter().skip(1).filter_map(Sexpr::text).collect();
            if toks.first() == Some(&"oval") {
                let nums: Vec<f64> = toks[1..].iter().filter_map(|s| s.parse::<f64>().ok()).collect();
                let dw = nums.first().copied().unwrap_or(0.0);
                let dh = nums.get(1).copied().unwrap_or(dw);
                (None, Some((mm_to_nm(dw), mm_to_nm(dh))))
            } else {
                let d0 = toks.iter().find_map(|s| s.parse::<f64>().ok());
                (d0.map(mm_to_nm), None)
            }
        }
        None => (None, None),
    };

    let mut out = Pad { number, at: (mm_to_nm(px), mm_to_nm(py)), size: (mm_to_nm(w), mm_to_nm(h)), shape: read.shape, kind: pad_kind, drill, drill_slot, rot, roundrect_ratio: read.roundrect_ratio, opposite_side, ext: read.ext };
    // The padstack (`(padstack ..)`, `(remove_unused_layers ..)`, `(keep_end_layers ..)`): the other layers' shapes and what becomes of the layers nothing connects to.
    // The main fields are the shape on `F.Cu`; the pad's own shape is the footprint's side, which on the bottom is the file's `B.Cu`.
    let main = eda_model::padstack::PadLayerShape::of(&out);
    let layers: Vec<String> = if copper_layers.is_empty() { vec!["F.Cu".to_string(), "B.Cu".to_string()] } else { copper_layers.to_vec() };
    let read_stack = crate::padstack_io::read_padstack(pad, &main, fp_side, &layers);
    if let Some(own) = &read_stack.own {
        own.put_on(&mut out);
    }
    out.ext.stack = read_stack.stack;
    Some(out)
}

/// The cache key `import_footprints` should use for an instance whose lib id
/// is `lib_id` and whose own freshly-parsed pad geometry is `pads`: `lib_id`
/// itself if nothing is cached there yet or the cached entry already has
/// this exact geometry, else `"{lib_id}#2"`/`"#3"`/... -- the first slot
/// (plain or suffixed) whose pads match, or a freshly inserted one if none
/// do. See `import_footprints`'s call site for why this can legitimately
/// happen (the same lib id placed on both sides of one board).
fn dedup_footprint_key(explicit: &mut BTreeMap<String, Footprint>, lib_id: &str, pads: &[Pad], models3d: &[eda_model::footprint::Model3d], art: &eda_model::fp_art::FpArt) -> String {
    let same = |existing: &Footprint| existing.pads.as_slice() == pads && existing.models3d.as_slice() == models3d && existing.art == *art;
    match explicit.get(lib_id) {
        None => lib_id.to_string(),
        Some(existing) if same(existing) => lib_id.to_string(),
        Some(_) => {
            for n in 2.. {
                let key = format!("{lib_id}#{n}");
                match explicit.get(&key) {
                    None => return key,
                    Some(existing) if same(existing) => return key,
                    Some(_) => continue,
                }
            }
            unreachable!()
        }
    }
}

/// This instance's own courtyard half-extents `(hw, hh)`, derived from its
/// `F.CrtYd`/`B.CrtYd` graphics (`fp_line`/`fp_rect`/`fp_poly`/`fp_circle`)
/// -- `None` when it has none, in which case `Footprint::courtyard_half`
/// falls back to the pad bounding box plus a flat margin, same as before
/// this existed. A real courtyard is routinely much larger than the pad
/// bbox (a connector's shroud, a mounting flange, a mechanical keep-out) --
/// measured on the QA corpus, the bbox-derived fallback's `courtyards_overlap`
/// was a 100% false-positive rate (KiCad 0 across every sampled board, this
/// port dozens) precisely because it has no way to see that margin.
///
/// Every point is read in the footprint's own local frame exactly like a
/// pad's `(at ...)`, so a back-side instance needs the same x-un-mirror
/// `parse_pad_geometry` applies to a pad position (see that function's doc
/// comment for why: `to_board` mirrors `x` for a `Side::Bottom` instance,
/// so this importer must feed it the pre-image of that mirror, not the raw
/// file value, or every back-side footprint's courtyard comes out shifted
/// to its mirror image same as a pad would).
///
/// `Footprint::courtyard` is a single symmetric-about-the-origin half-extent
/// (see that field's own doc comment on why), so an off-centre real
/// courtyard is conservatively enclosed by the smallest symmetric box that
/// contains every point found here -- the same simplification
/// `courtyard_half` already documents for a hand-authored one.
/// `FOOTPRINT::BuildCourtyardCaches`: the courtyard graphics chained into
/// closed outlines, in the footprint's local frame (a back-side instance's
/// x un-mirrored like a pad's -- see [`footprint_courtyard_half`]).
/// `fp_rect`/`fp_poly`/`fp_circle` are closed already; `fp_line`/`fp_arc`
/// segments are chained end to end. An open chain is dropped, like the
/// malformed-courtyard case in source.
pub(crate) fn footprint_courtyard_outlines(fp: &[Sexpr], side: Side) -> Vec<Vec<(Nm, Nm)>> {
    let is_crtyd = |item: &[Sexpr]| -> bool { matches!(sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)), Some("F.CrtYd") | Some("B.CrtYd")) };
    let mut outlines: Vec<Vec<Point>> = Vec::new();
    for item in sexpr::find_all(fp, "fp_rect").filter(|it| is_crtyd(it)) {
        if let (Some(a), Some(b)) = (sexpr::find(item, "start").and_then(xy_point), sexpr::find(item, "end").and_then(xy_point)) {
            outlines.push(vec![a, Point { x: b.x, y: a.y }, b, Point { x: a.x, y: b.y }]);
        }
    }
    for item in sexpr::find_all(fp, "fp_poly").filter(|it| is_crtyd(it)) {
        if let Some(poly) = poly_points(item) {
            outlines.push(poly);
        }
    }
    for item in sexpr::find_all(fp, "fp_circle").filter(|it| is_crtyd(it)) {
        let (Some(c), Some(e)) = (sexpr::find(item, "center").and_then(xy_point), sexpr::find(item, "end").and_then(xy_point)) else { continue };
        let r = ((e.x - c.x).as_f64()).hypot((e.y - c.y).as_f64());
        outlines.push((0..32).map(|i| { let t = i as f64 * std::f64::consts::TAU / 32.0; Point { x: c.x + Nm::kiround(r * t.cos()), y: c.y + Nm::kiround(r * t.sin()) } }).collect());
    }
    let mut edges: Vec<(Point, Point)> = Vec::new();
    for item in sexpr::find_all(fp, "fp_line").filter(|it| is_crtyd(it)) {
        if let (Some(a), Some(b)) = (sexpr::find(item, "start").and_then(xy_point), sexpr::find(item, "end").and_then(xy_point)) {
            edges.push((a, b));
        }
    }
    for item in sexpr::find_all(fp, "fp_arc").filter(|it| is_crtyd(it)) {
        if let (Some(a), Some(m), Some(b)) = (sexpr::find(item, "start").and_then(xy_point), sexpr::find(item, "mid").and_then(xy_point), sexpr::find(item, "end").and_then(xy_point)) {
            let pts = eda_model::ir::tessellate_arc(a, m, b, 8);
            edges.extend(pts.windows(2).map(|w| (w[0], w[1])));
        }
    }
    // Chain every closed loop out of the loose segments.
    let mut used = vec![false; edges.len()];
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut chain = vec![edges[start].0, edges[start].1];
        loop {
            let tail = *chain.last().unwrap();
            if tail == chain[0] {
                break;
            }
            let Some(k) = (0..edges.len()).find(|&k| !used[k] && (edges[k].0 == tail || edges[k].1 == tail)) else { break };
            used[k] = true;
            chain.push(if edges[k].0 == tail { edges[k].1 } else { edges[k].0 });
        }
        if chain.len() >= 4 && chain.first() == chain.last() {
            chain.pop();
            outlines.push(chain);
        }
    }
    outlines
        .into_iter()
        .filter(|o| o.len() >= 3)
        .map(|o| o.into_iter().map(|p| (if side == Side::Bottom { -p.x } else { p.x }, p.y)).collect())
        .collect()
}

fn footprint_courtyard_half(fp: &[Sexpr], side: Side) -> Option<(Nm, Nm)> {
    let is_crtyd = |item: &[Sexpr]| -> bool { matches!(sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)), Some("F.CrtYd") | Some("B.CrtYd")) };

    let mut pts: Vec<Point> = Vec::new();
    for item in sexpr::find_all(fp, "fp_line").filter(|it| is_crtyd(it)) {
        pts.extend(sexpr::find(item, "start").and_then(xy_point));
        pts.extend(sexpr::find(item, "end").and_then(xy_point));
    }
    for item in sexpr::find_all(fp, "fp_rect").filter(|it| is_crtyd(it)) {
        pts.extend(sexpr::find(item, "start").and_then(xy_point));
        pts.extend(sexpr::find(item, "end").and_then(xy_point));
    }
    for item in sexpr::find_all(fp, "fp_poly").filter(|it| is_crtyd(it)) {
        if let Some(poly) = poly_points(item) {
            pts.extend(poly);
        }
    }
    for item in sexpr::find_all(fp, "fp_circle").filter(|it| is_crtyd(it)) {
        let (Some(c), Some(e)) = (sexpr::find(item, "center").and_then(xy_point), sexpr::find(item, "end").and_then(xy_point)) else { continue };
        let r = Nm::kiround(((e.x - c.x).as_f64().powi(2) + (e.y - c.y).as_f64().powi(2)).sqrt());
        pts.extend([Point { x: c.x - r, y: c.y }, Point { x: c.x + r, y: c.y }, Point { x: c.x, y: c.y - r }, Point { x: c.x, y: c.y + r }]);
    }
    if pts.is_empty() {
        return None;
    }

    let (mut hw, mut hh) = (Nm::ZERO, Nm::ZERO);
    for p in pts {
        let x = if side == Side::Bottom { -p.x } else { p.x }; // undo the file's mirror -- see this function's doc comment
        hw = hw.max(x.abs());
        hh = hh.max(p.y.abs());
    }
    Some((hw, hh))
}

#[allow(clippy::type_complexity)]
fn import_footprints(
    root: &[Sexpr],
    net_names: &BTreeMap<i64, String>,
    layers: &[String],
    notes: &mut ImportNotes,
    refs: &mut Refs,
) -> Result<(Vec<FootprintInstance>, Vec<Part>, BTreeMap<String, Footprint>, Vec<(String, String)>, Vec<FootprintExtra>, Vec<eda_model::fp_edit::FootprintEdit>), Vec<CheckResult>> {
    let file_version = sexpr::find(root, "version").and_then(|v| sexpr::num(v, 1)).unwrap_or(0.0) as i64;
    let mut extras: Vec<FootprintExtra> = Vec::new();
    let mut edits: Vec<eda_model::fp_edit::FootprintEdit> = Vec::new();
    let mut footprints_ir = Vec::new();
    let mut parts = Vec::new();
    let mut explicit: BTreeMap<String, Footprint> = BTreeMap::new();
    let mut pin_nets: Vec<(String, String)> = Vec::new();
    let mut errors = Vec::new();

    // `module` is the pre-KiCad-6 tag for the same thing; the pad grammar
    // inside it is unchanged, so the rest of this function does not care
    // which one it was.
    let raw: Vec<&[Sexpr]> = sexpr::find_all(root, "footprint").chain(sexpr::find_all(root, "module")).collect();

    let mut seen_refs: BTreeMap<String, usize> = BTreeMap::new();
    for (idx, fp) in raw.iter().enumerate() {
        let lib_id = sexpr::txt(fp, 1).unwrap_or("unknown").to_string();
        let layer = sexpr::find(fp, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.Cu");
        let side = if layer == "B.Cu" { Side::Bottom } else { Side::Top };
        let Some(at) = sexpr::find(fp, "at") else {
            errors.push(CheckResult::fail("kicad_import.footprint_no_at", lib_id.clone(), "footprint has no (at ...); cannot place it"));
            continue;
        };
        let (Some(fx), Some(fy)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else {
            errors.push(CheckResult::fail("kicad_import.footprint_bad_at", lib_id.clone(), "footprint (at ...) is missing x or y"));
            continue;
        };
        let file_rot = sexpr::num(at, 3).unwrap_or(0.0);
        let (x, y) = (mm_to_nm(fx), mm_to_nm(fy));
        let rot = import_rot_millideg(file_rot);

        let base_ref = footprint_field(fp, "Reference").filter(|s| !s.is_empty()).unwrap_or_else(|| format!("FP{}", idx + 1));
        // KiCad allows several footprints to share a reference (an
        // unannotated "REF**", a duplicate); this IR keys footprints and
        // their pins by reference, so a repeat gets a "#n" suffix to stay a
        // distinct footprint with its own pad nets.
        let reference = {
            let n = seen_refs.entry(base_ref.clone()).or_insert(0usize);
            *n += 1;
            if *n == 1 { base_ref.clone() } else { format!("{base_ref}#{n}") }
        };
        let value = footprint_field(fp, "Value");

        let mut pads: Vec<Pad> = Vec::new();
        let mut pins = Vec::new();
        let fp_inst = FootprintInstance { id: reference.clone(), at: Point { x, y }, rot, side, label: Default::default() };
        let mut extra = footprint_extra_header(fp, &reference, file_version);
        // The Reference and Value text as the file has it, the user fields and the attributes; the pad offsets and margins join below.
        let mut edit = crate::fp_fields::parse_footprint_edit_in(fp, &reference, value.as_deref(), side, rot, file_version);
        for pad in sexpr::find_all(fp, "pad") {
            let Some(p) = parse_pad_geometry_in(pad, side, rot, notes, false, layers) else {
                if let Some(g) = mask_only_pad_graphics(pad, &fp_inst, rot, net_names, notes) {
                    extra.graphics.extend(g);
                }
                if let Some(h) = layerless_hole(pad, side, rot, file_version, notes) {
                    extra.holes.push(h);
                } else if let Some(aux) = aux_pad(pad, net_names) {
                    // A paste-only or mask-only pad (no copper): kept as the file had it (`FootprintExtra::aux_pads`).
                    extra.aux_pads.push(aux);
                    extra.aux_pads_rot = rot;
                }
                continue;
            };
            extra.pads.push(pad_mask_info(pad, layers, file_version, side));
            // Offset and margins: the k-th kept pad that carries this number (the engine footprint's own list is what an edit counts in).
            let mut pad_edit = crate::fp_fields::parse_pad_extras(pad, side, file_version);
            if !pad_edit.is_empty() {
                pad_edit.number = p.number.clone();
                pad_edit.nth = pads.iter().filter(|q| q.number == p.number).count() as u32 + 1;
                edit.set_pad(pad_edit);
            }

            // A non-plated hole is mechanical, not electrical: it has no
            // net and is not a schematic pin (nothing a symbol would draw
            // a stub for), but it still occupies space in `pads`, so it
            // still counts as a hole for clearance.
            if p.kind != PadKind::NonPlatedHole {
                let name = net_ref(sexpr::find(pad, "net"), net_names);
                if !name.is_empty() {
                    pin_nets.push((format!("{reference}.{}", p.number), name));
                }
                pins.push(Pin { number: p.number.clone(), name: None, kind: PinKind::Signal });
            }
            if let Some(u) = import_items::item_uuid(pad) {
                notes.item_ids.entry(u).or_insert_with(|| format!("{reference}.{}", p.number));
            }
            pads.push(p);
        }

        // Pad geometry is keyed by lib id and shared across instances (the
        // same library footprint, wherever it is placed); net membership
        // is per-instance and lives in `pin_nets`/`Part.pins` instead.
        // A board file embeds each placed footprint's own definition in
        // full (pads, courtyard graphics, and its `(model ...)` 3D
        // reference) -- reusing footprint_lib's model_from here rather
        // than re-deriving it keeps the two readers from drifting apart
        // on what that one line means, same reasoning this module's own
        // doc comment already gives for sharing parse_pad_geometry.
        //
        // Almost every real board instantiates a given lib id on one side
        // consistently, in which case every instance's own freshly-parsed
        // `pads` agrees with the one already cached and the plain `lib_id`
        // key is reused as-is. The rare real exception (confirmed on
        // `issue11814`: the same `R_0402_..._HandSolder` lib id placed both
        // on `F.Cu` and `B.Cu`) needs its own cache slot: `parse_pad_geometry`
        // resolves each pad's *own* instance geometry (its side-corrected
        // local offset -- see that function's doc comment), so two
        // instances of one lib id on opposite sides legitimately disagree
        // on it, and collapsing them into one shared `Footprint` would make
        // one side's pads silently wrong. `dedup_footprint_key` finds (or
        // starts) whichever cache slot actually matches this instance's own
        // geometry, so `model.footprint_of` -- a plain name lookup with no
        // side of its own to disambiguate by -- still resolves to the right
        // one for every instance.
        extras.push(extra);
        if !edit.is_empty() {
            edits.push(edit);
        }
        // The 3D models with their placement as the file has them: a back-side footprint is read in the library's frame below, where its models are the
        // library's placement (KiCad's flip turns the model with the footprint, not the placement). A slot is shared only by instances whose models agree,
        // like their pads.
        let models3d: Vec<eda_model::footprint::Model3d> = crate::footprint_lib::models_from(fp);
        // The footprint's silkscreen, fab drawings and texts, in the definition's frame (`crate::fp_art_io`): an instance whose own differ from another's of the
        // same lib id gets a slot of its own, like one whose pads differ.
        let art = crate::fp_art_io::read_art(fp, side, rot);
        let courtyard = footprint_courtyard_half(fp, side);
        let courtyard_outlines = footprint_courtyard_outlines(fp, side);
        let mut definition = Footprint { name: String::new(), pads, courtyard, courtyard_outlines, model: crate::footprint_lib::model_from(fp), models3d, art };
        // A footprint on the back is in KiCad's frame in the file (`FOOTPRINT::Flip`: y mirrored, the half turn in its orientation); the pieces above are
        // read mirrored in x (`to_board`'s frame), half a turn from the library's. Turned half a turn, and the orientation half a turn back, they are the
        // library's footprint and its 3D models as the library places them -- as a footprint flipped here -- and the writer turns them back
        // (`pcb::write_footprint`, `FootprintExtra::library_frame`).
        let mut rot = rot;
        if side == Side::Bottom {
            definition = crate::pcb::half_turned(&definition);
            rot = (rot + 180_000) % 360_000;
            if let Some(e) = extras.last_mut().filter(|e| e.id == reference) {
                e.library_frame = true;
                for h in e.holes.iter_mut() {
                    crate::pcb::half_turn_pad(&mut h.pad);
                }
            }
            if let Some(e) = edits.last_mut().filter(|e| e.id == reference) {
                e.reference.iter_mut().chain(e.value.iter_mut()).for_each(crate::pcb::half_turn_layout);
                e.fields.iter_mut().for_each(|f| crate::pcb::half_turn_layout(&mut f.layout));
            }
        }
        let key = dedup_footprint_key(&mut explicit, &lib_id, &definition.pads, &definition.models3d, &definition.art);
        definition.name = key.clone();
        explicit.entry(key.clone()).or_insert(definition);

        parts.push(Part { reference: reference.clone(), mpn: None, lcsc: None, value, package: None, footprint: Some(key), pins, body_nm: None, symbol: None, datasheet: None, edge: None });
        footprints_ir.push(FootprintInstance { id: reference, at: Point { x, y }, rot, side, label: Default::default() });
        refs.footprints.push(ItemRef::of(fp));
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    edits.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((footprints_ir, parts, explicit, pin_nets, extras, edits))
}

/// Footprint-level solder-mask facts: `(solder_mask_margin ..)` (pre-9.0
/// files: 0 meant "inherit"), `(attr .. allow_soldermask_bridges)`,
/// `(net_tie_pad_groups ..)`.
fn footprint_extra_header(fp: &[Sexpr], reference: &str, file_version: i64) -> FootprintExtra {
    let mut e = FootprintExtra { id: reference.to_string(), ..Default::default() };
    e.uuid = sexpr::find(fp, "uuid").or_else(|| sexpr::find(fp, "tstamp")).and_then(|u| sexpr::txt(u, 1)).filter(|u| !u.is_empty()).map(str::to_string);
    // `(footprint "D5" ..)`: a name with no `Library:` in front of it is a footprint with no library link (`LIB_ID::GetLibNickname()` is empty).
    let name = sexpr::txt(fp, 1).unwrap_or_default();
    if !name.contains(':') {
        e.unlinked_name = Some(name.to_string());
    }
    // The tokens nothing else reads or regenerates, as the file had them (see `FootprintExtra::tokens`).
    // (`solder_mask_margin` is `FootprintExtra::solder_mask_margin`, which the writer makes it from; `thermal_width` and `thermal_gap` are read and dropped by KiCad itself.)
    const OWN: &[&str] = &[
        "layer", "uuid", "tstamp", "tedit", "at", "transform", "locked", "attr", "clearance", "zone_connect", "solder_mask_margin", "thermal_width", "thermal_gap", "net_tie_pad_groups", "fp_line", "fp_arc",
        "fp_circle", "fp_rect", "fp_poly", "fp_curve", "fp_text", "pad", "model", "zone", "group", "component_classes", "sheetname", "sheetfile", "units", "path", "embedded_fonts", "embedded_files",
    ];
    for item in fp.iter().skip(2).filter_map(Sexpr::as_list) {
        let tag = sexpr::tag(item);
        let kept = match tag {
            Some("property") => matches!(sexpr::txt(item, 1), Some("Datasheet") | Some("Description") | Some("ki_fp_filters") | Some("ki_keywords") | Some("ki_locked") | Some("ki_description")),
            // In pre-9.0 files a paste margin of 0 meant "inherit" (the parser reads it so, `parseFOOTPRINT_unchecked`); written to a new file it would be a margin of its own.
            Some("solder_paste_margin") | Some("solder_paste_ratio") | Some("solder_paste_margin_ratio") => !(file_version <= 20240201 && sexpr::num(item, 1) == Some(0.0)),
            // KiCad 10's typed model, the Extruded 3D Body (`(model (type extruded) (overall_height ..) (body_pcb_gap ..) ..)`: `parseFOOTPRINT`'s `case T_model:` "Typed
            // model", written by `format( FOOTPRINT )`'s "Save extruded 3D body info."): it has no path, so `models_from` does not take it; kept as the file wrote it.
            Some("model") => sexpr::txt(item, 1).is_none(),
            Some(t) => !OWN.contains(&t),
            None => false,
        };
        if kept {
            e.tokens.push(sexpr::to_text(&Sexpr::List(item.to_vec())));
        }
    }
    e.source_name = name.to_string();
    // The files the footprint lists (`(embedded_files ..)` inside the footprint: names, types and checksums; the data is the board's).
    e.embedded = crate::embedded::read_owner(fp, &mut Vec::new());
    // `(component_classes (class "A") (class "B"))`: the classes the file gives the footprint (`parseFOOTPRINT_unchecked`'s `T_component_classes`), without repeats.
    if let Some(classes) = sexpr::find(fp, "component_classes") {
        for name in sexpr::find_all(classes, "class").filter_map(|c| sexpr::txt(c, 1)) {
            if !e.component_classes.iter().any(|known| known == name) {
                e.component_classes.push(name.to_string());
            }
        }
    }
    // The sheet the footprint's symbol is on.
    e.sheetname = sexpr::find(fp, "sheetname").and_then(|s| sexpr::txt(s, 1)).map(str::to_string);
    e.sheetfile = sexpr::find(fp, "sheetfile").and_then(|s| sexpr::txt(s, 1)).map(str::to_string);
    // `(units (unit (name "A") (pins "1" "2" ..)) ..)`: the footprint's gates (`parseFOOTPRINT_unchecked`'s `T_units`), which Swap Gate Nets reads.
    if let Some(units) = sexpr::find(fp, "units") {
        for unit in sexpr::find_all(units, "unit") {
            let name = sexpr::find(unit, "name").and_then(|n| sexpr::txt(n, 1)).unwrap_or_default().to_string();
            let pins: Vec<String> = sexpr::find(unit, "pins").map(|p| p.iter().skip(1).filter_map(|x| x.text().map(str::to_string)).collect()).unwrap_or_default();
            e.units.push(eda_model::fp_units::FootprintUnit { name, pins });
        }
    }
    // A footprint with nothing on its courtyard layers has no courtyard (`BuildCourtyardCaches` finds none, and kicad-cli says `missing_courtyard` or nothing).
    e.no_courtyard = !["fp_line", "fp_arc", "fp_rect", "fp_circle", "fp_poly", "fp_curve"].iter().any(|tag| sexpr::find_all(fp, tag).any(|it| matches!(sexpr::find(it, "layer").and_then(|l| sexpr::txt(l, 1)), Some("F.CrtYd") | Some("B.CrtYd"))));
    // The courtyard graphics as the file wrote them, in file order (see `FootprintExtra::courtyard_items`).
    e.courtyard_items = fp
        .iter()
        .skip(2)
        .filter_map(Sexpr::as_list)
        .filter(|it| matches!(sexpr::tag(it), Some("fp_line") | Some("fp_arc") | Some("fp_circle") | Some("fp_rect") | Some("fp_poly") | Some("fp_curve")))
        .filter(|it| matches!(sexpr::find(it, "layer").and_then(|l| sexpr::txt(l, 1)), Some("F.CrtYd") | Some("B.CrtYd")))
        .map(|it| sexpr::to_text(&Sexpr::List(it.to_vec())))
        .collect();
    e.courtyard_bottom = sexpr::find(fp, "layer").and_then(|l| sexpr::txt(l, 1)) == Some("B.Cu");
    if let Some(m) = sexpr::find(fp, "solder_mask_margin").and_then(|f| sexpr::num(f, 1)) {
        let m = mm_to_nm(m);
        e.solder_mask_margin = if file_version <= 20240201 && m == Nm::ZERO { None } else { Some(m) };
    }
    if let Some(attr) = sexpr::find(fp, "attr") {
        e.allow_soldermask_bridges = attr.iter().skip(1).filter_map(Sexpr::text).any(|t| t == "allow_soldermask_bridges");
    }
    if let Some(g) = sexpr::find(fp, "net_tie_pad_groups") {
        e.net_tie_pad_groups = g.iter().skip(1).filter_map(Sexpr::text).map(String::from).collect();
    }
    // `(zone_connect N)` (`FOOTPRINT::SetLocalZoneConnection`); the legacy `(thermal_width ..)` / `(thermal_gap ..)` are
    // read and dropped by KiCad itself ("never exposed in the GUI").
    e.zone_connection = sexpr::find(fp, "zone_connect").and_then(|z| sexpr::num(z, 1)).and_then(|v| crate::zone_connection_from_file(v as i64));
    // `(clearance C)` (`FOOTPRINT::SetLocalClearance`); in pre-9.0 files 0 meant "inherit".
    if let Some(c) = sexpr::find(fp, "clearance").and_then(|f| sexpr::num(f, 1)) {
        let c = mm_to_nm(c);
        e.clearance = if file_version <= 20240201 && c == Nm::ZERO { None } else { Some(c) };
    }
    e
}

/// A pad's `(layers ..)` with wildcards expanded against the board's copper
/// layers (`*.Cu`, `F&B.Cu`, `*.Mask`, ...).
fn expand_layer_names(names: &[&str], copper: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        match *n {
            "*.Cu" => out.extend(copper.iter().cloned()),
            "F&B.Cu" => out.extend(["F.Cu".to_string(), "B.Cu".to_string()]),
            w if w.starts_with("*.") => {
                out.push(format!("F.{}", &w[2..]));
                out.push(format!("B.{}", &w[2..]));
            }
            other => out.push(other.to_string()),
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `PADSTACK` facts beyond geometry: layer set, `(solder_mask_margin ..)`
/// (pre-9.0: 0 = inherit), `(tenting ..)`, `(pintype ..)`.
fn pad_mask_info(pad: &[Sexpr], copper: &[String], file_version: i64, side: Side) -> PadMaskInfo {
    let mut info = PadMaskInfo::default();
    if let Some(l) = sexpr::find(pad, "layers") {
        let names: Vec<&str> = l.iter().skip(1).filter_map(Sexpr::text).collect();
        info.layers = expand_layer_names(&names, copper);
        // The layers beyond copper, paste and mask, as for a top-side footprint (see `PadMaskInfo::art_layers`).
        info.art_layers = info.layers.iter().filter(|l| !(l.ends_with(".Cu") || l.ends_with(".Mask") || l.ends_with(".Paste"))).map(|l| eda_model::fp_art::layer_on_side(l, side)).collect();
        info.art_layers.sort();
        // What the pad lacks of the layers its kind has: an SMD pad's paste (every SMD pad written here has its side's), a pad's mask. An edge connector pad is an SMD pad
        // with mask on both sides (`connector`).
        info.connector = sexpr::txt(pad, 2) == Some("connect");
        let smd = matches!(sexpr::txt(pad, 2), Some("smd") | Some("connect"));
        info.no_paste = smd && !info.layers.iter().any(|l| l.ends_with(".Paste"));
        // An SMD pad on several copper layers keeps its set (`PadMaskInfo::smd_layers`).
        if sexpr::txt(pad, 2) == Some("smd") && info.layers.iter().filter(|l| l.ends_with(".Cu")).count() > 1 {
            info.smd_layers = info.layers.iter().filter(|l| l.ends_with(".Cu") || l.ends_with(".Mask") || l.ends_with(".Paste")).map(|l| eda_model::fp_art::layer_on_side(l, side)).collect();
            info.smd_layers.sort();
        }
        info.no_mask = !info.layers.iter().any(|l| l.ends_with(".Mask"));
    }
    if let Some(m) = sexpr::find(pad, "solder_mask_margin").and_then(|f| sexpr::num(f, 1)) {
        let m = mm_to_nm(m);
        info.solder_mask_margin = if file_version <= 20240201 && m == Nm::ZERO { None } else { Some(m) };
    }
    if let Some(t) = sexpr::find(pad, "tenting") {
        let (f, b) = front_back_opt_bool(t);
        info.tent_front = f;
        info.tent_back = b;
    }
    if let Some(t) = sexpr::find(pad, "pintype").and_then(|f| sexpr::txt(f, 1)) {
        info.pin_type = t.to_string();
    }
    if let Some(f) = sexpr::find(pad, "pinfunction").and_then(|f| sexpr::txt(f, 1)) {
        info.pin_function = f.to_string();
    }
    // Every other token of the pad, as the file had it (see `PadMaskInfo::tokens`): the ones the pad's fields and the shape reader stand for are not among them.
    const OWN: &[&str] = &[
        "at", "size", "rect_delta", "roundrect_rratio", "chamfer_ratio", "chamfer", "drill", "layers", "net", "pinfunction", "pintype", "solder_mask_margin", "solder_paste_margin", "solder_paste_margin_ratio",
        "clearance", "zone_connect", "thermal_bridge_width", "thermal_width", "thermal_bridge_angle", "thermal_gap", "options", "primitives", "uuid", "tstamp",
        // The padstack is the pad's own (`Pad::ext.stack`): its layers and what becomes of the layers nothing connects to.
        "padstack", "remove_unused_layers", "keep_end_layers",
    ];
    info.tokens = pad.iter().skip(1).filter_map(Sexpr::as_list).filter(|l| !sexpr::tag(l).is_some_and(|t| OWN.contains(&t))).map(|l| sexpr::to_text(&Sexpr::List(l.to_vec()))).collect();
    // `parsePAD`: `(zone_connect N)`, `(thermal_bridge_width ..)` (legacy `(thermal_width ..)`), `(thermal_bridge_angle ..)`, `(thermal_gap ..)`.
    if let Some(c) = sexpr::find(pad, "clearance").and_then(|f| sexpr::num(f, 1)) {
        let c = mm_to_nm(c);
        info.clearance = if file_version <= 20240201 && c == Nm::ZERO { None } else { Some(c) };
    }
    info.zone_connection = sexpr::find(pad, "zone_connect").and_then(|z| sexpr::num(z, 1)).and_then(|v| crate::zone_connection_from_file(v as i64));
    info.thermal_spoke_width = sexpr::find(pad, "thermal_bridge_width").or_else(|| sexpr::find(pad, "thermal_width")).and_then(|f| sexpr::num(f, 1)).map(mm_to_nm);
    info.thermal_spoke_angle_mdeg = sexpr::find(pad, "thermal_bridge_angle").and_then(|f| sexpr::num(f, 1)).map(|d| ((d * 1000.0).round() as i64).rem_euclid(360_000) as eda_model::ir::Millideg);
    info.thermal_gap = sexpr::find(pad, "thermal_gap").and_then(|f| sexpr::num(f, 1)).map(mm_to_nm);
    info
}

fn rotated_extent(rot_millideg: i64, size: (Nm, Nm)) -> (Nm, Nm) {
    let rad = (rot_millideg as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let (w, h) = (size.0.as_f64(), size.1.as_f64());
    (Nm::kiround(w * cos.abs() + h * sin.abs()), Nm::kiround(w * sin.abs() + h * cos.abs()))
}

/// A non-plated hole whose `(layers ..)` name no copper layer (the mounting holes and plugs of `*.Mask`-only pads): no pad copper, but a drilled hole that zones keep clear of
/// on every copper layer (`ZONE_FILLER::knockoutThermalReliefs`'s `npthWithHole`). `None` for any other pad.
fn layerless_hole(pad: &[Sexpr], fp_side: Side, fp_rot: u32, file_version: i64, notes: &mut ImportNotes) -> Option<eda_model::ir::FootprintHole> {
    if sexpr::txt(pad, 2) != Some("np_thru_hole") {
        return None;
    }
    let p = parse_pad_geometry_opt(pad, fp_side, fp_rot, notes, true)?;
    if p.drill.is_none() && p.drill_slot.is_none() {
        return None;
    }
    // `(clearance C)` on the pad (`PAD::SetLocalClearance`); in pre-9.0 files 0 meant "inherit".
    let clearance = sexpr::find(pad, "clearance").and_then(|f| sexpr::num(f, 1)).map(mm_to_nm).filter(|&c| !(file_version <= 20240201 && c == Nm::ZERO));
    Some(eda_model::ir::FootprintHole { pad: p, clearance })
}

/// A pad with no copper layer, as the file wrote it, its `(net ..)` taken out and named ([`eda_model::ir::FootprintAuxPad`]); `None` for a pad with copper or no
/// `(layers ..)` (one [`parse_pad_geometry_in`] keeps or cannot place).
fn aux_pad(pad: &[Sexpr], net_names: &BTreeMap<i64, String>) -> Option<eda_model::ir::FootprintAuxPad> {
    let names: Vec<&str> = sexpr::find(pad, "layers")?.iter().skip(1).filter_map(Sexpr::text).collect();
    if names.is_empty() || names.iter().any(|l| l.ends_with(".Cu")) {
        return None;
    }
    let net = net_ref(sexpr::find(pad, "net"), net_names);
    let kept: Vec<Sexpr> = pad.iter().filter(|c| !c.as_list().is_some_and(|l| sexpr::tag(l) == Some("net"))).cloned().collect();
    Some(eda_model::ir::FootprintAuxPad { text: sexpr::to_text(&Sexpr::List(kept)), net })
}

/// A pad with no copper layer but a mask layer is a pure mask aperture
/// (`isMaskAperture`'s `maskLayers.count() > 0 && copperLayers.count() == 0`),
/// still a `PCB_PAD_T` for the solder-mask provider. Reduced to the same
/// axis-aligned board-space shape every other pad here gets.
fn mask_only_pad_graphics(pad: &[Sexpr], fp: &FootprintInstance, fp_rot: u32, net_names: &BTreeMap<i64, String>, notes: &mut ImportNotes) -> Option<Vec<FootprintGraphic>> {
    let names: Vec<&str> = sexpr::find(pad, "layers")?.iter().skip(1).filter_map(Sexpr::text).collect();
    let mask_layers: Vec<String> = expand_layer_names(&names, &[]).into_iter().filter(|l| l == "F.Mask" || l == "B.Mask").collect();
    if mask_layers.is_empty() {
        return None;
    }
    let p = parse_pad_geometry_opt(pad, fp.side, fp_rot, notes, true)?;
    let center = eda_model::footprint::to_board(fp, p.at);
    let (w, h) = rotated_extent(fp.rot as i64 + p.rot as i64, p.size);
    let net = net_ref(sexpr::find(pad, "net"), net_names);
    let pin_type = sexpr::find(pad, "pintype").and_then(|f| sexpr::txt(f, 1)).unwrap_or("").to_string();
    let mut out = Vec::new();
    for layer in mask_layers {
        let shape = match p.shape {
            PadShape::Circle => Shape::Circle { id: String::new(), layer, stroke_width: Nm::ZERO, filled: true, center, end: Point { x: center.x + w.max(h) / 2, y: center.y } },
            PadShape::Oval => {
                let (a, b, sw) = if w >= h {
                    (Point { x: center.x - (w - h) / 2, y: center.y }, Point { x: center.x + (w - h) / 2, y: center.y }, h)
                } else {
                    (Point { x: center.x, y: center.y - (h - w) / 2 }, Point { x: center.x, y: center.y + (h - w) / 2 }, w)
                };
                Shape::Segment { id: String::new(), layer, stroke_width: sw, filled: true, start: a, end: b }
            }
            _ => Shape::Polygon {
                id: String::new(),
                layer,
                stroke_width: Nm::ZERO,
                filled: true,
                pts: vec![Point { x: center.x - w / 2, y: center.y - h / 2 }, Point { x: center.x + w / 2, y: center.y - h / 2 }, Point { x: center.x + w / 2, y: center.y + h / 2 }, Point { x: center.x - w / 2, y: center.y + h / 2 }],
            },
        };
        out.push(FootprintGraphic { shape, solder_mask_margin: None, pad_number: Some(p.number.clone()), net: net.clone(), pin_type: pin_type.clone() });
    }
    Some(out)
}

/// A board-level `(gr_text ..)` on a silkscreen layer, laid out like a
/// footprint text (board-level text is never kept upright).
fn import_board_silk_texts(root: &[Sexpr]) -> Vec<FootprintText> {
    let ident = |p: Point| p;
    let vars = title_block_vars(root);
    let mut out = Vec::new();
    for item in sexpr::find_all(root, "gr_text") {
        let Some(raw) = sexpr::txt(item, 1) else { continue };
        out.extend(parse_silk_text(item, &resolve_text_vars(raw, &vars), &ident, false));
    }
    out
}

/// A board-level `(gr_text ..)` on a copper layer (`PCB_TEXT` is copper there).
fn import_board_copper_texts(root: &[Sexpr]) -> Vec<FootprintText> {
    let ident = |p: Point| p;
    let vars = title_block_vars(root);
    let mut out = Vec::new();
    for item in sexpr::find_all(root, "gr_text") {
        // A hidden text has no copper (`IsVisible()`).
        if sexpr::find(item, "hide").is_some() || item.iter().any(|c| matches!(c, Sexpr::Atom(a) if a == "hide")) {
            continue;
        }
        let Some(raw) = sexpr::txt(item, 1) else { continue };
        out.extend(parse_text_on(item, &resolve_text_vars(raw, &vars), &ident, false, &|l| l.ends_with(".Cu")));
    }
    out
}

/// The `(title_block ..)` text variables (`BOARD::GetTextVar`): `TITLE`,
/// `ISSUE_DATE`, `REVISION`, `COMPANY`, `COMMENT1..9`.
fn title_block_vars(root: &[Sexpr]) -> Vec<(String, String)> {
    let mut v = Vec::new();
    let Some(tb) = sexpr::find(root, "title_block") else { return v };
    for (tag, name) in [("title", "TITLE"), ("date", "ISSUE_DATE"), ("rev", "REVISION"), ("company", "COMPANY")] {
        if let Some(t) = sexpr::find(tb, tag).and_then(|f| sexpr::txt(f, 1)) {
            v.push((name.to_string(), t.to_string()));
        }
    }
    for c in sexpr::find_all(tb, "comment") {
        if let (Some(n), Some(t)) = (sexpr::num(c, 1), sexpr::txt(c, 2)) {
            v.push((format!("COMMENT{}", n as i64), t.to_string()));
        }
    }
    v
}

/// `${NAME}` -> value for every known variable; an unknown one stays as written.
fn resolve_text_vars(text: &str, vars: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("${{{k}}}"), v);
    }
    out
}

/// The shared tail of `parsePCB_TEXT` / `parsePCB_TEXT_effects`: `(at x y [angle])`,
/// `(layer ..)`, `(effects (font (size ..) (thickness ..) bold) (justify ..))`.
/// Only silkscreen text is kept. `tf` maps the file anchor to board space.
fn parse_silk_text(item: &[Sexpr], text: &str, tf: &dyn Fn(Point) -> Point, keep_upright_default: bool) -> Option<FootprintText> {
    parse_text_on(item, text, tf, keep_upright_default, &|l| l == "F.SilkS" || l == "B.SilkS")
}

/// [`parse_silk_text`] for any layer `accept` takes.
fn parse_text_on(item: &[Sexpr], text: &str, tf: &dyn Fn(Point) -> Point, keep_upright_default: bool, accept: &dyn Fn(&str) -> bool) -> Option<FootprintText> {
    let layer = sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1))?;
    if !accept(layer) {
        return None;
    }
    // KiCad's lexer turns the two characters `\n` of a quoted string into a newline.
    let text = text.replace("\\n", "\n");
    if text.is_empty() {
        return None;
    }
    let at = sexpr::find(item, "at")?;
    let (x, y) = (sexpr::num(at, 1)?, sexpr::num(at, 2)?);
    let angle = sexpr::num(at, 3).unwrap_or(0.0);
    let unlocked_legacy = at.iter().skip(1).any(|c| matches!(c, Sexpr::Atom(a) if a == "unlocked"));
    let unlocked = unlocked_legacy || sexpr::find(item, "unlocked").is_some_and(|u| sexpr::txt(u, 1).map_or(true, |v| v == "yes"));
    let effects = sexpr::find(item, "effects");
    let font = effects.and_then(|e| sexpr::find(e, "font"));
    let size = font.and_then(|f| sexpr::find(f, "size")).map(|s| (mm_to_nm(sexpr::num(s, 1).unwrap_or(1.0)), mm_to_nm(sexpr::num(s, 2).unwrap_or(1.0)))).unwrap_or((mm(1.0), mm(1.0)));
    let thickness = font.and_then(|f| sexpr::find(f, "thickness")).and_then(|t| sexpr::num(t, 1)).map(mm_to_nm).unwrap_or(Nm::ZERO);
    let bold = font.is_some_and(|f| f.iter().skip(1).any(|c| matches!(c, Sexpr::Atom(a) if a == "bold")) || sexpr::find(f, "bold").is_some_and(|b| sexpr::txt(b, 1).map_or(true, |v| v == "yes")));
    let (mut halign, mut valign, mut mirror) = (0i8, 0i8, false);
    if let Some(j) = effects.and_then(|e| sexpr::find(e, "justify")) {
        for tok in j.iter().skip(1).filter_map(Sexpr::text) {
            match tok {
                "left" => halign = -1,
                "right" => halign = 1,
                "top" => valign = -1,
                "bottom" => valign = 1,
                "mirror" => mirror = true,
                _ => {}
            }
        }
    }
    Some(FootprintText {
        text,
        layer: layer.to_string(),
        at: tf(Point { x: mm_to_nm(x), y: mm_to_nm(y) }),
        angle_file_mdeg: (angle * 1000.0).round() as i64,
        size,
        thickness,
        halign,
        valign,
        mirror,
        keep_upright: keep_upright_default && !unlocked,
        bold,
    })
}

// ---------------------------------------------------------------- routing

fn import_routing(root: &[Sexpr], net_names: &BTreeMap<i64, String>, notes: &mut ImportNotes, refs: &mut Refs) -> (Vec<Track>, Vec<Option<eda_model::track_mask::TrackMask>>, Vec<Via>, Vec<ViaTenting>, Vec<eda_model::via_extra::ViaExtra>) {

    let mut tracks = Vec::new();
    // The solder-mask opening of each track, one per track in the same order, for the caller to name once the tracks have ids.
    let mut track_masks = Vec::new();
    for seg in sexpr::find_all(root, "segment") {
        let (Some(s), Some(e)) = (sexpr::find(seg, "start").and_then(xy_point), sexpr::find(seg, "end").and_then(xy_point)) else { continue };
        let net = net_ref(sexpr::find(seg, "net"), net_names);
        // Net 0 (no net) stays as an empty `net`: KiCad still checks a
        // netless track (clearance, shorting, dangling) like any other.
        let width = sexpr::find(seg, "width").and_then(|w| sexpr::num(w, 1)).map(mm_to_nm).unwrap_or(mm(0.2));
        let (layer, mask) = cu_item_layers(seg).unwrap_or_else(|| ("F.Cu".to_string(), false));
        track_masks.push(mask.then(|| eda_model::track_mask::TrackMask { id: String::new(), margin: sexpr::find(seg, "solder_mask_margin").and_then(|m| sexpr::num(m, 1)).map(mm_to_nm) }));
        tracks.push(Track { id: String::new(), net, pins: vec![], layer, width, pts: vec![s, e], arc_mid_offset: None });
        refs.tracks.push(ItemRef::of(seg));
    }

    for arc in sexpr::find_all(root, "arc") {
        let (Some(s), Some(m), Some(e)) =
            (sexpr::find(arc, "start").and_then(xy_point), sexpr::find(arc, "mid").and_then(xy_point), sexpr::find(arc, "end").and_then(xy_point))
        else {
            continue;
        };
        let net = net_ref(sexpr::find(arc, "net"), net_names);
        let width = sexpr::find(arc, "width").and_then(|w| sexpr::num(w, 1)).map(mm_to_nm).unwrap_or(mm(0.2));
        let (layer, mask) = cu_item_layers(arc).unwrap_or_else(|| ("F.Cu".to_string(), false));
        track_masks.push(mask.then(|| eda_model::track_mask::TrackMask { id: String::new(), margin: sexpr::find(arc, "solder_mask_margin").and_then(|m| sexpr::num(m, 1)).map(mm_to_nm) }));
        notes.track_arcs_kept += 1;
        tracks.push(Track::new_arc(net, layer, width, s, m, e));
        refs.tracks.push(ItemRef::of(arc));
    }

    let mut vias = Vec::new();
    let mut via_tenting = Vec::new();
    // What each via says beyond `Via`'s fields, one per via in the same order, for the caller to name once the vias have ids.
    let mut via_extras = Vec::new();
    for via in sexpr::find_all(root, "via") {
        let Some(at) = sexpr::find(via, "at").and_then(xy_point) else { continue };
        let net = net_ref(sexpr::find(via, "net"), net_names);
        let dia = sexpr::find(via, "size").and_then(|s| sexpr::num(s, 1)).map(mm_to_nm).unwrap_or(mm(0.6));
        let drill = sexpr::find(via, "drill").and_then(|d| sexpr::num(d, 1)).map(mm_to_nm).unwrap_or(mm(0.3));
        let (from_layer, to_layer) = sexpr::find(via, "layers")
            .map(|l| (sexpr::txt(l, 1).unwrap_or("F.Cu").to_string(), sexpr::txt(l, 2).unwrap_or("B.Cu").to_string()))
            .unwrap_or_else(|| ("F.Cu".into(), "B.Cu".into()));
        // `(tenting ..)`: an explicit per-via override of the board's via
        // tenting (`PCB_VIA::IsTented`); `none` inherits the board setting.
        if let Some(t) = sexpr::find(via, "tenting") {
            let (front, back) = front_back_opt_bool(t);
            if front.is_some() || back.is_some() {
                via_tenting.push(ViaTenting { at, net: net.clone(), front, back });
            }
        }
        vias.push(Via { id: String::new(), net, at, drill, diameter: dia, from_layer, to_layer });
        via_extras.push(crate::via_io::read(via));
        refs.vias.push(ItemRef::of(via));
    }

    (tracks, track_masks, vias, via_tenting, via_extras)
}

// Track arcs go through `Track::new_arc` (eda_model), which keeps the true
// `mid` so DRC and the exporter can treat the arc as one `PCB_ARC` item (a
// single DRC item, no `tracks_crossing` special case --
// `drc_test_provider_copper_clearance.cpp` only takes that path for two
// `PCB_TRACE_T`s) instead of 32 chords.

// --------------------------------------------------------- drawings

/// Board-level graphics (`gr_line`/`gr_arc`/`gr_rect`/`gr_circle`/
/// `gr_poly`/`gr_curve`) and text (`gr_text`), any layer. Those on Edge.Cuts are
/// the board outline's items; [`import_outline`] decides what becomes of them.
fn import_drawings(root: &[Sexpr], net_names: &BTreeMap<i64, String>, refs: &mut Refs) -> (Vec<Shape>, Vec<Text>, Vec<eda_model::board_text::TextExtra>) {
    let mut shapes = Vec::new();
    // A shape's `(net ..)` (`parsePCB_SHAPE`'s `T_net`): the net of a shape on copper. Net 0, the empty name, is no net.
    let shape_ref = |item: &[Sexpr]| ItemRef { net: Some(net_ref(sexpr::find(item, "net"), net_names)).filter(|n| !n.is_empty()), ..ItemRef::of(item) };
    for item in sexpr::find_all(root, "gr_line") {
        if let Some(shape) = graphic_shape(item, "gr_line", |p| p) {
            shapes.push(shape);
            refs.shapes.push(shape_ref(item));
        }
    }
    for tag in ["gr_arc", "gr_rect", "gr_circle", "gr_poly", "gr_curve"] {
        for item in sexpr::find_all(root, tag) {
            if let Some(shape) = graphic_shape(item, tag, |p| p) {
                shapes.push(shape);
                refs.shapes.push(shape_ref(item));
            }
        }
    }

    // Each text's looks: `Text` holds the square size, the stroke width and the horizontal justification, and the rest of `(effects ..)` (a glyph width that is not the
    // height, a vertical justification, bold, italic, a knockout, ..) goes to `extras`, one per text in the same order, for the caller to name once the texts have ids.
    let mut texts = Vec::new();
    let mut extras = Vec::new();
    for item in sexpr::find_all(root, "gr_text") {
        let Some(content) = sexpr::txt(item, 1) else { continue };
        let Some(at) = sexpr::find(item, "at") else { continue };
        let (Some(x), Some(y)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
        let angle = ((sexpr::num(at, 3).unwrap_or(0.0) * 1000.0).round() as i64).rem_euclid(360_000) as u32;
        let style = crate::board_text_io::read_style(item);
        texts.push(Text { id: String::new(), content: content.to_string(), at: Point { x: mm_to_nm(x), y: mm_to_nm(y) }, angle, layer: layer_of(item), size_nm: style.size_nm, stroke_width: style.stroke_width, justify: style.justify, mirror: style.mirror });
        extras.push(style.extra);
        refs.texts.push(ItemRef::of(item));
    }

    (shapes, texts, extras)
}

/// An item's `(stroke (width ..))`, or the `(width ..)` of the files before it (KiCad 6 and older), micrometres.
fn stroke_width_of(item: &[Sexpr]) -> Nm {
    sexpr::find(item, "stroke").and_then(|s| sexpr::find(s, "width")).or_else(|| sexpr::find(item, "width")).and_then(|w| sexpr::num(w, 1)).map(mm_to_nm).unwrap_or(Nm::ZERO)
}

/// The arc of a file written before `LEGACY_ARC_FORMATTING` (20210925), `(start CENTER) (end ARC_START) (angle A)` in degrees, as the
/// three points of the arc (`parsePCB_SHAPE`: `SetCenter`, `SetStart`, `SetArcAngleAndEnd( A )` -- the end is the start turned by A, the
/// middle by half of it, the same way, in the file's own axes). Returns (start, mid, end).
pub(crate) fn legacy_arc(center: Point, start: Point, angle_deg: f64) -> (Point, Point, Point) {
    let turn = |deg: f64| -> Point {
        let (s, c) = deg.to_radians().sin_cos();
        let (dx, dy) = ((start.x - center.x).as_f64(), (start.y - center.y).as_f64());
        Point { x: center.x + Nm::kiround(dx * c - dy * s), y: center.y + Nm::kiround(dx * s + dy * c) }
    };
    (start, turn(angle_deg / 2.0), turn(angle_deg))
}

/// An item's `(layer ..)`, or the copper layer of the `(layers "F.Cu" "F.Mask")` a track or a copper shape with a solder-mask opening has instead (see [`cu_item_layers`]).
fn layer_of(item: &[Sexpr]) -> String {
    cu_item_layers(item).map(|(layer, _)| layer).unwrap_or_else(|| "Cmts.User".to_string())
}

/// The layer of a track, an arc or a board shape, and whether it has a solder-mask opening. KiCad 9 writes such an item with a mask opening as `(layers "B.Cu" "B.Mask")`
/// instead of `(layer "B.Cu")` (`pcb_io_kicad_sexpr.cpp::format( PCB_TRACK )` and `format( PCB_SHAPE )`: `formatLayers( GetLayerSet() )` when the set has more than one layer),
/// and the parser gives the item every copper layer of the set as its layer and a mask opening for a mask layer (`parseLayersForCuItemWithSoldermask`, `PCB_TRACK::SetLayerSet`,
/// `PCB_SHAPE::SetLayerSet`: `RunOnLayers`, `IsCopperLayer` then `SetLayer`, `IsSolderMaskLayer` then `SetHasSolderMask( true )`). A set with no copper layer keeps its first
/// layer (a shape's `(layers ..)` of KiCad 10's other kinds). `None` when the item names no layer.
pub(crate) fn cu_item_layers(item: &[Sexpr]) -> Option<(String, bool)> {
    if let Some(layer) = sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)) {
        return Some((layer.to_string(), false));
    }
    let names: Vec<&str> = sexpr::find(item, "layers").map(|l| (1..l.len()).filter_map(|i| sexpr::txt(l, i)).collect()).unwrap_or_default();
    let mask = names.iter().any(|n| n.ends_with(".Mask"));
    let layer = names.iter().rev().find(|n| n.ends_with(".Cu")).or(names.first())?;
    Some((layer.to_string(), mask))
}

/// `(fill yes|solid)`.
fn filled_of(item: &[Sexpr]) -> bool {
    sexpr::find(item, "fill").and_then(|f| sexpr::txt(f, 1)).is_some_and(|s| s == "yes" || s == "solid")
}

/// One `PCB_SHAPE` item (`gr_line`, `gr_arc`, `gr_rect`, `gr_circle`, `gr_poly`, `gr_curve`, or the footprint-level `fp_` form of
/// each: `tag` is the board-level name) as a [`Shape`]; every point goes through `at`, which takes a footprint's local frame to the
/// board's. `None` when the item lacks what it needs.
pub(crate) fn graphic_shape(item: &[Sexpr], tag: &str, at: impl Fn(Point) -> Point) -> Option<Shape> {
    let pt = |name: &str| sexpr::find(item, name).and_then(xy_point).map(&at);
    let (layer, stroke_width, filled) = (layer_of(item), stroke_width_of(item), filled_of(item));
    Some(match tag {
        "gr_line" => Shape::Segment { id: String::new(), layer, stroke_width, filled, start: pt("start")?, end: pt("end")? },
        "gr_arc" => match sexpr::find(item, "angle").and_then(|a| sexpr::num(a, 1)).filter(|_| sexpr::find(item, "mid").is_none()) {
            // The legacy grammar: `start` is the centre and `end` the start of the arc.
            Some(angle) => {
                let (start, mid, end) = legacy_arc(pt("start")?, pt("end")?, angle);
                Shape::Arc { id: String::new(), layer, stroke_width, filled, start, mid, end }
            }
            None => Shape::Arc { id: String::new(), layer, stroke_width, filled, start: pt("start")?, mid: pt("mid")?, end: pt("end")? },
        },
        "gr_rect" => Shape::Rect { id: String::new(), layer, stroke_width, filled, start: pt("start")?, end: pt("end")? },
        "gr_circle" => Shape::Circle { id: String::new(), layer, stroke_width, filled, center: pt("center")?, end: pt("end")? },
        "gr_poly" => Shape::Polygon { id: String::new(), layer, stroke_width, filled, pts: poly_points(item)?.into_iter().map(&at).collect() },
        // `(gr_curve (pts (xy start) (xy c1) (xy c2) (xy end)))`: a cubic Bezier (`SHAPE_T::BEZIER`), never filled. The four points as they are (`parsePCB_SHAPE`,
        // `case T_pts` then `SetStart`, `SetBezierC1`, `SetBezierC2`, `SetEnd`): a control point on an end point, which KiCad's curve tool leaves until it is dragged, is no
        // repeated point to drop as in a polygon.
        "gr_curve" => {
            let pts: Vec<Point> = sexpr::find(item, "pts")?.iter().skip(1).filter_map(Sexpr::as_list).filter(|l| sexpr::tag(l) == Some("xy")).filter_map(xy_point).map(&at).collect();
            let [start, c1, c2, end] = pts[..] else { return None };
            Shape::Bezier { id: String::new(), layer, stroke_width, filled: false, start, c1, c2, end }
        }
        _ => return None,
    })
}

// ---------------------------------------------------------------- outline

/// The board outline, from the board-level Edge.Cuts shapes in `shapes` (a footprint's own stay in the footprint).
///
/// KiCad builds the board outline from these (`BOARD::GetBoardPolygonOutlines`, ported in `eda_drc::outline`): lines, arcs, circles, rectangles,
/// polygons and curves chained end to end into closed contours, a contour inside another being a cutout. The common board is lines that chain
/// into one closed loop, and that loop is `placement.outline`, as it always was; its shapes are dropped here, the writer emits the loop as lines.
/// Any other board keeps every item as the shape it is -- an arc is an arc, not the chord of one -- and `placement.outline` is the summary of
/// what they make. An outline that does not chain is kept as it is, too, and reported ([`ImportNotes::outline_errors`]): the exported file then
/// has the same gap, and kicad-cli says so, instead of a loop closed behind the user's back.
///
/// Returns the polygon for `placement.outline`, whether the shapes are the outline, and, when the outline is the loop of lines, the one width they are drawn at
/// (`DrawingsSection::outline_width_nm`; lines of different widths are not one loop of lines here, they stay shapes).
fn import_outline(shapes: &mut Vec<Shape>, refs: &mut Refs, notes: &mut ImportNotes) -> (Vec<Point>, bool, Option<Nm>) {
    // A footprint's own Edge.Cuts stay in the footprint, as its art (`fp_art_io`), where KiCad keeps them (`FOOTPRINT::GraphicalItems`): they move with it, and the
    // outline takes them in with `BuildBoardPolygonOutlines`' footprint pass (`eda_drc::outline::board_outline_with`).
    let items: Vec<Shape> = shapes.iter().filter(|s| edge::is_edge_cuts(s)).cloned().collect();
    if items.is_empty() {
        notes.outline_source = "none";
        return (Vec::new(), false, None);
    }

    let (polys, chained, _) = eda_drc::outline::convert_outline_to_polygon(&items, edge::MAX_ERROR_NM.as_f64(), edge::CHAINING_EPSILON_NM, true);
    let one_loop = chained && polys.outline_count() == 1 && !polys.has_holes();
    let line_widths: std::collections::BTreeSet<Nm> = items.iter().filter_map(|s| if let Shape::Segment { stroke_width, .. } = s { Some(*stroke_width) } else { None }).collect();
    if one_loop && items.iter().all(|s| matches!(s, Shape::Segment { .. })) && line_widths.len() == 1 {
        // The loop, in the order its sides chain; the sides themselves are not kept.
        let ring: Vec<Point> = polys.outline(0).iter().map(|p| Point { x: Nm::new(p.x), y: Nm::new(p.y) }).collect();
        let keep: Vec<bool> = shapes.iter().map(|s| !edge::is_edge_cuts(s)).collect();
        let mut k = keep.iter();
        shapes.retain(|_| *k.next().expect("one flag per shape"));
        let mut k = keep.iter();
        refs.shapes.retain(|_| *k.next().expect("one flag per shape"));
        notes.outline_source = "lines";
        return (ring, false, line_widths.into_iter().next().filter(|w| *w > Nm::ZERO));
    }

    let built = eda_drc::outline::board_outline_of(&items, true);
    notes.outline_source = "shapes";
    notes.outline_open = !built.valid;
    notes.outline_shapes = items.len();
    notes.outline_errors = eda_drc::outline::check_edge_cuts(&items).iter().map(|e| format!("{} at ({:.3}, {:.3}) mm", e.describe(), eda_model::units::to_mm(e.at.x), eda_model::units::to_mm(e.at.y))).collect();
    (built.main_ring(), true, None)
}

// ---------------------------------------------------------------- zones

/// `PCB_IO_KICAD_SEXPR_PARSER::parseZONE`, for board-level `(zone ...)`s:
/// copper pours, teardrops (`(attr (teardrop ...))`) and rule areas
/// (`(keepout ...)`/`(placement ...)`). A zone keeps its layer set
/// (`Zone::layers`: `(layers "F.Cu" "B.Cu")`, `*.Cu`, `F&B.Cu`, `*.Mask`), and a zone
/// with no copper layer in it is a non-copper zone. A zone has one
/// outline and its holes (`Zone::holes`): the first `(polygon ...)` is the outline, the others
/// its holes (`ZONE::AddPolygon`). The stored `(filled_polygon ...)`s are not part of the
/// design: the scan of the file (`eda_kicad::zone_fills::scan_zone_fills`) is handed out in [`ImportNotes::zone_fills`] and kept beside `design.json` as the board's fills (KiCad's
/// own, as saved), which is what kicad-cli's `pcb drc --refill-zones` replaces on the next Fill All Zones. Footprint-level
/// zones are imported too, tagged with their parent footprint.
fn import_zones(root: &[Sexpr], net_names: &BTreeMap<i64, String>, copper_layers: &[String], notes: &mut ImportNotes, refs: &mut Refs) -> Vec<Zone> {
    // Board-level zones, then each footprint's own (`forEachGeometryItem`
    // walks `footprint->Zones()` too; a footprint zone's points are stored
    // in board coordinates). The parent reference is recomputed exactly as
    // `import_footprints` assigns it (a repeat gets a "#n" suffix).
    let mut all: Vec<(&[Sexpr], Option<String>)> = sexpr::find_all(root, "zone").map(|z| (z, None)).collect();
    let mut seen_refs: BTreeMap<String, usize> = BTreeMap::new();
    let raw: Vec<&[Sexpr]> = sexpr::find_all(root, "footprint").chain(sexpr::find_all(root, "module")).collect();
    for (idx, fp) in raw.iter().enumerate() {
        let Some(at) = sexpr::find(fp, "at") else { continue };
        if sexpr::num(at, 1).is_none() || sexpr::num(at, 2).is_none() {
            continue;
        }
        let base_ref = footprint_field(fp, "Reference").filter(|s| !s.is_empty()).unwrap_or_else(|| format!("FP{}", idx + 1));
        let n = seen_refs.entry(base_ref.clone()).or_insert(0usize);
        *n += 1;
        let reference = if *n == 1 { base_ref } else { format!("{base_ref}#{n}") };
        all.extend(sexpr::find_all(fp, "zone").map(|z| (z, Some(reference.clone()))));
    }
    let mut out = Vec::new();
    for (z, parent) in all {
        // `SetIslandRemovalMode( ISLAND_REMOVAL_MODE::ALWAYS )` and priority 0 before parsing.
        let mut zone = Zone { parent_footprint: parent, ..Zone::default() };
        // `(net N)` (code) or, in newer files, `(net "name")`; `(net_name ...)` as a fallback.
        if let Some(n) = sexpr::find(z, "net") {
            zone.net = net_of(n, net_names);
        }
        if zone.net.is_empty() {
            if let Some(n) = sexpr::find(z, "net_name") {
                zone.net = sexpr::txt(n, 1).unwrap_or("").to_string();
            }
        }
        // `T_layer` (a zone on one layer) and `T_layers` (`parseBoardItemLayersAsMask`: the wildcards `*.Cu`, `F&B.Cu` and `*.Mask` stand for the layers they name).
        let mut names: Vec<&str> = Vec::new();
        if let Some(l) = sexpr::find(z, "layer") {
            names.extend(sexpr::txt(l, 1));
        }
        if let Some(l) = sexpr::find(z, "layers") {
            names.extend((1..l.len()).filter_map(|i| sexpr::txt(l, i)));
        }
        let mut layers: Vec<String> = expand_layer_names(&names, copper_layers);
        if let Some(p) = sexpr::find(z, "priority") {
            zone.priority = sexpr::num(p, 1).unwrap_or(0.0).max(0.0) as u32;
        }
        // `(name "..")` and `(hatch none|edge|full <pitch>)`; an absent `hatch` is `NO_HATCH` in the parser.
        if let Some(n) = sexpr::find(z, "name") {
            zone.name = sexpr::txt(n, 1).unwrap_or("").to_string();
        }
        zone.border_style = match sexpr::find(z, "hatch").and_then(|h| sexpr::txt(h, 1)) {
            Some("edge") => eda_model::ir::ZoneBorderStyle::Edge,
            Some("full") => eda_model::ir::ZoneBorderStyle::Full,
            _ => eda_model::ir::ZoneBorderStyle::None,
        };
        if let Some(cp) = sexpr::find(z, "connect_pads") {
            match sexpr::txt(cp, 1) {
                Some("yes") => zone.pad_connection = PadConnection::Full,
                Some("no") => zone.pad_connection = PadConnection::None,
                Some("thru_hole_only") => zone.pad_connection = PadConnection::ThtThermal,
                _ => {}
            }
            if let Some(c) = sexpr::find(cp, "clearance").and_then(|c| sexpr::num(c, 1)) {
                zone.clearance = mm_to_nm(c);
            }
        }
        if let Some(t) = sexpr::find(z, "min_thickness").and_then(|t| sexpr::num(t, 1)) {
            zone.min_thickness = mm_to_nm(t);
        }
        if let Some(f) = sexpr::find(z, "fill") {
            if let Some(m) = sexpr::find(f, "mode") {
                if sexpr::txt(m, 1) == Some("hatch") {
                    zone.fill_mode = FillMode::HatchPattern;
                }
            }
            let um = |k: &str| sexpr::find(f, k).and_then(|v| sexpr::num(v, 1)).map(mm_to_nm);
            let raw = |k: &str| sexpr::find(f, k).and_then(|v| sexpr::num(v, 1));
            if let Some(v) = um("hatch_thickness") {
                zone.hatch_thickness = v;
            }
            if let Some(v) = um("hatch_gap") {
                zone.hatch_gap = v;
            }
            if let Some(v) = raw("hatch_orientation") {
                zone.hatch_orientation_mdeg = ((v * 1000.0).round() as i64).rem_euclid(360_000) as _;
            }
            if let Some(v) = raw("hatch_smoothing_level") {
                zone.hatch_smoothing_level = v as i32;
            }
            if let Some(v) = raw("hatch_smoothing_value") {
                zone.hatch_smoothing_value = v;
            }
            if let Some(v) = raw("hatch_min_hole_area") {
                zone.hatch_hole_min_area = v;
            }
            // `(hatch_border_algorithm hatch_thickness|min_thickness)`: which width the hatch border is drawn with.
            match sexpr::find(f, "hatch_border_algorithm").and_then(|v| sexpr::txt(v, 1)) {
                Some("hatch_thickness") => zone.hatch_border_algorithm = 1,
                Some("min_thickness") => zone.hatch_border_algorithm = 0,
                _ => {}
            }
            if let Some(v) = um("thermal_gap") {
                zone.thermal_gap = v;
            }
            if let Some(v) = um("thermal_bridge_width") {
                zone.thermal_spoke_width = v;
            }
            if let Some(v) = raw("island_removal_mode") {
                zone.island_removal_mode = match v as i64 {
                    1 => IslandRemovalMode::Never,
                    2 => IslandRemovalMode::Area,
                    _ => IslandRemovalMode::Always,
                };
            }
            // `parseBoardUnits( T_island_area_min ) * pcbIUScale.IU_PER_MM`: mm^2 in the file, nm^2 here.
            if let Some(v) = raw("island_area_min") {
                zone.min_island_area = mm_to_nm(v).get() * eda_model::units::NM_PER_MM;
            }
            // `(smoothing none|chamfer|fillet)` and `(radius r)`; both are ignored for a rule area (`parseZONE`).
            let smoothing = sexpr::find(f, "smoothing").and_then(|v| sexpr::txt(v, 1));
            let radius = um("radius");
            if sexpr::find(z, "keepout").is_none() && sexpr::find(z, "placement").is_none() {
                zone.smoothing = match smoothing {
                    Some("chamfer") => eda_model::ir::ZoneSmoothing::Chamfer,
                    Some("fillet") => eda_model::ir::ZoneSmoothing::Fillet,
                    _ => eda_model::ir::ZoneSmoothing::None,
                };
                zone.corner_radius = radius.unwrap_or(Nm::ZERO);
            }
        }
        if let Some(k) = sexpr::find(z, "keepout") {
            zone.is_rule_area = true;
            let not_allowed = |key: &str| sexpr::find(k, key).and_then(|v| sexpr::txt(v, 1)) == Some("not_allowed");
            zone.keepout_tracks = not_allowed("tracks");
            zone.keepout_vias = not_allowed("vias");
            zone.keepout_copper_pour = not_allowed("copperpour");
            zone.keepout_pads = not_allowed("pads");
            zone.keepout_footprints = not_allowed("footprints");
        }
        // `T_placement` of `parseZONE`: `(enabled yes|no)` and one source, `(sheetname ..)`, `(component_class ..)` or `(group ..)`; the last of them in the file stands.
        if let Some(pl) = sexpr::find(z, "placement") {
            zone.is_rule_area = true;
            for item in &pl[1..] {
                let Some(list) = item.as_list() else { continue };
                match sexpr::tag(list) {
                    Some("enabled") => zone.placement.enabled = sexpr::txt(list, 1) == Some("yes"),
                    Some(token) => {
                        if let Some(source_type) = eda_model::multichannel::PlacementSourceType::from_token(token) {
                            zone.placement.source_type = source_type;
                            zone.placement.source = sexpr::txt(list, 1).unwrap_or("").to_string();
                        }
                    }
                    None => {}
                }
            }
        }
        if let Some(a) = sexpr::find(z, "attr") {
            zone.teardrop = sexpr::find(a, "teardrop").is_some();
        }

        // The zone keeps its whole layer set (`ZONE::SetLayerSet`): the copper layers the board has and the technical and user layers KiCad names. A copper layer the
        // board lacks is dropped (`GetLayerSet() & boardCopperLayers`); a set with no copper layer left is a non-copper zone (`fillNonCopperZone`), which has no net
        // (`if( !zone->IsOnCopperLayer() ) zone->SetNetCode( NETINFO_LIST::UNCONNECTED )`).
        layers.retain(|l| if eda_model::layers::is_copper_layer_name(l) { copper_layers.contains(l) } else { eda_model::layers::is_optional_layer(l) });
        let outlines: Vec<Vec<Point>> = sexpr::find_all(z, "polygon").filter_map(poly_points).collect();
        if outlines.is_empty() || layers.is_empty() {
            notes.zones_skipped += 1;
            continue;
        }
        zone.set_layer_set(&layers);
        if !zone.is_on_copper_layer() {
            zone.net = String::new();
        }
        // `ZONE::m_Uuid` (`(uuid ..)`, `(tstamp ..)` before KiCad 8): the tie between zones of equal priority is decided by it (`ZONE::HigherPriority`).
        zone.uuid = sexpr::find(z, "uuid").or_else(|| sexpr::find(z, "tstamp")).and_then(|u| sexpr::txt(u, 1)).unwrap_or("").to_string();
        // `case T_polygon` then `ZONE::AddPolygon`: the first `(polygon ...)` is the outline, every later one a hole in it (`m_Poly->AddHole( aPolygon )`).
        let mut outlines = outlines.into_iter();
        zone.outline = outlines.next().unwrap_or_default();
        zone.holes = outlines.collect();
        out.push(zone);
        refs.zones.push(ItemRef::of(z));
    }
    out
}

/// Points of a `gr_poly`'s (or a zone's) `(pts ...)` list: `xy` entries verbatim, and an `(arc (start ..) (mid ..) (end ..))` of KiCad 9's polygons as the polyline
/// `SHAPE_ARC::ConvertToPolyline` makes of it at the 5 micrometres KiCad flattens arcs to everywhere else (`eda_model::outline::Arc::polyline`): a polygon with rounded
/// corners keeps them, where the chord between an arc's ends made chamfers of them. (A line joins the end of one arc to the start of the next, as in the file.)
fn poly_points(p: &[Sexpr]) -> Option<Vec<Point>> {
    let pts = sexpr::find(p, "pts")?;
    let mut out: Vec<Point> = Vec::new();
    fn push(pt: Point, out: &mut Vec<Point>) {
        if out.last() != Some(&pt) {
            out.push(pt);
        }
    }
    for item in &pts[1..] {
        let Some(l) = item.as_list() else { continue };
        match sexpr::tag(l) {
            Some("xy") => {
                if let Some(pt) = xy_point(l) {
                    push(pt, &mut out);
                }
            }
            Some("arc") => {
                let (start, mid, end) = (sexpr::find(l, "start").and_then(xy_point), sexpr::find(l, "mid").and_then(xy_point), sexpr::find(l, "end").and_then(xy_point));
                match (start, mid, end) {
                    (Some(s), Some(m), Some(e)) => {
                        for pt in edge::Arc::through(s, m, e).polyline(edge::MAX_ERROR_NM.as_f64()) {
                            push(pt, &mut out);
                        }
                    }
                    // An arc with no middle: its ends, as the chord.
                    (s, _, e) => {
                        for pt in s.into_iter().chain(e) {
                            push(pt, &mut out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (out.len() >= 3).then_some(out)
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use eda_model::ir::TextJustify;

    /// The import says which kinds of board item it has no reader for, and how many of each: a text box, a reference image, a target or a board-level property is in the file and
    /// not in the design (nor in what is exported from it), where a silent import left the user to find out at the first plot.
    #[test]
    fn the_import_counts_the_kinds_of_item_it_has_no_reader_for() {
        let board = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
  (general (thickness 1.6)) (paper "A4") (embedded_fonts no)
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (property "Owner" "me")
  (gr_text_box "one" (start 0 0) (end 5 5) (layer "F.SilkS") (effects (font (size 1 1))))
  (gr_text_box "two" (start 0 0) (end 5 5) (layer "F.SilkS") (effects (font (size 1 1))))
  (image (at 79 138.5) (layer "B.Cu") (scale 2) (data "AAAA"))
  (gr_text "kept" (at 1 1 0) (layer "F.SilkS") (effects (font (size 1 1))))
  (barcode (at 1 2) (layer "F.SilkS") (type qr) (text "123"))
  (gr_line (start 0 0) (end 5 0) (stroke (width 0.1) (type default)) (layer "Edge.Cuts"))
)"#;
        let (_, _, notes) = import_kicad_pcb(board).unwrap();
        // The text boxes have a reader (`pcb_textbox.rs`); the image and the board-level property are kept as text.
        assert_eq!(notes.unread_items.iter().map(|(k, n)| (k.as_str(), *n)).collect::<Vec<_>>(), [("image", 1), ("property", 1)]);
        assert_eq!(import_kicad_pcb(board).unwrap().0.drawings.unwrap().board_items.text_boxes.len(), 2);
        let (_, _, plain) = import_kicad_pcb(r#"(kicad_pcb (version 20240108) (general (thickness 1.6)) (layers (0 "F.Cu" signal)) (net 0 ""))"#).unwrap();
        assert!(plain.unread_items.is_empty());
    }

    /// In a pre-9.0 file a footprint margin of 0 means "inherit" (`parseFOOTPRINT_unchecked`): neither the mask margin nor a paste margin token of 0 comes through as a margin
    /// of its own, which in the file the writer makes (a new version) it would be. A margin that is not 0 comes through in any version.
    #[test]
    fn a_footprint_margin_of_zero_in_an_old_file_is_no_margin() {
        let board = |version: &str, fp_tokens: &str| {
            format!(
                r#"(kicad_pcb (version {version}) (generator "pcbnew") (general (thickness 1.6)) (paper "A4") (layers (0 "F.Cu" signal) (31 "B.Cu" signal)) (net 0 "")
  (footprint "L:F" (layer "F.Cu") (at 1 1) (property "Reference" "U1" (at 0 0 0) (layer "F.SilkS")) (property "Value" "U" (at 0 0 0) (layer "F.Fab")) {fp_tokens}
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))))"#
            )
        };
        let extra = |text: &str| {
            let (design, _, _) = import_kicad_pcb(text).unwrap();
            design.drawings.and_then(|d| d.footprint_extras.into_iter().find(|e| e.id == "U1")).unwrap_or_default()
        };
        let old = extra(&board("20240108", "(solder_mask_margin 0) (solder_paste_margin 0) (solder_paste_ratio 0) (solder_paste_margin_ratio 0)"));
        assert_eq!((old.solder_mask_margin, old.tokens.is_empty()), (None, true), "{old:?}");
        let new = extra(&board("20241229", "(solder_mask_margin 0) (solder_paste_margin 0) (solder_paste_margin_ratio 0)"));
        assert_eq!(new.solder_mask_margin, Some(Nm::ZERO), "{new:?}");
        assert_eq!(new.tokens, ["(solder_paste_margin 0)", "(solder_paste_margin_ratio 0)"]);
        let nonzero = extra(&board("20240108", "(solder_mask_margin 0.02) (solder_paste_margin -0.01)"));
        assert_eq!((nonzero.solder_mask_margin, nonzero.tokens), (Some(mm(0.02)), vec!["(solder_paste_margin -0.01)".to_string()]));
    }

    /// A polygon's `(arc (start ..) (mid ..) (end ..))` is the arc, flattened as KiCad flattens it (5 micrometres from the curve), and no longer the chord between its
    /// ends: `api_kitchen_sink`'s silkscreen polygon with four rounded corners came back with four chamfers.
    #[test]
    fn the_arcs_of_a_polygon_keep_their_curvature() {
        // A 6 x 4 mm rectangle whose four corners are quarter circles of radius 1 (a gr_poly of arcs and lines, as KiCad 9 writes a rounded polygon).
        let s2 = std::f64::consts::FRAC_1_SQRT_2;
        let corner = |cx: f64, cy: f64, sx: f64, sy: f64, ex: f64, ey: f64, sign: (f64, f64)| format!("(arc (start {sx} {sy}) (mid {} {}) (end {ex} {ey}))", cx + sign.0 * s2, cy + sign.1 * s2);
        let poly = format!(
            "(gr_poly (pts {} {} {} {}) (stroke (width 0.1) (type solid)) (fill none) (layer \"F.SilkS\"))",
            corner(5.0, 1.0, 5.0, 0.0, 6.0, 1.0, (1.0, -1.0)),
            corner(5.0, 3.0, 6.0, 3.0, 5.0, 4.0, (1.0, 1.0)),
            corner(1.0, 3.0, 1.0, 4.0, 0.0, 3.0, (-1.0, 1.0)),
            corner(1.0, 1.0, 0.0, 1.0, 1.0, 0.0, (-1.0, -1.0)),
        );
        let tree = crate::sexpr::parse(&poly).unwrap();
        let item = tree.as_list().unwrap();
        let pts = poly_points(item).expect("a polygon");
        // Chords would leave 8 points; the arcs are polylines.
        assert!(pts.len() > 16, "{} points", pts.len());
        // Every point sits on one of the four circles, within the 5 micrometres (5000 nm) the flattening is held to, and the ends of the arcs are exact.
        let centres = [(5_000_000.0, 1_000_000.0), (5_000_000.0, 3_000_000.0), (1_000_000.0, 3_000_000.0), (1_000_000.0, 1_000_000.0)];
        let on_arc = |p: &Point| centres.iter().any(|c| ((p.x.as_f64() - c.0).hypot(p.y.as_f64() - c.1) - 1_000_000.0).abs() <= 5_000.0);
        assert!(pts.iter().all(on_arc), "{pts:?}");
        for exact in [Point { x: mm(5.0), y: Nm::ZERO }, Point { x: mm(6.0), y: mm(1.0) }, Point { x: mm(6.0), y: mm(3.0) }, Point { x: mm(5.0), y: mm(4.0) }, Point { x: mm(1.0), y: mm(4.0) }, Point { x: Nm::ZERO, y: mm(3.0) }, Point { x: Nm::ZERO, y: mm(1.0) }, Point { x: mm(1.0), y: Nm::ZERO }] {
            assert!(pts.contains(&exact), "{exact:?} in {pts:?}");
        }
        // The area is the rounded rectangle's, 24 - (4 - pi) = 23.14 mm^2; the chords of the four arcs would leave 22.
        let area: f64 = (0..pts.len()).map(|i| { let (a, b) = (pts[i], pts[(i + 1) % pts.len()]); a.x.as_f64() * b.y.as_f64() - b.x.as_f64() * a.y.as_f64() }).sum::<f64>().abs() / 2.0 / 1.0e12;
        assert!((area - (24.0 - (4.0 - std::f64::consts::PI))).abs() < 0.01, "area {area}");
        // An arc with no middle is its chord, as before.
        let broken = crate::sexpr::parse("(gr_poly (pts (xy 0 0) (arc (start 1 0) (end 1 1)) (xy 0 1)))").unwrap();
        assert_eq!(poly_points(broken.as_list().unwrap()).unwrap().len(), 4);
    }

    /// KiCad 6's `(gr_arc (start CENTRE) (end START) (angle A))`: the end is the start turned by A about the centre (`SetArcAngleAndEnd`), the
    /// middle by A/2, in the file's own axes, whichever way A goes.
    #[test]
    fn a_legacy_arc_is_three_points_turned_about_its_centre() {
        let p = |x, y| Point { x, y };
        assert_eq!(legacy_arc(p(mm(15.0), mm(5.0)), p(mm(20.0), mm(5.0)), 90.0), (p(mm(20.0), mm(5.0)), p(Nm::new(18_535_534), Nm::new(8_535_534)), p(mm(15.0), mm(10.0))));
        assert_eq!(legacy_arc(p(mm(15.0), mm(5.0)), p(mm(20.0), mm(5.0)), -90.0), (p(mm(20.0), mm(5.0)), p(Nm::new(18_535_534), Nm::new(1_464_466)), p(mm(15.0), Nm::ZERO)));
        // A half turn goes through the far side of the circle: the middle is a quarter turn on.
        assert_eq!(legacy_arc(p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO), 180.0), (p(mm(1.0), Nm::ZERO), p(Nm::ZERO, mm(1.0)), p(mm(-1.0), Nm::ZERO)));
    }

    #[test]
    fn rotation_negation_round_trips() {
        assert_eq!(import_rot_millideg(0.0), 0);
        assert_eq!(import_rot_millideg(-90.0), 90_000);
        assert_eq!(import_rot_millideg(90.0), 270_000);
    }

    /// GAPS.md #3's clearance-over-firing root cause, pinned down directly:
    /// a real `.kicad_pcb` already writes a back-side pad's `x` pre-mirrored
    /// (confirmed against live `kicad-cli pcb drc` on a minimal repro and on
    /// `issue11814` itself -- see `parse_pad_geometry`'s doc comment), so
    /// this importer must undo that mirror once, not leave it for
    /// `to_board` to apply a second time. These are `issue11814.kicad_pcb`'s
    /// own numbers verbatim: footprint `R_0402_..._HandSolder` at
    /// `(at 114.8 137.8 90)` on `B.Cu`, pad "1" at local `(at -0.5975 0 90)`.
    /// Real KiCad reports this pad's absolute position as
    /// `(114.8, 138.3975)` (`kicad-cli pcb drc` on the isolated footprint);
    /// before this fix, `to_board` re-mirrored the un-undone x and produced
    /// `(114.8, 137.2025)` instead -- exactly R34's *other* pad's position,
    /// 1.196mm away, which is what put this pad's copper on top of a
    /// foreign net's track and fired a false `shorting_items`/`clearance`.
    #[test]
    fn back_side_pad_position_matches_kicad_cli_ground_truth() {
        let pad_sexpr = sexpr::parse(
            r#"(pad "1" smd roundrect (at -0.5975 0 90) (size 0.715 0.64) (layers "B.Cu" "B.Mask" "B.Paste") (roundrect_rratio 0.25))"#,
        )
        .unwrap();
        let mut notes = ImportNotes::default();
        // `fp_rot` here is already-imported (negated) millidegrees for a
        // footprint whose file angle is 90 -- `import_rot_millideg(90.0)`.
        let fp_rot = import_rot_millideg(90.0);
        let pad = parse_pad_geometry(pad_sexpr.as_list().unwrap(), Side::Bottom, fp_rot, &mut notes).expect("pad parses");
        // Pre-mirror local frame (what `to_board` expects to mirror itself):
        // negating the file's raw -0.5975mm gives +597.5um -> rounds to 598.
        assert_eq!(pad.at, (Nm::new(597_500), Nm::ZERO), "importer must undo the file's own back-side mirror, not pass x through as-is");

        let fp = FootprintInstance { id: "R34".into(), at: Point { x: mm(114.8), y: mm(137.8) }, rot: fp_rot, side: Side::Bottom, label: Default::default() };
        let board_pos = eda_model::footprint::to_board(&fp, pad.at);
        assert_eq!(board_pos, Point { x: mm(114.8), y: Nm::new(138_397_500) }, "must match kicad-cli's own reported absolute pad position (114.8, 138.3975mm), not the other pad's position 1.196mm away");
    }

    /// GAPS.md #3's second over-firing source: `courtyards_overlap` was a
    /// 100% false-positive rate (KiCad 0, ours dozens) because every
    /// imported footprint fell back to a pad-bbox-derived courtyard that
    /// has no way to represent a connector shroud or mounting flange
    /// sticking out past the pads. This pins the fix to a real, asymmetric
    /// `F.CrtYd` rectangle (pads only reach +-0.5mm; the courtyard reaches
    /// 2mm on the +x side for a shroud) and confirms the un-mirror applies
    /// to courtyard graphics the same way `parse_pad_geometry` applies it
    /// to a pad's own `(at ...)`.
    #[test]
    fn footprint_courtyard_half_reads_real_crtyd_graphics_and_unmirrors_for_back_side() {
        let fp = sexpr::parse(
            r#"(footprint "test:conn"
                (layer "F.Cu")
                (at 0 0)
                (fp_rect (start -0.5 -0.5) (end 2.0 0.5) (stroke (width 0.05) (type default)) (fill none) (layer "F.CrtYd"))
                (pad "1" smd rect (at 0 0) (size 0.3 0.3) (layers "F.Cu")))"#,
        )
        .unwrap();
        let body = fp.as_list().unwrap();

        // Top side: no mirror, so the asymmetric +x reach (2mm) and the -x
        // reach (0.5mm) land exactly where they're written.
        let (hw, hh) = footprint_courtyard_half(body, Side::Top).expect("courtyard found");
        assert_eq!((hw, hh), (mm(2.0), mm(0.5)), "symmetric half-extent must enclose the farthest point on each axis: max(|{{-500,2000}}|)=2000, max(|{{-500,500}}|)=500");

        // Back side: the same raw numbers must be un-mirrored (x negated)
        // before taking the half-extent, exactly like a pad's `at` -- the
        // farthest-x point is still the one that was at file-x=2.0 (now at
        // internal x=-2.0), so the half-extent magnitude is unchanged, but
        // getting the sign backwards here is exactly the bug this is
        // guarding: it would silently still pass this symmetric-extent
        // check while leaving the *centre* wrong for a caller that (unlike
        // this one) cares about which side the shroud is on.
        let (hw_b, hh_b) = footprint_courtyard_half(body, Side::Bottom).expect("courtyard found");
        assert_eq!((hw_b, hh_b), (mm(2.0), mm(0.5)));
    }

    #[test]
    fn footprint_courtyard_half_is_none_without_crtyd_graphics() {
        let fp = sexpr::parse(r#"(footprint "test:bare" (layer "F.Cu") (at 0 0) (pad "1" smd rect (at 0 0) (size 0.3 0.3) (layers "F.Cu")))"#).unwrap();
        assert!(footprint_courtyard_half(fp.as_list().unwrap(), Side::Top).is_none(), "no F.CrtYd/B.CrtYd graphics -> None, same as before this existed (falls back to the pad-bbox heuristic)");
    }

    /// GAPS.md #3's exact `issue11814` shape: the identical lib id
    /// (`R_0402_..._HandSolder`) placed on both `F.Cu` (R35) and `B.Cu`
    /// (R34). Each instance's own un-mirrored pad geometry is correct in
    /// isolation (the test above), but caching pad geometry by bare lib id
    /// -- the overwhelming majority case, where the same lib id is only
    /// ever on one side -- would make whichever instance is parsed *first*
    /// win the cache for every later instance on the *other* side, silently
    /// handing it the wrong (mirror-image) pad positions. This is what
    /// actually put R34's pad 1 on top of a foreign net's track before this
    /// fix: R35 (first in file order) cached the Top-side geometry, and R34
    /// (Bottom) got it unchanged instead of its own.
    #[test]
    fn dedup_footprint_key_gives_mixed_side_instances_of_one_lib_id_separate_slots() {
        let mut explicit: BTreeMap<String, Footprint> = BTreeMap::new();
        let top_pads = vec![Pad { opposite_side: false, ext: Default::default(), number: "1".into(), at: (mm(-0.598), Nm::ZERO), size: (mm(0.715), mm(0.64)), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: Some(0.25) }];
        let bottom_pads = vec![Pad { opposite_side: false, ext: Default::default(), number: "1".into(), at: (mm(0.598), Nm::ZERO), size: (mm(0.715), mm(0.64)), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: Some(0.25) }];

        let lib_id = "fixed_standard:R_0402_1005Metric_Pad0.72x0.64mm_HandSolder";
        let key_top = dedup_footprint_key(&mut explicit, lib_id, &top_pads, &[], &Default::default());
        assert_eq!(key_top, lib_id, "first instance (R35, top) gets the plain lib id");
        explicit.insert(key_top, Footprint { name: lib_id.into(), pads: top_pads.clone(), courtyard: None, courtyard_outlines: vec![], model: None, models3d: vec![], art: Default::default() });

        let key_bottom = dedup_footprint_key(&mut explicit, lib_id, &bottom_pads, &[], &Default::default());
        assert_ne!(key_bottom, lib_id, "R34 (bottom)'s own un-mirrored geometry disagrees with R35's cached one -- it must get its own slot, not silently reuse R35's");
        explicit.insert(key_bottom.clone(), Footprint { name: key_bottom.clone(), pads: bottom_pads.clone(), courtyard: None, courtyard_outlines: vec![], model: None, models3d: vec![], art: Default::default() });

        // A third instance matching either existing slot exactly must reuse
        // it rather than growing a new one every time.
        assert_eq!(dedup_footprint_key(&mut explicit, lib_id, &top_pads, &[], &Default::default()), lib_id);
        assert_eq!(dedup_footprint_key(&mut explicit, lib_id, &bottom_pads, &[], &Default::default()), key_bottom);
    }

    /// GAPS.md #3's third over-firing source, pinned to `issue11814`'s U4
    /// (`WSON-8-1EP_..._PullBack`): a thermal pad split into real copper
    /// (pad "9", `F.Cu`) plus four unnumbered, netless `F.Paste`-only
    /// quadrant pads layered on top for paste-stencil control. Before this
    /// fix, every one of those four was imported as if it flashed real
    /// copper (this crate's old kind-only layer convention), so it was
    /// clearance/shorting-tested against pad 9 and a nearby via sitting at
    /// the exact same spot -- a 100% false positive, since a paste aperture
    /// has no copper to short or crowd anything.
    #[test]
    fn paste_only_pad_has_no_copper_and_is_dropped() {
        let mut notes = ImportNotes::default();
        let paste_only = sexpr::parse(r#"(pad "" smd roundrect (at -0.3 -0.375 180) (size 0.48 0.6) (layers "F.Paste") (roundrect_rratio 0.25))"#).unwrap();
        assert!(parse_pad_geometry(paste_only.as_list().unwrap(), Side::Top, 0, &mut notes).is_none(), "an F.Paste-only pad has no copper and must not become a phantom copper pad");
        assert_eq!(notes.non_copper_pads_skipped, 1);

        // A real copper pad (even one that also carries F.Paste/F.Mask)
        // must be unaffected.
        let mut notes2 = ImportNotes::default();
        let real = sexpr::parse(r#"(pad "9" smd rect (at 0 0 180) (size 1.2 1.5) (layers "F.Cu" "F.Paste" "F.Mask") (net 2 "GND"))"#).unwrap();
        assert!(parse_pad_geometry(real.as_list().unwrap(), Side::Top, 0, &mut notes2).is_some());
        assert_eq!(notes2.non_copper_pads_skipped, 0);

        // A through-hole pad's `(layers "*.Cu" "*.Mask")` must count as
        // copper via the wildcard, not just a literal `F.Cu`/`B.Cu`.
        let mut notes3 = ImportNotes::default();
        let th = sexpr::parse(r#"(pad "1" thru_hole circle (at 0 0) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask") (net 1 "GND"))"#).unwrap();
        assert!(parse_pad_geometry(th.as_list().unwrap(), Side::Top, 0, &mut notes3).is_some());
        assert_eq!(notes3.non_copper_pads_skipped, 0);
    }

    #[test]
    fn tessellated_arc_keeps_exact_endpoints() {
        let start = Point { x: mm(10.0), y: Nm::ZERO };
        let mid = Point { x: mm(7.071), y: mm(7.071) };
        let end = Point { x: Nm::ZERO, y: mm(10.0) };
        let pts = eda_model::ir::tessellate_arc(start, mid, end, eda_model::ir::TRACK_ARC_SEGMENTS);
        assert_eq!(*pts.first().unwrap(), start);
        assert_eq!(*pts.last().unwrap(), end);
        assert!(pts.len() > 2);
    }

    /// Shape confirmed against a real `.kicad_pro` in the KiCad QA corpus
    /// (`issue12609.kicad_pro`'s `net_settings`), field-for-field.
    const SAMPLE_PROJECT_JSON: &str = r#"{
        "net_settings": {
            "classes": [
                { "name": "Default", "clearance": 0.15, "track_width": 0.25, "via_diameter": 0.6, "via_drill": 0.3 },
                { "name": "1kV", "clearance": 2.4, "track_width": 0.5, "via_diameter": 1.0, "via_drill": 0.5 },
                { "name": "500V", "clearance": 1.2, "track_width": 0.3 }
            ],
            "netclass_patterns": [
                { "netclass": "1kV", "pattern": "/1kV" },
                { "netclass": "500V", "pattern": "/500Vpp" },
                { "netclass": "500V", "pattern": "/500Vpn" }
            ]
        }
    }"#;

    #[test]
    fn parses_project_net_classes_skipping_default() {
        let classes = parse_project_net_classes(SAMPLE_PROJECT_JSON);
        assert_eq!(classes.len(), 2, "{classes:?}");
        assert_eq!(classes[0].name, "1kV");
        assert_eq!(classes[0].nets, vec!["/1kV".to_string()]);
        assert_eq!(classes[0].clearance, Some(mm(2.4)));
        assert_eq!(classes[0].track_width, Some(mm(0.5)));
        assert_eq!(classes[0].via_diameter, Some(mm(1.0)));
        assert_eq!(classes[0].via_drill, Some(mm(0.5)));
        assert_eq!(classes[1].name, "500V");
        assert_eq!(classes[1].nets, vec!["/500Vpp".to_string(), "/500Vpn".to_string()], "a class can own more than one pattern");
    }

    #[test]
    fn merge_puts_project_classes_ahead_of_pcb_level_ones() {
        let mut model = eda_model::ConstraintModel::default();
        model.board.net_classes.push(NetClass { name: "pcb_level".into(), nets: vec!["*".into()], track_width: None, clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
        merge_project_net_classes(&mut model, SAMPLE_PROJECT_JSON);
        assert_eq!(model.board.net_classes.len(), 3);
        assert_eq!(model.board.net_classes[0].name, "1kV", "project classes come first: class_of is first-match-wins, same as KiCad's own earliest-declared-wins precedence");
        assert_eq!(model.board.net_classes[2].name, "pcb_level");
    }

    #[test]
    fn merge_applies_the_projects_default_class_to_board_scalars() {
        let mut model = eda_model::ConstraintModel::default();
        let before = model.board.clearance;
        merge_project_net_classes(&mut model, SAMPLE_PROJECT_JSON);
        assert_eq!(model.board.clearance, mm(0.15), "Default class's 0.15mm clearance, not this crate's own placeholder default ({before})");
        assert_eq!(model.board.track_width, mm(0.25));
        assert_eq!(model.board.via_diameter, mm(0.6));
        assert_eq!(model.board.via_drill, mm(0.3));
    }

    /// The nets a schematic's net class labels assign are in the project as `netclass_assignments` (net name -> class names), beside the patterns: a net is in the classes of both
    /// (`NET_SETTINGS::GetEffectiveNetClass`), and a class only a label names is a class a rule can read. Dropped, `time_calculations`' seven classes matched no net and its
    /// skew rule judged nothing.
    #[test]
    fn the_nets_a_label_assigns_to_a_class_are_in_the_class() {
        let project = r#"{"net_settings": {
            "classes": [
                {"name": "Default", "clearance": 0.2, "track_width": 0.25, "via_diameter": 0.8, "via_drill": 0.4},
                {"name": "CLASS1", "priority": 1, "clearance": 0.3},
                {"name": "CLASS2", "priority": 0},
                {"name": "UNUSED", "priority": 2}
            ],
            "netclass_assignments": {"Net-(R1-Pad2)": ["CLASS1", "ONLYALABEL"], "/NET_P": ["CLASS2"], "/NET_N": ["CLASS2", "CLASS1"]},
            "netclass_patterns": [{"netclass": "CLASS1", "pattern": "PWR*"}]
        }}"#;
        let classes = parse_project_net_classes(project);
        let by = |name: &str| classes.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("class {name} in {:?}", classes.iter().map(|c| &c.name).collect::<Vec<_>>()));
        assert_eq!(classes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["CLASS2", "CLASS1", "ONLYALABEL"], "by priority, then the implicit classes; UNUSED names no net");
        assert!(by("CLASS1").matches("PWR3") && by("CLASS1").matches("Net-(R1-Pad2)") && by("CLASS1").matches("/NET_N") && !by("CLASS1").matches("/NET_P"));
        assert!(by("CLASS2").matches("/NET_P") && by("CLASS2").matches("/NET_N") && !by("CLASS2").matches("Net-R1-Pad2"), "a net named with a parenthesis matches only itself, not what the parentheses would group");
        assert!(by("ONLYALABEL").matches("Net-(R1-Pad2)") && by("ONLYALABEL").clearance.is_none());
        let mut model = eda_model::ConstraintModel::default();
        merge_project_net_classes(&mut model, project);
        assert_eq!(model.board.matching_classes("Net-(R1-Pad2)").iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["CLASS1", "ONLYALABEL"]);
        assert_eq!(model.board.clearance, mm(0.2));
    }

    #[test]
    fn unparseable_or_absent_net_settings_is_an_empty_no_op() {
        assert!(parse_project_net_classes("not json").is_empty());
        assert!(parse_project_net_classes("{}").is_empty());
        assert!(parse_project_net_classes(r#"{"net_settings": {}}"#).is_empty());
    }

    /// Shape confirmed against a real `.kicad_pro` in the KiCad QA corpus
    /// (`issue11814.kicad_pro`'s `board.design_settings.rules`),
    /// field-for-field -- note this board's own `min_track_width`
    /// (0.1016mm) is deliberately *smaller* than its `net_settings`
    /// Default class's nominal `track_width` (0.1524mm, GAPS.md #10's
    /// exact shape): [`merge_project_design_rules`] must read the former,
    /// never the latter.
    const SAMPLE_DESIGN_RULES_JSON: &str = r#"{
        "board": {
            "design_settings": {
                "rules": {
                    "min_clearance": 0.0,
                    "min_copper_edge_clearance": 0.25,
                    "min_hole_clearance": 0.254,
                    "min_hole_to_hole": 0.25,
                    "min_silk_clearance": 0.15,
                    "min_text_height": 0.8,
                    "min_text_thickness": 0.12,
                    "min_through_hole_diameter": 0.2,
                    "min_track_width": 0.1016,
                    "min_via_annular_width": 0.125,
                    "min_via_diameter": 0.45
                }
            }
        }
    }"#;

    #[test]
    fn merge_project_design_rules_reads_the_board_wide_minimums() {
        let mut model = eda_model::ConstraintModel::default();
        merge_project_design_rules(&mut model, SAMPLE_DESIGN_RULES_JSON);
        assert_eq!(model.board.track_width_min_nm, Nm::new(101_600), "0.1016 mm is 101600 nm");
        assert_eq!(model.board.min_clearance_nm, Nm::ZERO);
        assert_eq!(model.board.via_diameter_min_nm, mm(0.45));
        assert_eq!(model.board.via_drill_min_nm, mm(0.2));
        assert_eq!(model.board.hole_to_hole_min_nm, mm(0.25));
        assert_eq!(model.board.hole_clearance_nm, mm(0.254));
        assert_eq!(model.board.annular_width_min_nm, mm(0.125));
        assert_eq!(model.board.silk_clearance_nm, mm(0.15));
        assert_eq!(model.board.min_silk_text_height_nm, mm(0.8));
        assert_eq!(model.board.min_silk_text_thickness_nm, mm(0.12));
    }

    #[test]
    fn merge_project_design_rules_is_a_no_op_without_a_rules_object() {
        let mut model = eda_model::ConstraintModel::default();
        let before = model.board.clone();
        merge_project_design_rules(&mut model, SAMPLE_PROJECT_JSON); // has net_settings but no board.design_settings.rules
        assert_eq!(model.board, before);
        merge_project_design_rules(&mut model, "not json");
        assert_eq!(model.board, before);
    }

    /// The pre-KiCad-7 fallback path: a bare `.kicad_pcb` with no sidecar
    /// `.kicad_pro` can still carry these as legacy `(setup ...)` tokens.
    #[test]
    fn legacy_setup_minimums_are_imported() {
        let text = r#"(kicad_pcb (version 20221018) (generator "pcbnew")
            (setup
                (trace_min 0.15)
                (clearance_min 0.1)
                (via_min_size 0.4)
                (via_min_drill 0.25)
                (hole_to_hole_min 0.2)
                (via_min_annulus 0.09)
            )
            (layers (0 "F.Cu" signal) (31 "B.Cu" signal))
            (net 0 "")
        )"#;
        let (_design, model, _notes) = import_kicad_pcb(text).expect("parses");
        assert_eq!(model.board.track_width_min_nm, mm(0.15));
        assert_eq!(model.board.min_clearance_nm, mm(0.1));
        assert_eq!(model.board.via_diameter_min_nm, mm(0.4));
        assert_eq!(model.board.via_drill_min_nm, mm(0.25), "falls back to the legacy via_min_drill token when through_hole_min is absent");
        assert_eq!(model.board.hole_to_hole_min_nm, mm(0.2));
        assert_eq!(model.board.annular_width_min_nm, mm(0.09));
    }

    #[test]
    fn parses_minimal_board() {
        let text = r#"(kicad_pcb (version 20241229) (generator "eda-kicad")
            (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
            (net 0 "") (net 1 "GND")
            (net_class "Default" "" (clearance 0.2) (trace_width 0.25) (via_dia 0.6) (via_drill 0.3) (add_net "GND"))
            (footprint "eda:0603" (layer "F.Cu") (uuid "u1") (at 10 10 0)
                (property "Reference" "R1" (at 0 0 0) (layer "F.SilkS"))
                (pad "1" smd roundrect (at -0.5 0 0) (size 0.8 0.95) (layers "F.Cu") (net 1 "GND"))
                (pad "2" smd roundrect (at 0.5 0 0) (size 0.8 0.95) (layers "F.Cu") (net 1 "GND")))
            (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
            (gr_line (start 20 0) (end 20 20) (layer "Edge.Cuts"))
            (gr_line (start 20 20) (end 0 20) (layer "Edge.Cuts"))
            (gr_line (start 0 20) (end 0 0) (layer "Edge.Cuts"))
        )"#;
        let (design, model, notes) = import_kicad_pcb(text).expect("parses");
        assert_eq!(model.board.layers, vec!["F.Cu".to_string(), "B.Cu".to_string()]);
        assert_eq!(model.board.track_width, mm(0.25));
        assert_eq!(model.board.clearance, mm(0.2));
        assert_eq!(model.parts.len(), 1);
        assert_eq!(model.parts[0].reference, "R1");
        assert_eq!(model.footprints.len(), 1);
        assert_eq!(model.footprints[0].pads.len(), 2);
        assert_eq!(model.nets.len(), 1);
        assert_eq!(model.nets[0].pins, vec!["R1.1".to_string(), "R1.2".to_string()]);
        let pl = design.placement.unwrap();
        assert_eq!(pl.footprints.len(), 1);
        assert_eq!(pl.footprints[0].at, Point { x: mm(10.0), y: mm(10.0) });
        assert_eq!(pl.outline.len(), 4);
        assert_eq!(notes.outline_source, "lines");
        assert!(!notes.outline_open);
    }

    /// Board-level graphics and text on non-Edge.Cuts layers become
    /// `Shape`/`Text` items, ided, while the Edge.Cuts lines still go only
    /// into the outline (never duplicated into `drawings`).
    #[test]
    fn imports_graphics_and_text_into_drawings() {
        let text = r#"(kicad_pcb (version 20241229) (generator "eda-kicad")
            (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
            (net 0 "")
            (net_class "Default" "" (clearance 0.2) (trace_width 0.25) (via_dia 0.6) (via_drill 0.3))
            (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
            (gr_line (start 20 0) (end 20 20) (layer "Edge.Cuts"))
            (gr_line (start 20 20) (end 0 20) (layer "Edge.Cuts"))
            (gr_line (start 0 20) (end 0 0) (layer "Edge.Cuts"))
            (gr_line (start 1 1) (end 5 1) (stroke (width 0.15) (type solid)) (layer "F.SilkS") (uuid "s1"))
            (gr_rect (start 2 2) (end 6 6) (stroke (width 0.1) (type solid)) (fill yes) (layer "F.Fab") (uuid "s2"))
            (gr_circle (center 10 10) (end 12 10) (stroke (width 0.1) (type solid)) (fill no) (layer "B.SilkS") (uuid "s3"))
            (gr_poly (pts (xy 0 0) (xy 4 0) (xy 4 4)) (stroke (width 0.1) (type solid)) (fill yes) (layer "F.CrtYd") (uuid "s4"))
            (gr_text "REV A" (at 3 3 90) (layer "F.SilkS") (uuid "t1")
                (effects (font (size 1 1) (thickness 0.15)) (justify left mirror)))
        )"#;
        let (design, _model, notes) = import_kicad_pcb(text).expect("parses");
        assert_eq!(notes.outline_source, "lines", "Edge.Cuts lines must still build the outline");
        let dr = design.drawings.expect("graphics/text on non-Edge.Cuts layers must produce a drawings section");
        assert_eq!(dr.shapes.len(), 4, "the four Edge.Cuts gr_lines must not also become shapes");

        let seg = dr.shapes.iter().find(|s| s.layer() == "F.SilkS" && matches!(s, Shape::Segment { .. })).expect("segment on F.SilkS");
        assert!(!seg.id().is_empty());
        let Shape::Segment { stroke_width, start, end, .. } = seg else { unreachable!() };
        assert_eq!(*stroke_width, mm(0.15));
        assert_eq!(*start, Point { x: mm(1.0), y: mm(1.0) });
        assert_eq!(*end, Point { x: mm(5.0), y: mm(1.0) });

        let rect = dr.shapes.iter().find(|s| s.layer() == "F.Fab").expect("rect on F.Fab");
        assert!(matches!(rect, Shape::Rect { filled: true, .. }), "{rect:?}");

        let circle = dr.shapes.iter().find(|s| s.layer() == "B.SilkS").expect("circle on B.SilkS");
        assert!(matches!(circle, Shape::Circle { filled: false, .. }), "{circle:?}");

        let poly = dr.shapes.iter().find(|s| s.layer() == "F.CrtYd").expect("polygon on F.CrtYd");
        let Shape::Polygon { pts, filled, .. } = poly else { panic!("{poly:?}") };
        assert!(filled);
        assert_eq!(pts.len(), 3);

        assert_eq!(dr.texts.len(), 1);
        let t = &dr.texts[0];
        assert!(!t.id.is_empty());
        assert_eq!(t.content, "REV A");
        assert_eq!(t.at, Point { x: mm(3.0), y: mm(3.0) });
        assert_eq!(t.angle, 90_000);
        assert_eq!(t.layer, "F.SilkS");
        assert_eq!(t.size_nm, mm(1.0));
        assert_eq!(t.stroke_width, mm(0.15));
        assert_eq!(t.justify, TextJustify::Left);
        assert!(t.mirror);
    }

    /// `(gr_curve (pts (xy start) (xy c1) (xy c2) (xy end)))` is a cubic Bezier
    /// (`SHAPE_T::BEZIER`): kept as `Shape::Bezier` with its control points, on
    /// any layer but Edge.Cuts.
    #[test]
    fn imports_a_gr_curve_as_a_bezier_shape() {
        let text = r#"(kicad_pcb (version 20241229) (generator "eda-kicad")
            (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
            (net 0 "")
            (net_class "Default" "" (clearance 0.2) (trace_width 0.25) (via_dia 0.6) (via_drill 0.3))
            (gr_curve (pts (xy 1 1) (xy 1 3) (xy 4 3) (xy 4 1)) (stroke (width 0.15) (type solid)) (layer "F.SilkS") (uuid "c1"))
        )"#;
        let (design, _model, _notes) = import_kicad_pcb(text).expect("parses");
        let dr = design.drawings.expect("the curve must produce a drawings section");
        assert_eq!(dr.shapes.len(), 1);
        let Shape::Bezier { layer, stroke_width, start, c1, c2, end, .. } = &dr.shapes[0] else { panic!("{:?}", dr.shapes[0]) };
        assert_eq!(layer, "F.SilkS");
        assert_eq!(*stroke_width, mm(0.15));
        assert_eq!((*start, *c1, *c2, *end), (Point { x: mm(1.0), y: mm(1.0) }, Point { x: mm(1.0), y: mm(3.0) }, Point { x: mm(4.0), y: mm(3.0) }, Point { x: mm(4.0), y: mm(1.0) }));
    }

    /// `(setup (pad_to_mask_clearance ..) (solder_mask_min_width ..)
    /// (allow_soldermask_bridges_in_footprints ..) (tenting ..))`, in both the
    /// KiCad 9 `(front yes)` and the legacy bare-token tenting grammar, plus
    /// the project file's `solder_mask_to_copper_clearance`.
    #[test]
    fn imports_board_solder_mask_settings() {
        let text = |tenting: &str| {
            format!(
                r#"(kicad_pcb (version 20241229) (generator "eda-kicad")
                (layers (0 "F.Cu" signal) (31 "B.Cu" signal))
                (setup (pad_to_mask_clearance 0.05) (solder_mask_min_width 0.1) (allow_soldermask_bridges_in_footprints yes) {tenting})
            )"#
            )
        };
        let (_, model, _) = import_kicad_pcb(&text("(tenting (front yes) (back no))")).expect("parses");
        let m = &model.board.solder_mask;
        assert_eq!((m.expansion_nm, m.min_width_nm), (mm(0.05), mm(0.1)));
        assert!(m.allow_bridges_in_footprints);
        assert!(m.tent_vias_front && !m.tent_vias_back);

        let (_, legacy, _) = import_kicad_pcb(&text("(tenting front)")).expect("parses");
        assert!(legacy.board.solder_mask.tent_vias_front && !legacy.board.solder_mask.tent_vias_back);

        // No (tenting ..) at all: KiCad's factory default tents both sides.
        let (_, none, _) = import_kicad_pcb(r#"(kicad_pcb (version 20241229) (layers (0 "F.Cu" signal) (31 "B.Cu" signal)))"#).expect("parses");
        assert!(none.board.solder_mask.is_default());

        let (_, mut with_pro, _) = import_kicad_pcb(&text("(tenting (front yes) (back yes))")).expect("parses");
        merge_project_design_rules(&mut with_pro, r#"{"board":{"design_settings":{"rules":{"solder_mask_to_copper_clearance":0.07}}}}"#);
        assert_eq!(with_pro.board.solder_mask.to_copper_clearance_nm, mm(0.07));
    }

    /// Per-pad layer sets and mask margins, footprint margin / allow-bridges /
    /// net-tie groups, via tenting, the footprint's silk, fab and mask graphics
    /// (the footprint's own, in its frame) and mask-only aperture pads (kept in
    /// board space).
    #[test]
    fn imports_pad_footprint_via_and_graphic_mask_facts() {
        let text = r#"(kicad_pcb (version 20241229) (generator "eda-kicad")
            (layers (0 "F.Cu" signal) (31 "B.Cu" signal))
            (net 0 "") (net 1 "A") (net 2 "B")
            (footprint "lib:fp" (layer "F.Cu") (uuid "u1") (at 10 20 90)
                (solder_mask_margin 0.03)
                (attr smd allow_soldermask_bridges)
                (net_tie_pad_groups "1, 2")
                (property "Reference" "U1" (at 0 0 0) (layer "F.SilkS"))
                (fp_line (start -1 0) (end 1 0) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
                (fp_rect (start 0 0) (end 2 1) (stroke (width 0.1) (type solid)) (fill yes) (layer "F.Mask"))
                (fp_line (start 0 0) (end 1 0) (stroke (width 0.1) (type solid)) (layer "F.Fab"))
                (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.SilkS") (solder_mask_margin 0.02) (net 1 "A"))
                (pad "2" thru_hole circle (at 1 0) (size 1.5 1.5) (drill 0.8) (layers "*.Cu" "*.Mask") (net 2 "B"))
                (pad "3" smd circle (at 3 0) (size 1 1) (layers "F.Mask") (pintype "free")))
            (via (at 5 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (tenting (front no) (back none)) (net 1))
            (via (at 6 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (tenting none) (net 1))
        )"#;
        let (design, model, notes) = import_kicad_pcb(text).expect("parses");
        // The mask-only pad is not copper: counted as skipped, kept as an aperture.
        assert_eq!(notes.non_copper_pads_skipped, 1);
        let inst = design.placement.as_ref().expect("placement").footprints.iter().find(|f| f.id == "U1").expect("U1").clone();
        let dr = design.drawings.expect("drawings");
        let fp = dr.footprint_extras.iter().find(|e| e.id == "U1").expect("extra for U1");
        assert_eq!(fp.solder_mask_margin, Some(mm(0.03)));
        assert!(fp.allow_soldermask_bridges);
        assert_eq!(fp.net_tie_pad_groups, vec!["1, 2".to_string()]);
        assert_eq!(fp.pads.len(), 2);
        assert_eq!(fp.pads[0].solder_mask_margin, Some(mm(0.02)));
        assert_eq!(fp.pads[0].layers, vec!["F.Cu".to_string(), "F.Mask".to_string(), "F.SilkS".to_string()]);
        assert_eq!(fp.pads[1].layers, vec!["B.Cu".to_string(), "B.Mask".to_string(), "F.Cu".to_string(), "F.Mask".to_string()]);
        assert_eq!(fp.pads[1].solder_mask_margin, None);

        // The footprint's graphics are not kept in board space: the silk line, the F.Fab line and the mask rectangle are its own (`Footprint::art`), in its frame.
        assert_eq!(fp.graphics.iter().filter(|g| g.pad_number.is_none()).count(), 0);
        assert!(fp.texts.is_empty());
        let art = &model.footprints.iter().find(|f| f.name == "lib:fp").expect("the footprint").art;
        assert_eq!(art.shapes.len(), 3);
        let silk = art.shapes.iter().find(|g| g.shape.layer() == "F.SilkS").expect("silk line");
        assert_eq!(silk.shape.points(), vec![Point { x: mm(-1.0), y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }]);
        assert_eq!(silk.shape.stroke_width(), mm(0.12));
        // Placed rotated 90 deg (file angle 90 = counter-clockwise) about (10, 20): local (-1, 0) -> (10, 21).
        assert_eq!(silk.placed(&inst).shape.points(), vec![Point { x: mm(10.0), y: mm(21.0) }, Point { x: mm(10.0), y: mm(19.0) }]);
        let mask = art.shapes.iter().find(|g| g.shape.layer() == "F.Mask").expect("mask rect");
        assert!(matches!(&mask.shape, Shape::Rect { filled: true, .. } | Shape::Polygon { filled: true, .. }), "{:?}", mask.shape);
        assert!(art.shapes.iter().any(|g| g.shape.layer() == "F.Fab"));
        let aperture = fp.graphics.iter().find(|g| g.pad_number.as_deref() == Some("3")).expect("aperture pad");
        assert_eq!(aperture.pin_type, "free");
        assert_eq!(aperture.shape.layer(), "F.Mask");

        // Via tenting: an explicit `no` is kept, `none` inherits (no entry).
        assert_eq!(dr.via_tenting.len(), 1);
        assert_eq!((dr.via_tenting[0].at, dr.via_tenting[0].front, dr.via_tenting[0].back), (Point { x: mm(5.0), y: mm(5.0) }, Some(false), None));
    }

    /// Footprint text and `gr_text` on silk: the footprint's Reference and Value are its fields (hidden one hidden, back-side anchor
    /// un-mirrored), the board's text has its variables resolved and `\n` a newline.
    #[test]
    fn imports_silk_text_with_layout_attributes() {
        let text = r#"(kicad_pcb (version 20241229) (generator "eda-kicad")
            (layers (0 "F.Cu" signal) (31 "B.Cu" signal))
            (title_block (rev "v2"))
            (net 0 "")
            (footprint "lib:fp" (layer "B.Cu") (uuid "u1") (at 10 10)
                (property "Reference" "TP4" (at 0 1 180) (layer "B.Fab") (hide yes))
                (property "Value" "~{CLK}" (at 3.7 0 180) (layer "B.SilkS") (effects (font (size 1 1) (thickness 0.15)) (justify mirror)))
                (property "Datasheet" "" (at 0 0 0) (layer "B.Fab") (hide yes)))
            (gr_text "REV ${REVISION}\nline2" (at 5 5 90) (layer "F.SilkS") (effects (font (size 1.5 1.2) (thickness 0.2) bold) (justify left bottom)))
        )"#;
        let (design, _model, _notes) = import_kicad_pcb(text).expect("parses");
        let inst = design.placement.as_ref().expect("placement").footprints[0].clone();
        let dr = design.drawings.expect("drawings");
        // The footprint's texts are its fields, not items of the extras in board space.
        assert!(dr.footprint_extras.iter().all(|e| e.texts.is_empty()));
        let edit = dr.footprint_edits.iter().find(|e| e.id == "TP4").expect("TP4's fields");
        assert!(!edit.reference.as_ref().expect("Reference").visible, "the hidden Reference stays hidden");
        let t = edit.value.as_ref().expect("Value");
        assert_eq!((t.layer.as_str(), t.visible, t.mirror, t.keep_upright, t.board_angle(&inst)), ("B.SilkS", true, true, true, 180_000));
        // Back footprint: the file's local x is already mirrored, so +3.7 lands to the right.
        assert_eq!(t.board_position(&inst), Point { x: mm(13.7), y: mm(10.0) });
        assert_eq!(dr.silk_texts.len(), 1);
        let g = &dr.silk_texts[0];
        assert_eq!(g.text, "REV v2\nline2");
        assert_eq!((g.size, g.thickness, g.halign, g.valign, g.keep_upright, g.bold), ((mm(1.5), mm(1.2)), mm(0.2), -1, 1, false, true));
        assert_eq!(g.angle_file_mdeg, 90_000);
    }
    /// A KiCad 10 file lists no nets at the top and names a net where it is used (`PCB_IO_KICAD_SEXPR_PARSER::parseNet`): the net of a pad, a track, a via or a zone is the
    /// name, and a name made of digits is a name and not a net code -- a SPICE netlist calls ground "0" -- and a net only copper uses is a net of the board (the board a
    /// kicad-cli import of an EasyEDA design makes has one: a pour on "0" and no pad on it).
    #[test]
    fn a_kicad_10_net_is_its_name_even_when_the_name_is_digits_and_a_net_only_copper_uses_is_a_net() {
        let text = r#"(kicad_pcb (version 20260901) (generator "pcbnew")
            (layers (0 "F.Cu" signal) (31 "B.Cu" signal))
            (footprint "L:R" (layer "F.Cu") (uuid "f1") (at 10 10)
                (property "Reference" "R1" (at 0 0 0) (layer "F.SilkS") (uuid "p1"))
                (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu") (net "12") (uuid "a1"))
                (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu") (net "GND") (uuid "a2")))
            (segment (start 9 10) (end 9 14) (width 0.2) (layer "F.Cu") (net "12") (uuid "s1"))
            (via (at 9 14) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net "VIA_ONLY") (uuid "v1"))
            (zone (net "0") (layer "B.Cu") (uuid "z1") (hatch edge 0.5)
                (polygon (pts (xy 0 0) (xy 20 0) (xy 20 20) (xy 0 20))))
        )"#;
        let (design, model, _notes) = import_kicad_pcb(text).expect("parses");
        let names: Vec<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["0", "12", "GND", "VIA_ONLY"], "every name, the digits too, and the ones no pad uses");
        let pins = |net: &str| model.nets.iter().find(|n| n.name == net).unwrap().pins.clone();
        assert_eq!(pins("12"), vec!["R1.1".to_string()], "the pad on net \"12\" is on net 12, not on net code 12 (which is none)");
        assert_eq!(pins("GND"), vec!["R1.2".to_string()]);
        let routing = design.routing.expect("copper");
        assert_eq!(routing.tracks[0].net, "12");
        assert_eq!(routing.vias[0].net, "VIA_ONLY");
        assert_eq!(routing.zones[0].net, "0");

        // A legacy file lists its nets, and there a number is a code: `(net 12)` is the twelfth net, `(net 12 "name")` says its name again.
        let legacy = r#"(kicad_pcb (version 20221018) (generator "pcbnew")
            (layers (0 "F.Cu" signal) (31 "B.Cu" signal))
            (net 0 "") (net 12 "SIG")
            (segment (start 9 10) (end 9 14) (width 0.2) (layer "F.Cu") (net 12) (tstamp "s1"))
        )"#;
        let (design, model, _) = import_kicad_pcb(legacy).expect("parses");
        assert_eq!(design.routing.expect("copper").tracks[0].net, "SIG");
        assert_eq!(model.nets.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), vec!["SIG"]);
    }
}

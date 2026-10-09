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
//!   make. A footprint's own Edge.Cuts graphics become board-level shapes. Nothing is closed that was not
//!   closed: what KiCad reports as a malformed outline is in [`ImportNotes::outline_errors`]. A track
//!   `(arc ...)` is kept as an arc ([`eda_model::ir::Track::new_arc`], counted in
//!   [`ImportNotes::track_arcs_kept`], and written back as an arc), and a board-level `gr_arc` is a
//!   [`eda_model::ir::Shape::Arc`], exactly (KiCad's own three-point arc storage is our `Arc`'s storage
//!   too).
//! - **Non-rect/roundrect/circle/oval pads** (trapezoid, custom): mapped to
//!   `PadShape::Rect` at the pad's nominal `size`, counted in
//!   [`ImportNotes::non_rect_pad_shapes_approximated`].
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

use std::collections::BTreeMap;

use eda_model::ir::{Design, DrawingsSection, FillMode, FootprintExtra, FootprintGraphic, FootprintInstance, FootprintText, PadMaskInfo, ViaTenting, IslandRemovalMode, PadConnection, Point, PlacementSection, Provenance, RoutingSection, Shape, Side, Text, TextJustify, Track, Via, Zone};
use eda_model::outline as edge;
use eda_model::{BoardRules, CheckResult, ConstraintModel, Footprint, Net, NetClass, Pad, PadKind, PadShape, Part, Pin, PinKind};

use crate::import_items::{self, ItemRef, Refs};
use crate::sexpr::{self, Sexpr};

/// Counts of things the importer saw but could not carry into our model
/// exactly, or at all -- meant to be printed, never silently absorbed.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportNotes {
    pub zones_skipped: usize,
    /// Always 0 now: an Edge.Cuts arc is kept as an arc ([`Self::outline_shapes`]), not tessellated into the outline's straight segments.
    /// (Kept so an `ImportNotes` written by an older build still reads.)
    pub track_arcs_approximated: usize,
    /// Track `(arc ...)`s kept as arcs (`Track::arc_mid_offset`); informational, nothing was lost.
    #[serde(default)]
    pub track_arcs_kept: usize,
    pub non_rect_pad_shapes_approximated: usize,
    /// A `(pad ...)` whose own `(layers ...)` list names no copper layer at
    /// all (no `F.Cu`/`B.Cu`/`*.Cu`/inner `.Cu`) -- dropped on import rather
    /// than kept as a phantom copper pad. Real boards use this for a
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
}

/// Parse a `.kicad_pcb` file's text into our own design + constraint
/// model. See the module docs for exactly what is ported vs. approximated
/// vs. dropped (reported in the returned [`ImportNotes`]).
pub fn import_kicad_pcb(text: &str) -> Result<(Design, ConstraintModel, ImportNotes), Vec<CheckResult>> {
    let tree = sexpr::parse(text)
        .map_err(|e| vec![CheckResult::fail("kicad_import.parse", "file", format!("not a valid s-expression file: {e}"))])?;
    let root = tree
        .as_list()
        .filter(|l| sexpr::tag(l) == Some("kicad_pcb"))
        .ok_or_else(|| vec![CheckResult::fail("kicad_import.not_a_board", "file", "top-level form is not (kicad_pcb ...); not a KiCad PCB file")])?;

    let mut notes = ImportNotes::default();

    let layers = import_layers(root);
    let net_names = import_net_names(root);
    let mut board = import_board_rules(root, &layers);

    // Each item parser records the uuid and lock of what it emits (`Refs`), so the groups and locks of the
    // file can be turned into ids once the items have them.
    let mut refs = Refs::default();
    let (footprints_ir, parts, explicit_footprints, pin_nets, footprint_extras) = import_footprints(root, &net_names, &layers, &mut notes, &mut refs)?;
    let nets = build_nets(&net_names, &pin_nets);
    let (tracks, vias, via_tenting) = import_routing(root, &net_names, &mut notes, &mut refs);
    let (mut shapes, texts) = import_drawings(root, &mut refs);
    // The Edge.Cuts shapes just read are the outline's items, or, when they are a plain closed loop of lines, that loop.
    let (outline, outline_is_shapes) = import_outline(root, &mut shapes, &mut refs, &mut notes);
    let dimensions = import_items::import_dimensions(root, &mut refs);
    let silk_texts = import_board_silk_texts(root);
    let copper_texts = import_board_copper_texts(root);
    let zones = import_zones(root, &net_names, &layers, &mut notes, &mut refs);

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
        schema: 1,
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
        drawings: if shapes.is_empty() && texts.is_empty() && dimensions.is_empty() && footprint_extras.is_empty() && via_tenting.is_empty() && copper_texts.is_empty() { None } else { Some(DrawingsSection { shapes, texts, dimensions, footprint_extras, via_tenting, silk_texts, copper_texts, outline_is_shapes, ..Default::default() }) },
        footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
    };
    // `(setup (aux_axis_origin x y))`: the drill/place file origin the board was saved with.
    if let Some(aux) = sexpr::find(root, "setup").and_then(|s| sexpr::find(s, "aux_axis_origin")) {
        if let (Some(x), Some(y)) = (sexpr::num(aux, 1), sexpr::num(aux, 2)) {
            let at = Point { x: mm_to_um(x), y: mm_to_um(y) };
            if at.x != 0 || at.y != 0 {
                design.drawings.get_or_insert_with(Default::default).aux_origin = Some(at);
            }
        }
    }
    // `(setup (grid_origin x y))`: the point the editing grid was anchored at.
    if let Some(grid) = sexpr::find(root, "setup").and_then(|s| sexpr::find(s, "grid_origin")) {
        if let (Some(x), Some(y)) = (sexpr::num(grid, 1), sexpr::num(grid, 2)) {
            let at = Point { x: mm_to_um(x), y: mm_to_um(y) };
            if at.x != 0 || at.y != 0 {
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
    // The file's groups and locks, now that every item has its id: a `(group ..)` names its members by file
    // uuid, and `locked` rides on each item (`BOARD_ITEM::IsLocked()`).
    let (groups, locked_ids) = refs.resolve(&design, &import_items::import_groups(root));
    if !groups.is_empty() || !locked_ids.is_empty() {
        let dr = design.drawings.get_or_insert_with(Default::default);
        dr.groups = groups;
        dr.locked_ids = locked_ids;
        dr.assign_missing_ids();
    }
    let model = ConstraintModel {
        parts,
        nets,
        clusters: vec![],
        placement_rules: vec![],
        stackup: import_stackup(root),
        impedance_targets: vec![],
        footprints: explicit_footprints.into_values().collect(),
        symbols: vec![],
        board,
        allow: Default::default(),
        solver: Default::default(),
    };
    Ok((design, model, notes))
}

pub fn mm_to_um(mm: f64) -> i64 {
    (mm * 1000.0).round() as i64
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

fn xy_point(list: &[Sexpr]) -> Option<Point> {
    let (x, y) = (sexpr::num(list, 1)?, sexpr::num(list, 2)?);
    Some(Point { x: mm_to_um(x), y: mm_to_um(y) })
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
    match sexpr::num(n, 1) {
        Some(code) => net_names.get(&(code as i64)).cloned().or_else(|| sexpr::txt(n, 2).map(str::to_string)).unwrap_or_default(),
        None => sexpr::txt(n, 1).unwrap_or("").to_string(),
    }
}

fn build_nets(net_names: &BTreeMap<i64, String>, pin_nets: &[(String, String)]) -> Vec<Net> {
    let mut pins_by_name: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (pin, net) in pin_nets {
        pins_by_name.entry(net.as_str()).or_default().push(pin.clone());
    }
    // KiCad 10 files carry no top-level net table: every name used by a pad
    // is a net too.
    let mut names: Vec<&str> = net_names.values().map(String::as_str).collect();
    names.extend(pin_nets.iter().map(|(_, n)| n.as_str()));
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
    let mut board = BoardRules { layers: layers.to_vec(), constraints_explicit: true, ..BoardRules::default() };
    // Legacy boards keep the copper-to-edge clearance in `(setup (edge_clearance ..))`.
    if let Some(v) = sexpr::find(root, "setup").and_then(|st| sexpr::find(st, "edge_clearance")).and_then(|e| sexpr::num(e, 1)) {
        board.copper_edge_clearance_um = Some(mm_to_um(v));
    }
    let classes: Vec<&[Sexpr]> = sexpr::find_all(root, "net_class").collect();
    let default_idx = classes.iter().position(|nc| sexpr::txt(nc, 1) == Some("Default")).or(if classes.len() == 1 { Some(0) } else { None });

    if let Some(i) = default_idx {
        let nc = classes[i];
        if let Some(v) = sexpr::find(nc, "trace_width").and_then(|f| sexpr::num(f, 1)) {
            board.track_width = mm_to_um(v);
        }
        if let Some(v) = sexpr::find(nc, "clearance").and_then(|f| sexpr::num(f, 1)) {
            board.clearance = mm_to_um(v);
        }
        if let Some(v) = sexpr::find(nc, "via_dia").and_then(|f| sexpr::num(f, 1)) {
            board.via_diameter = mm_to_um(v);
        }
        if let Some(v) = sexpr::find(nc, "via_drill").and_then(|f| sexpr::num(f, 1)) {
            board.via_drill = mm_to_um(v);
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
            let track_width = sexpr::find(nc, "trace_width").and_then(|f| sexpr::num(f, 1)).map(mm_to_um);
            let clearance = sexpr::find(nc, "clearance").and_then(|f| sexpr::num(f, 1)).map(mm_to_um);
            let via_diameter = sexpr::find(nc, "via_dia").and_then(|f| sexpr::num(f, 1)).map(mm_to_um);
            let via_drill = sexpr::find(nc, "via_drill").and_then(|f| sexpr::num(f, 1)).map(mm_to_um);
            let extras = default_class_extras(nc);
            let (microvia_diameter, microvia_drill, diff_pair_width, diff_pair_gap) = extras.map_or((None, None, None, None), |e| (e.microvia_diameter, e.microvia_drill, e.diff_pair_width, e.diff_pair_gap));
            Some(NetClass { name, nets, track_width, clearance, via_diameter, via_drill, microvia_diameter, microvia_drill, diff_pair_width, diff_pair_gap, diff_pair_via_gap: None, priority: 0 })
        })
        .collect();
    import_legacy_setup_minimums(root, &mut board);
    import_solder_mask_setup(root, &mut board);
    board
}

/// A `(net_class ..)` block's microvia size and differential-pair sizes (`uvia_dia`, `uvia_drill`, `diff_pair_width`,
/// `diff_pair_gap`), as a class with only those set -- `None` when the class has none. KiCad's own 0.3 / 0.1 mm microvia
/// (what the writer gives a class that sets none) reads back as unset, so a board that never set one round-trips unchanged.
fn default_class_extras(nc: &[Sexpr]) -> Option<NetClass> {
    let um = |key: &str| sexpr::find(nc, key).and_then(|f| sexpr::num(f, 1)).map(mm_to_um);
    let microvia_diameter = um("uvia_dia").filter(|v| *v != 300);
    let microvia_drill = um("uvia_drill").filter(|v| *v != 100);
    let (diff_pair_width, diff_pair_gap) = (um("diff_pair_width"), um("diff_pair_gap"));
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
        board.solder_mask.expansion_um = mm_to_um(v);
    }
    if let Some(v) = sexpr::find(setup, "solder_mask_min_width").and_then(|f| sexpr::num(f, 1)) {
        board.solder_mask.min_width_um = mm_to_um(v);
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
        board.solder_mask.paste_margin_um = mm_to_um(v);
    }
    if let Some(v) = sexpr::find(setup, "pad_to_paste_clearance_ratio").and_then(|f| sexpr::num(f, 1)) {
        board.solder_mask.paste_margin_ratio = v;
    }
    // `(general (thickness ..))`: `BOARD_DESIGN_SETTINGS::SetBoardThickness`.
    if let Some(v) = sexpr::find(root, "general").and_then(|g| sexpr::find(g, "thickness")).and_then(|f| sexpr::num(f, 1)) {
        board.board_thickness_um = mm_to_um(v);
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
        board.track_width_min_um = mm_to_um(v);
    }
    if let Some(v) = sexpr::find(setup, "clearance_min").and_then(|f| sexpr::num(f, 1)) {
        board.min_clearance_um = mm_to_um(v);
    }
    if let Some(v) = sexpr::find(setup, "via_min_size").and_then(|f| sexpr::num(f, 1)) {
        board.via_diameter_min_um = mm_to_um(v);
    }
    let through_hole_min = sexpr::find(setup, "through_hole_min").or_else(|| sexpr::find(setup, "via_min_drill"));
    if let Some(v) = through_hole_min.and_then(|f| sexpr::num(f, 1)) {
        board.via_drill_min_um = mm_to_um(v);
    }
    if let Some(v) = sexpr::find(setup, "hole_to_hole_min").and_then(|f| sexpr::num(f, 1)) {
        board.hole_to_hole_min_um = mm_to_um(v);
    }
    if let Some(v) = sexpr::find(setup, "via_min_annulus").and_then(|f| sexpr::num(f, 1)) {
        board.annular_width_min_um = mm_to_um(v);
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
    let classes = net_settings.get("classes").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let patterns = net_settings.get("netclass_patterns").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let num_mm = |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_f64()).map(mm_to_um);

    let mut out = Vec::new();
    for class in &classes {
        let Some(name) = class.get("name").and_then(|v| v.as_str()) else { continue };
        if name == "Default" {
            continue; // the board-default fields already cover this; see the doc comment above
        }
        let nets: Vec<String> = patterns
            .iter()
            .filter(|p| p.get("netclass").and_then(|v| v.as_str()) == Some(name))
            .filter_map(|p| p.get("pattern").and_then(|v| v.as_str()).map(String::from))
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
        let mm = |key: &str| default.get(key).and_then(|v| v.as_f64()).map(mm_to_um);
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
    let mm = |key: &str| rules.get(key).and_then(|v| v.as_f64()).map(mm_to_um);
    let b = &mut model.board;
    // The project stated its minimums: they are exact, and written back exactly (`BoardRules::constraints_explicit`).
    b.constraints_explicit = true;
    if let Some(v) = mm("min_track_width") {
        b.track_width_min_um = v;
    }
    if let Some(v) = mm("min_clearance") {
        b.min_clearance_um = v;
    }
    if let Some(v) = mm("min_copper_edge_clearance") {
        b.copper_edge_clearance_um = Some(v);
    }
    if let Some(v) = mm("min_via_diameter") {
        b.via_diameter_min_um = v;
    }
    if let Some(v) = mm("min_through_hole_diameter") {
        b.via_drill_min_um = v;
    }
    if let Some(v) = mm("min_hole_to_hole") {
        b.hole_to_hole_min_um = v;
    }
    if let Some(v) = mm("min_hole_clearance") {
        b.hole_clearance_um = v;
    }
    if let Some(v) = mm("min_via_annular_width") {
        b.annular_width_min_um = v;
    }
    if let Some(v) = mm("min_silk_clearance") {
        b.silk_clearance_um = v;
    }
    if let Some(v) = mm("min_text_height") {
        b.min_silk_text_height_um = v;
    }
    if let Some(v) = mm("min_text_thickness") {
        b.min_silk_text_thickness_um = v;
    }
    // `m_SolderMaskToCopperClearance` lives only in the project file.
    if let Some(v) = mm("solder_mask_to_copper_clearance") {
        b.solder_mask.to_copper_clearance_um = v;
    }
    // The rest of Board Setup > Constraints.
    if let Some(v) = mm("min_connection") {
        b.min_connection_um = v;
    }
    if let Some(v) = mm("min_microvia_diameter") {
        b.microvia_diameter_min_um = v;
    }
    if let Some(v) = mm("min_microvia_drill") {
        b.microvia_drill_min_um = v;
    }
    if let Some(v) = mm("min_groove_width") {
        b.min_groove_width_um = v;
    }
    if let Some(v) = mm("max_error") {
        b.max_error_um = v.max(1);
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
        let num = |key: &str| def.get(key).and_then(|v| v.as_f64()).map(mm_to_um);
        let flag = |key: &str| def.get(key).and_then(|v| v.as_bool());
        let class = |prefix: &str, c: &mut eda_model::rules::LayerClassDefaults| {
            if let Some(v) = num(&format!("{prefix}_line_width")) {
                c.line_width_um = v;
            }
            if let Some(v) = num(&format!("{prefix}_text_size_h")) {
                c.text_width_um = v;
            }
            if let Some(v) = num(&format!("{prefix}_text_size_v")) {
                c.text_height_um = v;
            }
            if let Some(v) = num(&format!("{prefix}_text_thickness")) {
                c.text_thickness_um = v;
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
            b.text_graphics.edge_cuts_line_width_um = v;
        }
        if let Some(v) = num("courtyard_line_width") {
            b.text_graphics.courtyard_line_width_um = v;
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
    let number = sexpr::txt(pad, 1).unwrap_or("").to_string();
    let kind_tok = sexpr::txt(pad, 2).unwrap_or("");
    let pad_kind = match kind_tok {
        "np_thru_hole" => PadKind::NonPlatedHole,
        "smd" | "connect" => PadKind::Smd,
        _ => PadKind::ThroughHole, // "thru_hole"
    };
    let shape_tok = sexpr::txt(pad, 3).unwrap_or("");
    let pad_shape = match shape_tok {
        "circle" => PadShape::Circle,
        "oval" => PadShape::Oval,
        "roundrect" => PadShape::RoundRect,
        "rect" => PadShape::Rect,
        _ => {
            // trapezoid / custom: no equivalent shape, keep the pad's
            // nominal rectangle so it still occupies roughly the right
            // footprint.
            notes.non_rect_pad_shapes_approximated += 1;
            PadShape::Rect
        }
    };

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
        // on file are already flipped for a back-side footprint).
        if pad_kind == PadKind::Smd {
            let (f, b) = (names.contains(&"F.Cu"), names.contains(&"B.Cu"));
            opposite_side = if fp_side == Side::Bottom { f && !b } else { b && !f };
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
                (None, Some((mm_to_um(dw), mm_to_um(dh))))
            } else {
                let d0 = toks.iter().find_map(|s| s.parse::<f64>().ok());
                (d0.map(mm_to_um), None)
            }
        }
        None => (None, None),
    };

    let roundrect_ratio = sexpr::find(pad, "roundrect_rratio").and_then(|r| sexpr::num(r, 1));

    Some(Pad { number, at: (mm_to_um(px), mm_to_um(py)), size: (mm_to_um(w), mm_to_um(h)), shape: pad_shape, kind: pad_kind, drill, drill_slot, rot, roundrect_ratio, opposite_side })
}

/// The cache key `import_footprints` should use for an instance whose lib id
/// is `lib_id` and whose own freshly-parsed pad geometry is `pads`: `lib_id`
/// itself if nothing is cached there yet or the cached entry already has
/// this exact geometry, else `"{lib_id}#2"`/`"#3"`/... -- the first slot
/// (plain or suffixed) whose pads match, or a freshly inserted one if none
/// do. See `import_footprints`'s call site for why this can legitimately
/// happen (the same lib id placed on both sides of one board).
fn dedup_footprint_key(explicit: &mut BTreeMap<String, Footprint>, lib_id: &str, pads: &[Pad]) -> String {
    match explicit.get(lib_id) {
        None => lib_id.to_string(),
        Some(existing) if existing.pads.as_slice() == pads => lib_id.to_string(),
        Some(_) => {
            for n in 2.. {
                let key = format!("{lib_id}#{n}");
                match explicit.get(&key) {
                    None => return key,
                    Some(existing) if existing.pads.as_slice() == pads => return key,
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
fn footprint_courtyard_outlines(fp: &[Sexpr], side: Side) -> Vec<Vec<(i64, i64)>> {
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
        let r = ((e.x - c.x) as f64).hypot((e.y - c.y) as f64);
        outlines.push((0..32).map(|i| { let t = i as f64 * std::f64::consts::TAU / 32.0; Point { x: c.x + (r * t.cos()).round() as i64, y: c.y + (r * t.sin()).round() as i64 } }).collect());
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

fn footprint_courtyard_half(fp: &[Sexpr], side: Side) -> Option<(i64, i64)> {
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
        let r = (((e.x - c.x) as f64).powi(2) + ((e.y - c.y) as f64).powi(2)).sqrt().round() as i64;
        pts.extend([Point { x: c.x - r, y: c.y }, Point { x: c.x + r, y: c.y }, Point { x: c.x, y: c.y - r }, Point { x: c.x, y: c.y + r }]);
    }
    if pts.is_empty() {
        return None;
    }

    let (mut hw, mut hh) = (0, 0);
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
) -> Result<(Vec<FootprintInstance>, Vec<Part>, BTreeMap<String, Footprint>, Vec<(String, String)>, Vec<FootprintExtra>), Vec<CheckResult>> {
    let file_version = sexpr::find(root, "version").and_then(|v| sexpr::num(v, 1)).unwrap_or(0.0) as i64;
    let mut extras: Vec<FootprintExtra> = Vec::new();
    let title_vars = title_block_vars(root);
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
        let (x, y) = (mm_to_um(fx), mm_to_um(fy));
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

        let mut pads = Vec::new();
        let mut pins = Vec::new();
        let fp_inst = FootprintInstance { id: reference.clone(), at: Point { x, y }, rot, side, label: Default::default() };
        let mut extra = footprint_extra_header(fp, &reference, file_version);
        for pad in sexpr::find_all(fp, "pad") {
            let Some(p) = parse_pad_geometry(pad, side, rot, notes) else {
                if let Some(g) = mask_only_pad_graphics(pad, &fp_inst, rot, net_names, notes) {
                    extra.graphics.extend(g);
                }
                continue;
            };
            extra.pads.push(pad_mask_info(pad, layers, file_version));

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
        extra.graphics.extend(import_fp_graphics(fp, &fp_inst));
        extra.texts = import_fp_texts(fp, &fp_inst, &reference, value.as_deref().unwrap_or(""), &title_vars);
        extras.push(extra);
        let key = dedup_footprint_key(&mut explicit, &lib_id, &pads);
        let courtyard = footprint_courtyard_half(fp, side);
        let courtyard_outlines = footprint_courtyard_outlines(fp, side);
        explicit.entry(key.clone()).or_insert_with(|| Footprint { name: key.clone(), pads, courtyard, courtyard_outlines, model: crate::footprint_lib::model_from(fp) });

        parts.push(Part { reference: reference.clone(), mpn: None, lcsc: None, value, package: None, footprint: Some(key), pins, body_um: None, symbol: None, datasheet: None, edge: None });
        footprints_ir.push(FootprintInstance { id: reference, at: Point { x, y }, rot, side, label: Default::default() });
        refs.footprints.push(ItemRef::of(fp));
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    Ok((footprints_ir, parts, explicit, pin_nets, extras))
}

/// Footprint-level solder-mask facts: `(solder_mask_margin ..)` (pre-9.0
/// files: 0 meant "inherit"), `(attr .. allow_soldermask_bridges)`,
/// `(net_tie_pad_groups ..)`.
fn footprint_extra_header(fp: &[Sexpr], reference: &str, file_version: i64) -> FootprintExtra {
    let mut e = FootprintExtra { id: reference.to_string(), ..Default::default() };
    if let Some(m) = sexpr::find(fp, "solder_mask_margin").and_then(|f| sexpr::num(f, 1)) {
        let m = mm_to_um(m);
        e.solder_mask_margin = if file_version <= 20240201 && m == 0 { None } else { Some(m) };
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
        let c = mm_to_um(c);
        e.clearance = if file_version <= 20240201 && c == 0 { None } else { Some(c) };
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
fn pad_mask_info(pad: &[Sexpr], copper: &[String], file_version: i64) -> PadMaskInfo {
    let mut info = PadMaskInfo::default();
    if let Some(l) = sexpr::find(pad, "layers") {
        let names: Vec<&str> = l.iter().skip(1).filter_map(Sexpr::text).collect();
        info.layers = expand_layer_names(&names, copper);
    }
    if let Some(m) = sexpr::find(pad, "solder_mask_margin").and_then(|f| sexpr::num(f, 1)) {
        let m = mm_to_um(m);
        info.solder_mask_margin = if file_version <= 20240201 && m == 0 { None } else { Some(m) };
    }
    if let Some(t) = sexpr::find(pad, "tenting") {
        let (f, b) = front_back_opt_bool(t);
        info.tent_front = f;
        info.tent_back = b;
    }
    if let Some(t) = sexpr::find(pad, "pintype").and_then(|f| sexpr::txt(f, 1)) {
        info.pin_type = t.to_string();
    }
    // `parsePAD`: `(zone_connect N)`, `(thermal_bridge_width ..)` (legacy `(thermal_width ..)`), `(thermal_bridge_angle ..)`, `(thermal_gap ..)`.
    if let Some(c) = sexpr::find(pad, "clearance").and_then(|f| sexpr::num(f, 1)) {
        let c = mm_to_um(c);
        info.clearance = if file_version <= 20240201 && c == 0 { None } else { Some(c) };
    }
    info.zone_connection = sexpr::find(pad, "zone_connect").and_then(|z| sexpr::num(z, 1)).and_then(|v| crate::zone_connection_from_file(v as i64));
    info.thermal_spoke_width = sexpr::find(pad, "thermal_bridge_width").or_else(|| sexpr::find(pad, "thermal_width")).and_then(|f| sexpr::num(f, 1)).map(mm_to_um);
    info.thermal_spoke_angle_mdeg = sexpr::find(pad, "thermal_bridge_angle").and_then(|f| sexpr::num(f, 1)).map(|d| ((d * 1000.0).round() as i64).rem_euclid(360_000) as eda_model::ir::Millideg);
    info.thermal_gap = sexpr::find(pad, "thermal_gap").and_then(|f| sexpr::num(f, 1)).map(mm_to_um);
    info
}

fn rotated_extent(rot_millideg: i64, size: (i64, i64)) -> (i64, i64) {
    let rad = (rot_millideg as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let (w, h) = (size.0 as f64, size.1 as f64);
    ((w * cos.abs() + h * sin.abs()).round() as i64, (w * sin.abs() + h * cos.abs()).round() as i64)
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
            PadShape::Circle => Shape::Circle { id: String::new(), layer, stroke_width: 0, filled: true, center, end: Point { x: center.x + w.max(h) / 2, y: center.y } },
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
                stroke_width: 0,
                filled: true,
                pts: vec![Point { x: center.x - w / 2, y: center.y - h / 2 }, Point { x: center.x + w / 2, y: center.y - h / 2 }, Point { x: center.x + w / 2, y: center.y + h / 2 }, Point { x: center.x - w / 2, y: center.y + h / 2 }],
            },
        };
        out.push(FootprintGraphic { shape, solder_mask_margin: None, pad_number: Some(p.number.clone()), net: net.clone(), pin_type: pin_type.clone() });
    }
    Some(out)
}

/// `(hide yes)` / legacy bare `hide`, directly on `item` or inside its `(effects ..)`.
fn is_hidden(item: &[Sexpr]) -> bool {
    let direct = |l: &[Sexpr]| l.iter().skip(1).any(|c| matches!(c, Sexpr::Atom(a) if a == "hide")) || sexpr::find(l, "hide").is_some_and(|h| sexpr::txt(h, 1).map_or(true, |v| v == "yes"));
    direct(item) || sexpr::find(item, "effects").is_some_and(direct)
}

/// A footprint's visible silkscreen text: `(property ..)` fields and
/// `(fp_text ..)` items (`parsePCB_TEXT` / `parsePCB_TEXT_effects`): the
/// anchor is relative to the footprint (un-mirrored for a back footprint
/// like a pad's `(at ..)`), the angle is absolute, footprint text keeps
/// itself upright unless `unlocked`.
fn import_fp_texts(fp: &[Sexpr], inst: &FootprintInstance, reference: &str, value: &str, vars: &[(String, String)]) -> Vec<FootprintText> {
    let tf = |p: Point| -> Point { eda_model::footprint::to_board(inst, (if inst.side == Side::Bottom { -p.x } else { p.x }, p.y)) };
    let mut out = Vec::new();
    for item in fp.iter().filter_map(Sexpr::as_list) {
        let tag = sexpr::tag(item);
        if tag != Some("property") && tag != Some("fp_text") {
            continue;
        }
        let Some(raw) = sexpr::txt(item, 2) else { continue };
        if is_hidden(item) {
            continue;
        }
        let text = raw.replace("${REFERENCE}", reference).replace("${VALUE}", value).replace("%R", reference).replace("%V", value);
        out.extend(parse_silk_text(item, &resolve_text_vars(&text, vars), &tf, true));
    }
    out
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
    let size = font.and_then(|f| sexpr::find(f, "size")).map(|s| (mm_to_um(sexpr::num(s, 1).unwrap_or(1.0)), mm_to_um(sexpr::num(s, 2).unwrap_or(1.0)))).unwrap_or((1000, 1000));
    let thickness = font.and_then(|f| sexpr::find(f, "thickness")).and_then(|t| sexpr::num(t, 1)).map(mm_to_um).unwrap_or(0);
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
        at: tf(Point { x: mm_to_um(x), y: mm_to_um(y) }),
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

/// A footprint's `fp_line`/`fp_arc`/`fp_circle`/`fp_rect`/`fp_poly` on the
/// silkscreen and solder-mask layers, in board space (the same
/// un-mirror-then-`to_board` convention a pad's `(at ..)` uses -- see
/// [`parse_pad_geometry_opt`]).
fn import_fp_graphics(fp: &[Sexpr], inst: &FootprintInstance) -> Vec<FootprintGraphic> {
    const KEEP: [&str; 4] = ["F.SilkS", "B.SilkS", "F.Mask", "B.Mask"];
    let tf = |p: Point| -> Point { eda_model::footprint::to_board(inst, (if inst.side == Side::Bottom { -p.x } else { p.x }, p.y)) };
    let pt = |item: &[Sexpr], key: &str| -> Option<Point> { sexpr::find(item, key).and_then(xy_point).map(tf) };
    let stroke_width = |item: &[Sexpr]| -> i64 {
        sexpr::find(item, "stroke")
            .and_then(|s| sexpr::find(s, "width"))
            .and_then(|w| sexpr::num(w, 1))
            .or_else(|| sexpr::find(item, "width").and_then(|w| sexpr::num(w, 1)))
            .map(mm_to_um)
            .unwrap_or(0)
    };
    let filled = |item: &[Sexpr]| -> bool {
        match sexpr::find(item, "fill") {
            Some(f) => sexpr::txt(f, 1).is_some_and(|s| s == "yes" || s == "solid"),
            None => false,
        }
    };
    let layers_of = |item: &[Sexpr]| -> Vec<String> {
        if let Some(l) = sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)) {
            return vec![l.to_string()];
        }
        let names: Vec<&str> = sexpr::find(item, "layers").map(|l| l.iter().skip(1).filter_map(Sexpr::text).collect()).unwrap_or_default();
        expand_layer_names(&names, &[])
    };
    let mut out = Vec::new();
    for tag in ["fp_line", "fp_arc", "fp_circle", "fp_rect", "fp_poly", "fp_curve"] {
        for item in sexpr::find_all(fp, tag) {
            let margin = sexpr::find(item, "solder_mask_margin").and_then(|f| sexpr::num(f, 1)).map(mm_to_um);
            for layer in layers_of(item).into_iter().filter(|l| KEEP.contains(&l.as_str())) {
                let (sw, fl) = (stroke_width(item), filled(item));
                let shape = match tag {
                    "fp_line" => match (pt(item, "start"), pt(item, "end")) {
                        (Some(start), Some(end)) => Shape::Segment { id: String::new(), layer, stroke_width: sw, filled: false, start, end },
                        _ => continue,
                    },
                    "fp_arc" => match (pt(item, "start"), pt(item, "mid"), pt(item, "end")) {
                        (Some(start), Some(mid), Some(end)) => Shape::Arc { id: String::new(), layer, stroke_width: sw, filled: fl, start, mid, end },
                        _ => continue,
                    },
                    "fp_circle" => match (pt(item, "center"), pt(item, "end")) {
                        (Some(center), Some(end)) => Shape::Circle { id: String::new(), layer, stroke_width: sw, filled: fl, center, end },
                        _ => continue,
                    },
                    "fp_rect" => {
                        let (Some(a), Some(b)) = (sexpr::find(item, "start").and_then(xy_point), sexpr::find(item, "end").and_then(xy_point)) else { continue };
                        let corners = [Point { x: a.x, y: a.y }, Point { x: b.x, y: a.y }, Point { x: b.x, y: b.y }, Point { x: a.x, y: b.y }];
                        Shape::Polygon { id: String::new(), layer, stroke_width: sw, filled: fl, pts: corners.iter().map(|c| tf(*c)).collect() }
                    }
                    "fp_curve" => {
                        let Some(pts) = poly_points(item) else { continue };
                        let [start, c1, c2, end] = pts[..] else { continue };
                        Shape::Bezier { id: String::new(), layer, stroke_width: sw, filled: false, start: tf(start), c1: tf(c1), c2: tf(c2), end: tf(end) }
                    }
                    _ => {
                        let Some(pts) = poly_points(item) else { continue };
                        Shape::Polygon { id: String::new(), layer, stroke_width: sw, filled: fl, pts: pts.into_iter().map(tf).collect() }
                    }
                };
                out.push(FootprintGraphic { shape, solder_mask_margin: margin, pad_number: None, net: String::new(), pin_type: String::new() });
            }
        }
    }
    out
}

// ---------------------------------------------------------------- routing

fn import_routing(root: &[Sexpr], net_names: &BTreeMap<i64, String>, notes: &mut ImportNotes, refs: &mut Refs) -> (Vec<Track>, Vec<Via>, Vec<ViaTenting>) {

    let mut tracks = Vec::new();
    for seg in sexpr::find_all(root, "segment") {
        let (Some(s), Some(e)) = (sexpr::find(seg, "start").and_then(xy_point), sexpr::find(seg, "end").and_then(xy_point)) else { continue };
        let net = net_ref(sexpr::find(seg, "net"), net_names);
        // Net 0 (no net) stays as an empty `net`: KiCad still checks a
        // netless track (clearance, shorting, dangling) like any other.
        let width = sexpr::find(seg, "width").and_then(|w| sexpr::num(w, 1)).map(mm_to_um).unwrap_or(200);
        let layer = sexpr::find(seg, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.Cu").to_string();
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
        let width = sexpr::find(arc, "width").and_then(|w| sexpr::num(w, 1)).map(mm_to_um).unwrap_or(200);
        let layer = sexpr::find(arc, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.Cu").to_string();
        notes.track_arcs_kept += 1;
        tracks.push(Track::new_arc(net, layer, width, s, m, e));
        refs.tracks.push(ItemRef::of(arc));
    }

    let mut vias = Vec::new();
    let mut via_tenting = Vec::new();
    for via in sexpr::find_all(root, "via") {
        let Some(at) = sexpr::find(via, "at").and_then(xy_point) else { continue };
        let net = net_ref(sexpr::find(via, "net"), net_names);
        let dia = sexpr::find(via, "size").and_then(|s| sexpr::num(s, 1)).map(mm_to_um).unwrap_or(600);
        let drill = sexpr::find(via, "drill").and_then(|d| sexpr::num(d, 1)).map(mm_to_um).unwrap_or(300);
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
        refs.vias.push(ItemRef::of(via));
    }

    (tracks, vias, via_tenting)
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
fn import_drawings(root: &[Sexpr], refs: &mut Refs) -> (Vec<Shape>, Vec<Text>) {
    let mut shapes = Vec::new();
    for item in sexpr::find_all(root, "gr_line") {
        if let Some(shape) = graphic_shape(item, "gr_line", |p| p) {
            shapes.push(shape);
            refs.shapes.push(ItemRef::of(item));
        }
    }
    for tag in ["gr_arc", "gr_rect", "gr_circle", "gr_poly", "gr_curve"] {
        for item in sexpr::find_all(root, tag) {
            if let Some(shape) = graphic_shape(item, tag, |p| p) {
                shapes.push(shape);
                refs.shapes.push(ItemRef::of(item));
            }
        }
    }

    let mut texts = Vec::new();
    for item in sexpr::find_all(root, "gr_text") {
        let Some(content) = sexpr::txt(item, 1) else { continue };
        let Some(at) = sexpr::find(item, "at") else { continue };
        let (Some(x), Some(y)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
        let angle = ((sexpr::num(at, 3).unwrap_or(0.0) * 1000.0).round() as i64).rem_euclid(360_000) as u32;
        let effects = sexpr::find(item, "effects");
        let font = effects.and_then(|e| sexpr::find(e, "font"));
        let size_um = font.and_then(|f| sexpr::find(f, "size")).and_then(|s| sexpr::num(s, 1)).map(mm_to_um).unwrap_or(1000);
        let stroke_width = font.and_then(|f| sexpr::find(f, "thickness")).and_then(|t| sexpr::num(t, 1)).map(mm_to_um).unwrap_or(150);
        let (mut justify, mut mirror) = (TextJustify::Center, false);
        if let Some(j) = effects.and_then(|e| sexpr::find(e, "justify")) {
            for tok in j.iter().skip(1).filter_map(Sexpr::text) {
                match tok {
                    "left" => justify = TextJustify::Left,
                    "right" => justify = TextJustify::Right,
                    "mirror" => mirror = true,
                    _ => {}
                }
            }
        }
        texts.push(Text { id: String::new(), content: content.to_string(), at: Point { x: mm_to_um(x), y: mm_to_um(y) }, angle, layer: layer_of(item), size_um, stroke_width, justify, mirror });
        refs.texts.push(ItemRef::of(item));
    }

    (shapes, texts)
}

/// An item's `(stroke (width ..))`, or the `(width ..)` of the files before it (KiCad 6 and older), micrometres.
fn stroke_width_of(item: &[Sexpr]) -> i64 {
    sexpr::find(item, "stroke").and_then(|s| sexpr::find(s, "width")).or_else(|| sexpr::find(item, "width")).and_then(|w| sexpr::num(w, 1)).map(mm_to_um).unwrap_or(0)
}

/// The arc of a file written before `LEGACY_ARC_FORMATTING` (20210925), `(start CENTER) (end ARC_START) (angle A)` in degrees, as the
/// three points of the arc (`parsePCB_SHAPE`: `SetCenter`, `SetStart`, `SetArcAngleAndEnd( A )` -- the end is the start turned by A, the
/// middle by half of it, the same way, in the file's own axes). Returns (start, mid, end).
fn legacy_arc(center: Point, start: Point, angle_deg: f64) -> (Point, Point, Point) {
    let turn = |deg: f64| -> Point {
        let (s, c) = deg.to_radians().sin_cos();
        let (dx, dy) = ((start.x - center.x) as f64, (start.y - center.y) as f64);
        Point { x: center.x + (dx * c - dy * s).round() as i64, y: center.y + (dx * s + dy * c).round() as i64 }
    };
    (start, turn(angle_deg / 2.0), turn(angle_deg))
}

/// An item's `(layer ..)`.
fn layer_of(item: &[Sexpr]) -> String {
    sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("Cmts.User").to_string()
}

/// `(fill yes|solid)`.
fn filled_of(item: &[Sexpr]) -> bool {
    sexpr::find(item, "fill").and_then(|f| sexpr::txt(f, 1)).is_some_and(|s| s == "yes" || s == "solid")
}

/// One `PCB_SHAPE` item (`gr_line`, `gr_arc`, `gr_rect`, `gr_circle`, `gr_poly`, `gr_curve`, or the footprint-level `fp_` form of
/// each: `tag` is the board-level name) as a [`Shape`]; every point goes through `at`, which takes a footprint's local frame to the
/// board's. `None` when the item lacks what it needs.
fn graphic_shape(item: &[Sexpr], tag: &str, at: impl Fn(Point) -> Point) -> Option<Shape> {
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
        // `(gr_curve (pts (xy start) (xy c1) (xy c2) (xy end)))`: a cubic Bezier (`SHAPE_T::BEZIER`), never filled.
        "gr_curve" => {
            let pts: Vec<Point> = poly_points(item)?.into_iter().map(&at).collect();
            let [start, c1, c2, end] = pts[..] else { return None };
            Shape::Bezier { id: String::new(), layer, stroke_width, filled: false, start, c1, c2, end }
        }
        _ => return None,
    })
}

// ---------------------------------------------------------------- outline

fn is_edge_cuts(item: &[Sexpr]) -> bool {
    sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)) == Some("Edge.Cuts")
}

/// A footprint's own Edge.Cuts graphics (`fp_line`, `fp_arc`, `fp_rect`, `fp_circle`, `fp_poly`, `fp_curve`) as board-level shapes.
/// `BuildBoardPolygonOutlines` collects every Edge.Cuts shape of the board, a footprint's included -- a card-edge connector's tab, a
/// connector's mounting slot -- and chains them with the board's own, so they are board-level shapes here. A footprint's items are in its own
/// frame: board = position + RotatePoint( local, orientation ). A rectangle that the footprint's turn leaves tilted is a polygon.
fn footprint_edge_cuts(root: &[Sexpr], shapes: &mut Vec<Shape>, refs: &mut Refs) {
    for fp in sexpr::find_all(root, "footprint").chain(sexpr::find_all(root, "module")) {
        let Some(at) = sexpr::find(fp, "at") else { continue };
        let (Some(fx), Some(fy)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
        let angle = sexpr::num(at, 3).unwrap_or(0.0);
        let (sin, cos) = angle.to_radians().sin_cos();
        let (ox, oy) = (mm_to_um(fx), mm_to_um(fy));
        let to_board = |p: Point| -> Point {
            let (x, y) = (p.x as f64, p.y as f64);
            Point { x: ox + (x * cos + y * sin).round() as i64, y: oy + (-x * sin + y * cos).round() as i64 }
        };
        let square_turn = angle.rem_euclid(90.0) == 0.0;
        for tag in ["fp_line", "fp_arc", "fp_rect", "fp_circle", "fp_poly", "fp_curve"] {
            for item in sexpr::find_all(fp, tag).filter(|it| is_edge_cuts(it)) {
                let board_tag = tag.replace("fp_", "gr_");
                let Some(shape) = graphic_shape(item, &board_tag, to_board) else { continue };
                let shape = match shape {
                    Shape::Rect { id, layer, stroke_width, filled, start, end } if !square_turn => {
                        // Turn the corners one by one: `start` and `end` of a tilted rectangle are not opposite corners of a rectangle.
                        let (a, b) = (sexpr::find(item, "start").and_then(xy_point).unwrap_or(start), sexpr::find(item, "end").and_then(xy_point).unwrap_or(end));
                        let pts = edge::rect_corners(a, b).map(to_board).to_vec();
                        Shape::Polygon { id, layer, stroke_width, filled, pts }
                    }
                    other => other,
                };
                shapes.push(shape);
                refs.shapes.push(ItemRef::of(item));
            }
        }
    }
}

/// The board outline, from the Edge.Cuts shapes in `shapes` (the board's and, promoted, its footprints').
///
/// KiCad builds the board outline from these (`BOARD::GetBoardPolygonOutlines`, ported in `eda_drc::outline`): lines, arcs, circles, rectangles,
/// polygons and curves chained end to end into closed contours, a contour inside another being a cutout. The common board is lines that chain
/// into one closed loop, and that loop is `placement.outline`, as it always was; its shapes are dropped here, the writer emits the loop as lines.
/// Any other board keeps every item as the shape it is -- an arc is an arc, not the chord of one -- and `placement.outline` is the summary of
/// what they make. An outline that does not chain is kept as it is, too, and reported ([`ImportNotes::outline_errors`]): the exported file then
/// has the same gap, and kicad-cli says so, instead of a loop closed behind the user's back.
///
/// Returns the polygon for `placement.outline` and whether the shapes are the outline.
fn import_outline(root: &[Sexpr], shapes: &mut Vec<Shape>, refs: &mut Refs, notes: &mut ImportNotes) -> (Vec<Point>, bool) {
    footprint_edge_cuts(root, shapes, refs);
    let items: Vec<Shape> = shapes.iter().filter(|s| edge::is_edge_cuts(s)).cloned().collect();
    if items.is_empty() {
        notes.outline_source = "none";
        return (Vec::new(), false);
    }

    let (polys, chained, _) = eda_drc::outline::convert_outline_to_polygon(&items, edge::MAX_ERROR_UM as f64, edge::CHAINING_EPSILON_UM, true);
    let one_loop = chained && polys.outline_count() == 1 && !polys.has_holes();
    if one_loop && items.iter().all(|s| matches!(s, Shape::Segment { .. })) {
        // The loop, in the order its sides chain; the sides themselves are not kept.
        let ring: Vec<Point> = polys.outline(0).iter().map(|p| Point { x: p.x, y: p.y }).collect();
        let keep: Vec<bool> = shapes.iter().map(|s| !edge::is_edge_cuts(s)).collect();
        let mut k = keep.iter();
        shapes.retain(|_| *k.next().expect("one flag per shape"));
        let mut k = keep.iter();
        refs.shapes.retain(|_| *k.next().expect("one flag per shape"));
        notes.outline_source = "lines";
        return (ring, false);
    }

    let built = eda_drc::outline::board_outline_of(&items, true);
    notes.outline_source = "shapes";
    notes.outline_open = !built.valid;
    notes.outline_shapes = items.len();
    notes.outline_errors = eda_drc::outline::check_edge_cuts(&items).iter().map(|e| format!("{} at ({:.3}, {:.3}) mm", e.describe(), e.at.x as f64 / 1000.0, e.at.y as f64 / 1000.0)).collect();
    (built.main_ring(), true)
}

// ---------------------------------------------------------------- zones

/// `PCB_IO_KICAD_SEXPR_PARSER::parseZONE`, for board-level `(zone ...)`s:
/// copper pours, teardrops (`(attr (teardrop ...))`) and rule areas
/// (`(keepout ...)`/`(placement ...)`). This IR's `Zone` is single-layer
/// and single-outline, so a zone on several layers (`(layers ...)`) or
/// with several `(polygon ...)` outlines becomes one `Zone` per
/// (layer, outline). The stored `(filled_polygon ...)`s are not read --
/// fills are always derived (`eda_zone_filler`), which is also what
/// kicad-cli's own `pcb drc --refill-zones` run does. Footprint-level
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
            zone.net = match sexpr::num(n, 1) {
                Some(code) => net_names.get(&(code as i64)).cloned().unwrap_or_default(),
                None => sexpr::txt(n, 1).unwrap_or("").to_string(),
            };
        }
        if zone.net.is_empty() {
            if let Some(n) = sexpr::find(z, "net_name") {
                zone.net = sexpr::txt(n, 1).unwrap_or("").to_string();
            }
        }
        let mut layers: Vec<String> = Vec::new();
        if let Some(l) = sexpr::find(z, "layer") {
            layers.extend(sexpr::txt(l, 1).map(str::to_string));
        }
        if let Some(l) = sexpr::find(z, "layers") {
            for i in 1..l.len() {
                match sexpr::txt(l, i) {
                    Some("*.Cu") => layers.extend(copper_layers.iter().cloned()),
                    Some("F&B.Cu") => layers.extend(["F.Cu".to_string(), "B.Cu".to_string()]),
                    Some(name) => layers.push(name.to_string()),
                    None => {}
                }
            }
        }
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
                zone.clearance = mm_to_um(c);
            }
        }
        if let Some(t) = sexpr::find(z, "min_thickness").and_then(|t| sexpr::num(t, 1)) {
            zone.min_thickness = mm_to_um(t);
        }
        if let Some(f) = sexpr::find(z, "fill") {
            if let Some(m) = sexpr::find(f, "mode") {
                if sexpr::txt(m, 1) == Some("hatch") {
                    zone.fill_mode = FillMode::HatchPattern;
                }
            }
            let um = |k: &str| sexpr::find(f, k).and_then(|v| sexpr::num(v, 1)).map(mm_to_um);
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
            // `area * pcbIUScale.IU_PER_MM` after `parseBoardUnits`: mm^2 in the file.
            if let Some(v) = raw("island_area_min") {
                zone.min_island_area = (v * 1e6).round() as i64;
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
                zone.corner_radius = radius.unwrap_or(0);
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
        if sexpr::find(z, "placement").is_some() {
            zone.is_rule_area = true;
        }
        if let Some(a) = sexpr::find(z, "attr") {
            zone.teardrop = sexpr::find(a, "teardrop").is_some();
        }

        // Only copper layers make a copper zone (`GetLayerSet() & boardCopperLayers`).
        layers.retain(|l| copper_layers.contains(l));
        layers.dedup();
        let outlines: Vec<Vec<Point>> = sexpr::find_all(z, "polygon").filter_map(poly_points).collect();
        if outlines.is_empty() || layers.is_empty() {
            notes.zones_skipped += 1;
            continue;
        }
        let item_ref = ItemRef::of(z);
        for layer in &layers {
            for outline in &outlines {
                out.push(Zone { layer: layer.clone(), outline: outline.clone(), ..zone.clone() });
                refs.zones.push(item_ref.clone());
            }
        }
    }
    out
}

/// Points of a `gr_poly`'s `(pts ...)` list: `xy` entries verbatim, `arc`
/// sub-entries by their start/end (curvature dropped, as elsewhere).
fn poly_points(p: &[Sexpr]) -> Option<Vec<Point>> {
    let pts = sexpr::find(p, "pts")?;
    let mut out = Vec::new();
    for item in &pts[1..] {
        let Some(l) = item.as_list() else { continue };
        match sexpr::tag(l) {
            Some("xy") => {
                if let Some(pt) = xy_point(l) {
                    out.push(pt);
                }
            }
            Some("arc") => {
                if let Some(s) = sexpr::find(l, "start").and_then(xy_point) {
                    out.push(s);
                }
                if let Some(e) = sexpr::find(l, "end").and_then(xy_point) {
                    out.push(e);
                }
            }
            _ => {}
        }
    }
    (out.len() >= 3).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(pad.at, (598, 0), "importer must undo the file's own back-side mirror, not pass x through as-is");

        let fp = FootprintInstance { id: "R34".into(), at: Point { x: 114_800, y: 137_800 }, rot: fp_rot, side: Side::Bottom, label: Default::default() };
        let board_pos = eda_model::footprint::to_board(&fp, pad.at);
        assert_eq!(board_pos, Point { x: 114_800, y: 138_398 }, "must match kicad-cli's own reported absolute pad position (114.8, 138.3975mm), not the other pad's position 1.196mm away");
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
        assert_eq!((hw, hh), (2000, 500), "symmetric half-extent must enclose the farthest point on each axis: max(|{{-500,2000}}|)=2000, max(|{{-500,500}}|)=500");

        // Back side: the same raw numbers must be un-mirrored (x negated)
        // before taking the half-extent, exactly like a pad's `at` -- the
        // farthest-x point is still the one that was at file-x=2.0 (now at
        // internal x=-2.0), so the half-extent magnitude is unchanged, but
        // getting the sign backwards here is exactly the bug this is
        // guarding: it would silently still pass this symmetric-extent
        // check while leaving the *centre* wrong for a caller that (unlike
        // this one) cares about which side the shroud is on.
        let (hw_b, hh_b) = footprint_courtyard_half(body, Side::Bottom).expect("courtyard found");
        assert_eq!((hw_b, hh_b), (2000, 500));
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
        let top_pads = vec![Pad { opposite_side: false, number: "1".into(), at: (-598, 0), size: (715, 640), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: Some(0.25) }];
        let bottom_pads = vec![Pad { opposite_side: false, number: "1".into(), at: (598, 0), size: (715, 640), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: Some(0.25) }];

        let lib_id = "fixed_standard:R_0402_1005Metric_Pad0.72x0.64mm_HandSolder";
        let key_top = dedup_footprint_key(&mut explicit, lib_id, &top_pads);
        assert_eq!(key_top, lib_id, "first instance (R35, top) gets the plain lib id");
        explicit.insert(key_top, Footprint { name: lib_id.into(), pads: top_pads.clone(), courtyard: None, courtyard_outlines: vec![], model: None });

        let key_bottom = dedup_footprint_key(&mut explicit, lib_id, &bottom_pads);
        assert_ne!(key_bottom, lib_id, "R34 (bottom)'s own un-mirrored geometry disagrees with R35's cached one -- it must get its own slot, not silently reuse R35's");
        explicit.insert(key_bottom.clone(), Footprint { name: key_bottom.clone(), pads: bottom_pads.clone(), courtyard: None, courtyard_outlines: vec![], model: None });

        // A third instance matching either existing slot exactly must reuse
        // it rather than growing a new one every time.
        assert_eq!(dedup_footprint_key(&mut explicit, lib_id, &top_pads), lib_id);
        assert_eq!(dedup_footprint_key(&mut explicit, lib_id, &bottom_pads), key_bottom);
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
        let start = Point { x: 10_000, y: 0 };
        let mid = Point { x: 7_071, y: 7_071 };
        let end = Point { x: 0, y: 10_000 };
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
        assert_eq!(classes[0].clearance, Some(2400));
        assert_eq!(classes[0].track_width, Some(500));
        assert_eq!(classes[0].via_diameter, Some(1000));
        assert_eq!(classes[0].via_drill, Some(500));
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
        assert_eq!(model.board.clearance, 150, "Default class's 0.15mm clearance, not this crate's own placeholder default ({before})");
        assert_eq!(model.board.track_width, 250);
        assert_eq!(model.board.via_diameter, 600);
        assert_eq!(model.board.via_drill, 300);
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
        assert_eq!(model.board.track_width_min_um, 102, "0.1016mm rounds to 102um");
        assert_eq!(model.board.min_clearance_um, 0);
        assert_eq!(model.board.via_diameter_min_um, 450);
        assert_eq!(model.board.via_drill_min_um, 200);
        assert_eq!(model.board.hole_to_hole_min_um, 250);
        assert_eq!(model.board.hole_clearance_um, 254);
        assert_eq!(model.board.annular_width_min_um, 125);
        assert_eq!(model.board.silk_clearance_um, 150);
        assert_eq!(model.board.min_silk_text_height_um, 800);
        assert_eq!(model.board.min_silk_text_thickness_um, 120);
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
        assert_eq!(model.board.track_width_min_um, 150);
        assert_eq!(model.board.min_clearance_um, 100);
        assert_eq!(model.board.via_diameter_min_um, 400);
        assert_eq!(model.board.via_drill_min_um, 250, "falls back to the legacy via_min_drill token when through_hole_min is absent");
        assert_eq!(model.board.hole_to_hole_min_um, 200);
        assert_eq!(model.board.annular_width_min_um, 90);
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
        assert_eq!(model.board.track_width, 250);
        assert_eq!(model.board.clearance, 200);
        assert_eq!(model.parts.len(), 1);
        assert_eq!(model.parts[0].reference, "R1");
        assert_eq!(model.footprints.len(), 1);
        assert_eq!(model.footprints[0].pads.len(), 2);
        assert_eq!(model.nets.len(), 1);
        assert_eq!(model.nets[0].pins, vec!["R1.1".to_string(), "R1.2".to_string()]);
        let pl = design.placement.unwrap();
        assert_eq!(pl.footprints.len(), 1);
        assert_eq!(pl.footprints[0].at, Point { x: 10_000, y: 10_000 });
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
        assert_eq!(*stroke_width, 150);
        assert_eq!(*start, Point { x: 1000, y: 1000 });
        assert_eq!(*end, Point { x: 5000, y: 1000 });

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
        assert_eq!(t.at, Point { x: 3000, y: 3000 });
        assert_eq!(t.angle, 90_000);
        assert_eq!(t.layer, "F.SilkS");
        assert_eq!(t.size_um, 1000);
        assert_eq!(t.stroke_width, 150);
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
        assert_eq!(*stroke_width, 150);
        assert_eq!((*start, *c1, *c2, *end), (Point { x: 1000, y: 1000 }, Point { x: 1000, y: 3000 }, Point { x: 4000, y: 3000 }, Point { x: 4000, y: 1000 }));
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
        assert_eq!((m.expansion_um, m.min_width_um), (50, 100));
        assert!(m.allow_bridges_in_footprints);
        assert!(m.tent_vias_front && !m.tent_vias_back);

        let (_, legacy, _) = import_kicad_pcb(&text("(tenting front)")).expect("parses");
        assert!(legacy.board.solder_mask.tent_vias_front && !legacy.board.solder_mask.tent_vias_back);

        // No (tenting ..) at all: KiCad's factory default tents both sides.
        let (_, none, _) = import_kicad_pcb(r#"(kicad_pcb (version 20241229) (layers (0 "F.Cu" signal) (31 "B.Cu" signal)))"#).expect("parses");
        assert!(none.board.solder_mask.is_default());

        let (_, mut with_pro, _) = import_kicad_pcb(&text("(tenting (front yes) (back yes))")).expect("parses");
        merge_project_design_rules(&mut with_pro, r#"{"board":{"design_settings":{"rules":{"solder_mask_to_copper_clearance":0.07}}}}"#);
        assert_eq!(with_pro.board.solder_mask.to_copper_clearance_um, 70);
    }

    /// Per-pad layer sets and mask margins, footprint margin / allow-bridges /
    /// net-tie groups, via tenting, footprint silk+mask graphics (in board
    /// space) and mask-only aperture pads.
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
        let (design, _model, notes) = import_kicad_pcb(text).expect("parses");
        // The mask-only pad is not copper: counted as skipped, kept as an aperture.
        assert_eq!(notes.non_copper_pads_skipped, 1);
        let dr = design.drawings.expect("drawings");
        let fp = dr.footprint_extras.iter().find(|e| e.id == "U1").expect("extra for U1");
        assert_eq!(fp.solder_mask_margin, Some(30));
        assert!(fp.allow_soldermask_bridges);
        assert_eq!(fp.net_tie_pad_groups, vec!["1, 2".to_string()]);
        assert_eq!(fp.pads.len(), 2);
        assert_eq!(fp.pads[0].solder_mask_margin, Some(20));
        assert_eq!(fp.pads[0].layers, vec!["F.Cu".to_string(), "F.Mask".to_string(), "F.SilkS".to_string()]);
        assert_eq!(fp.pads[1].layers, vec!["B.Cu".to_string(), "B.Mask".to_string(), "F.Cu".to_string(), "F.Mask".to_string()]);
        assert_eq!(fp.pads[1].solder_mask_margin, None);

        // Silk line and mask rect only (the F.Fab line is not kept), rotated 90 deg
        // (file angle 90 = counter-clockwise) about (10, 20): local (-1, 0) -> (10, 21).
        assert_eq!(fp.graphics.iter().filter(|g| g.pad_number.is_none()).count(), 2);
        let silk = fp.graphics.iter().find(|g| g.shape.layer() == "F.SilkS").expect("silk line");
        let pts = silk.shape.points();
        assert_eq!(pts, vec![Point { x: 10_000, y: 21_000 }, Point { x: 10_000, y: 19_000 }]);
        let mask = fp.graphics.iter().find(|g| g.shape.layer() == "F.Mask" && g.pad_number.is_none()).expect("mask rect");
        assert!(matches!(&mask.shape, Shape::Polygon { filled: true, pts, .. } if pts.len() == 4));
        let aperture = fp.graphics.iter().find(|g| g.pad_number.as_deref() == Some("3")).expect("aperture pad");
        assert_eq!(aperture.pin_type, "free");
        assert_eq!(aperture.shape.layer(), "F.Mask");

        // Via tenting: an explicit `no` is kept, `none` inherits (no entry).
        assert_eq!(dr.via_tenting.len(), 1);
        assert_eq!((dr.via_tenting[0].at, dr.via_tenting[0].front, dr.via_tenting[0].back), (Point { x: 5000, y: 5000 }, Some(false), None));
    }

    /// Visible footprint text and `gr_text` on silk: variables resolved,
    /// `\n` a newline, hidden fields dropped, back-side anchor un-mirrored.
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
        let dr = design.drawings.expect("drawings");
        let fp = &dr.footprint_extras[0];
        assert_eq!(fp.texts.len(), 1, "{:?}", fp.texts);
        let t = &fp.texts[0];
        assert_eq!((t.text.as_str(), t.layer.as_str(), t.mirror, t.keep_upright, t.angle_file_mdeg), ("~{CLK}", "B.SilkS", true, true, 180_000));
        // Back footprint: the file's local x is already mirrored, so +3.7 lands to the right.
        assert_eq!(t.at, Point { x: 13_700, y: 10_000 });
        assert_eq!(dr.silk_texts.len(), 1);
        let g = &dr.silk_texts[0];
        assert_eq!(g.text, "REV v2\nline2");
        assert_eq!((g.size, g.thickness, g.halign, g.valign, g.keep_upright, g.bold), ((1500, 1200), 200, -1, 1, false, true));
        assert_eq!(g.angle_file_mdeg, 90_000);
    }
}

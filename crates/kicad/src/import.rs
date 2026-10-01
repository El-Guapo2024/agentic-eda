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
//! - **Zones/pours**: counted in [`ImportNotes::zones_skipped`], not
//!   imported. Our `Zone` has no home for KiCad's fill/thermal-relief
//!   settings, and a zone's real content is its *filled* copper, which we
//!   do not compute.
//! - **Track/board-edge arcs**: KiCad's `(arc ...)`/`(gr_arc ...)` have no
//!   analogue in our polyline-only `Track`/`outline`; both are tessellated
//!   into short straight segments (see [`tessellate_arc`]), counted in
//!   [`ImportNotes::track_arcs_approximated`]. Exact at the sampled points,
//!   not bit-identical on re-export. A board-level `gr_arc` *not* on
//!   Edge.Cuts becomes a [`eda_model::ir::Shape::Arc`] instead, exactly
//!   (KiCad's own three-point arc storage is our `Arc`'s storage too).
//! - **Non-rect/roundrect/circle/oval pads** (trapezoid, custom): mapped to
//!   `PadShape::Rect` at the pad's nominal `size`, counted in
//!   [`ImportNotes::non_rect_pad_shapes_approximated`].
//! - **Board-level graphics and text** (`gr_line`/`gr_rect`/`gr_circle`/
//!   `gr_poly`/`gr_arc`/`gr_text`, any layer other than Edge.Cuts, which
//!   stays outline-only as before): imported into `design.drawings` as
//!   [`eda_model::ir::Shape`]/[`eda_model::ir::Text`] (see
//!   [`import_drawings`]).
//! - **3D models, stackup dielectric/material/thickness, net ties,
//!   group/generator objects, footprint-local graphics/text (`fp_line`,
//!   `fp_text` other than Reference/Value)**: not imported at all.

use std::collections::{BTreeMap, HashMap};

use eda_model::ir::{Design, DrawingsSection, FootprintInstance, Point, PlacementSection, Provenance, RoutingSection, Shape, Side, Text, TextJustify, Track, Via};
use eda_model::{BoardRules, CheckResult, ConstraintModel, Footprint, Net, NetClass, Pad, PadKind, PadShape, Part, Pin, PinKind};

use crate::sexpr::{self, Sexpr};

/// Counts of things the importer saw but could not carry into our model
/// exactly, or at all -- meant to be printed, never silently absorbed.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportNotes {
    pub zones_skipped: usize,
    pub track_arcs_approximated: usize,
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
    /// How the board outline was reconstructed: "poly" (one closed
    /// `gr_poly`), "circle" (one `gr_circle`), "lines" (chained
    /// `gr_line`/`gr_rect`/`gr_arc` edges), or "none" (nothing found).
    #[serde(default)]
    pub outline_source: &'static str,
    /// The chained edges did not close into a loop (bad/partial board, or
    /// an outline shape this importer's chaining does not follow).
    #[serde(default)]
    pub outline_open: bool,
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

    let (footprints_ir, parts, explicit_footprints, pin_nets) = import_footprints(root, &mut notes)?;
    let nets = build_nets(&net_names, &pin_nets);
    let outline = import_outline(root, &mut notes);
    let (tracks, vias) = import_routing(root, &net_names, &mut notes);
    let (shapes, texts) = import_drawings(root);
    notes.zones_skipped = sexpr::find_all(root, "zone").count();

    // The outline override lives on `board` too (used when a downstream
    // tool re-derives placement); keep it in step with what we actually
    // found on Edge.Cuts.
    if outline.len() >= 3 {
        board.outline = Some(outline.clone());
    }
    // `eda_drc::providers::outline`'s `invalid_outline` check (task item 5)
    // reads this back at DRC time -- see `BoardRules::outline_closed`'s own
    // doc comment for why it lives here rather than on `PlacementSection`.
    // A `gr_poly`/`gr_circle`-derived outline is inherently one closed
    // loop; a chained-edges one carries `chain_edges`'s own verdict
    // (`ImportNotes::outline_open`, negated); no Edge.Cuts graphics at all
    // leaves this `None` (nothing to assert either way).
    board.outline_closed = match notes.outline_source {
        "poly" | "circle" => Some(true),
        "lines" => Some(!notes.outline_open),
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
        routing: if tracks.is_empty() && vias.is_empty() { None } else { Some(RoutingSection { tracks, vias, zones: vec![], track_width_presets: vec![], via_presets: vec![] }) },
        drawings: if shapes.is_empty() && texts.is_empty() { None } else { Some(DrawingsSection { shapes, texts }) },
        footprint_library: None, sheet_contents: None, bus_aliases: vec![],
    };
    // Every track/via this parse just built, and every shape/text, has no
    // id yet (the file does not carry ours) -- assign the same
    // deterministic ids a fresh route or a hand-add would get, so an
    // imported board is addressable from the moment it lands.
    design.assign_missing_ids();
    let model = ConstraintModel {
        parts,
        nets,
        clusters: vec![],
        placement_rules: vec![],
        stackup: None,
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

fn build_nets(net_names: &BTreeMap<i64, String>, pin_nets: &[(String, String)]) -> Vec<Net> {
    let mut pins_by_name: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (pin, net) in pin_nets {
        pins_by_name.entry(net.as_str()).or_default().push(pin.clone());
    }
    net_names
        .values()
        .filter(|n| !n.is_empty())
        .map(|n| Net { name: n.clone(), pins: pins_by_name.get(n.as_str()).cloned().unwrap_or_default() })
        .collect()
}

// -------------------------------------------------------------- board rules

/// Track width / clearance / via size, from the board's `"Default"` net
/// class (or its only one) -- see the module doc for why that is where
/// KiCad 6+ actually keeps these, not `(setup ...)`. Any other declared
/// net class becomes one of our `NetClass`es, matched by its literal
/// member list rather than a glob (an exact list is what KiCad wrote).
fn import_board_rules(root: &[Sexpr], layers: &[String]) -> BoardRules {
    let mut board = BoardRules { layers: layers.to_vec(), ..BoardRules::default() };
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
            Some(NetClass { name, nets, track_width, clearance, via_diameter, via_drill, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 })
        })
        .collect();
    import_legacy_setup_minimums(root, &mut board);
    board
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
    let Some(rules) = root.get("board").and_then(|b| b.get("design_settings")).and_then(|d| d.get("rules")) else {
        return;
    };
    let mm = |key: &str| rules.get(key).and_then(|v| v.as_f64()).map(mm_to_um);
    let b = &mut model.board;
    if let Some(v) = mm("min_track_width") {
        b.track_width_min_um = v;
    }
    if let Some(v) = mm("min_clearance") {
        b.min_clearance_um = v;
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
    if let Some(layers) = sexpr::find(pad, "layers") {
        let names: Vec<&str> = layers.iter().skip(1).filter_map(Sexpr::text).collect();
        if !names.is_empty() && !names.iter().any(|l| *l == "*.Cu" || l.ends_with(".Cu")) {
            notes.non_copper_pads_skipped += 1;
            return None;
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

    Some(Pad { number, at: (mm_to_um(px), mm_to_um(py)), size: (mm_to_um(w), mm_to_um(h)), shape: pad_shape, kind: pad_kind, drill, drill_slot, rot, roundrect_ratio })
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
    notes: &mut ImportNotes,
) -> Result<(Vec<FootprintInstance>, Vec<Part>, BTreeMap<String, Footprint>, Vec<(String, String)>), Vec<CheckResult>> {
    let mut footprints_ir = Vec::new();
    let mut parts = Vec::new();
    let mut explicit: BTreeMap<String, Footprint> = BTreeMap::new();
    let mut pin_nets: Vec<(String, String)> = Vec::new();
    let mut errors = Vec::new();

    // `module` is the pre-KiCad-6 tag for the same thing; the pad grammar
    // inside it is unchanged, so the rest of this function does not care
    // which one it was.
    let raw: Vec<&[Sexpr]> = sexpr::find_all(root, "footprint").chain(sexpr::find_all(root, "module")).collect();

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

        let reference = footprint_field(fp, "Reference").filter(|s| !s.is_empty()).unwrap_or_else(|| format!("FP{}", idx + 1));
        let value = footprint_field(fp, "Value");

        let mut pads = Vec::new();
        let mut pins = Vec::new();
        for pad in sexpr::find_all(fp, "pad") {
            let Some(p) = parse_pad_geometry(pad, side, rot, notes) else { continue };

            // A non-plated hole is mechanical, not electrical: it has no
            // net and is not a schematic pin (nothing a symbol would draw
            // a stub for), but it still occupies space in `pads`, so it
            // still counts as a hole for clearance.
            if p.kind != PadKind::NonPlatedHole {
                if let Some(net) = sexpr::find(pad, "net") {
                    if let Some(name) = sexpr::txt(net, 2) {
                        if !name.is_empty() {
                            pin_nets.push((format!("{reference}.{}", p.number), name.to_string()));
                        }
                    }
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
        let key = dedup_footprint_key(&mut explicit, &lib_id, &pads);
        let courtyard = footprint_courtyard_half(fp, side);
        explicit.entry(key.clone()).or_insert_with(|| Footprint { name: key.clone(), pads, courtyard, model: crate::footprint_lib::model_from(fp) });

        parts.push(Part { reference: reference.clone(), mpn: None, lcsc: None, value, package: None, footprint: Some(key), pins, body_um: None, symbol: None, datasheet: None, edge: None });
        footprints_ir.push(FootprintInstance { id: reference, at: Point { x, y }, rot, side, label: Default::default() });
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    Ok((footprints_ir, parts, explicit, pin_nets))
}

// ---------------------------------------------------------------- routing

fn import_routing(root: &[Sexpr], net_names: &BTreeMap<i64, String>, notes: &mut ImportNotes) -> (Vec<Track>, Vec<Via>) {
    let net_of = |code: Option<f64>| -> String { code.map(|c| c as i64).and_then(|c| net_names.get(&c)).cloned().unwrap_or_default() };

    let mut tracks = Vec::new();
    for seg in sexpr::find_all(root, "segment") {
        let (Some(s), Some(e)) = (sexpr::find(seg, "start").and_then(xy_point), sexpr::find(seg, "end").and_then(xy_point)) else { continue };
        let net = net_of(sexpr::find(seg, "net").and_then(|n| sexpr::num(n, 1)));
        if net.is_empty() {
            continue; // KiCad's net 0: not connected to anything our model can name
        }
        let width = sexpr::find(seg, "width").and_then(|w| sexpr::num(w, 1)).map(mm_to_um).unwrap_or(200);
        let layer = sexpr::find(seg, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.Cu").to_string();
        tracks.push(Track { id: String::new(), net, pins: vec![], layer, width, pts: vec![s, e] });
    }

    for arc in sexpr::find_all(root, "arc") {
        let (Some(s), Some(m), Some(e)) =
            (sexpr::find(arc, "start").and_then(xy_point), sexpr::find(arc, "mid").and_then(xy_point), sexpr::find(arc, "end").and_then(xy_point))
        else {
            continue;
        };
        let net = net_of(sexpr::find(arc, "net").and_then(|n| sexpr::num(n, 1)));
        if net.is_empty() {
            continue;
        }
        let width = sexpr::find(arc, "width").and_then(|w| sexpr::num(w, 1)).map(mm_to_um).unwrap_or(200);
        let layer = sexpr::find(arc, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.Cu").to_string();
        notes.track_arcs_approximated += 1;
        tracks.push(Track { id: String::new(), net, pins: vec![], layer, width, pts: tessellate_arc(s, m, e) });
    }

    let mut vias = Vec::new();
    for via in sexpr::find_all(root, "via") {
        let Some(at) = sexpr::find(via, "at").and_then(xy_point) else { continue };
        let net = net_of(sexpr::find(via, "net").and_then(|n| sexpr::num(n, 1)));
        if net.is_empty() {
            continue;
        }
        let dia = sexpr::find(via, "size").and_then(|s| sexpr::num(s, 1)).map(mm_to_um).unwrap_or(600);
        let drill = sexpr::find(via, "drill").and_then(|d| sexpr::num(d, 1)).map(mm_to_um).unwrap_or(300);
        let (from_layer, to_layer) = sexpr::find(via, "layers")
            .map(|l| (sexpr::txt(l, 1).unwrap_or("F.Cu").to_string(), sexpr::txt(l, 2).unwrap_or("B.Cu").to_string()))
            .unwrap_or_else(|| ("F.Cu".into(), "B.Cu".into()));
        vias.push(Via { id: String::new(), net, at, drill, diameter: dia, from_layer, to_layer });
    }

    (tracks, vias)
}

/// Approximate a KiCad track/board-edge arc (given as three points on its
/// circumference: start, mid, end) as a short polyline, since our model
/// has no arc primitive. Exact at the sampled points; the endpoints are
/// kept bit-exact regardless of any trig round-off in between.
///
/// `SEGMENTS` was 8 until this was measured as the root cause of
/// `docs/parity/GAPS.md` #3's `tracks_crossing` false positives (1567 on
/// one real QA board, 100% of them wrong): KiCad's own `tracks_crossing`
/// fast path only ever special-cases two genuine straight `PCB_TRACE_T`
/// segments (`drc_test_provider_copper_clearance.cpp`'s
/// `item->Type() == PCB_TRACE_T && other->Type() == PCB_TRACE_T` gate) --
/// a real arc (`PCB_ARC_T`) never takes it, so KiCad always judges an
/// arc's clearance against its true curve, which its own routing keeps
/// genuinely clear. Our model has no arc primitive to fall back on the
/// same way, so a tessellated arc's piecewise-straight chords are the only
/// shape this port can test -- and at 8 segments, two closely-routed,
/// similarly-curved different-net arcs (common on a round/flex-style
/// board, which is exactly what the offending QA board is: 666 of its
/// ~1500 track primitives are arcs) can have chords that cross even though
/// the true curves never do, purely from each chord's deviation (sagitta)
/// from its arc. Sagitta shrinks with the *square* of the segment count
/// (`r * sweep^2 / (8 * N^2)`).
///
/// `N = 32`, not a smaller value, despite the real cost that comes with it
/// (see below): `N = 16` was tried first as a cheaper-looking compromise
/// (half the extra segments, and the sagitta math alone suggested plenty of
/// margin) and measured directly against the real offending QA board --
/// it cut the false `tracks_crossing` count from 1567 to 1177, **not**
/// to zero. The remaining false positives are concentrated on arcs with a
/// larger radius and/or sweep than the "fillet-scale" case the sagitta
/// estimate above assumed, so the formula's comfortable-looking margin
/// did not hold in practice. `N = 32` was re-measured directly on the
/// same board and does eliminate it completely (0 `tracks_crossing` false
/// positives). The real cost: at `N = 32` this one arc-heavy board's
/// `eda_drc::run` exceeds the parity harness's 60s per-board watchdog
/// (unrelated to custom-rule evaluation, which has its own fix -- see
/// `constraints::CompiledClearanceRules` -- and does not touch this
/// board). Between "one large QA board excluded from the measurement
/// entirely" and "the headline false-positive bug this was ported to fix
/// is only mostly gone," the former is the honest trade: an excluded board
/// contributes neither a false positive nor a true match, while a
/// half-fixed correctness bug is still a correctness bug. See
/// `docs/parity/GAPS.md` #3 and `crates/drc/tests/parity_drc.rs`.
fn tessellate_arc(start: Point, mid: Point, end: Point) -> Vec<Point> {
    const SEGMENTS: usize = 32;
    let (sx, sy) = (start.x as f64, start.y as f64);
    let (mx, my) = (mid.x as f64, mid.y as f64);
    let (ex, ey) = (end.x as f64, end.y as f64);

    // Circumcenter of the three points.
    let d = 2.0 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
    if d.abs() < 1e-6 {
        return vec![start, end]; // collinear (degenerate arc): a straight chord
    }
    let ux = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / d;
    let uy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / d;
    let r = ((sx - ux).powi(2) + (sy - uy).powi(2)).sqrt();

    let ang = |x: f64, y: f64| (y - uy).atan2(x - ux);
    let two_pi = std::f64::consts::TAU;
    let norm = |a: f64| a.rem_euclid(two_pi);
    let (a0, a1, a2) = (ang(sx, sy), ang(mx, my), ang(ex, ey));
    let mut sweep = norm(a2 - a0);
    let mid_sweep = norm(a1 - a0);
    if mid_sweep > sweep {
        // The short way around does not pass through the file's own mid
        // point, so the real arc sweeps the other way.
        sweep -= two_pi;
    }

    let mut pts = Vec::with_capacity(SEGMENTS + 1);
    for i in 0..=SEGMENTS {
        let t = i as f64 / SEGMENTS as f64;
        let a = a0 + sweep * t;
        pts.push(Point { x: (ux + r * a.cos()).round() as i64, y: (uy + r * a.sin()).round() as i64 });
    }
    pts[0] = start;
    *pts.last_mut().expect("SEGMENTS + 1 >= 1") = end;
    pts
}

// --------------------------------------------------------- drawings

/// Board-level graphics (`gr_line`/`gr_arc`/`gr_rect`/`gr_circle`/
/// `gr_poly`) and text (`gr_text`), any layer other than Edge.Cuts -- that
/// one stays dedicated to [`import_outline`], unchanged, so a shape is
/// never represented twice.
fn import_drawings(root: &[Sexpr]) -> (Vec<Shape>, Vec<Text>) {
    let stroke_width = |item: &[Sexpr]| -> i64 {
        sexpr::find(item, "stroke").and_then(|s| sexpr::find(s, "width")).and_then(|w| sexpr::num(w, 1)).map(mm_to_um).unwrap_or(0)
    };
    let layer_of = |item: &[Sexpr]| -> String { sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("Cmts.User").to_string() };
    let filled = |item: &[Sexpr]| -> bool { sexpr::find(item, "fill").and_then(|f| sexpr::txt(f, 1)).is_some_and(|s| s == "yes" || s == "solid") };

    let mut shapes = Vec::new();
    for item in sexpr::find_all(root, "gr_line").filter(|it| !is_edge_cuts(it)) {
        let (Some(start), Some(end)) = (sexpr::find(item, "start").and_then(xy_point), sexpr::find(item, "end").and_then(xy_point)) else { continue };
        shapes.push(Shape::Segment { id: String::new(), layer: layer_of(item), stroke_width: stroke_width(item), filled: filled(item), start, end });
    }
    for item in sexpr::find_all(root, "gr_arc").filter(|it| !is_edge_cuts(it)) {
        let (Some(start), Some(mid), Some(end)) =
            (sexpr::find(item, "start").and_then(xy_point), sexpr::find(item, "mid").and_then(xy_point), sexpr::find(item, "end").and_then(xy_point))
        else {
            continue;
        };
        shapes.push(Shape::Arc { id: String::new(), layer: layer_of(item), stroke_width: stroke_width(item), filled: filled(item), start, mid, end });
    }
    for item in sexpr::find_all(root, "gr_rect").filter(|it| !is_edge_cuts(it)) {
        let (Some(start), Some(end)) = (sexpr::find(item, "start").and_then(xy_point), sexpr::find(item, "end").and_then(xy_point)) else { continue };
        shapes.push(Shape::Rect { id: String::new(), layer: layer_of(item), stroke_width: stroke_width(item), filled: filled(item), start, end });
    }
    for item in sexpr::find_all(root, "gr_circle").filter(|it| !is_edge_cuts(it)) {
        let (Some(center), Some(end)) = (sexpr::find(item, "center").and_then(xy_point), sexpr::find(item, "end").and_then(xy_point)) else { continue };
        shapes.push(Shape::Circle { id: String::new(), layer: layer_of(item), stroke_width: stroke_width(item), filled: filled(item), center, end });
    }
    for item in sexpr::find_all(root, "gr_poly").filter(|it| !is_edge_cuts(it)) {
        let Some(pts) = poly_points(item) else { continue };
        shapes.push(Shape::Polygon { id: String::new(), layer: layer_of(item), stroke_width: stroke_width(item), filled: filled(item), pts });
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
    }

    (shapes, texts)
}

// ---------------------------------------------------------------- outline

fn is_edge_cuts(item: &[Sexpr]) -> bool {
    sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)) == Some("Edge.Cuts")
}

/// Board outline from Edge.Cuts graphics, in one of three shapes real
/// boards use: a single closed polygon, a single circle, or a set of
/// lines/rects/arcs to chain end-to-end into a loop. See [`ImportNotes`]
/// for which one was used (and whether chaining actually closed).
fn import_outline(root: &[Sexpr], notes: &mut ImportNotes) -> Vec<Point> {
    let polys: Vec<&[Sexpr]> = sexpr::find_all(root, "gr_poly").filter(|it| is_edge_cuts(it)).collect();
    if polys.len() == 1 {
        if let Some(pts) = poly_points(polys[0]) {
            notes.outline_source = "poly";
            return pts;
        }
    }

    let circles: Vec<&[Sexpr]> = sexpr::find_all(root, "gr_circle").filter(|it| is_edge_cuts(it)).collect();
    let any_other = sexpr::find_all(root, "gr_line").any(is_edge_cuts)
        || sexpr::find_all(root, "gr_rect").any(is_edge_cuts)
        || sexpr::find_all(root, "gr_arc").any(is_edge_cuts);
    if circles.len() == 1 && !any_other {
        if let Some(pts) = circle_points(circles[0]) {
            notes.outline_source = "circle";
            return pts;
        }
    }

    let mut edges: Vec<(Point, Point)> = Vec::new();
    for l in sexpr::find_all(root, "gr_line").filter(|it| is_edge_cuts(it)) {
        if let (Some(s), Some(e)) = (sexpr::find(l, "start").and_then(xy_point), sexpr::find(l, "end").and_then(xy_point)) {
            edges.push((s, e));
        }
    }
    for r in sexpr::find_all(root, "gr_rect").filter(|it| is_edge_cuts(it)) {
        if let (Some(p0), Some(p1)) = (sexpr::find(r, "start").and_then(xy_point), sexpr::find(r, "end").and_then(xy_point)) {
            let (tl, br) = (Point { x: p0.x, y: p0.y }, Point { x: p1.x, y: p1.y });
            let (tr, bl) = (Point { x: br.x, y: tl.y }, Point { x: tl.x, y: br.y });
            edges.extend([(tl, tr), (tr, br), (br, bl), (bl, tl)]);
        }
    }
    for a in sexpr::find_all(root, "gr_arc").filter(|it| is_edge_cuts(it)) {
        if let (Some(s), Some(e)) = (sexpr::find(a, "start").and_then(xy_point), sexpr::find(a, "end").and_then(xy_point)) {
            notes.track_arcs_approximated += 1;
            edges.push((s, e));
        }
    }

    if edges.is_empty() {
        notes.outline_source = "none";
        return Vec::new();
    }
    notes.outline_source = "lines";
    let (pts, closed) = chain_edges(&edges);
    notes.outline_open = !closed;
    pts
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

fn circle_points(c: &[Sexpr]) -> Option<Vec<Point>> {
    let center = sexpr::find(c, "center").and_then(xy_point)?;
    let end = sexpr::find(c, "end").and_then(xy_point)?;
    let r = (((end.x - center.x).pow(2) + (end.y - center.y).pow(2)) as f64).sqrt();
    const N: usize = 48;
    Some(
        (0..N)
            .map(|i| {
                let a = i as f64 / N as f64 * std::f64::consts::TAU;
                Point { x: center.x + (r * a.cos()).round() as i64, y: center.y + (r * a.sin()).round() as i64 }
            })
            .collect(),
    )
}

/// Walk a bag of undirected edges into the longest closed loop they form
/// (a board with an internal cutout has more than one; the outer boundary
/// is the one this heuristic keeps). Tries every edge as a chain start,
/// which is quadratic in edge count -- fine for the dozens of segments a
/// real board outline has.
fn chain_edges(edges: &[(Point, Point)]) -> (Vec<Point>, bool) {
    let mut by_point: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, (a, b)) in edges.iter().enumerate() {
        by_point.entry((a.x, a.y)).or_default().push(i);
        by_point.entry((b.x, b.y)).or_default().push(i);
    }

    let mut best: Vec<Point> = Vec::new();
    let mut best_closed = false;
    for start_idx in 0..edges.len() {
        let mut used = vec![false; edges.len()];
        used[start_idx] = true;
        let (a0, b0) = edges[start_idx];
        let mut chain = vec![a0, b0];
        let mut cur = b0;
        let closed = loop {
            if cur == a0 && chain.len() > 2 {
                break true;
            }
            let Some(candidates) = by_point.get(&(cur.x, cur.y)) else { break false };
            let Some(&ei) = candidates.iter().find(|&&i| !used[i]) else { break false };
            used[ei] = true;
            let (a, b) = edges[ei];
            cur = if a == cur { b } else { a };
            chain.push(cur);
        };
        let pts = if closed { chain[..chain.len() - 1].to_vec() } else { chain };
        if pts.len() > best.len() {
            best = pts;
            best_closed = closed;
        }
    }
    (best, best_closed)
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

    #[test]
    fn chains_a_rectangle_regardless_of_edge_order() {
        let a = Point { x: 0, y: 0 };
        let b = Point { x: 10, y: 0 };
        let c = Point { x: 10, y: 10 };
        let d = Point { x: 0, y: 10 };
        // Deliberately out of geometric order and reversed on one edge.
        let edges = vec![(c, d), (a, b), (d, a), (c, b)];
        let (pts, closed) = chain_edges(&edges);
        assert!(closed);
        assert_eq!(pts.len(), 4);
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
        let top_pads = vec![Pad { number: "1".into(), at: (-598, 0), size: (715, 640), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: Some(0.25) }];
        let bottom_pads = vec![Pad { number: "1".into(), at: (598, 0), size: (715, 640), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: Some(0.25) }];

        let lib_id = "fixed_standard:R_0402_1005Metric_Pad0.72x0.64mm_HandSolder";
        let key_top = dedup_footprint_key(&mut explicit, lib_id, &top_pads);
        assert_eq!(key_top, lib_id, "first instance (R35, top) gets the plain lib id");
        explicit.insert(key_top, Footprint { name: lib_id.into(), pads: top_pads.clone(), courtyard: None, model: None });

        let key_bottom = dedup_footprint_key(&mut explicit, lib_id, &bottom_pads);
        assert_ne!(key_bottom, lib_id, "R34 (bottom)'s own un-mirrored geometry disagrees with R35's cached one -- it must get its own slot, not silently reuse R35's");
        explicit.insert(key_bottom.clone(), Footprint { name: key_bottom.clone(), pads: bottom_pads.clone(), courtyard: None, model: None });

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
        let pts = tessellate_arc(start, mid, end);
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
}

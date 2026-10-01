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
use eda_model::ir::{Design, NetLabel, NoConnect, PowerSymbol, SchematicText, SymbolInstance, Wire};
use eda_model::{CheckResult, ConstraintModel, Part, PinKind};

mod pcb;
pub use pcb::{export_kicad_pcb, export_kicad_pro};

mod sexpr;
mod import;
pub use import::{import_kicad_pcb, merge_project_net_classes, mm_to_um, parse_project_net_classes, ImportNotes};

mod footprint_lib;
pub use footprint_lib::{default_footprint_library_root, export_kicad_mod, find_footprint_file, parse_footprint_file, resolve_library_footprints, LIBRARY_ROOT_ENV};

mod symbol_lib;
pub use symbol_lib::{default_symbol_library_root, find_symbol_library_file, list_symbol_libraries, list_symbols_in_library, resolve_library_symbols, resolve_symbol, SYMBOL_LIBRARY_ROOT_ENV};

mod erc_style;

mod erc;
pub use erc::{check_erc, check_erc_excluding, Exclusions};

mod sch_import;
pub use sch_import::{import_kicad_sch, pin_kind_from_electrical_type, reconcile, transform_local_point};

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

    let mut power_symbols: Vec<&PowerSymbol> = sch.power_symbols.iter().collect();
    power_symbols.sort_by(|a, b| a.id.cmp(&b.id));

    let mut no_connects: Vec<&NoConnect> = sch.no_connects.iter().collect();
    no_connects.sort_by(|a, b| a.at.cmp(&b.at));

    let mut texts: Vec<&SchematicText> = sch.texts.iter().collect();
    texts.sort_by(|a, b| (&a.content, a.at).cmp(&(&b.content, b.at)));

    // ---- resolve every distinct lib_id used, once, into a `lib_symbols`-block entry ----
    let mut lib_ids: Vec<String> = symbols.iter().map(|s| sym_lib_id(s)).collect();
    lib_ids.extend(power_symbols.iter().map(|p| p.lib_id.clone()));
    lib_ids.sort();
    lib_ids.dedup();

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
    let tb = sch.title_block.as_ref();
    let title = tb.map(|t| t.title.as_str()).filter(|s| !s.is_empty()).unwrap_or(meta.title);
    let date = tb.map(|t| t.date.as_str()).filter(|s| !s.is_empty()).unwrap_or(meta.date);
    writeln!(out, "\t\t(title {})", sexpr_str(title)).unwrap();
    writeln!(out, "\t\t(date {})", sexpr_str(date)).unwrap();
    if let Some(t) = tb.filter(|t| !t.rev.is_empty()) {
        writeln!(out, "\t\t(rev {})", sexpr_str(&t.rev)).unwrap();
    }
    if let Some(t) = tb.filter(|t| !t.company.is_empty()) {
        writeln!(out, "\t\t(company {})", sexpr_str(&t.company)).unwrap();
    }
    if let Some(t) = tb.filter(|t| !t.comments.is_empty()) {
        for (i, c) in t.comments.iter().enumerate() {
            writeln!(out, "\t\t(comment {} {})", i + 1, sexpr_str(c)).unwrap();
        }
    } else {
        writeln!(out, "\t\t(comment 1 {})", sexpr_str(&format!("engine_version: {}", design.provenance.engine_version))).unwrap();
        writeln!(out, "\t\t(comment 2 {})", sexpr_str(&format!("intent_hash: {}", design.provenance.intent_hash))).unwrap();
        writeln!(out, "\t\t(comment 3 {})", sexpr_str(&format!("seed: {}", design.provenance.seed))).unwrap();
    }
    writeln!(out, "\t)").unwrap();

    // ---- lib_symbols ----
    writeln!(out, "\t(lib_symbols").unwrap();
    for lib_id in &lib_ids {
        if let Some((part, sym)) = symbols.iter().find_map(|s| (sym_lib_id(s) == *lib_id).then(|| (parts_by_ref[s.id.as_str()], *s))) {
            write_regular_lib_symbol(&mut out, lib_id, sym, part, model);
        } else {
            // A power symbol (never a bare part): `model.symbol_of` always
            // resolves it, real library first, then `symbol::builtin`'s
            // generic rail fallback for any net name.
            let resolved = model.symbol_of(lib_id).unwrap_or_else(|| eda_model::symbol::builtin(lib_id).expect("power lib_id always resolves"));
            write_power_lib_symbol(&mut out, lib_id, &resolved);
        }
    }
    writeln!(out, "\t)").unwrap();

    // ---- symbol instances ----
    // KiCad 9 nests the sheet-path -> reference mapping *inside* each
    // symbol (an `instances` block), not in a separate top-level
    // `symbol_instances` list — that older shape parses as an unknown
    // token here and kicad-cli refuses to load the file.
    for sym in &symbols {
        let part = parts_by_ref[sym.id.as_str()];
        let lib_id = sym_lib_id(sym);
        let resolved = if eda_model::is_synthetic_lib_id(&lib_id) { None } else { model.symbol_of(&lib_id) };
        let x = mm(sym.at.x);
        let y = mm(sym.at.y);
        let uuid = duid(&format!("sym:{}", sym.id));
        writeln!(out, "\t(symbol (lib_id {}) (at {x} {y} 0) (unit 1)", sexpr_str(&lib_id)).unwrap();
        writeln!(out, "\t\t(exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no)").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        let value = if !sym.value.is_empty() { sym.value.as_str() } else { part.value.as_deref().unwrap_or(&sym.id) };
        let footprint = if !sym.footprint.is_empty() { sym.footprint.as_str() } else { part.footprint.as_deref().unwrap_or("") };
        let datasheet = if !sym.datasheet.is_empty() {
            sym.datasheet.as_str()
        } else {
            resolved.as_ref().map(|s| s.datasheet.as_str()).filter(|s| !s.is_empty()).unwrap_or("")
        };
        write_property(&mut out, "Reference", &sym.id, 0.0, -2.0, false);
        write_property(&mut out, "Value", value, 0.0, 2.0, false);
        write_property(&mut out, "Footprint", footprint, 0.0, 4.0, true);
        write_property(&mut out, "Datasheet", datasheet, 0.0, 6.0, true);
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

    // ---- power symbol instances ----
    for ps in &power_symbols {
        let x = mm(ps.at.x);
        let y = mm(ps.at.y);
        let uuid = duid(&format!("pwr:{}", ps.id));
        writeln!(out, "\t(symbol (lib_id {}) (at {x} {y} 0) (unit 1)", sexpr_str(&ps.lib_id)).unwrap();
        writeln!(out, "\t\t(exclude_from_sim no) (in_bom no) (on_board no) (dnp no)").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        write_property(&mut out, "Reference", &ps.id, 0.0, -2.0, true);
        write_property(&mut out, "Value", &ps.net, 0.0, 2.0, false);
        write_property(&mut out, "Footprint", "", 0.0, 0.0, true);
        write_property(&mut out, "Datasheet", "", 0.0, 0.0, true);
        let pin_uuid = duid(&format!("pwrpin:{}", ps.id));
        writeln!(out, "\t\t(pin \"1\" (uuid \"{pin_uuid}\"))").unwrap();
        writeln!(out, "\t\t(instances").unwrap();
        writeln!(out, "\t\t\t(project \"eda-kicad\"").unwrap();
        writeln!(out, "\t\t\t\t(path \"/{sheet_uuid}\"").unwrap();
        writeln!(out, "\t\t\t\t\t(reference {})", sexpr_str(&ps.id)).unwrap();
        writeln!(out, "\t\t\t\t\t(unit 1)").unwrap();
        writeln!(out, "\t\t\t\t)").unwrap();
        writeln!(out, "\t\t\t)").unwrap();
        writeln!(out, "\t\t)").unwrap();
        writeln!(out, "\t)").unwrap();
    }

    // ---- wires (each polyline segment as one KiCad wire) ----
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

    // ---- junctions: every point where 3+ same-net wire-segment endpoints
    // meet, *including* a T-junction with no shared vertex (one wire's
    // endpoint landing mid-span on another same-net wire's segment) --
    // `geometry::wire_junction_points` is the single definition
    // `eda_render`/`eda_gates` already hold every dot to, so drawing
    // anything narrower here (the old plain vertex-coincidence count) is
    // exactly what `schematic_missing_junction` exists to catch: a real
    // connection real KiCad accepts silently, left with no dot marking it.
    // Net-scoped (not just by point), so two different nets whose wires
    // happen to cross at the same coordinate never draw a false short.
    for (net, pt) in eda_engine::geometry::wire_junction_points(&sch.wires) {
        let x = mm(pt.x);
        let y = mm(pt.y);
        let uuid = duid(&format!("junction:{net}:{}:{}", pt.x, pt.y));
        writeln!(out, "\t(junction (at {x} {y}) (diameter 0) (color 0 0 0 0)").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        writeln!(out, "\t)").unwrap();
    }

    // ---- no-connect flags ----
    for nc in &no_connects {
        let x = mm(nc.at.x);
        let y = mm(nc.at.y);
        let uuid = duid(&format!("nc:{}:{}", nc.at.x, nc.at.y));
        writeln!(out, "\t(no_connect (at {x} {y}) (uuid \"{uuid}\"))").unwrap();
    }

    // ---- labels: local, global or hierarchical, per `NetLabel::kind` ----
    for l in &labels {
        let x = mm(l.at.x);
        let y = mm(l.at.y);
        let uuid = duid(&format!("label:{}:{}:{}", l.net, l.at.x, l.at.y));
        let (tag, shape) = match &l.kind {
            eda_model::ir::LabelKind::Local => ("label", None),
            eda_model::ir::LabelKind::Global { shape } => ("global_label", Some(*shape)),
            eda_model::ir::LabelKind::Hierarchical { shape } => ("hierarchical_label", Some(*shape)),
        };
        write!(out, "\t({tag} {}", sexpr_str(&l.net)).unwrap();
        if let Some(shape) = shape {
            write!(out, " (shape {})", label_shape_token(shape)).unwrap();
        }
        writeln!(out, " (at {x} {y} 0)").unwrap();
        writeln!(out, "\t\t(effects (font (size 1.27 1.27)) (justify left))").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        writeln!(out, "\t)").unwrap();
    }

    // ---- free text (`T`) -- same shape as a label's own s-expr, minus the
    // net/shape fields a plain KiCad `(text ...)` has neither of ----
    for t in &texts {
        let x = mm(t.at.x);
        let y = mm(t.at.y);
        let angle_deg = t.angle as f64 / 1000.0;
        let size_mm = t.size_um as f64 / 1000.0;
        let uuid = duid(&format!("text:{}:{}:{}", t.content, t.at.x, t.at.y));
        writeln!(out, "\t(text {}", sexpr_str(&t.content)).unwrap();
        writeln!(out, "\t\t(at {x} {y} {angle_deg})").unwrap();
        writeln!(out, "\t\t(effects (font (size {size_mm} {size_mm})))").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        writeln!(out, "\t)").unwrap();
    }

    // ---- sheet instances (required by KiCad 9 for a valid project-less sheet) ----
    writeln!(out, "\t(sheet_instances").unwrap();
    writeln!(out, "\t\t(path \"/\" (page \"1\"))").unwrap();
    writeln!(out, "\t)").unwrap();
    writeln!(out, "\t(embedded_fonts no)").unwrap();

    writeln!(out, ")").unwrap();
    Ok(out)
}

/// A `SymbolInstance`'s lib_id, with the pre-`lib_id`-field
/// (`design.json` written before this port) fallback to the old synthetic
/// `"eda:<id>"` form.
fn sym_lib_id(sym: &SymbolInstance) -> String {
    if !sym.lib_id.is_empty() {
        sym.lib_id.clone()
    } else {
        format!("eda:{}", sym.id)
    }
}

fn label_shape_token(shape: eda_model::ir::LabelShape) -> &'static str {
    use eda_model::ir::LabelShape;
    match shape {
        LabelShape::Input => "input",
        LabelShape::Output => "output",
        LabelShape::Bidirectional => "bidirectional",
        LabelShape::TriState => "tri_state",
        LabelShape::Passive => "passive",
    }
}

fn write_property(out: &mut String, key: &str, value: &str, x: f64, y: f64, hide: bool) {
    if hide {
        writeln!(
            out,
            "\t\t(property {} {} (at {x} {y} 0)\n\t\t\t(effects (font (size 1.27 1.27)) (hide yes))\n\t\t)",
            sexpr_str(key),
            sexpr_str(value)
        )
        .unwrap();
    } else {
        writeln!(
            out,
            "\t\t(property {} {} (at {x} {y} 0)\n\t\t\t(effects (font (size 1.27 1.27)))\n\t\t)",
            sexpr_str(key),
            sexpr_str(value)
        )
        .unwrap();
    }
}

/// Feeds a real library symbol's own local point (library frame, mm, +y
/// **up** -- [`eda_model::symbol::SPoint`]'s own convention) through
/// [`baked_local`]: first shifted into the same sheet-frame,
/// box-corner-relative local convention [`local_port_point`]/
/// [`local_stub_tip`] already use for the synthetic path, then given the
/// same rotation/mirror treatment, so a real pin/graphic composes correctly
/// if a rotated/mirrored instance is ever emitted (`derive_schematic` never
/// emits one today -- see this module's own doc comment).
///
/// `(x0, y1)` is the real symbol's own box corner in *library* frame --
/// [`eda_engine::geometry::real_symbol_bbox`]'s own `(x0, _, _, y1)`, the
/// same corner `node_size`/`build_ports` already measured this part's box
/// and every pin's `Port::offset` from. This instance's own `sym.at` *is*
/// that corner (not the library's `(0,0)` origin -- a real symbol's origin
/// is almost never its own box's top-left), so a real point has to be
/// shifted by it before it means anything relative to `sym.at`: `p.x - x0`
/// along library +x (untouched by the sheet's y-flip), `y1 - p.y` along
/// library +y (flipped, since library "up" is sheet "into the box" from the
/// top) -- the exact shift [`eda_engine::geometry::build_ports_from_real_symbol`]'s
/// own `body_x - x0` / `y1 - body_y` offsets already use, and
/// [`eda_engine::geometry::nc_pin_local_points`]'s real-symbol branch
/// already applies for a `nc`-kind pin. Skipping this shift (as an earlier
/// version of this function did, treating `p` as already box-corner
/// relative) draws a real pin exactly `(x0, y1)` away from wherever its own
/// wire/power-symbol stub actually terminates -- confirmed empirically:
/// `Device:C`'s box corner sits 2.54mm from its library origin, and
/// `CIN`/`COUT`'s power-flagged pin landed 2.54mm/3.81mm off its own
/// GND power symbol until this shift was added.
fn baked_real_point(sym: &SymbolInstance, width_um: f64, x0: f64, y1: f64, p: eda_model::symbol::SPoint) -> eda_model::symbol::SPoint {
    let (x, y) = baked_local(sym, width_um, (p.x - x0) * 1000.0, (y1 - p.y) * 1000.0);
    eda_model::symbol::SPoint::new(x, y)
}

/// A real pin's own `angle_deg`, composed with the instance's mirror the
/// same way [`baked_real_point`] composes position (rotation, never
/// non-zero today, is left for whoever adds rotated-instance support).
/// Mirroring is a flip across a vertical axis, so it swaps East/West
/// (0<->180) and leaves North/South (90/270) alone -- the standard
/// `angle -> 180 - angle` reflection.
fn baked_real_angle(sym: &SymbolInstance, angle_deg: f64) -> f64 {
    if sym.mirrored {
        (180.0 - angle_deg).rem_euclid(360.0)
    } else {
        angle_deg
    }
}

/// [`write_symbol_graphic`]'s input, but with every point run through
/// [`baked_real_point`]/[`baked_real_angle`] first -- i.e. the same
/// graphic, placed the way *this* instance's own rotation/mirror would
/// draw it, in the on-disk library-frame convention [`write_symbol_graphic`]
/// itself expects. `unit`/`stroke_mm`/`filled`/`size_mm`/`text` carry no
/// position and pass through untouched.
fn baked_graphic(g: &eda_model::SymbolGraphic, sym: &SymbolInstance, width_um: f64, x0: f64, y1: f64) -> eda_model::SymbolGraphic {
    use eda_model::SymbolGraphic::*;
    let p = |pt| baked_real_point(sym, width_um, x0, y1, pt);
    match *g {
        Rectangle { unit, start, end, stroke_mm, filled } => Rectangle { unit, start: p(start), end: p(end), stroke_mm, filled },
        Polyline { unit, ref pts, stroke_mm, filled } => Polyline { unit, pts: pts.iter().map(|&pt| p(pt)).collect(), stroke_mm, filled },
        Circle { unit, center, radius_mm, stroke_mm, filled } => Circle { unit, center: p(center), radius_mm, stroke_mm, filled },
        Arc { unit, start, mid, end, stroke_mm, filled } => Arc { unit, start: p(start), mid: p(mid), end: p(end), stroke_mm, filled },
        Text { unit, ref text, at, angle_deg, size_mm } => Text { unit, text: text.clone(), at: p(at), angle_deg: baked_real_angle(sym, angle_deg), size_mm },
    }
}

/// Embeds one part-backed symbol's `lib_symbol` definition: when a real
/// library symbol resolved for `lib_id`, its own graphics and pins, drawn
/// verbatim (baked through this instance's own rotation/mirror -- see
/// [`baked_graphic`]/[`baked_real_point`]) so an R/C/LED/connector/IC looks
/// exactly as real KiCad draws it; otherwise the synthetic generic box this
/// exporter has always drawn. Either way, `eda_engine::geometry`'s own
/// `node_size`/`build_ports` already derived this part's box/port geometry
/// from the *same* resolved symbol (see `write_schematic`'s own call),
/// so a wire's stub tip and this function's own drawn pin position can
/// never disagree about where a pin actually is.
fn write_regular_lib_symbol(out: &mut String, lib_id: &str, sym: &SymbolInstance, part: &Part, model: &ConstraintModel) {
    let resolved = model.real_symbol_of(lib_id, part);
    let (width, height) = eda_engine::geometry::node_size(part, resolved.as_ref());
    let (ports, pin_port) = eda_engine::geometry::build_ports(part, width, height, resolved.as_ref());
    // This part's box corner in *library* frame -- see `baked_real_point`'s
    // doc comment. Only meaningful when `resolved` is `Some`; the `None`
    // branches below never read it.
    let (x0, _, _, y1) = resolved.as_ref().map(|s| eda_engine::geometry::real_symbol_bbox(s)).unwrap_or((0.0, 0.0, 0.0, 0.0));
    let mut pin_of_port: Vec<Option<usize>> = vec![None; ports.len()];
    for (pin_idx, port_idx) in pin_port.iter().enumerate() {
        if let Some(pi) = port_idx {
            pin_of_port[*pi] = Some(pin_idx);
        }
    }

    // KiCad requires a unit/body-style sub-symbol's own name to be
    // `<bare-name>_<unit>_<style>`, where `<bare-name>` is the *library*
    // symbol's own name (the part of `lib_id` after its `Library:` prefix)
    // -- never an instance's reference designator. This matters once a
    // real `lib_id` like `"Device:C"` is shared by more than one instance
    // (`CIN` and `COUT` both resolve to it): naming the sub-units after
    // whichever instance happened to trigger this block (`"CIN_0_1"`)
    // parses as a name/prefix mismatch and `kicad-cli` refuses to load the
    // file at all. Confirmed empirically against real `kicad-cli sch erc`.
    let bare_name = lib_id.rsplit(':').next().unwrap_or(lib_id);
    writeln!(out, "\t\t(symbol {}", sexpr_str(lib_id)).unwrap();
    writeln!(out, "\t\t\t(exclude_from_sim no) (in_bom yes) (on_board yes)").unwrap();
    write_property(out, "Reference", "U", 0.0, 0.0, false);
    write_property(out, "Value", bare_name, 0.0, 0.0, false);

    // ---- unit _0_1: the body ----
    writeln!(out, "\t\t\t(symbol {}", sexpr_str(&format!("{bare_name}_0_1"))).unwrap();
    match &resolved {
        Some(sym_data) => {
            // Every unit (0 = "every unit") or specifically unit 1 -- this
            // project only ever places a symbol's unit 1, so a graphic
            // scoped to another unit is never this instance's own body.
            for g in sym_data.graphics.iter().filter(|g| matches!(g.unit(), 0 | 1)) {
                write_symbol_graphic(out, &baked_graphic(g, sym, width as f64, x0, y1));
            }
        }
        None => {
            let corners = [(0.0, 0.0), (width as f64, 0.0), (width as f64, height as f64), (0.0, height as f64), (0.0, 0.0)];
            write!(out, "\t\t\t\t(polyline\n\t\t\t\t\t(pts").unwrap();
            for (lx, ly) in corners {
                let (bx, by) = baked_local(sym, width as f64, lx, ly);
                write!(out, " (xy {} {})", fmt_mm_f(bx), fmt_mm_f(by)).unwrap();
            }
            writeln!(out, ")\n\t\t\t\t\t(stroke (width 0.254) (type default))\n\t\t\t\t\t(fill (type none))\n\t\t\t\t)").unwrap();
        }
    }
    writeln!(out, "\t\t\t)").unwrap();

    // ---- unit _1_1: the pins ----
    writeln!(out, "\t\t\t(symbol {}", sexpr_str(&format!("{bare_name}_1_1"))).unwrap();
    for (port_idx, port) in ports.iter().enumerate() {
        let Some(pin_idx) = pin_of_port[port_idx] else { continue };
        let pin = &part.pins[pin_idx];
        let real_pin = resolved.as_ref().and_then(|s| s.pin_by_number(&pin.number));
        let (bx, by, angle, length_mm) = match real_pin {
            // The real pin's own outer point/angle/length -- exactly
            // where its own drawn line (just written above, as part of
            // the body's graphics when it's a separate primitive, or
            // implicit when the pin line itself *is* the graphic) and a
            // wire's own stub tip (computed from the very same point by
            // `build_ports`'s real-symbol path) both place it.
            Some(rp) => {
                let p = baked_real_point(sym, width as f64, x0, y1, rp.at);
                (p.x, p.y, baked_real_angle(sym, rp.angle_deg), rp.length_mm)
            }
            // No matching real pin (shouldn't happen for a resolved
            // symbol whose numbers agree with ours -- defensive only):
            // fall back to the synthetic stub tip so the instance's own
            // pin-uuid list still has a drawn pin for every part.pins
            // entry.
            None => {
                let (plx, ply) = local_port_point(port, width, height);
                let (slx, sly) = local_stub_tip(port, plx, ply);
                let (bx, by) = baked_local(sym, width as f64, slx, sly);
                (bx, by, 0.0, STUB_MM)
            }
        };
        let etype = resolve_pin_electrical_type(pin, resolved.as_ref());
        let name = pin.name.clone().unwrap_or_else(|| "~".to_string());
        writeln!(
            out,
            "\t\t\t\t(pin {etype} line (at {} {} {}) (length {})\n\t\t\t\t\t(name {} (effects (font (size 1.27 1.27))))\n\t\t\t\t\t(number {} (effects (font (size 1.27 1.27))))\n\t\t\t\t)",
            fmt_mm_f(bx),
            fmt_mm_f(by),
            fmt_mm_f(angle),
            fmt_mm_f(length_mm),
            sexpr_str(&name),
            sexpr_str(&pin.number),
        )
        .unwrap();
    }
    // `nc`-kind pins carry no `Port` (see `geometry::nc_pin_local_points`);
    // draw them here too, at the same points `derive_schematic` placed a
    // `no_connects` marker at, so the instance's own pin-uuid list (every
    // `part.pins`, unconditionally) always has a matching drawn pin.
    for (i, local) in eda_engine::geometry::nc_pin_local_points(part, width, height, resolved.as_ref()) {
        let pin = &part.pins[i];
        // `baked_local` takes um (like every other call site in this
        // function — `local_stub_tip`'s output, `width`/`height`
        // themselves) and does its own um->mm division at the end; do not
        // pre-convert `local` here too, or every nc pin lands 1000x closer
        // to the origin than intended.
        let (bx, by) = baked_local(sym, width as f64, local.x as f64, local.y as f64);
        let etype = resolve_pin_electrical_type(pin, resolved.as_ref());
        let name = pin.name.clone().unwrap_or_else(|| "~".to_string());
        writeln!(
            out,
            "\t\t\t\t(pin {etype} line (at {} {} 0) (length 0)\n\t\t\t\t\t(name {} (effects (font (size 1.27 1.27))))\n\t\t\t\t\t(number {} (effects (font (size 1.27 1.27))))\n\t\t\t\t)",
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

/// A power symbol's `lib_symbol`: the resolved definition's real graphics
/// and pin(s), embedded verbatim (already in the correct on-disk
/// convention — see [`baked_local`]'s doc comment). One shared definition
/// per `lib_id`, regardless of how many instances place it.
fn write_power_lib_symbol(out: &mut String, lib_id: &str, resolved: &eda_model::LibSymbol) {
    writeln!(out, "\t\t(symbol {}", sexpr_str(lib_id)).unwrap();
    writeln!(out, "\t\t\t(power)").unwrap();
    writeln!(out, "\t\t\t(pin_names (offset 0) (hide yes))").unwrap();
    writeln!(out, "\t\t\t(exclude_from_sim no) (in_bom yes) (on_board yes)").unwrap();
    let value = lib_id.strip_prefix("power:").unwrap_or(lib_id);
    write_property(out, "Reference", "#PWR", 0.0, -2.0, true);
    write_property(out, "Value", value, 0.0, 2.0, false);
    write_property(out, "Footprint", "", 0.0, 0.0, true);
    write_property(out, "Datasheet", &resolved.datasheet, 0.0, 0.0, true);

    writeln!(out, "\t\t\t(symbol {}", sexpr_str(&format!("{value}_0_1"))).unwrap();
    for g in &resolved.graphics {
        write_symbol_graphic(out, g);
    }
    writeln!(out, "\t\t\t)").unwrap();

    writeln!(out, "\t\t\t(symbol {}", sexpr_str(&format!("{value}_1_1"))).unwrap();
    for pin in &resolved.pins {
        writeln!(
            out,
            "\t\t\t\t(pin {} line (at {} {} {}) (length {})\n\t\t\t\t\t(name {} (effects (font (size 1.27 1.27))))\n\t\t\t\t\t(number {} (effects (font (size 1.27 1.27))))\n\t\t\t\t)",
            pin.electrical_type,
            fmt_mm_f(pin.at.x),
            fmt_mm_f(pin.at.y),
            fmt_mm_f(pin.angle_deg),
            fmt_mm_f(pin.length_mm),
            sexpr_str(&pin.name),
            sexpr_str(&pin.number),
        )
        .unwrap();
    }
    writeln!(out, "\t\t\t)").unwrap();
    writeln!(out, "\t\t)").unwrap();
}

fn write_symbol_graphic(out: &mut String, g: &eda_model::SymbolGraphic) {
    use eda_model::SymbolGraphic::*;
    match g {
        Rectangle { start, end, stroke_mm, filled, .. } => {
            writeln!(
                out,
                "\t\t\t\t(rectangle\n\t\t\t\t\t(start {} {}) (end {} {})\n\t\t\t\t\t(stroke (width {}) (type default))\n\t\t\t\t\t(fill (type {}))\n\t\t\t\t)",
                fmt_mm_f(start.x),
                fmt_mm_f(start.y),
                fmt_mm_f(end.x),
                fmt_mm_f(end.y),
                fmt_mm_f(*stroke_mm),
                if *filled { "background" } else { "none" },
            )
            .unwrap();
        }
        Polyline { pts, stroke_mm, filled, .. } => {
            write!(out, "\t\t\t\t(polyline\n\t\t\t\t\t(pts").unwrap();
            for p in pts {
                write!(out, " (xy {} {})", fmt_mm_f(p.x), fmt_mm_f(p.y)).unwrap();
            }
            writeln!(
                out,
                ")\n\t\t\t\t\t(stroke (width {}) (type default))\n\t\t\t\t\t(fill (type {}))\n\t\t\t\t)",
                fmt_mm_f(*stroke_mm),
                if *filled { "background" } else { "none" },
            )
            .unwrap();
        }
        Circle { center, radius_mm, stroke_mm, filled, .. } => {
            writeln!(
                out,
                "\t\t\t\t(circle\n\t\t\t\t\t(center {} {}) (radius {})\n\t\t\t\t\t(stroke (width {}) (type default))\n\t\t\t\t\t(fill (type {}))\n\t\t\t\t)",
                fmt_mm_f(center.x),
                fmt_mm_f(center.y),
                fmt_mm_f(*radius_mm),
                fmt_mm_f(*stroke_mm),
                if *filled { "background" } else { "none" },
            )
            .unwrap();
        }
        Arc { start, mid, end, stroke_mm, filled, .. } => {
            writeln!(
                out,
                "\t\t\t\t(arc\n\t\t\t\t\t(start {} {}) (mid {} {}) (end {} {})\n\t\t\t\t\t(stroke (width {}) (type default))\n\t\t\t\t\t(fill (type {}))\n\t\t\t\t)",
                fmt_mm_f(start.x),
                fmt_mm_f(start.y),
                fmt_mm_f(mid.x),
                fmt_mm_f(mid.y),
                fmt_mm_f(end.x),
                fmt_mm_f(end.y),
                fmt_mm_f(*stroke_mm),
                if *filled { "background" } else { "none" },
            )
            .unwrap();
        }
        Text { text, at, angle_deg, size_mm, .. } => {
            writeln!(
                out,
                "\t\t\t\t(text {} (at {} {} {})\n\t\t\t\t\t(effects (font (size {} {})))\n\t\t\t\t)",
                sexpr_str(text),
                fmt_mm_f(at.x),
                fmt_mm_f(at.y),
                fmt_mm_f(*angle_deg),
                fmt_mm_f(*size_mm),
                fmt_mm_f(*size_mm),
            )
            .unwrap();
        }
    }
}

/// A pin's electrical type for the file: the real resolved library
/// symbol's own pin, matched by number (the "map pins by number" this
/// port's loader exists for), when one resolved; else the same coarse
/// `PinKind` mapping this exporter has always used.
fn resolve_pin_electrical_type(pin: &eda_model::Pin, resolved: Option<&eda_model::LibSymbol>) -> String {
    if let Some(p) = resolved.and_then(|s| s.pin_by_number(&pin.number)) {
        return p.electrical_type.clone();
    }
    electrical_type(pin.kind, pin.name.as_deref()).to_string()
}

/// The coarse `PinKind` -> KiCad electrical-type fallback this exporter
/// uses whenever no real library symbol resolved a pin's real type. A
/// `Power`-kind pin whose name reads as an output (a regulator's own
/// VOUT) maps to `power_out`, not `power_in` — the one place this coarse
/// mapping needs to distinguish a rail's source from its sinks, since
/// `power_pin_not_driven` (see `eda_kicad::erc`) requires *some*
/// `power_out` pin on every power net, and nothing else in this project's
/// model says which `Power`-kind pin, if any, plays that role. Kept in
/// lockstep with `eda_kicad::erc::ElectricalPinType::from_pin_kind` and
/// with `eda_engine::derive_schematic`'s own `PWR_FLAG` decision, which
/// uses the exact same name convention.
fn electrical_type(kind: PinKind, name: Option<&str>) -> &'static str {
    match kind {
        PinKind::Power if name.unwrap_or("").to_ascii_uppercase().contains("OUT") => "power_out",
        PinKind::Power | PinKind::Ground => "power_in",
        PinKind::Signal => "bidirectional",
        PinKind::Passive => "passive",
        PinKind::Nc => "no_connect",
    }
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
/// KiCad's `lib_symbols` graphics/pins live in the *library's own* frame,
/// which is +y **up**; everything else in a `.kicad_sch` (the sheet itself,
/// symbol instance `(at ...)`, wires, labels) is +y **down**, matching this
/// crate's own `ir::Point`. KiCad negates a library symbol's local y when
/// it composites that symbol onto the sheet (instance position + library
/// point, with the library point's y flipped) -- so a lib_symbol's own
/// graphics/pins must be written with y already negated, or the two
/// negations fail to cancel and KiCad places the *instance's* pins
/// somewhere our own wires never reach. Confirmed against real
/// `kicad-cli sch erc`: without this negation, every pin whose local y is
/// non-zero comes back `pin_not_connected` (and, for a power-style pin,
/// `power_pin_not_driven` too) even though the wire in the file terminates
/// at exactly the un-negated point -- KiCad is looking for it at `-y`.
fn baked_local(sym: &SymbolInstance, width: f64, lx: f64, ly: f64) -> (f64, f64) {
    let lx = if sym.mirrored { width - lx } else { lx };
    let theta = (sym.rot as f64 / 1000.0) * std::f64::consts::PI / 180.0;
    let rx = lx * theta.cos() - ly * theta.sin();
    let ry = lx * theta.sin() + ly * theta.cos();
    (rx / 1000.0, -ry / 1000.0)
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

/// KiCad stores a pad's drawn rotation as its footprint's own file angle
/// plus the pad's own -- but mirroring (a bottom-side part) flips the
/// *sense* of a rotation that is defined, as `Pad::rot` is, on the
/// footprint's un-mirrored local shape: mirroring a shape that carries its
/// own rotation of `theta` is the same as mirroring the unrotated shape and
/// then rotating it by `-theta`. So a pad's own rotation contributes with
/// the opposite sign on the bottom side; the footprint's own `fp_rot` needs
/// no such flip (it is applied *after* the mirror, in `to_board`, so its
/// sign is the same either side).
///
/// Returns the pad's absolute file angle, degrees, ready for `(at x y
/// angle)` -- the same single negation `write_footprint` already applies
/// to a footprint with no rotated pads.
pub(crate) fn pad_file_angle(fp_side: eda_model::ir::Side, fp_rot: eda_model::ir::Millideg, pad_rot: eda_model::ir::Millideg) -> String {
    let effective = if fp_side == eda_model::ir::Side::Bottom { -(pad_rot as i64) } else { pad_rot as i64 };
    fmt_mm_f(-((fp_rot as i64 + effective) as f64) / 1000.0)
}

/// Inverse of `pad_file_angle`: recover a pad's own `rot` (relative to its
/// footprint, our internal convention) from its file angle and its
/// footprint's own rotation (already imported, `ir::Millideg`).
pub(crate) fn pad_rot_from_file(fp_side: eda_model::ir::Side, fp_rot: eda_model::ir::Millideg, pad_file_deg: f64) -> eda_model::ir::Millideg {
    let combined = import::import_rot_millideg(pad_file_deg) as i64;
    let delta = (combined - fp_rot as i64).rem_euclid(360_000);
    (if fp_side == eda_model::ir::Side::Bottom { -delta } else { delta }).rem_euclid(360_000) as eda_model::ir::Millideg
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
        Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(format!("{reference}_val")), package: None, footprint: Some("Foo:Bar".into()), pins, body_um: None, symbol: None, datasheet: None, edge: None }
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
        // Every part instance *and* every power symbol (a `PWR_FLAG`
        // included) is written as its own `(symbol (lib_id ...) ...)`
        // sheet block, so the pattern's count is the sum of both, not
        // just the part instances.
        assert_eq!(out.matches("(symbol (lib_id").count(), sch.symbols.len() + sch.power_symbols.len());
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
        let (width, height) = eda_engine::geometry::node_size(part, None);
        let (ports, pin_port) = eda_engine::geometry::build_ports(part, width, height, None);
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
    fn power_nets_use_power_symbols_not_wires() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
        let out = export(&model);
        // GND is a name-recognized ground net, so the engine drops a real
        // `power:GND` symbol at each of its pins instead of wiring them or
        // labeling them — no `global_label`/`label` for GND at all.
        let sch = design.schematic.as_ref().unwrap();
        assert!(sch.power_symbols.iter().any(|p| p.net == "GND" && p.lib_id == "power:GND"), "{:#?}", sch.power_symbols);
        assert!(out.contains("(lib_id \"power:GND\")"), "GND should be a real power symbol:\n{out}");
        assert!(!out.contains("(global_label \"GND\""));
        assert!(!out.contains("(label \"GND\""));
    }

    #[test]
    fn missing_schematic_errors() {
        let design = Design {
            footprint_library: None,
            schema: 1,
            provenance: eda_model::ir::Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: None,
            routing: None,
            drawings: None,
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

//! A hierarchical schematic: one sheet per module, a sheet symbol for each on the root.
//!
//! `derive_hierarchy` is the module-sheet counterpart of `derive_schematic`. The cut into modules comes from
//! `eda_model::modules::infer_modules`; what is drawn on each sheet is `sheet.rs`, the root is `root.rs`, and `kit.rs` /
//! `items.rs` are the measuring and bookkeeping they share.
//!
//! Power nets are never sheet pins: a supply or ground rail is a power symbol at each pin on whichever sheet the pin is, and the
//! names join them across sheets, as in any KiCad design. One `PWR_FLAG` per rail that needs a source goes on the first sheet that
//! has a pin on it.

use std::collections::{BTreeMap, BTreeSet};

use eda_model::ir::{Design, PowerSymbol, Provenance, SchematicSection, TitleBlock};
use eda_model::modules::{infer_modules, is_ground_net_name, FunctionalModule};
use eda_model::sch_extras::SchExtras;
use eda_model::{CheckResult, ConstraintModel, PinKind};

pub mod items;
pub mod kit;
pub mod root;
pub mod sheet;

#[cfg(test)]
mod tests;

pub use items::Keep;
use items::Ctx;
use kit::Paper;

use crate::EngineOptions;

/// The locked symbols among `refs`, sorted: the lock is part of the symbol and goes with it to its sheet.
fn locked_among(keep: &Keep, refs: &[String]) -> Vec<String> {
    let mut locked: Vec<String> = refs.iter().filter(|r| keep.locked.contains(r)).cloned().collect();
    locked.sort();
    locked.dedup();
    locked
}

/// A file name for a module's sheet: its name in lower case, anything but letters and digits an underscore.
pub fn sheet_file_name(name: &str, taken: &BTreeSet<String>) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('_') {
            s.push('_');
        }
    }
    let s = s.trim_matches('_').to_string();
    let base = if s.is_empty() { "sheet".to_string() } else { s };
    let mut candidate = format!("{base}.kicad_sch");
    let mut n = 1;
    while taken.contains(&candidate) {
        n += 1;
        candidate = format!("{base}_{n}.kicad_sch");
    }
    candidate
}

fn validate(model: &ConstraintModel) -> Vec<CheckResult> {
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    for p in &model.parts {
        if !seen.insert(p.reference.clone()) {
            errors.push(CheckResult::fail("engine.duplicate_reference", p.reference.clone(), "duplicate part reference"));
        }
    }
    for net in &model.nets {
        for pin_ref in &net.pins {
            let (r, n) = pin_ref.split_once('.').unwrap_or((pin_ref.as_str(), ""));
            if model.part(r).and_then(|p| p.pins.iter().find(|x| x.number == n)).is_none() {
                errors.push(CheckResult::fail("engine.unresolved_pin", format!("{}:{pin_ref}", net.name), "pin reference does not resolve to a known part/pin"));
            }
        }
    }
    errors
}

/// Nets that need a `PWR_FLAG`: a power or ground pin on them and no power output (a regulator's output), exactly the rule
/// `derive_schematic` uses.
pub(super) fn flag_nets(ctx: &Ctx) -> Vec<(String, Vec<String>)> {
    let mut nets = ctx.model.nets.clone();
    nets.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out = Vec::new();
    for net in &nets {
        let mut has_in = ctx.is_rail(&net.name);
        let mut has_out = false;
        for pin_ref in &net.pins {
            let (r, n) = pin_ref.split_once('.').unwrap_or((pin_ref.as_str(), ""));
            let Some(pin) = ctx.model.part(r).and_then(|p| p.pins.iter().find(|x| x.number == n)) else { continue };
            let is_out = pin.kind == PinKind::Power && pin.name.as_deref().unwrap_or("").to_ascii_uppercase().contains("OUT");
            match pin.kind {
                PinKind::Power if is_out => has_out = true,
                PinKind::Power | PinKind::Ground => has_in = true,
                _ => {}
            }
        }
        if has_in && !has_out {
            let mut pins = net.pins.clone();
            pins.sort();
            out.push((net.name.clone(), pins));
        }
    }
    out
}

/// Move every item of a section by `(dx, dy)`.
pub fn translate_section(sch: &mut SchematicSection, dx: i64, dy: i64) {
    let shift = |p: &mut eda_model::ir::Point| {
        p.x += dx;
        p.y += dy;
    };
    sch.symbols.iter_mut().for_each(|s| shift(&mut s.at));
    sch.wires.iter_mut().flat_map(|w| w.pts.iter_mut()).for_each(shift);
    sch.labels.iter_mut().for_each(|l| shift(&mut l.at));
    sch.texts.iter_mut().for_each(|t| shift(&mut t.at));
    sch.power_symbols.iter_mut().for_each(|p| shift(&mut p.at));
    sch.no_connects.iter_mut().for_each(|n| shift(&mut n.at));
    sch.bus_entries.iter_mut().for_each(|b| shift(&mut b.at));
    sch.junctions.iter_mut().for_each(|j| shift(&mut j.at));
    sch.lines.iter_mut().flat_map(|l| l.pts.iter_mut()).for_each(shift);
    for s in sch.sheets.iter_mut() {
        shift(&mut s.at);
        s.pins.iter_mut().for_each(|p| shift(&mut p.at));
    }
}

/// Put a flat schematic inside the drawing sheet's frame: choose the smallest paper that holds what is drawn and shift everything
/// onto it, on the grid, centred across and with the top at the frame's margin. (`derive_schematic` packs from the origin, so its
/// first row used to sit on the frame line, off the top of the page.)
pub fn fit_flat(sch: &mut SchematicSection, model: &ConstraintModel) {
    use kit::{smallest_paper, snap_down, snap_up};
    let Some(bb) = flat_extent(sch, model) else { return };
    let (w, h) = (snap_up(bb.x1) - snap_down(bb.x0), snap_up(bb.y1) - snap_down(bb.y0));
    let paper = smallest_paper(w, h);
    let u = paper.usable();
    let dx = u.x0 + snap_down((u.w() - w) / 2) - snap_down(bb.x0);
    let dy = u.y0 - snap_down(bb.y0);
    translate_section(sch, dx, dy);
    let tb = sch.title_block.get_or_insert_with(TitleBlock::default);
    tb.paper = paper.name.to_string();
}

/// Whether content of this extent fits the largest paper there is.
pub fn fits_a_sheet(bb: kit::Rect) -> bool {
    use kit::{snap_down, snap_up, PAPERS};
    let (w, h) = (snap_up(bb.x1) - snap_down(bb.x0), snap_up(bb.y1) - snap_down(bb.y0));
    let u = PAPERS[PAPERS.len() - 1].usable();
    w <= u.w() && h <= u.h()
}

/// The box of everything a flat section draws: its symbols with their pins and texts, its labels, power symbols and no-connect flags,
/// the points of its wires.
pub fn flat_extent(sch: &SchematicSection, model: &ConstraintModel) -> Option<kit::Rect> {
    use kit::{label_rect, nc_rect, power_rect, union_all, Placed, Rect};
    let mut rects: Vec<Rect> = Vec::new();
    for s in &sch.symbols {
        let Some(part) = model.part(&s.id) else { continue };
        let lib_id = if s.lib_id.is_empty() { format!("eda:{}", s.id) } else { s.lib_id.clone() };
        let resolved = model.real_symbol_of(&lib_id, part);
        let package = part.package.clone().unwrap_or_default();
        let mut p = Placed::new(part, &lib_id, resolved, &s.value, &package);
        p.flip = s.rot == 180_000;
        p.x = if p.flip { s.at.x - p.w() } else { s.at.x };
        p.y = if p.flip { s.at.y - p.h() } else { s.at.y };
        rects.push(p.keepout_in(sch));
    }
    for l in &sch.labels {
        let hierarchical = !matches!(l.kind, eda_model::ir::LabelKind::Local);
        rects.push(label_rect(l.at, crate::obstacles::label_run_dir(sch, l), &l.net, hierarchical));
    }
    for p in &sch.power_symbols {
        rects.push(power_rect(p.at, p.rot, &p.net, &p.lib_id));
    }
    for n in &sch.no_connects {
        rects.push(nc_rect(n.at));
    }
    for w in &sch.wires {
        for p in &w.pts {
            rects.push(Rect { x0: p.x, y0: p.y, x1: p.x, y1: p.y });
        }
    }
    union_all(rects)
}

/// Number the power symbols across the whole design, in sheet order, and drop one `PWR_FLAG` per net that needs a source on the
/// first sheet with a pin on it.
fn number_power(ctx: &Ctx, modules: &[FunctionalModule], outs: &mut [sheet::SheetOut]) {
    let mut n = 0;
    for o in outs.iter_mut() {
        for p in o.items.power.iter_mut() {
            n += 1;
            p.id = format!("#PWR{n:02}");
        }
    }
    let mut flag_n = n;
    for (net, pins) in flag_nets(ctx) {
        let ground = is_ground_net_name(&net);
        'find: for pin_ref in &pins {
            for (mi, m) in modules.iter().enumerate() {
                let r = pin_ref.split('.').next().unwrap_or("");
                if !m.refs.iter().any(|x| x == r) {
                    continue;
                }
                let Some(tip) = outs[mi].items.tips.get(pin_ref).copied() else { continue };
                let side = outs[mi].items.tip_sides.get(pin_ref).copied().unwrap_or(eda_layout::Side::Top);
                flag_n += 1;
                // on the pin's end, its glyph running out along the wire to the rail's symbol (its value is not shown)
                let _ = ground;
                let rot = items::outward_rot(true, side);
                outs[mi].items.power.push(PowerSymbol { id: format!("#FLG{flag_n:02}"), lib_id: "power:PWR_FLAG".to_string(), at: tip, rot, net: net.clone(), pin: String::new() });
                break 'find;
            }
        }
    }
}

/// A design that is one module: one sheet, laid out like a module sheet (the anchor in the middle, the passives that serve it
/// beside it, everything inside the frame), no root above it.
pub fn derive_single_sheet(model: &ConstraintModel, opts: &EngineOptions, module: &FunctionalModule, keep: &Keep) -> Result<Design, Vec<CheckResult>> {
    let errors = validate(model);
    if !errors.is_empty() {
        return Err(errors);
    }
    let ctx = Ctx::new(model, keep);
    let modules = std::slice::from_ref(module);
    let mut outs = vec![sheet::layout_module(&ctx, module)];
    number_power(&ctx, modules, &mut outs);
    let o = &mut outs[0];
    let sec = SchematicSection {
        symbols: std::mem::take(&mut o.items.symbols),
        wires: std::mem::take(&mut o.items.wires),
        labels: std::mem::take(&mut o.items.labels),
        power_symbols: std::mem::take(&mut o.items.power),
        no_connects: std::mem::take(&mut o.items.ncs),
        user_fields: keep.user_fields.clone(),
        erc_exclusions: keep.erc_exclusions.clone(),
        erc_pin_map: keep.erc_pin_map.clone(),
        title_block: Some(TitleBlock { paper: o.paper.name.to_string(), ..keep.title_block.clone().unwrap_or_default() }),
        extras: SchExtras { locked: locked_among(keep, &module.refs), ..Default::default() },
        ..Default::default()
    };
    let mut design = Design {
        schema: 1,
        provenance: Provenance { engine_version: opts.engine_version.clone(), intent_hash: opts.intent_hash.clone(), seed: opts.seed, stage_hashes: Vec::new() },
        schematic: Some(sec),
        nets: None,
        placement: None,
        routing: None,
        drawings: None,
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
    };
    design.assign_missing_ids();
    if let Some(sec) = design.schematic.as_mut() {
        crate::fields::fill_layout(sec, model);
    }
    Ok(design)
}

/// The sheets of a hierarchical schematic and the paper each is drawn on.
pub struct Derived {
    pub design: Design,
    pub papers: BTreeMap<String, Paper>,
}

/// Derive a root sheet and one sheet per module.
pub fn derive_hierarchy(model: &ConstraintModel, opts: &EngineOptions, modules: &[FunctionalModule], keep: &Keep) -> Result<Design, Vec<CheckResult>> {
    derive_hierarchy_full(model, opts, modules, keep).map(|d| d.design)
}

pub fn derive_hierarchy_full(model: &ConstraintModel, opts: &EngineOptions, modules: &[FunctionalModule], keep: &Keep) -> Result<Derived, Vec<CheckResult>> {
    let errors = validate(model);
    if !errors.is_empty() {
        return Err(errors);
    }
    let ctx = Ctx::new(model, keep);

    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut files: Vec<String> = Vec::new();
    for m in modules {
        let f = sheet_file_name(&m.name, &taken);
        taken.insert(f.clone());
        files.push(f);
    }
    let crossing: Vec<Vec<String>> = modules.iter().map(|m| sheet::crossing_nets(&ctx, m)).collect();

    let mut outs: Vec<sheet::SheetOut> = modules.iter().map(|m| sheet::layout_module(&ctx, m)).collect();
    number_power(&ctx, modules, &mut outs);

    let root = root::layout_root(modules, &files, &crossing);

    // ---- assemble ----
    let mut papers: BTreeMap<String, Paper> = BTreeMap::new();
    let mut contents: BTreeMap<String, SchematicSection> = BTreeMap::new();
    for (mi, m) in modules.iter().enumerate() {
        let o = &mut outs[mi];
        let user_fields: BTreeMap<String, BTreeMap<String, String>> = m.refs.iter().filter_map(|r| keep.user_fields.get(r).map(|f| (r.clone(), f.clone()))).collect();
        let sec = SchematicSection {
            symbols: std::mem::take(&mut o.items.symbols),
            wires: std::mem::take(&mut o.items.wires),
            labels: std::mem::take(&mut o.items.labels),
            power_symbols: std::mem::take(&mut o.items.power),
            no_connects: std::mem::take(&mut o.items.ncs),
            user_fields,
            title_block: Some(TitleBlock { title: m.name.clone(), paper: o.paper.name.to_string(), ..Default::default() }),
            extras: SchExtras { locked: locked_among(keep, &m.refs), ..Default::default() },
            ..Default::default()
        };
        papers.insert(files[mi].clone(), o.paper);
        contents.insert(files[mi].clone(), sec);
    }
    let mut root_items = root.items;
    let title_block = Some(match keep.title_block.clone() {
        Some(mut tb) => {
            tb.paper = root.paper.name.to_string();
            tb
        }
        None => TitleBlock { paper: root.paper.name.to_string(), ..Default::default() },
    });
    let root_section = SchematicSection {
        wires: std::mem::take(&mut root_items.wires),
        labels: std::mem::take(&mut root_items.labels),
        sheets: root.sheets,
        erc_exclusions: keep.erc_exclusions.clone(),
        erc_pin_map: keep.erc_pin_map.clone(),
        title_block,
        ..Default::default()
    };
    papers.insert(String::new(), root.paper);

    let mut design = Design {
        schema: 1,
        provenance: Provenance { engine_version: opts.engine_version.clone(), intent_hash: opts.intent_hash.clone(), seed: opts.seed, stage_hashes: Vec::new() },
        schematic: Some(root_section),
        nets: None,
        placement: None,
        routing: None,
        drawings: None,
        footprint_library: None,
        sheet_contents: Some(contents),
        bus_aliases: vec![],
        symbol_library: None,
    };
    design.assign_missing_ids();
    if let Some(sec) = design.schematic.as_mut() {
        crate::fields::fill_layout(sec, model);
    }
    for sec in design.sheet_contents.iter_mut().flat_map(|c| c.values_mut()) {
        crate::fields::fill_layout(sec, model);
    }
    Ok(Derived { design, papers })
}

/// The module-sheet schematic of `model`: modules inferred from the netlist (or taken from `recorded`, a placement's modules);
/// a design with only one module is one sheet, laid out the same way. A model with no parts at all gets `derive_schematic`'s
/// empty one.
pub fn derive_schematic_modules_with(model: &ConstraintModel, opts: &EngineOptions, recorded: &[eda_model::ir::ModuleRegion]) -> Result<Design, Vec<CheckResult>> {
    let modules = infer_modules(model, recorded);
    match modules.len() {
        0 => crate::derive_schematic(model, opts),
        1 => derive_single_sheet(model, opts, &modules[0], &Keep::default()),
        _ => derive_hierarchy(model, opts, &modules, &Keep::default()),
    }
}

//! What Reorganize into Module Sheets keeps of a symbol's look, and how it lays out a multi-unit part.
//!
//! The module layout (`eda_engine::derive_hierarchy`) knows one placed symbol per reference. A part whose library symbol has several
//! units is placed as several symbols of one reference, each drawing its own pins (`SchematicSection::symbols` holds one entry per
//! `(id, unit)`), so for the layout every placed unit is a part of its own, named by [`field_key`] (`U1`, `U1#2`, `U1#3`) and holding only
//! the pins of that unit; the nets name those. [`split`] makes that model, [`expand_modules`] puts the units in the module their part is
//! in, and [`restore`] turns what the layout drew back into the one reference with its units -- the symbols, the pins the wires, power
//! symbols and no-connect flags name, and everything kept per unit.
//!
//! The rest of the look a symbol had is carried the same way: the body style it was drawn in, where its fields were put when the user put
//! them ([`Split::fields`]; fields Autoplace Fields placed are placed again round the symbol where it is now, but keep their size, weight
//! and visibility -- [`carry_field_looks`]), and the look of a label ([`carry_label_looks`], by the net it names).

use eda_engine::Keep;
use eda_model::ir::{field_key, FieldPlacement, SchematicSection, SymbolInstance};
use eda_model::modules::{natural_cmp, FunctionalModule};
use eda_model::sch_extras::LabelLook;
use eda_model::{ConstraintModel, Net, Part};
use std::collections::{BTreeMap, BTreeSet};

/// The model the layout draws, and what it needs to know about each part of it.
pub(crate) struct Split {
    /// `model` with every multi-unit part replaced by one part per placed unit, and the nets naming those.
    pub model: ConstraintModel,
    /// Each part of `model` as it was placed on the sheet being reorganized (the unit's own instance for a split part).
    pub symbols: BTreeMap<String, SymbolInstance>,
    pub units: BTreeMap<String, u32>,
    pub body_styles: BTreeMap<String, u32>,
    /// The places of the fields of the symbols whose fields the user placed.
    pub fields: BTreeMap<String, Vec<FieldPlacement>>,
    /// A part of `model` made from a unit -> the reference it comes from and the unit.
    pub back: BTreeMap<String, (String, u32)>,
    /// A reference that was split -> the parts of `model` made from it, unit 1 first.
    pub expand: BTreeMap<String, Vec<String>>,
}

/// Is the symbol's fields' place the user's own? (An entry in the layout that Autoplace Fields did not put there.)
fn hand_placed(root: &SchematicSection, sym: &SymbolInstance) -> Option<Vec<FieldPlacement>> {
    let key = field_key(&sym.id, sym.unit);
    root.field_layout.get(&key).filter(|_| !root.extras.fields_autoplaced.contains_key(&key)).cloned()
}

/// Split the multi-unit parts of `model` by the units `root` has placed, and take the nets from `nets`.
pub(crate) fn split(model: &ConstraintModel, nets: &[Net], root: &SchematicSection) -> Split {
    let mut out = Split {
        model: model.clone(),
        symbols: BTreeMap::new(),
        units: BTreeMap::new(),
        body_styles: BTreeMap::new(),
        fields: BTreeMap::new(),
        back: BTreeMap::new(),
        expand: BTreeMap::new(),
    };
    out.model.parts.clear();
    // reference -> pin number -> the part of the new model that owns the pin
    let mut owner_of: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let record = |out: &mut Split, name: &str, inst: &SymbolInstance| {
        out.symbols.insert(name.to_string(), inst.clone());
        let style = root.body_style_of(inst);
        if style > 1 {
            out.body_styles.insert(name.to_string(), style);
        }
        if let Some(f) = hand_placed(root, inst) {
            out.fields.insert(name.to_string(), f);
        }
    };
    for part in &model.parts {
        let mut instances: Vec<&SymbolInstance> = root.symbols.iter().filter(|s| s.id == part.reference).collect();
        instances.sort_by_key(|s| s.unit);
        let lib = instances.first().and_then(|s| model.real_symbol_of(&s.lib_id, part));
        let multi = lib.as_ref().is_some_and(|l| l.unit_count > 1);
        let (Some(lib), true) = (lib, multi) else {
            out.model.parts.push(part.clone());
            if let Some(inst) = instances.first() {
                record(&mut out, &part.reference, inst);
            }
            continue;
        };
        let placed: Vec<u32> = instances.iter().map(|s| s.unit).collect();
        // a pin belongs to the first placed unit that has it; a pin every unit draws (unit 0) to the first unit placed
        let mut pin_unit: BTreeMap<String, u32> = BTreeMap::new();
        for pin in &part.pins {
            let units: Vec<u32> = lib.pins.iter().filter(|p| p.number == pin.number).map(|p| p.unit).collect();
            let owner = units.iter().copied().find(|u| *u != 0 && placed.contains(u)).or_else(|| units.contains(&0).then(|| placed[0]));
            if let Some(u) = owner {
                pin_unit.insert(pin.number.clone(), u);
            }
        }
        let mut names = Vec::new();
        for inst in &instances {
            let name = field_key(&part.reference, inst.unit);
            let pins = part.pins.iter().filter(|p| pin_unit.get(&p.number) == Some(&inst.unit)).cloned().collect();
            out.model.parts.push(Part { reference: name.clone(), pins, ..part.clone() });
            out.back.insert(name.clone(), (part.reference.clone(), inst.unit));
            out.units.insert(name.clone(), inst.unit);
            record(&mut out, &name, inst);
            for (number, u) in &pin_unit {
                if *u == inst.unit {
                    owner_of.entry(part.reference.clone()).or_default().insert(number.clone(), name.clone());
                }
            }
            names.push(name);
        }
        out.expand.insert(part.reference.clone(), names);
    }
    out.model.nets = nets
        .iter()
        .map(|n| Net {
            name: n.name.clone(),
            pins: n
                .pins
                .iter()
                .filter_map(|pin| {
                    let (r, number) = pin.split_once('.').unwrap_or((pin.as_str(), ""));
                    match owner_of.get(r) {
                        Some(owners) => owners.get(number).map(|name| format!("{name}.{number}")),
                        None if out.expand.contains_key(r) => None,
                        None => Some(pin.clone()),
                    }
                })
                .collect(),
        })
        .collect();
    out
}

impl Split {
    /// The `Keep` the layout is given: what the sheet had, by the names of `model`.
    pub(crate) fn keep(&self, root: &SchematicSection) -> Keep {
        let real = |name: &str| self.back.get(name).map_or_else(|| name.to_string(), |b| b.0.clone());
        let mut user_fields = root.user_fields.clone();
        for name in self.back.keys() {
            if let Some(f) = root.user_fields.get(&real(name)) {
                user_fields.insert(name.clone(), f.clone());
            }
        }
        Keep {
            locked: self.symbols.keys().filter(|name| root.extras.locked.contains(&real(name))).cloned().collect(),
            symbols: self.symbols.clone(),
            user_fields,
            erc_exclusions: root.erc_exclusions.clone(),
            erc_pin_map: root.erc_pin_map.clone(),
            title_block: root.title_block.clone(),
            units: self.units.clone(),
            body_styles: self.body_styles.clone(),
            fields: self.fields.clone(),
        }
    }
}

/// Modules made of references, with each split reference replaced by its units: the units of a part are in the module the part is in.
pub(crate) fn expand_modules(modules: &[FunctionalModule], expand: &BTreeMap<String, Vec<String>>) -> Vec<FunctionalModule> {
    let spread = |refs: &[String]| -> Vec<String> { refs.iter().flat_map(|r| expand.get(r).cloned().unwrap_or_else(|| vec![r.clone()])).collect() };
    modules
        .iter()
        .map(|m| {
            let mut refs = spread(&m.refs);
            refs.sort_by(|a, b| natural_cmp(a, b));
            FunctionalModule {
                name: m.name.clone(),
                kind: m.kind,
                lead: m.lead.as_ref().map(|l| expand.get(l).and_then(|v| v.first().cloned()).unwrap_or_else(|| l.clone())),
                refs,
                channels: m.channels.iter().map(|c| spread(c)).collect(),
            }
        })
        .collect()
}

/// A section the layout drew, turned back: a part of the layout's model that is one unit of a reference is that reference's unit again.
pub(crate) fn restore(sec: &mut SchematicSection, back: &BTreeMap<String, (String, u32)>) {
    if back.is_empty() {
        return;
    }
    // what is kept per placed unit moves to the key of the reference (taken out first, put back after: the names can swap places)
    let mut layout = Vec::new();
    let mut autoplaced = Vec::new();
    let mut styles = Vec::new();
    let mut path_units = Vec::new();
    let mut pos_excluded = Vec::new();
    for s in sec.symbols.iter_mut() {
        let Some((real, unit)) = back.get(&s.id) else { continue };
        let (old, new) = (field_key(&s.id, s.unit), field_key(real, *unit));
        if let Some(v) = sec.field_layout.remove(&old) {
            layout.push((new.clone(), v));
        }
        if let Some(v) = sec.extras.fields_autoplaced.remove(&old) {
            autoplaced.push((new.clone(), v));
        }
        if let Some(v) = sec.extras.body_styles.remove(&old) {
            styles.push((new.clone(), v));
        }
        if let Some(v) = sec.extras.path_units.remove(&old) {
            path_units.push((new.clone(), v));
        }
        if sec.extras.excluded_from_pos_files.remove(&old) {
            pos_excluded.push(new);
        }
        s.id = real.clone();
        s.unit = *unit;
    }
    sec.field_layout.extend(layout);
    sec.extras.fields_autoplaced.extend(autoplaced);
    sec.extras.body_styles.extend(styles);
    sec.extras.path_units.extend(path_units);
    sec.extras.excluded_from_pos_files.extend(pos_excluded);
    sec.symbols.sort_by(|a, b| (&a.id, a.unit).cmp(&(&b.id, b.unit)));
    // the pins the drawing names
    let rename_pin = |pin: &mut String| {
        if let Some((r, n)) = pin.split_once('.') {
            if let Some((real, _)) = back.get(r) {
                *pin = format!("{real}.{n}");
            }
        }
    };
    sec.wires.iter_mut().flat_map(|w| w.pins.iter_mut()).for_each(rename_pin);
    sec.power_symbols.iter_mut().for_each(|p| rename_pin(&mut p.pin));
    sec.no_connects.iter_mut().for_each(|n| rename_pin(&mut n.pin));
    // the part-wide things are kept under the reference
    let mut fields = BTreeMap::new();
    for (k, v) in std::mem::take(&mut sec.user_fields) {
        let k = back.get(&k).map_or(k, |b| b.0.clone());
        fields.entry(k).or_insert(v);
    }
    sec.user_fields = fields;
    let locked: BTreeSet<String> = sec.extras.locked.iter().map(|l| back.get(l).map_or_else(|| l.clone(), |b| b.0.clone())).collect();
    sec.extras.locked = locked.into_iter().collect();
}

/// The fields Autoplace Fields placed round a symbol keep how they were set -- their size, weight, whether they are shown and called by their
/// name -- from the sheet the symbol came from. Their places are the new ones: the symbol is somewhere else now.
pub(crate) fn carry_field_looks(old: &SchematicSection, new: &mut SchematicSection) {
    for s in &new.symbols {
        let key = field_key(&s.id, s.unit);
        let Some(was) = old.field_layout.get(&key) else { continue };
        // the user's own places came across whole; a field Autoplace Fields placed is the one that takes the old look
        if !old.extras.fields_autoplaced.contains_key(&key) {
            continue;
        }
        let Some(now) = new.field_layout.get_mut(&key) else { continue };
        for f in now.iter_mut() {
            if let Some(w) = was.iter().find(|w| w.name == f.name) {
                f.visible = w.visible;
                f.size_nm = w.size_nm;
                f.bold = w.bold;
                f.italic = w.italic;
                f.name_shown = w.name_shown;
            }
        }
    }
}

/// A label on the new sheets takes the look (size, bold, italic) the labels of its net had: a net's labels were set in one look, or the
/// first one's (by id) is the net's. The spin is not carried: a label is drawn at the end of a pin's stub, facing away from the part.
pub(crate) fn carry_label_looks(old: &SchematicSection, new: &mut [&mut SchematicSection]) {
    let mut by_net: BTreeMap<&str, LabelLook> = BTreeMap::new();
    let mut labels: Vec<&eda_model::ir::NetLabel> = old.labels.iter().collect();
    labels.sort_by(|a, b| a.id.cmp(&b.id));
    for l in labels {
        if let Some(look) = old.extras.label_looks.get(&l.id).filter(|k| !k.is_default()) {
            by_net.entry(l.net.as_str()).or_insert(*look);
        }
    }
    if by_net.is_empty() {
        return;
    }
    for sec in new.iter_mut() {
        let ids: Vec<(String, LabelLook)> = sec.labels.iter().filter_map(|l| by_net.get(l.net.as_str()).map(|k| (l.id.clone(), *k))).collect();
        sec.extras.label_looks.extend(ids);
    }
}

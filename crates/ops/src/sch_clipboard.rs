//! `Cmd::PasteSch`: the schematic clipboard's Paste, Paste Special and Duplicate -- one verb that adds a whole fragment to the sheet in view.
//!
//! Ported from `SCH_EDITOR_CONTROL::Paste` (`eeschema/tools/sch_editor_control.cpp`): the library symbols of the fragment are looked up in the
//! design first and taken from the fragment only when the design has none (`currentScreen->GetLibSymbols()` before `tempScreen`'s), every item
//! gets a new identity and a lock it had is dropped (`schItem->SetLocked( false )`), the pasted symbols are numbered by
//! [`annotate_paste`](eda_model::sch_clipboard::annotate_paste), and the whole paste is one commit (`commit.Push( _( "Paste" ) )`) -- here, one
//! command, so one undo step.
//!
//! The fragment is in KiCad's frame (a symbol's `at` is its library origin). A sheet this project drew places a symbol by the corner of its
//! box instead, so a symbol pasted there is moved from KiCad's origin to that corner -- the inverse of what the writer does on copy.
//! The translation `(dx, dy)` is what the cursor carried the items by (the move that follows the paste in KiCad), applied last.

use super::Board;
use eda_model::ir::{LibrarySymbol, NetLabel, NoConnect, Point, PowerSymbol, SchLine, SchematicSection, SchematicText, SymbolInstance, Nm, Wire};
use eda_model::sch_clipboard::{annotate_paste, place_offset_nm, translate_graphic, PastedRef, PasteMode, SchFragment, UsedRef};
use eda_model::symbol::{is_synthetic_lib_id, LibSymbol};
use eda_model::CheckResult;
use std::collections::BTreeMap;

impl<'a> Board<'a> {
    /// Every placed symbol of every sheet -- power symbols included -- as the numbering sees it (`hierarchy.GetSymbols( existingRefs, SYMBOL_FILTER_ALL )`).
    pub(crate) fn used_references(&self) -> Vec<UsedRef> {
        let mut out: Vec<UsedRef> = Vec::new();
        let reachable = eda_model::sheet_list::reachable_files(&self.design);
        let sheets = self.design.schematic.iter().chain(self.design.sheet_contents.iter().flat_map(|c| c.iter().filter(|(f, _)| reachable.contains(*f)).map(|(_, s)| s)));
        for sch in sheets {
            for s in &sch.symbols {
                let value = if s.value.is_empty() { self.model.part(&s.id).and_then(|p| p.value.clone()).unwrap_or_default() } else { s.value.clone() };
                out.push(UsedRef { reference: s.id.clone(), unit: s.unit, lib_id: s.lib_id.clone(), value });
            }
            for p in &sch.power_symbols {
                out.push(UsedRef { reference: p.id.clone(), unit: 1, lib_id: p.lib_id.clone(), value: p.net.clone() });
            }
        }
        // a screen shown twice holds the references its other placements give its symbols too (`SCH_SHEET_LIST::GetSymbols` walks every path)
        for e in eda_model::sheet_list::walk(&self.design).into_iter().skip(1) {
            let Some(sec) = eda_model::sheet_list::screen_of(&self.design, &e.path) else { continue };
            for s in &sec.symbols {
                let reference = eda_model::sheet_list::reference_under(sec, s, e.path.last().map(String::as_str));
                if reference != s.id {
                    out.push(UsedRef { reference, unit: s.unit, lib_id: s.lib_id.clone(), value: s.value.clone() });
                }
            }
        }
        out
    }

    /// The library symbol the design draws `lib_id` from, if it has one at all: a real library's, a built-in, or one published into the project.
    pub(crate) fn design_symbol(&self, lib_id: &str) -> Option<LibSymbol> {
        if lib_id.is_empty() || is_synthetic_lib_id(lib_id) {
            return None;
        }
        if let Some(s) = self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)).filter(|s| s.published) {
            return Some(s.to_engine_symbol());
        }
        self.model.symbol_of(lib_id)
    }

    /// The library symbols a set of items names, as the design is to have them: the design's own win; a symbol it lacks comes in from `provided`
    /// (a clipboard's `(lib_symbols ...)`, a file's), published under its own name -- or, when the design has another symbol of that name or the
    /// symbol is a generated box (`eda:<ref>`, named for a part this design does not have), under a name of its own. Returns the name each
    /// `lib_id` goes by now, the engine's symbol for it, and the entries to publish.
    pub(crate) fn merge_lib_symbols(&self, wanted: &[String], provided: &[LibrarySymbol]) -> LibMerge {
        let mut lib_name: BTreeMap<String, String> = BTreeMap::new();
        let mut engine_of: BTreeMap<String, LibSymbol> = BTreeMap::new();
        let mut publish: Vec<LibrarySymbol> = Vec::new();
        for lib_id in wanted.iter().map(String::as_str) {
            if let Some(known) = self.design_symbol(lib_id) {
                // The design's own wins -- unless the definition that comes with the items draws its pins elsewhere (a file this project wrote bakes a
                // library symbol shifted to its box's corner, under the library's name): the items' wires end where THEIR pins are, so they bring
                // their definition, under a name the design does not use.
                if !provided.iter().any(|t| t.lib_id == lib_id && !same_pins(t, &known)) {
                    lib_name.insert(lib_id.to_string(), lib_id.to_string());
                    engine_of.insert(lib_id.to_string(), known);
                    continue;
                }
            }
            let Some(theirs) = provided.iter().find(|s| s.lib_id == lib_id).map(|t| {
                let mut t = t.clone();
                t.assign_missing_ids();
                t
            }) else {
                // Nothing defines it: KiCad pastes it as a symbol with a broken library link; the sheet draws it as a box.
                lib_name.insert(lib_id.to_string(), lib_id.to_string());
                continue;
            };
            // A generated symbol is a box drawn from its part's pins and named `eda:<ref>`; the design has no part for the copy yet, so it
            // gets a library symbol of its own under a real name.
            let base = if is_synthetic_lib_id(lib_id) { format!("clipboard:{}", lib_id.rsplit(':').next().unwrap_or(lib_id)) } else { lib_id.to_string() };
            let published_before = self.design.symbol_library.iter().flat_map(|l| l.symbols.iter()).chain(publish.iter());
            // Another box that draws exactly the same (two resistors' generated boxes do) is the same library symbol.
            let twin = if is_synthetic_lib_id(lib_id) { published_before.clone().find(|e| e.lib_id.starts_with("clipboard:") && same_drawing(e, &theirs)).map(|e| e.lib_id.clone()) } else { None };
            let mut name = twin.unwrap_or_else(|| base.clone());
            let mut n = 2;
            loop {
                let existing = self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(&name)).or_else(|| publish.iter().find(|p| p.lib_id == name));
                let clash = match existing {
                    Some(e) => !same_drawing(e, &theirs),
                    // a library symbol the design has but has not published (a name of an installed library): a different drawing may not take its name.
                    // Only the name the items came with is such a name: a `_N` this loop makes is judged by the design's own library alone (the built-in
                    // symbols answer every `power:<net>`, so `power:PWR_FLAG_2`, `_3`... would each be "taken" and the loop would never end).
                    None => name == base && self.design_symbol(&name).is_some_and(|k| !same_pins(&theirs, &k)),
                };
                if !clash {
                    break;
                }
                name = format!("{base}_{n}");
                n += 1;
            }
            let already = self.design.symbol_library.as_ref().is_some_and(|l| l.by_lib_id(&name).is_some()) || publish.iter().any(|p| p.lib_id == name);
            let mut ours = theirs.clone();
            ours.lib_id = name.clone();
            ours.published = true;
            engine_of.insert(lib_id.to_string(), ours.to_engine_symbol());
            lib_name.insert(lib_id.to_string(), name);
            if !already {
                publish.push(ours);
            }
        }
        LibMerge { lib_name, engine_of, publish }
    }

    /// `Cmd::PasteSch` -- see the module doc. Returns what the paste put on the sheet at its top: the ids of the new items that are in no new group and of the new groups that
    /// are in no other (what `SCH_SELECTION` then holds, and what Place as group groups).
    pub(crate) fn paste_sch(&mut self, fragment: &SchFragment, dx: Nm, dy: Nm, mode: PasteMode) -> Result<Vec<String>, Vec<CheckResult>> {
        if fragment.is_empty() {
            return Err(vec![CheckResult::fail("ops_empty_paste", "paste", "there is nothing to paste")]);
        }
        let src = &fragment.section;
        let imported = self.schematic_mut_or_create().imported_from_kicad;

        // ---- the library symbols: the design's own win; the fragment's come in only for a name the design lacks ----
        let mut wanted: Vec<String> = src.symbols.iter().map(|s| s.lib_id.clone()).chain(src.power_symbols.iter().map(|p| p.lib_id.clone())).collect();
        wanted.sort_unstable();
        wanted.dedup();
        let LibMerge { lib_name, engine_of, publish } = self.merge_lib_symbols(&wanted, &fragment.lib_symbols);

        // ---- the numbers ----
        let units_of = |lib_id: &str| fragment.lib_symbol(lib_id).map(|l| l.unit_count).or_else(|| engine_of.get(lib_id).map(|e| e.unit_count)).unwrap_or(1);
        let mut pasted: Vec<PastedRef> = Vec::new();
        for s in &src.symbols {
            pasted.push(PastedRef { reference: s.id.clone(), unit: s.unit, unit_count: units_of(&s.lib_id), lib_id: s.lib_id.clone(), value: s.value.clone(), at: (s.at.x, s.at.y), power: false });
        }
        for p in &src.power_symbols {
            pasted.push(PastedRef { reference: p.id.clone(), unit: 1, unit_count: 1, lib_id: p.lib_id.clone(), value: p.net.clone(), at: (p.at.x, p.at.y), power: true });
        }
        let used = self.used_references();
        let numbered = annotate_paste(&used, &pasted, mode);
        let (symbol_refs, power_refs) = numbered.split_at(src.symbols.len());
        for (r, p) in symbol_refs.iter().zip(&src.symbols) {
            if used.iter().any(|u| u.reference == *r && u.unit == p.unit) {
                return Err(vec![CheckResult::fail("ops_duplicate_symbol", r, format!("reference '{r}' already has unit {} in the design", p.unit))]);
            }
        }

        // ---- the items ----
        let mut new_symbols: Vec<SymbolInstance> = Vec::new();
        for (s, reference) in src.symbols.iter().zip(symbol_refs) {
            let target = lib_name.get(&s.lib_id).cloned().unwrap_or_else(|| s.lib_id.clone());
            // Where the sheet wants `at`: KiCad's origin, or (a sheet this project drew) the corner of the symbol's box.
            let origin = Point { x: s.at.x + dx, y: s.at.y + dy };
            let at = match engine_of.get(&s.lib_id) {
                Some(engine) if !imported => {
                    // the box of the body style the symbol was copied in
                    let drawn = if src.body_style_of(s) > 1 && engine.has_alternate_body() { engine.in_style(src.body_style_of(s)).into_owned() } else { engine.clone() };
                    let (x0, _, _, y1) = eda_engine::geometry::real_symbol_bbox(&drawn, s.unit);
                    let (ox, oy) = place_offset_nm(s.rot as f64 / 1000.0, s.mirrored, s.mirror_y, (-x0, -y1));
                    Point { x: origin.x - ox, y: origin.y - oy }
                }
                _ => origin,
            };
            new_symbols.push(SymbolInstance { id: reference.clone(), at, lib_id: target, ..s.clone() });
        }
        let mut new_powers: Vec<PowerSymbol> = Vec::new();
        for (p, reference) in src.power_symbols.iter().zip(power_refs) {
            let target = lib_name.get(&p.lib_id).cloned().unwrap_or_else(|| p.lib_id.clone());
            new_powers.push(PowerSymbol { id: reference.clone(), lib_id: target, at: Point { x: p.at.x + dx, y: p.at.y + dy }, rot: p.rot, net: p.net.clone(), pin: String::new() });
        }
        let shift = |p: &Point| Point { x: p.x + dx, y: p.y + dy };
        // the ids the pasted items had in the fragment, in the order they are added (a group of the fragment names its members by them)
        let wire_olds: Vec<String> = src.wires.iter().filter(|w| w.pts.len() >= 2).map(|w| w.id.clone()).collect();
        let new_wires: Vec<Wire> = src.wires.iter().filter(|w| w.pts.len() >= 2).map(|w| Wire { id: String::new(), net: String::new(), pins: Vec::new(), pts: w.pts.iter().map(shift).collect(), bus: w.bus }).collect();
        let new_labels: Vec<NetLabel> = src.labels.iter().map(|l| NetLabel { id: String::new(), net: l.net.clone(), at: shift(&l.at), kind: l.kind.clone() }).collect();
        let new_texts: Vec<SchematicText> = src.texts.iter().map(|t| SchematicText { id: String::new(), content: t.content.clone(), at: shift(&t.at), angle: t.angle, size_nm: t.size_nm }).collect();
        let new_ncs: Vec<NoConnect> = src.no_connects.iter().map(|n| NoConnect { id: String::new(), at: shift(&n.at), pin: String::new() }).collect();
        let new_entries: Vec<eda_model::ir::BusEntry> = src.bus_entries.iter().map(|b| eda_model::ir::BusEntry { id: String::new(), at: shift(&b.at), size: b.size }).collect();
        let new_lines: Vec<SchLine> = src.lines.iter().map(|l| SchLine { id: String::new(), pts: l.pts.iter().map(shift).collect(), width_nm: l.width_nm }).collect();
        // A junction the pasted wires already imply (a dot at a T) is not stored: the sheet draws it from the wires.
        let implied: Vec<Point> = eda_engine::geometry::wire_junction_points(&new_wires).into_iter().map(|(_, p)| p).collect();
        let new_junctions: Vec<eda_model::ir::Junction> = src.junctions.iter().map(|j| shift(&j.at)).filter(|at| !implied.contains(at)).map(|at| eda_model::ir::Junction { id: String::new(), at }).collect();
        let mut new_graphics = src.extras.graphics.clone();
        for g in &mut new_graphics {
            g.id.clear();
            translate_graphic(&mut g.shape, dx, dy);
        }

        // ---- commit them to the sheet ----
        if !publish.is_empty() {
            let lib = self.design.symbol_library.get_or_insert_with(Default::default);
            lib.symbols.extend(publish);
            lib.symbols.sort_by(|a, b| a.lib_id.cmp(&b.lib_id));
        }
        let user_fields: Vec<(String, BTreeMap<String, String>)> = src
            .symbols
            .iter()
            .zip(symbol_refs)
            .filter_map(|(s, new)| src.user_fields.get(&s.id).map(|f| (new.clone(), f.clone())))
            .collect();
        let sch: &mut SchematicSection = self.schematic_mut_or_create();
        let (first_wire, first_nc, first_entry, first_line, first_graphic) = (sch.wires.len(), sch.no_connects.len(), sch.bus_entries.len(), sch.lines.len(), sch.extras.graphics.len());
        let junctions_before: Vec<Point> = sch.junctions.iter().map(|j| j.at).collect();
        sch.symbols.extend(new_symbols);
        sch.symbols.sort_by(|a, b| (&a.id, a.unit).cmp(&(&b.id, b.unit)));
        sch.power_symbols.extend(new_powers);
        sch.wires.extend(new_wires);
        let first_label = sch.labels.len();
        sch.labels.extend(new_labels);
        let first_text = sch.texts.len();
        sch.texts.extend(new_texts);
        sch.no_connects.extend(new_ncs);
        sch.bus_entries.extend(new_entries);
        sch.lines.extend(new_lines);
        for j in new_junctions {
            if !sch.junctions.iter().any(|x| x.at == j.at) {
                sch.junctions.push(j);
            }
        }
        sch.junctions.sort_by_key(|j| j.at);
        sch.extras.graphics.extend(new_graphics);
        for (reference, fields) in user_fields {
            sch.user_fields.entry(reference).or_insert(fields);
        }
        sch.assign_missing_ids();
        // which item each id of the fragment became
        let mut remap: BTreeMap<String, String> = BTreeMap::new();
        for (s, new) in src.symbols.iter().zip(symbol_refs) {
            remap.insert(s.id.clone(), new.clone());
        }
        for (p, new) in src.power_symbols.iter().zip(power_refs) {
            remap.insert(p.id.clone(), new.clone());
        }
        for (k, old) in wire_olds.iter().enumerate() {
            if let Some(w) = sch.wires.get(first_wire + k) {
                remap.insert(old.clone(), w.id.clone());
            }
        }
        for (k, l) in src.labels.iter().enumerate() {
            if let Some(n) = sch.labels.get(first_label + k) {
                remap.insert(l.id.clone(), n.id.clone());
            }
        }
        for (k, t) in src.texts.iter().enumerate() {
            if let Some(n) = sch.texts.get(first_text + k) {
                remap.insert(t.id.clone(), n.id.clone());
            }
        }
        for (k, n) in src.no_connects.iter().enumerate() {
            if let Some(new) = sch.no_connects.get(first_nc + k) {
                remap.insert(n.id.clone(), new.id.clone());
            }
        }
        for (k, b) in src.bus_entries.iter().enumerate() {
            if let Some(new) = sch.bus_entries.get(first_entry + k) {
                remap.insert(b.id.clone(), new.id.clone());
            }
        }
        for (k, l) in src.lines.iter().enumerate() {
            if let Some(new) = sch.lines.get(first_line + k) {
                remap.insert(l.id.clone(), new.id.clone());
            }
        }
        for (k, g) in src.extras.graphics.iter().enumerate() {
            if let Some(new) = sch.extras.graphics.get(first_graphic + k) {
                remap.insert(g.id.clone(), new.id.clone());
            }
        }
        // a junction the sheet already had is not a pasted one; one the pasted wires imply is not stored
        for j in &src.junctions {
            let at = shift(&j.at);
            if !junctions_before.contains(&at) {
                if let Some(new) = sch.junctions.iter().find(|n| n.at == at) {
                    remap.insert(j.id.clone(), new.id.clone());
                }
            }
        }
        // a pasted symbol keeps the body style it was copied in (`SCH_SYMBOL::GetBodyStyle`)
        for (s, new) in src.symbols.iter().zip(symbol_refs) {
            let style = src.body_style_of(s);
            if style > 1 && engine_of.get(&s.lib_id).is_some_and(|e| e.has_alternate_body()) {
                sch.extras.body_styles.insert(eda_model::ir::field_key(new, s.unit), style);
            }
        }
        // a pasted symbol keeps the places Swap Pins gave its pins (`SchExtras::pin_swaps`; the clipboard names the symbol's own copy of its library symbol, `lib_name`)
        for (s, new) in src.symbols.iter().zip(symbol_refs) {
            if let Some(swaps) = src.extras.pin_swaps_of(&eda_model::ir::field_key(&s.id, s.unit)) {
                sch.extras.pin_swaps.insert(eda_model::ir::field_key(new, s.unit), swaps.clone());
            }
        }
        // a pasted label keeps its spin, size, bold and italic, and its fields
        for (k, l) in src.labels.iter().enumerate() {
            let Some(new_id) = sch.labels.get(first_label + k).map(|n| n.id.clone()) else { continue };
            if let Some(spin) = src.extras.label_spins.get(&l.id) {
                sch.extras.label_spins.insert(new_id.clone(), *spin);
            }
            if let Some(look) = src.extras.label_looks.get(&l.id) {
                sch.extras.label_looks.insert(new_id.clone(), *look);
            }
            // and its own fields (its net class, ...)
            if let Some(fields) = src.extras.label_fields.get(&l.id) {
                sch.extras.label_fields.insert(new_id, fields.clone());
            }
        }
        // a pasted text keeps its justification
        for (k, t) in src.texts.iter().enumerate() {
            let Some(new_id) = sch.texts.get(first_text + k).map(|n| n.id.clone()) else { continue };
            if let Some(j) = src.extras.text_justify.get(&t.id) {
                sch.extras.text_justify.insert(new_id, *j);
            }
        }
        // hierarchical sheets: each a new sheet, with copies of the screens it shows (the sheet part of `SCH_EDITOR_CONTROL::Paste`)
        if !src.sheets.is_empty() {
            let first_sheet = self.schematic()?.sheets.len();
            self.paste_sheets(fragment, dx, dy, mode)?;
            // they are added by name, naturally (`sortedLoadedItems`)
            let mut by_name: Vec<&eda_model::ir::SheetInstance> = src.sheets.iter().collect();
            by_name.sort_by(|a, b| eda_model::sheet_list::str_num_cmp(&a.name, &b.name));
            let added: Vec<String> = self.schematic()?.sheets.iter().skip(first_sheet).map(|s| s.id.clone()).collect();
            for (old, new) in by_name.iter().zip(added) {
                remap.insert(old.id.clone(), new);
            }
        }
        let new_groups = self.paste_groups(fragment, &remap)?;
        let tree = self.schematic()?.extras.group_tree();
        let mut top: Vec<String> = Vec::new();
        for id in remap.values().chain(new_groups.iter()) {
            if tree.parent_group(id).is_none() && !top.contains(id) {
                top.push(id.clone());
            }
        }
        Ok(top)
    }
}

impl<'a> Board<'a> {
    /// The groups of a pasted fragment (`SCH_EDITOR_CONTROL::Paste`: the group items are in `loadedItems` with their members, `parseGroup` / `resolveGroups`
    /// having made them): each becomes a group of the pasted items -- `remap` says which item each id of the fragment became -- and a group in a group
    /// stays in it. A member that was not pasted (a junction the wires imply, one the sheet had) is left out; a group left with fewer than two dissolves. A
    /// lock is not carried (`schItem->SetLocked( false )`).
    fn paste_groups(&mut self, fragment: &SchFragment, remap: &BTreeMap<String, String>) -> Result<Vec<String>, Vec<CheckResult>> {
        let tree = fragment.section.extras.group_tree();
        if fragment.section.extras.groups.is_empty() {
            return Ok(Vec::new());
        }
        // inner groups first, so the group that holds one names it by the id it got here
        fn make(tree: eda_model::groups::GroupTree<'_>, id: &str, remap: &mut BTreeMap<String, String>, out: &mut Vec<eda_model::ir::Group>, visiting: &mut Vec<String>) {
            if remap.contains_key(id) || visiting.iter().any(|v| v == id) {
                return;
            }
            let Some(g) = tree.group(id) else { return };
            visiting.push(id.to_string());
            let mut members: Vec<String> = Vec::new();
            for m in &g.member_ids {
                if tree.is_group(m) {
                    make(tree, m, remap, out, visiting);
                }
                if let Some(new) = remap.get(m) {
                    if !members.contains(new) {
                        members.push(new.clone());
                    }
                }
            }
            visiting.pop();
            // a group linked to a design block may hold one item (`SCH_DRAWING_TOOLS::DrawSheet` puts a placed sheet alone in its group)
            if members.len() >= 2 || (members.len() == 1 && !g.design_block.is_empty()) {
                let mut group = eda_model::ir::Group { id: String::new(), name: g.name.clone(), member_ids: members, design_block: g.design_block.clone() };
                group.id = format!("pasted:{id}");
                remap.insert(id.to_string(), group.id.clone());
                out.push(group);
            }
        }
        let mut remap = remap.clone();
        let mut made: Vec<eda_model::ir::Group> = Vec::new();
        for g in &fragment.section.extras.groups {
            make(tree, &g.id, &mut remap, &mut made, &mut Vec::new());
        }
        // name them from the inside out (a group is named after its members)
        let sch = self.schematic_mut()?;
        let mut names: BTreeMap<String, String> = BTreeMap::new();
        for g in &made {
            let mut g = g.clone();
            let temp = g.id.clone();
            g.member_ids = g.member_ids.iter().map(|m| names.get(m).cloned().unwrap_or_else(|| m.clone())).collect();
            g.id = String::new();
            sch.extras.groups.push(g);
            sch.extras.assign_missing_group_ids();
            let new_id = sch.extras.groups.last().map(|g| g.id.clone()).unwrap_or_default();
            names.insert(temp, new_id);
        }
        Ok(made.iter().filter_map(|g| names.get(&g.id).cloned()).collect())
    }
}

/// What [`Board::merge_lib_symbols`] settled.
pub(crate) struct LibMerge {
    /// The name each wanted `lib_id` goes by in the design now.
    pub lib_name: BTreeMap<String, String>,
    /// The engine's symbol for each `lib_id` that has a definition.
    pub engine_of: BTreeMap<String, LibSymbol>,
    /// The entries the design does not have yet.
    pub publish: Vec<LibrarySymbol>,
}

/// Do the two draw their pins in the same places (number, unit, place, direction, length -- the normal body style)? That, not the artwork, is what a
/// wire that ends on a pin depends on.
fn same_pins(a: &LibrarySymbol, b: &LibSymbol) -> bool {
    let r = |v: f64| (v * 1000.0).round() as i64;
    let mut pa: Vec<(String, u32, i64, i64, i64, i64)> = a.pins.iter().filter(|p| p.body_style <= 1).map(|p| (p.number.clone(), p.unit, r(p.at.x), r(p.at.y), r(p.angle_deg), r(p.length_mm))).collect();
    let mut pb: Vec<(String, u32, i64, i64, i64, i64)> = b.pins.iter().map(|p| (p.number.clone(), p.unit, r(p.at.x), r(p.at.y), r(p.angle_deg), r(p.length_mm))).collect();
    pa.sort();
    pb.sort();
    pa == pb
}

/// Two library symbols draw the same: everything but the name and whether the project has published it.
fn same_drawing(a: &LibrarySymbol, b: &LibrarySymbol) -> bool {
    let mut a = a.clone();
    a.lib_id = b.lib_id.clone();
    a.published = b.published;
    a == *b
}

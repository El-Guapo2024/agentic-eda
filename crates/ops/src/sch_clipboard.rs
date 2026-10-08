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
use eda_model::ir::{LibrarySymbol, NetLabel, NoConnect, Point, PowerSymbol, SchLine, SchematicSection, SchematicText, SymbolInstance, Um, Wire};
use eda_model::sch_clipboard::{annotate_paste, place_offset_um, translate_graphic, PastedRef, PasteMode, SchFragment, UsedRef};
use eda_model::symbol::{is_synthetic_lib_id, LibSymbol};
use eda_model::CheckResult;
use std::collections::BTreeMap;

impl<'a> Board<'a> {
    /// Every placed symbol of every sheet -- power symbols included -- as the numbering sees it (`hierarchy.GetSymbols( existingRefs, SYMBOL_FILTER_ALL )`).
    fn used_references(&self) -> Vec<UsedRef> {
        let mut out: Vec<UsedRef> = Vec::new();
        let sheets = self.design.schematic.iter().chain(self.design.sheet_contents.iter().flat_map(|c| c.values()));
        for sch in sheets {
            for s in &sch.symbols {
                let value = if s.value.is_empty() { self.model.part(&s.id).and_then(|p| p.value.clone()).unwrap_or_default() } else { s.value.clone() };
                out.push(UsedRef { reference: s.id.clone(), unit: s.unit, lib_id: s.lib_id.clone(), value });
            }
            for p in &sch.power_symbols {
                out.push(UsedRef { reference: p.id.clone(), unit: 1, lib_id: p.lib_id.clone(), value: p.net.clone() });
            }
        }
        out
    }

    /// The library symbol the design draws `lib_id` from, if it has one at all: a real library's, a built-in, or one published into the project.
    fn design_symbol(&self, lib_id: &str) -> Option<LibSymbol> {
        if lib_id.is_empty() || is_synthetic_lib_id(lib_id) {
            return None;
        }
        if let Some(s) = self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)).filter(|s| s.published) {
            return Some(s.to_engine_symbol());
        }
        self.model.symbol_of(lib_id)
    }

    /// `Cmd::PasteSch` -- see the module doc.
    pub(crate) fn paste_sch(&mut self, fragment: &SchFragment, dx: Um, dy: Um, mode: PasteMode) -> Result<(), Vec<CheckResult>> {
        if fragment.is_empty() {
            return Err(vec![CheckResult::fail("ops_empty_paste", "paste", "there is nothing to paste")]);
        }
        let src = &fragment.section;
        let imported = self.schematic_mut_or_create().imported_from_kicad;

        // ---- the library symbols: the design's own win; the fragment's come in only for a name the design lacks ----
        let mut wanted: Vec<&str> = src.symbols.iter().map(|s| s.lib_id.as_str()).chain(src.power_symbols.iter().map(|p| p.lib_id.as_str())).collect();
        wanted.sort_unstable();
        wanted.dedup();
        let mut lib_name: BTreeMap<String, String> = BTreeMap::new();
        let mut engine_of: BTreeMap<String, LibSymbol> = BTreeMap::new();
        let mut publish: Vec<LibrarySymbol> = Vec::new();
        for lib_id in wanted {
            if let Some(known) = self.design_symbol(lib_id) {
                lib_name.insert(lib_id.to_string(), lib_id.to_string());
                engine_of.insert(lib_id.to_string(), known);
                continue;
            }
            let Some(theirs) = fragment.lib_symbol(lib_id).map(|t| {
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
            let mut name = base.clone();
            let mut n = 2;
            loop {
                let existing = self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(&name)).or_else(|| publish.iter().find(|p| p.lib_id == name));
                match existing {
                    None => break,
                    Some(e) if same_drawing(e, &theirs) => break,
                    Some(_) => {
                        name = format!("{base}_{n}");
                        n += 1;
                    }
                }
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
                    let (x0, _, _, y1) = eda_engine::geometry::real_symbol_bbox(engine, s.unit);
                    let (ox, oy) = place_offset_um(s.rot as f64 / 1000.0, s.mirrored, s.mirror_y, (-x0, -y1));
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
        let new_wires: Vec<Wire> = src.wires.iter().filter(|w| w.pts.len() >= 2).map(|w| Wire { id: String::new(), net: String::new(), pins: Vec::new(), pts: w.pts.iter().map(shift).collect(), bus: w.bus }).collect();
        let new_labels: Vec<NetLabel> = src.labels.iter().map(|l| NetLabel { id: String::new(), net: l.net.clone(), at: shift(&l.at), kind: l.kind.clone() }).collect();
        let new_texts: Vec<SchematicText> = src.texts.iter().map(|t| SchematicText { id: String::new(), content: t.content.clone(), at: shift(&t.at), angle: t.angle, size_um: t.size_um }).collect();
        let new_ncs: Vec<NoConnect> = src.no_connects.iter().map(|n| NoConnect { id: String::new(), at: shift(&n.at), pin: String::new() }).collect();
        let new_entries: Vec<eda_model::ir::BusEntry> = src.bus_entries.iter().map(|b| eda_model::ir::BusEntry { id: String::new(), at: shift(&b.at), size: b.size }).collect();
        let new_lines: Vec<SchLine> = src.lines.iter().map(|l| SchLine { id: String::new(), pts: l.pts.iter().map(shift).collect(), width_um: l.width_um }).collect();
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
        sch.symbols.extend(new_symbols);
        sch.symbols.sort_by(|a, b| (&a.id, a.unit).cmp(&(&b.id, b.unit)));
        sch.power_symbols.extend(new_powers);
        sch.wires.extend(new_wires);
        sch.labels.extend(new_labels);
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
        Ok(())
    }
}

/// Two library symbols draw the same: everything but the name and whether the project has published it.
fn same_drawing(a: &LibrarySymbol, b: &LibrarySymbol) -> bool {
    let mut a = a.clone();
    a.lib_id = b.lib_id.clone();
    a.published = b.published;
    a == *b
}

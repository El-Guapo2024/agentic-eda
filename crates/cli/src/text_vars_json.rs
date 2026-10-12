//! The `shown` form of the texts the studio's state carries: what `${NAME}` in a board text, a footprint field, a symbol field, a sheet's text or a title block
//! stands for (`eda_model::text_vars`, KiCad's `ExpandTextVars` and the resolvers of each kind of item).
//!
//! The state keeps the raw text beside it (an edit starts from the raw one, as KiCad's dialogs do) and adds `shown` only where the two differ, so a board
//! with no variable in it sends the bytes it always did. The clock is the browser's (`?clock=YYYY-MM-DDTHH:MM:SS`, its local time); without it the server's
//! UTC day. The derived KiCad files keep the raw text and the project file carries the variables, so kicad-cli's plots show the same.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use eda_model::ir::{Design, SchematicSection, TitleBlock};
use eda_model::text_vars::{pcb_text_shown, Clock, PcbBoard, PcbField, PcbFootprint, PcbPad, Project, SchPin, SchSheet, SchSheetLevel, SchSymbol, TitleBlockVars};
use eda_model::{BoardRules, ConstraintModel};
use serde_json::{json, Value};

/// The time a request names (`?clock=2026-10-10T21:30:05`, percent-encoded or not), the server's UTC day when it names none.
pub(crate) fn clock_of(target: &str) -> Clock {
    let query = target.split('?').nth(1).unwrap_or("");
    query.split('&').find_map(|kv| kv.strip_prefix("clock=")).and_then(|v| Clock::from_local(&percent_decode(v))).unwrap_or_else(Clock::utc_now)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && b.get(i + 1).zip(b.get(i + 2)).is_some_and(|(a, c)| a.is_ascii_hexdigit() && c.is_ascii_hexdigit()) {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap_or(b'%'));
            i += 3;
        } else {
            out.push(if b[i] == b'+' { b' ' } else { b[i] });
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The project of the board in `dir`: the name kicad-cli's derived project has (`PROJECTNAME`), and the variables the Text Variables page holds.
pub(crate) fn project(dir: &Path, intent: &str, clock: Clock) -> Project {
    let stem = Path::new(intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board");
    Project { name: eda_kicad_engine::file_stem(stem), variables: crate::text_variables::project_variables(dir), clock }
}

/// A net's class by name (`PAD::GetNetClassName`): the class a pattern or the net's own name gives it, else `Default`.
fn net_class_name(rules: &BoardRules, net: &str) -> String {
    if net.is_empty() {
        return "Default".into();
    }
    rules.class_of(net).map_or_else(|| "Default".to_string(), |c| c.name.clone())
}

fn field_of(v: &Value) -> PcbField {
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    PcbField { name: s("name"), text: s("text"), layer: s("layer") }
}

// ------------------------------------------------------------------------------------------------ the board

/// Every placed footprint of the board with its fields and pads, for the texts on the board and its footprints.
pub(crate) struct PcbShown {
    board: PcbBoard,
    by_part: HashMap<String, usize>,
}

impl PcbShown {
    /// `model.parts` that are placed, in model order. The fields are what the board shows (`fp_json::fields_json`); a pad's net is the model's.
    pub(crate) fn new(dir: &Path, design: &Design, model: &ConstraintModel, project: Project) -> PcbShown {
        let net_of: HashMap<&str, &str> = model.nets.iter().flat_map(|n| n.pins.iter().map(move |p| (p.as_str(), n.name.as_str()))).collect();
        let stem = project.name.clone();
        let mut footprints = Vec::new();
        let mut by_part = HashMap::new();
        if let Some(pl) = design.placement.as_ref() {
            for part in &model.parts {
                let (Some(fp), Some(footprint)) = (pl.footprints.iter().find(|f| f.id == part.reference), model.footprint_of(part)) else { continue };
                let lib_name = part.footprint.clone().unwrap_or_else(|| footprint.name.clone());
                let lib_id = if lib_name.contains(':') { lib_name } else { format!("eda:{lib_name}") };
                let (nickname, item) = lib_id.split_once(':').unwrap_or(("", lib_id.as_str()));
                let fields: Vec<PcbField> = crate::fp_json::fields_json(design, part, fp, &footprint).iter().map(field_of).collect();
                let pads = footprint
                    .pads
                    .iter()
                    .map(|p| {
                        let net = net_of.get(format!("{}.{}", part.reference, p.number).as_str()).copied().unwrap_or("");
                        PcbPad { number: p.number.clone(), net: net.to_string(), net_class: net_class_name(&model.board, net), pin_function: String::new() }
                    })
                    .collect();
                by_part.insert(part.reference.clone(), footprints.len());
                footprints.push(PcbFootprint { layer: if fp.side == eda_model::ir::Side::Bottom { "B.Cu" } else { "F.Cu" }.into(), lib_nickname: nickname.into(), lib_item_name: item.into(), fields, pads });
            }
        }
        let title_block = design.drawings.as_ref().and_then(|d| d.title_block.as_ref()).map(TitleBlockVars::of);
        let board = PcbBoard { project, layer_names: BTreeMap::new(), file_name: format!("{stem}.kicad_pcb"), file_path: dir.join(".kicad").join(format!("{stem}.kicad_pcb")).display().to_string(), title_block, footprints };
        PcbShown { board, by_part }
    }

    /// `PCB_TEXT::GetShownText` of a free text on `layer`.
    pub(crate) fn text_shown(&self, raw: &str, layer: &str) -> String {
        if !eda_model::text_vars::has_text_vars(raw) && !raw.contains('{') {
            return raw.to_string();
        }
        pcb_text_shown(raw, layer, None, &self.board)
    }

    /// A board text's JSON (`content`, `layer`, ...) with `shown` added when its shown text is not its stored one.
    pub(crate) fn text_json(&self, mut text: Value) -> Value {
        let raw = text["content"].as_str().unwrap_or("").to_string();
        let shown = self.text_shown(&raw, text["layer"].as_str().unwrap_or(""));
        if shown != raw {
            text["shown"] = json!(shown);
        }
        text
    }

    /// Put `shown` on each field of the footprint `part` whose shown text is not its stored one (`PCB_FIELD::GetShownText`).
    pub(crate) fn annotate_fields(&self, part: &str, fields: &mut Value) {
        let (Some(&i), Some(list)) = (self.by_part.get(part), fields.as_array_mut()) else { return };
        let fp = &self.board.footprints[i];
        for f in list {
            let name = f["name"].as_str().unwrap_or("").to_string();
            let raw = f["text"].as_str().unwrap_or("").to_string();
            if !eda_model::text_vars::has_text_vars(&raw) && !raw.contains('{') {
                continue;
            }
            let shown = fp.field_shown(&self.board, &name, 0).unwrap_or_else(|| raw.clone());
            if shown != raw {
                f["shown"] = json!(shown);
            }
        }
    }

    /// The title block as the page reports it, with the title block's own `shown` form when a project variable changes any value of it.
    pub(crate) fn title_block_json(&self, tb: &TitleBlock) -> Value {
        title_block_json(tb, &self.board.project)
    }
}

/// `crate::page_json::title_block_json` plus `shown`: the values with the project's variables in them expanded (`TITLE_BLOCK::TextVarResolver`), when that
/// changes any of them.
pub(crate) fn title_block_json(tb: &TitleBlock, project: &Project) -> Value {
    let mut v = crate::page_json::title_block_json(tb);
    let raw = TitleBlockVars::of(tb);
    let shown = raw.shown(project);
    if shown != raw {
        v["shown"] = json!({ "title": shown.title, "date": shown.date, "rev": shown.revision, "company": shown.company, "comments": shown.comments });
    }
    v
}

// ------------------------------------------------------------------------------------------------ the schematic

/// The schematic as the resolvers read it, with the sheet in view: the symbols of every sheet (a `${REF:field}` may name one on another), the path to the sheet
/// in view with each sheet's fields and title block, and the page numbering.
pub(crate) struct SchShown {
    sheet: SchSheet,
    by_key: HashMap<String, usize>,
}

fn symbol_of(sch: &SchematicSection, model: &ConstraintModel, sym: &eda_model::ir::SymbolInstance) -> SchSymbol {
    let part = model.part(&sym.id);
    let lib = part.and_then(|p| model.real_symbol_of_instance(sch, sym, p));
    let units = lib.as_ref().map_or(1, |l| l.unit_count);
    let mut fields: Vec<(String, String)> = eda_engine::fields::symbol_specs(sym, part, lib.as_ref().map_or("", |l| l.datasheet.as_str())).into_iter().map(|f| (f.name, f.text)).collect();
    for (name, text) in sch.user_fields.get(&sym.id).into_iter().flatten() {
        fields.push((name.clone(), text.clone()));
    }
    let probe = SchSymbol::default();
    let letter = probe.unit_letter_of(sym.unit);
    let lib_id = if sym.lib_id.is_empty() { format!("eda:{}", sym.id) } else { sym.lib_id.clone() };
    let (nickname, item) = lib_id.split_once(':').unwrap_or(("", lib_id.as_str()));
    let footprint = fields.iter().find(|(n, _)| n == "Footprint").map(|(_, t)| t.clone()).unwrap_or_default();
    let pins = part
        .map(|p| {
            p.pins
                .iter()
                .map(|pin| {
                    let key = format!("{}.{}", sym.id, pin.number);
                    let net = model.nets.iter().find(|n| n.pins.iter().any(|q| *q == key)).map_or("", |n| n.name.as_str());
                    let unit = lib.as_ref().and_then(|l| l.pin_by_number(&pin.number)).map_or(0, |lp| lp.unit);
                    SchPin { number: pin.number.clone(), name: pin.name.clone().unwrap_or_default(), net: net.to_string(), net_class: net_class_name(&model.board, net), unit }
                })
                .collect()
        })
        .unwrap_or_default();
    SchSymbol {
        fields,
        reference: if units > 1 { format!("{}{letter}", sym.id) } else { sym.id.clone() },
        short_reference: sym.id.clone(),
        unit_letter: letter,
        lib_nickname: nickname.to_string(),
        lib_item_name: item.to_string(),
        footprint,
        description: lib.as_ref().map(|l| l.description.clone()).unwrap_or_default(),
        keywords: String::new(),
        dnp: sym.dnp,
        exclude_from_bom: sym.exclude_from_bom,
        exclude_from_board: sym.exclude_from_board,
        exclude_from_sim: sym.exclude_from_sim,
        pins,
    }
}

impl SchShown {
    /// `breadcrumb` is the `(placement id, name)` chain from the root down to the sheet in view, empty for the root.
    pub(crate) fn new(dir: &Path, design: &Design, model: &ConstraintModel, breadcrumb: &[(String, String)], project: Project) -> SchShown {
        let stem = project.name.clone();
        let tb = |s: Option<&SchematicSection>| s.and_then(|s| s.title_block.as_ref()).map(TitleBlockVars::of);
        let mut levels = vec![SchSheetLevel { fields: vec![], title_block: tb(design.schematic.as_ref()) }];
        let mut here = design.schematic.as_ref();
        for (id, name) in breadcrumb {
            let placement = here.and_then(|s| s.sheets.iter().find(|p| &p.id == id));
            let content = placement.and_then(|p| design.sheet_contents.as_ref().and_then(|c| c.get(&p.file)));
            levels.push(SchSheetLevel {
                fields: vec![("Sheetname".into(), name.clone()), ("Sheetfile".into(), placement.map(|p| p.file.clone()).unwrap_or_default())],
                title_block: tb(content),
            });
            here = content;
        }
        let entries = eda_model::sheet_list::walk(design);
        let ids: Vec<String> = breadcrumb.iter().map(|(id, _)| id.clone()).collect();
        let page = entries.iter().find(|e| e.path == ids).map(|e| e.effective_page()).unwrap_or_else(|| "1".into());
        let mut symbols = Vec::new();
        let mut by_key = HashMap::new();
        let mut screens: Vec<&SchematicSection> = design.schematic.iter().collect();
        if let Some(c) = design.sheet_contents.as_ref() {
            screens.extend(c.values());
        }
        for sch in screens {
            for sym in &sch.symbols {
                by_key.insert(eda_model::ir::field_key(&sym.id, sym.unit), symbols.len());
                symbols.push(symbol_of(sch, model, sym));
            }
        }
        let sheet_path = std::iter::once("/".to_string()).chain(breadcrumb.iter().map(|(_, n)| format!("{n}/"))).collect::<String>();
        let sheet = SchSheet {
            project,
            levels,
            page,
            page_count: entries.len(),
            sheet_path,
            sheet_name: breadcrumb.last().map(|(_, n)| n.clone()).unwrap_or_default(),
            file_name: format!("{stem}.kicad_sch"),
            file_path: dir.join(".kicad").join(format!("{stem}.kicad_sch")).display().to_string(),
            symbols,
        };
        SchShown { sheet, by_key }
    }

    /// `SCH_TEXT::GetShownText` of a free text on the sheet in view.
    pub(crate) fn text_shown(&self, raw: &str) -> String {
        if !eda_model::text_vars::has_text_vars(raw) && !raw.contains('{') {
            return raw.to_string();
        }
        self.sheet.text_shown(raw)
    }

    /// Put `shown` on each field of the symbol with the field key `key` whose shown text is not its stored one (`SCH_FIELD::GetShownText`). A Reference is
    /// left alone: the painter writes it with its unit letter.
    pub(crate) fn annotate_symbol_fields(&self, key: &str, fields: &mut [Value]) {
        let Some(&i) = self.by_key.get(key) else { return };
        let sym = &self.sheet.symbols[i];
        let level = self.sheet.levels.len().saturating_sub(1);
        for f in fields {
            if f["name"].as_str() == Some("Reference") {
                continue;
            }
            let raw = f["text"].as_str().unwrap_or("").to_string();
            if !eda_model::text_vars::has_text_vars(&raw) && !raw.contains('{') {
                continue;
            }
            let shown = self.sheet.field_shown(level, Some(sym), &raw, 0);
            if shown != raw {
                f["shown"] = json!(shown);
            }
        }
    }

    /// The shown Value and Footprint of the symbol with the field key `key` -- `{"value": .., "footprint": ..}` with the ones that differ from the stored text, or
    /// none when neither does. What the painter draws for a symbol whose fields the state does not list.
    pub(crate) fn symbol_shown_json(&self, key: &str) -> Option<Value> {
        let sym = &self.sheet.symbols[*self.by_key.get(key)?];
        let level = self.sheet.levels.len().saturating_sub(1);
        let mut out = serde_json::Map::new();
        for (name, key) in [("Value", "value"), ("Footprint", "footprint")] {
            let Some((_, raw)) = sym.fields.iter().find(|(n, _)| n == name) else { continue };
            if !eda_model::text_vars::has_text_vars(raw) && !raw.contains('{') {
                continue;
            }
            let shown = self.sheet.field_shown(level, Some(sym), raw, 0);
            if shown != *raw {
                out.insert(key.into(), json!(shown));
            }
        }
        (!out.is_empty()).then(|| Value::Object(out))
    }

    /// The same for the fields of a placed sheet (`Sheetname`, `Sheetfile`).
    pub(crate) fn annotate_sheet_fields(&self, fields: &mut [Value]) {
        let level = self.sheet.levels.len().saturating_sub(1);
        for f in fields {
            let raw = f["text"].as_str().unwrap_or("").to_string();
            if !eda_model::text_vars::has_text_vars(&raw) && !raw.contains('{') {
                continue;
            }
            let shown = self.sheet.field_shown(level, None, &raw, 0);
            if shown != raw {
                f["shown"] = json!(shown);
            }
        }
    }

    pub(crate) fn title_block_json(&self, tb: &TitleBlock) -> Value {
        title_block_json(tb, &self.sheet.project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_is_the_browsers_when_the_request_names_it() {
        let c = clock_of("/api/state?clock=2026-10-10T21%3A30%3A05");
        assert_eq!((c.date.as_str(), c.time_hms.as_str()), ("2026-10-10", "21h30m05s"));
        assert_eq!(clock_of("/api/state?x=1&clock=2026-01-02T03:04:05&y=2").date, "2026-01-02");
        assert_eq!(clock_of("/api/state").date.len(), 10, "without it, the server's UTC day");
        assert_eq!(clock_of("/api/state?clock=garbage").date.len(), 10);
        assert_eq!(percent_decode("a%3Ab+c%zz%"), "a:b c%zz%");
    }
}

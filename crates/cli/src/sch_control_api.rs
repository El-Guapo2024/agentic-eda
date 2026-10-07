//! `/api/sch/*` reads for the schematic-control actions (`eeschema.NavigateTool.*`, `eeschema.EditorControl.*`):
//! what the studio cannot work out from the one sheet it is showing.
//!
//! None of these runs kicad-cli (the ones that do live in `sch_export_api.rs`, behind the kicad-cli lane), and none
//! edits `design.json`: every edit is an `/api/cmd` verb.

use crate::fab_api::err;
use crate::kicad_engine;
use eda_model::ir::{Design, SchematicSection};
use serde_json::{json, Value};
use std::path::Path;

/// One sheet of the hierarchy, as `SCH_SHEET_LIST` holds it: the placement ids from the root down (`[]` is the root), the placement's
/// name and file, and the page number it was given (empty when none was set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyEntry {
    pub path: Vec<String>,
    pub name: String,
    pub file: String,
    pub page: String,
}

/// Every sheet that has content, depth first, the root first -- the order `SCH_SHEET_LIST::BuildSheetList` builds (the studio sorts
/// it by page number, `SortByPageNumbers`, so Next / Previous Sheet follow the page order). A placement whose file has no screen is
/// left out (nothing to show there), and so is anything deeper than 32 sheets (a file that places itself).
pub fn hierarchy_of(design: &Design) -> Vec<HierarchyEntry> {
    fn visit(design: &Design, sch: &SchematicSection, path: &mut Vec<String>, out: &mut Vec<HierarchyEntry>) {
        if path.len() > 32 {
            return;
        }
        for child in &sch.sheets {
            let Some(content) = design.sheet_contents.as_ref().and_then(|c| c.get(&child.file)) else { continue };
            path.push(child.id.clone());
            out.push(HierarchyEntry { path: path.clone(), name: child.name.clone(), file: child.file.clone(), page: child.page.clone() });
            visit(design, content, path, out);
            path.pop();
        }
    }
    let mut out = vec![HierarchyEntry { path: Vec::new(), name: String::new(), file: String::new(), page: String::new() }];
    if let Some(root) = &design.schematic {
        visit(design, root, &mut Vec::new(), &mut out);
    }
    out
}

/// `GET /api/sch/hierarchy`: `{ "ok": true, "sheets": [{ "path": ["id", ...], "name", "file", "page" }] }`, the root first.
pub fn hierarchy(dir: &Path) -> Value {
    match kicad_engine::load_with_schematic(dir) {
        Ok((design, _)) => json!({
            "ok": true,
            "sheets": hierarchy_of(&design).into_iter().map(|h| json!({ "path": h.path, "name": h.name, "file": h.file, "page": h.page })).collect::<Vec<_>>(),
        }),
        Err(e) => err(crate::board::reasons(&e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Point, Provenance, SheetInstance};
    use std::collections::BTreeMap;

    fn sch(sheets: Vec<SheetInstance>) -> SchematicSection {
        SchematicSection {
            symbols: vec![],
            wires: vec![],
            labels: vec![],
            texts: vec![],
            power_symbols: vec![],
            no_connects: vec![],
            bus_entries: vec![],
            erc_exclusions: vec![],
            erc_pin_map: None,
            user_fields: Default::default(),
            title_block: None,
            sheets,
            instance_overrides: vec![],
            junctions: vec![],
            lines: vec![],
            imported_from_kicad: false,
            extras: Default::default(),
        }
    }

    fn placement(id: &str, name: &str, file: &str, page: &str) -> SheetInstance {
        SheetInstance { id: id.into(), name: name.into(), file: file.into(), at: Point { x: 0, y: 0 }, size: (1000, 1000), pins: vec![], page: page.into() }
    }

    fn design(root: SchematicSection, screens: BTreeMap<String, SchematicSection>) -> Design {
        Design {
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(root),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: (!screens.is_empty()).then_some(screens),
            bus_aliases: vec![],
            symbol_library: None,
        }
    }

    #[test]
    fn a_flat_design_has_only_the_root() {
        let h = hierarchy_of(&design(sch(vec![]), BTreeMap::new()));
        assert_eq!(h.len(), 1);
        assert!(h[0].path.is_empty());
    }

    #[test]
    fn sheets_come_depth_first_with_their_paths_and_pages() {
        let mut screens = BTreeMap::new();
        screens.insert("a.kicad_sch".to_string(), sch(vec![placement("a1", "Deep", "c.kicad_sch", "")]));
        screens.insert("b.kicad_sch".to_string(), sch(vec![]));
        screens.insert("c.kicad_sch".to_string(), sch(vec![]));
        let d = design(sch(vec![placement("sa", "A", "a.kicad_sch", "7"), placement("sb", "B", "b.kicad_sch", "")]), screens);
        let h = hierarchy_of(&d);
        let seen: Vec<(Vec<&str>, &str, &str)> = h.iter().map(|e| (e.path.iter().map(String::as_str).collect(), e.name.as_str(), e.page.as_str())).collect();
        assert_eq!(seen, vec![(vec![], "", ""), (vec!["sa"], "A", "7"), (vec!["sa", "a1"], "Deep", ""), (vec!["sb"], "B", "")]);
    }

    #[test]
    fn a_placement_without_a_screen_and_a_sheet_that_places_itself_do_not_break_the_walk() {
        let mut screens = BTreeMap::new();
        screens.insert("loop.kicad_sch".to_string(), sch(vec![placement("again", "Loop", "loop.kicad_sch", "")]));
        let d = design(sch(vec![placement("gone", "Gone", "missing.kicad_sch", ""), placement("lp", "Loop", "loop.kicad_sch", "")]), screens);
        let h = hierarchy_of(&d);
        assert!(h.iter().all(|e| e.name != "Gone"), "no screen, nothing to show");
        assert!(h.len() < 40, "the self-placing file stops at the depth limit instead of looping forever: {}", h.len());
    }
}

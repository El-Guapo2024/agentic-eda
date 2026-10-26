//! The sheets of a hierarchy, in the orders eeschema walks them (`SCH_SHEET_LIST`, `eeschema/sch_sheet_path.cpp`).
//!
//! A sheet here is a *placement*: the chain of [`SheetInstance::id`]s from the root down (`SCH_SHEET_PATH`), the root being the empty
//! path. The walk is depth first with the root first (`SCH_SHEET_LIST::BuildSheetList`), and a sheet's *virtual page number* is its
//! 1-based place in that walk (`SCH_SHEET_PATH::GetVirtualPageNumber`). The page a sheet *shows* is the text the user gave it
//! (Edit Sheet Page Number, [`SheetInstance::page`]), else its virtual page number; Next / Previous Sheet, Find across sheets and the
//! "sheet number x 100" annotation all count by it (`GetPageNumberAsInt`, `SortByPageNumbers`).
//!
//! The studio keeps the same rules in `kicad-port/sheetPages.ts`; this is the server's copy, so a search or an annotation counts the
//! sheets exactly as the page order the Hierarchy panel shows.

use crate::ir::{Design, SchematicSection, SheetInstance};

/// One sheet of the hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetEntry {
    /// The placement ids from the root down; the root is `[]`.
    pub path: Vec<String>,
    /// The placement's name (empty for the root).
    pub name: String,
    /// The `Design::sheet_contents` key of the sheet's screen (empty for the root, whose screen is `Design::schematic`).
    pub file: String,
    /// The page number the user gave the placement; empty when none.
    pub page: String,
    /// 1-based position in the depth-first walk (the root is 1).
    pub virtual_page: usize,
}

impl SheetEntry {
    /// The page the sheet shows: the one set for it, else its virtual page number (`GetPageNumber` falls back that way).
    pub fn effective_page(&self) -> String {
        if self.page.is_empty() {
            self.virtual_page.to_string()
        } else {
            self.page.clone()
        }
    }

    /// `SCH_SHEET_PATH::GetPageNumberAsInt`: the page as a whole number when it is one, else the virtual page number.
    pub fn page_number_as_int(&self) -> i64 {
        to_long(&self.page).unwrap_or(self.virtual_page as i64)
    }

    /// `path` as the `/`-joined text the studio's API uses.
    pub fn path_text(&self) -> String {
        self.path.join("/")
    }
}

/// The deepest chain of sheets followed: a file that places itself would otherwise never end.
pub const MAX_DEPTH: usize = 32;

/// Every sheet that has a screen, depth first, the root first: the order `SCH_SHEET_LIST::BuildSheetList` builds. A placement whose file
/// has no screen in `design` is left out (there is nothing to show or edit there), and so is anything deeper than [`MAX_DEPTH`].
pub fn walk(design: &Design) -> Vec<SheetEntry> {
    fn visit(design: &Design, sch: &SchematicSection, path: &mut Vec<String>, out: &mut Vec<SheetEntry>) {
        if path.len() > MAX_DEPTH {
            return;
        }
        for child in &sch.sheets {
            let Some(content) = design.sheet_contents.as_ref().and_then(|c| c.get(&child.file)) else { continue };
            path.push(child.id.clone());
            out.push(SheetEntry { path: path.clone(), name: child.name.clone(), file: child.file.clone(), page: child.page.clone(), virtual_page: out.len() + 1 });
            visit(design, content, path, out);
            path.pop();
        }
    }
    let mut out = vec![SheetEntry { path: Vec::new(), name: String::new(), file: String::new(), page: String::new(), virtual_page: 1 }];
    if let Some(root) = &design.schematic {
        visit(design, root, &mut Vec::new(), &mut out);
    }
    out
}

/// The screens some sheet shows: the files of the placements reachable from the root. A screen no sheet shows any more (its sheet was deleted) is
/// still kept in `Design::sheet_contents` -- the file is still there to be shown again -- but it is not part of the schematic: its symbols hold no
/// reference and its pins are on no net.
pub fn reachable_files(design: &Design) -> std::collections::BTreeSet<String> {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let Some(root) = &design.schematic else { return seen };
    let mut queue: Vec<&str> = root.sheets.iter().map(|s| s.file.as_str()).collect();
    while let Some(file) = queue.pop() {
        if !seen.insert(file.to_string()) {
            continue;
        }
        if let Some(sec) = design.sheet_contents.as_ref().and_then(|c| c.get(file)) {
            queue.extend(sec.sheets.iter().map(|s| s.file.as_str()));
        }
    }
    seen
}

/// The screen of the sheet at `path` (the root's `design.schematic` for the empty path).
pub fn screen_of<'a>(design: &'a Design, path: &[String]) -> Option<&'a SchematicSection> {
    let mut here = design.schematic.as_ref()?;
    for id in path {
        let placement = here.sheets.iter().find(|s| s.id == *id)?;
        here = design.sheet_contents.as_ref()?.get(&placement.file)?;
    }
    Some(here)
}

/// The placement at `path` (none for the root).
pub fn placement_at<'a>(design: &'a Design, path: &[String]) -> Option<&'a SheetInstance> {
    let (last, parents) = path.split_last()?;
    screen_of(design, parents)?.sheets.iter().find(|s| s.id == *last)
}

/// The sheets inside the sheet at `path`, itself included when `include_self` (`SCH_SHEET_LIST::GetSheetsWithinPath`), in walk order.
pub fn within<'a>(entries: &'a [SheetEntry], path: &[String], include_self: bool) -> Vec<&'a SheetEntry> {
    entries.iter().filter(|e| e.path.starts_with(path) && (include_self || e.path.len() > path.len())).collect()
}

/// `wxString::ToLong`: the whole text is one integer (a sign and leading blanks are allowed), else `None`.
pub fn to_long(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse::<i64>().ok()
}

/// `StrNumCmp` (`common/string_utils.cpp`): a natural compare -- runs of digits compare as numbers, everything else by character.
pub fn str_num_cmp(s1: &str, s2: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (a, b): (Vec<char>, Vec<char>) = (s1.chars().collect(), s2.chars().collect());
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        let (mut c1, mut c2) = (a[i], b[j]);
        if c1.is_ascii_digit() && c2.is_ascii_digit() {
            let (mut n1, mut n2) = (0u64, 0u64);
            while i < a.len() && a[i].is_ascii_digit() {
                n1 = n1.saturating_mul(10).saturating_add(a[i] as u64 - '0' as u64);
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                n2 = n2.saturating_mul(10).saturating_add(b[j] as u64 - '0' as u64);
                j += 1;
            }
            match n1.cmp(&n2) {
                Ordering::Equal => {}
                other => return other,
            }
            c1 = a.get(i).copied().unwrap_or('\0');
            c2 = b.get(j).copied().unwrap_or('\0');
        }
        match c1.cmp(&c2) {
            Ordering::Equal => {}
            other => return other,
        }
        if i < a.len() {
            i += 1;
        }
        if j < b.len() {
            j += 1;
        }
    }
    match (i >= a.len(), j >= b.len()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

/// `SCH_SHEET::ComparePageNum`: equal texts are equal; two integers compare as numbers; an integer comes before any other text; two
/// other texts compare naturally.
pub fn compare_page_num(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a == b {
        return Ordering::Equal;
    }
    match (to_long(a), to_long(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => str_num_cmp(a, b),
    }
}

/// `SCH_SHEET_LIST::SortByPageNumbers`: the entries in page order; the walk order breaks ties.
pub fn sort_by_page_numbers(entries: &[SheetEntry]) -> Vec<SheetEntry> {
    let mut sorted: Vec<SheetEntry> = entries.to_vec();
    sorted.sort_by(|a, b| compare_page_num(&a.effective_page(), &b.effective_page()).then_with(|| a.virtual_page.cmp(&b.virtual_page)));
    sorted
}

/// The reference `sym` of `screen` has on the sheet at `path` (`SCH_SYMBOL::GetRef( const SCH_SHEET_PATH* aSheet )`, which looks the instance up
/// by the whole `aSheet->Path()`): the [`crate::ir::SymbolPathOverride`] filed under that path, else the symbol's own id. A screen shown once has
/// no overrides, and a symbol the placement did not renumber keeps its id; the root (`[]`) is always the id.
pub fn reference_under(screen: &SchematicSection, sym: &crate::ir::SymbolInstance, path: &[String]) -> String {
    override_under(screen, sym, path).map(|o| o.reference.clone()).unwrap_or_else(|| sym.id.clone())
}

/// The [`crate::ir::SymbolPathOverride`] of `sym` filed under the sheet at `path`, if it has one.
pub fn override_under<'a>(screen: &'a SchematicSection, sym: &crate::ir::SymbolInstance, path: &[String]) -> Option<&'a crate::ir::SymbolPathOverride> {
    if path.is_empty() {
        return None;
    }
    screen.instance_overrides.iter().find(|o| o.sheet_path == path && o.at == sym.at && o.unit == sym.unit)
}

/// `SCHEMATIC::RecomputeIntersheetRefs`: the virtual pages each global label text is on (`GetPageRefsMap`), over every sheet of the hierarchy.
pub fn global_label_pages(design: &Design) -> std::collections::BTreeMap<String, std::collections::BTreeSet<usize>> {
    let mut out: std::collections::BTreeMap<String, std::collections::BTreeSet<usize>> = std::collections::BTreeMap::new();
    for e in walk(design) {
        let Some(sec) = screen_of(design, &e.path) else { continue };
        for l in sec.labels.iter().filter(|l| matches!(l.kind, crate::ir::LabelKind::Global { .. })) {
            out.entry(l.net.clone()).or_default().insert(e.virtual_page);
        }
    }
    out
}

/// `SCH_SCREENS::PruneOrphanedSymbolInstances`: drop every per-placement reference ([`crate::ir::SymbolPathOverride`]) whose sheet path no longer shows
/// its screen -- a sheet symbol deleted, or its file changed, or one of the sheets above it -- so that a placement made later under the same id (an id is
/// made from the sheet's file and place) does not take the references the old one had. Returns how many were dropped.
pub fn prune_orphaned_overrides(design: &mut Design) -> usize {
    let live: std::collections::BTreeSet<(String, Vec<String>)> = walk(design).into_iter().skip(1).map(|e| (e.file, e.path)).collect();
    let mut dropped = 0;
    for (file, sec) in design.sheet_contents.iter_mut().flat_map(|c| c.iter_mut()) {
        let before = sec.instance_overrides.len();
        sec.instance_overrides.retain(|o| live.contains(&(file.clone(), o.sheet_path.clone())));
        dropped += before - sec.instance_overrides.len();
    }
    dropped
}

/// A design file written before [`crate::ir::SymbolPathOverride::sheet_path`] filed each per-placement reference under the immediate placement's
/// id alone (`parent_sheet_instance_id`), which the reader takes as the path of that one id. Where that is no path of the screen (the placement is
/// inside another sheet), the reference stood for every sheet path that ends in that placement -- all the placements of the screen the sheet is
/// on, which could not be told apart then -- so it becomes one reference for each of them: the file reads as it always did. A key that is a path of
/// its screen, or that no path ends in (an orphan, as before), is left as it is. Returns how many keys were changed.
pub fn migrate_override_paths(design: &mut Design) -> usize {
    let has_legacy = design.sheet_contents.iter().flat_map(|c| c.values()).flat_map(|s| s.instance_overrides.iter()).any(|o| o.sheet_path.len() == 1);
    if !has_legacy {
        return 0;
    }
    let mut paths_of: std::collections::BTreeMap<String, Vec<Vec<String>>> = std::collections::BTreeMap::new();
    for e in walk(design).into_iter().skip(1) {
        paths_of.entry(e.file).or_default().push(e.path);
    }
    let mut changed = 0;
    for (file, sec) in design.sheet_contents.iter_mut().flat_map(|c| c.iter_mut()) {
        let Some(paths) = paths_of.get(file) else { continue };
        if !sec.instance_overrides.iter().any(|o| o.sheet_path.len() == 1 && !paths.contains(&o.sheet_path)) {
            continue;
        }
        let mut out: Vec<crate::ir::SymbolPathOverride> = Vec::with_capacity(sec.instance_overrides.len());
        for o in std::mem::take(&mut sec.instance_overrides) {
            let stood_for: Vec<&Vec<String>> = if o.sheet_path.len() == 1 && !paths.contains(&o.sheet_path) { paths.iter().filter(|p| p.last() == o.sheet_path.last()).collect() } else { Vec::new() };
            if stood_for.is_empty() {
                out.push(o);
                continue;
            }
            changed += 1;
            for p in stood_for {
                out.push(crate::ir::SymbolPathOverride { sheet_path: p.clone(), ..o.clone() });
            }
        }
        sec.instance_overrides = out;
    }
    changed
}

/// How many placements show each screen: a file two sheet symbols name is one screen shown twice (`SCH_SCREEN::GetRefCount`).
pub fn placement_counts(design: &Design) -> std::collections::BTreeMap<String, usize> {
    let mut out = std::collections::BTreeMap::new();
    for e in walk(design).into_iter().skip(1) {
        *out.entry(e.file).or_insert(0) += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::Nm;
use super::*;
    use crate::ir::{Point, Provenance};
    use std::collections::BTreeMap;

    fn section(sheets: Vec<SheetInstance>) -> SchematicSection {
        SchematicSection { sheets, ..Default::default() }
    }

    fn placement(id: &str, name: &str, file: &str, page: &str) -> SheetInstance {
        SheetInstance { id: id.into(), name: name.into(), file: file.into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, size: (Nm::new(1000), Nm::new(1000)), pins: vec![], page: page.into() }
    }

    fn design(root: SchematicSection, screens: Vec<(&str, SchematicSection)>) -> Design {
        let contents: BTreeMap<String, SchematicSection> = screens.into_iter().map(|(f, s)| (f.to_string(), s)).collect();
        Design {
            schema: crate::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(root),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: (!contents.is_empty()).then_some(contents),
            bus_aliases: vec![],
            symbol_library: None,
        }
    }

    #[test]
    fn the_walk_is_depth_first_with_virtual_pages_and_skips_a_sheet_with_no_screen() {
        let d = design(
            section(vec![placement("a", "A", "a.kicad_sch", ""), placement("gone", "Gone", "nope.kicad_sch", ""), placement("b", "B", "b.kicad_sch", "7")]),
            vec![("a.kicad_sch", section(vec![placement("c", "C", "c.kicad_sch", "")])), ("b.kicad_sch", section(vec![])), ("c.kicad_sch", section(vec![]))],
        );
        let w = walk(&d);
        let seen: Vec<(String, usize, i64)> = w.iter().map(|e| (e.path_text(), e.virtual_page, e.page_number_as_int())).collect();
        assert_eq!(seen, vec![("".into(), 1, 1), ("a".into(), 2, 2), ("a/c".into(), 3, 3), ("b".into(), 4, 7)]);
        assert_eq!(within(&w, &["a".to_string()], true).len(), 2);
        assert_eq!(within(&w, &["a".to_string()], false).len(), 1);
        assert_eq!(within(&w, &[], true).len(), 4);
        assert!(screen_of(&d, &["a".to_string(), "c".to_string()]).is_some());
        assert!(placement_at(&d, &["b".to_string()]).is_some_and(|p| p.name == "B"));
    }

    #[test]
    fn a_screen_no_sheet_shows_is_not_reachable() {
        let d = design(section(vec![placement("a", "A", "a.kicad_sch", "")]), vec![("a.kicad_sch", section(vec![placement("c", "C", "c.kicad_sch", "")])), ("c.kicad_sch", section(vec![])), ("gone.kicad_sch", section(vec![]))]);
        let r: Vec<String> = reachable_files(&d).into_iter().collect();
        assert_eq!(r, vec!["a.kicad_sch".to_string(), "c.kicad_sch".to_string()]);
    }

    #[test]
    fn a_file_that_places_itself_ends_at_the_depth_limit() {
        let d = design(section(vec![placement("x", "Loop", "loop.kicad_sch", "")]), vec![("loop.kicad_sch", section(vec![placement("again", "Loop", "loop.kicad_sch", "")]))]);
        assert!(walk(&d).len() < 40);
    }

    #[test]
    fn page_numbers_sort_numbers_first_then_text_naturally() {
        let e = |page: &str, virtual_page: usize| SheetEntry { path: vec![virtual_page.to_string()], name: String::new(), file: String::new(), page: page.into(), virtual_page };
        let sorted = sort_by_page_numbers(&[e("", 1), e("A2", 2), e("10", 3), e("A10", 4), e("2", 5), e("", 6)]);
        // pages: 1, A2, 10, A10, 2, 6 -> 1, 2, 6, 10, A2, A10
        assert_eq!(sorted.iter().map(SheetEntry::effective_page).collect::<Vec<_>>(), vec!["1", "2", "6", "10", "A2", "A10"]);
        assert_eq!(compare_page_num("01", "1"), std::cmp::Ordering::Equal);
        assert_eq!(str_num_cmp("sheet9", "sheet10"), std::cmp::Ordering::Less);
        assert_eq!(to_long(" -4"), Some(-4));
        assert_eq!(to_long("4a"), None);
        assert_eq!(to_long(""), None);
    }

    /// Root -> O twice (`oa`, `ob`) -> I twice (`ia`, `ib`), I holding R1 with the references `refs` gives.
    fn nested(refs: Vec<crate::ir::SymbolPathOverride>) -> Design {
        let mut inner = section(vec![]);
        inner.symbols.push(serde_json::from_value(serde_json::json!({ "id": "R1", "at": { "x": 0, "y": 0 }, "rot": 0 })).unwrap());
        inner.instance_overrides = refs;
        design(section(vec![placement("oa", "OA", "o.kicad_sch", ""), placement("ob", "OB", "o.kicad_sch", "")]), vec![("o.kicad_sch", section(vec![placement("ia", "IA", "i.kicad_sch", ""), placement("ib", "IB", "i.kicad_sch", "")])), ("i.kicad_sch", inner)])
    }

    fn over(path: &[&str], reference: &str) -> crate::ir::SymbolPathOverride {
        crate::ir::SymbolPathOverride { at: Point { x: Nm::ZERO, y: Nm::ZERO }, sheet_path: path.iter().map(|s| s.to_string()).collect(), reference: reference.into(), unit: 1 }
    }

    fn refs_by_path(d: &Design) -> Vec<(String, String)> {
        walk(d).into_iter().filter(|e| e.file == "i.kicad_sch").map(|e| { let i = screen_of(d, &e.path).unwrap(); (e.path_text(), reference_under(i, &i.symbols[0], &e.path)) }).collect()
    }

    /// `SCH_SYMBOL::GetRef( &sheetPath )`: a screen shown twice inside a screen shown twice has a reference for each whole path (KiCad QA `issue10926_1`),
    /// which the last placement alone could not tell apart; pruning keeps exactly the paths that still show the screen.
    #[test]
    fn a_screen_shown_twice_inside_a_screen_shown_twice_has_a_reference_per_whole_path() {
        let mut d = nested(vec![over(&["oa", "ib"], "R2"), over(&["ob", "ia"], "R3"), over(&["ob", "ib"], "R4"), over(&["gone", "ib"], "R9")]);
        assert_eq!(refs_by_path(&d), vec![("oa/ia".into(), "R1".into()), ("oa/ib".into(), "R2".into()), ("ob/ia".into(), "R3".into()), ("ob/ib".into(), "R4".into())]);
        assert_eq!(prune_orphaned_overrides(&mut d), 1, "the path through a sheet that is not there");
        d.schematic.as_mut().unwrap().sheets.retain(|s| s.id != "ob");
        assert_eq!(prune_orphaned_overrides(&mut d), 2, "the paths through OB went with it");
        assert_eq!(refs_by_path(&d), vec![("oa/ia".into(), "R1".into()), ("oa/ib".into(), "R2".into())]);
    }

    /// A file written before the whole path was kept: `parent_sheet_instance_id` is read as a path of one id, and where that is no path of the screen it
    /// becomes every path that ends in it -- the references the file always gave. A key that is a path (a placement on the root) is left as it is.
    #[test]
    fn a_reference_filed_under_the_immediate_placement_is_read_for_every_path_that_ends_in_it() {
        let mut d = nested(vec![]);
        let old = serde_json::json!([{ "at": { "x": 0, "y": 0 }, "parent_sheet_instance_id": "ib", "reference": "R2" }]);
        d.sheet_contents.as_mut().unwrap().get_mut("i.kicad_sch").unwrap().instance_overrides = serde_json::from_value(old).unwrap();
        let o_old = serde_json::json!([{ "at": { "x": 0, "y": 0 }, "parent_sheet_instance_id": "ob", "reference": "C2" }]);
        d.sheet_contents.as_mut().unwrap().get_mut("o.kicad_sch").unwrap().instance_overrides = serde_json::from_value(o_old).unwrap();
        // through the design's own reader, as a design.json is read
        let read: Design = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        let i = &read.sheet_contents.as_ref().unwrap()["i.kicad_sch"];
        assert_eq!(i.instance_overrides.iter().map(|o| o.sheet_path.join("/")).collect::<Vec<_>>(), vec!["oa/ib", "ob/ib"]);
        assert_eq!(refs_by_path(&read), vec![("oa/ia".into(), "R1".into()), ("oa/ib".into(), "R2".into()), ("ob/ia".into(), "R1".into()), ("ob/ib".into(), "R2".into())]);
        assert_eq!(read.sheet_contents.as_ref().unwrap()["o.kicad_sch"].instance_overrides[0].sheet_path, vec!["ob".to_string()]);
        assert_eq!(migrate_override_paths(&mut read.clone()), 0, "read once is read");
    }
}

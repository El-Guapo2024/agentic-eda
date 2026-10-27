//! The sheets of a hierarchy, in the orders eeschema walks them (`SCH_SHEET_LIST`, `eeschema/sch_sheet_path.cpp`).
//!
//! A sheet here is a *placement*: the chain of [`SheetInstance::id`]s from the root down (`SCH_SHEET_PATH`), the root being the empty
//! path. The walk is depth first with the root first (`SCH_SHEET_LIST::BuildSheetList`), the sheets of a screen in `SCH_SCREEN::GetSheets`'
//! order (by place: left to right, then top to bottom), and a sheet's *virtual page number* is its 1-based place in that walk
//! (`SCH_SHEET_PATH::GetVirtualPageNumber`). The page a sheet *shows* is the page stored for its sheet path ([`page_on`]: Edit Sheet Page
//! Number, or the number a new sheet was given, [`keep_pages`]), else its virtual page number -- which, for a schematic where no page is
//! stored, is what `SCH_SHEET_LIST::SetInitialPageNumbers` numbers it on load; Next / Previous Sheet, Find across sheets and the
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
        for child in sheets_in_order(sch) {
            let Some(content) = design.sheet_contents.as_ref().and_then(|c| c.get(&child.file)) else { continue };
            let page = page_on(sch, child, path).to_string();
            path.push(child.id.clone());
            out.push(SheetEntry { path: path.clone(), name: child.name.clone(), file: child.file.clone(), page, virtual_page: out.len() + 1 });
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

/// `SCH_SCREEN::GetSheets`: the sheets drawn on `sch` in the order the hierarchy is walked -- by the x of their place, then its y, then (two
/// sheets at one place) by id, where KiCad compares the uuids (the id is the uuid for a sheet read from a file; `eda_kicad::sheet_uuid`
/// makes one from any other id, which may order such a tie otherwise).
pub fn sheets_in_order(sch: &SchematicSection) -> Vec<&SheetInstance> {
    let mut sheets: Vec<&SheetInstance> = sch.sheets.iter().collect();
    sheets.sort_by(|a, b| (a.at.x, a.at.y, &a.id).cmp(&(b.at.x, b.at.y, &b.id)));
    sheets
}

/// The page stored for `placement` (a sheet drawn on `screen`) on the sheet path `parent` + its id, `parent` being the path of the screen
/// (`SCH_SHEET::getPageNumber( aParentPath )`, which looks the `SCH_SHEET_INSTANCE` up by that path): the screen's per-path entry
/// ([`crate::sch_extras::SchExtras::sheet_pages`]), else the placement's own [`SheetInstance::page`]. Empty: none was stored.
pub fn page_on<'a>(screen: &'a SchematicSection, placement: &'a SheetInstance, parent: &[String]) -> &'a str {
    screen.extras.sheet_pages.get(&placement.id).and_then(|m| m.get(&parent.join("/"))).map_or(placement.page.as_str(), String::as_str)
}

/// The screen of the sheet at `path`, to change (the root's `design.schematic` for the empty path).
fn screen_of_mut<'a>(design: &'a mut Design, path: &[String]) -> Option<&'a mut SchematicSection> {
    let mut file: Option<String> = None;
    {
        let mut here = design.schematic.as_ref()?;
        for id in path {
            let placement = here.sheets.iter().find(|s| s.id == *id)?;
            here = design.sheet_contents.as_ref()?.get(&placement.file)?;
            file = Some(placement.file.clone());
        }
    }
    match file {
        None => design.schematic.as_mut(),
        Some(f) => design.sheet_contents.as_mut()?.get_mut(&f),
    }
}

/// `SCH_SHEET_PATH::SetPageNumber`: store `page` for the sheet at `path` only (`SCH_SHEET::setPageNumber( aParentPath, aPageNumber )`, its
/// instance on that path). A sheet whose screen is shown once has one path and its page is [`SheetInstance::page`]; a sheet on a screen shown on
/// several paths gets an entry of that screen's [`crate::sch_extras::SchExtras::sheet_pages`]. False when there is no sheet at `path`.
pub fn set_page(design: &mut Design, path: &[String], page: &str) -> bool {
    let Some((id, parent)) = path.split_last() else { return false };
    let shown_on = match parent.split_last() {
        None => 1,
        Some(_) => match placement_at(design, parent) {
            Some(p) => placement_counts(design).get(&p.file).copied().unwrap_or(0),
            None => return false,
        },
    };
    let Some(screen) = screen_of_mut(design, parent) else { return false };
    if !screen.sheets.iter().any(|s| s.id == *id) {
        return false;
    }
    if shown_on <= 1 {
        screen.extras.sheet_pages.remove(id);
        if let Some(s) = screen.sheets.iter_mut().find(|s| s.id == *id) {
            s.page = page.to_string();
        }
    } else {
        screen.extras.sheet_pages.entry(id.clone()).or_default().insert(parent.join("/"), page.to_string());
    }
    true
}

/// The placement `id` (on whatever screen it is drawn), its per-path pages dropped, so that what is set on its own [`SheetInstance::page`] is
/// the page of every sheet path it is on. `None` when no screen draws a sheet with that id.
pub fn placement_for_every_path<'a>(design: &'a mut Design, id: &str) -> Option<&'a mut SheetInstance> {
    let screens = design.schematic.iter_mut().chain(design.sheet_contents.iter_mut().flat_map(|c| c.values_mut()));
    for screen in screens {
        if screen.sheets.iter().any(|s| s.id == id) {
            screen.extras.sheet_pages.remove(id);
            return screen.sheets.iter_mut().find(|s| s.id == id);
        }
    }
    None
}

/// `SCH_SHEET_LIST::GetNextPageNumber`: the first whole number from 1 up that no page of `pages` is (a page counts when it reads as a whole
/// number above 0, `ToLong`).
pub fn next_page_number<'a>(pages: impl IntoIterator<Item = &'a str>) -> String {
    let used: std::collections::BTreeSet<i64> = pages.into_iter().filter_map(to_long).filter(|n| *n > 0).collect();
    let mut next = 1;
    while used.contains(&next) {
        next += 1;
    }
    next.to_string()
}

/// After an edit, the page numbers KiCad would show: KiCad stores a page for every sheet path -- `SCH_SHEET_LIST::SetInitialPageNumbers` numbers
/// a schematic whose pages are all empty when it is loaded (`SCH_EDIT_FRAME::OpenProjectFiles`, eeschema/files-io.cpp), and a sheet path made
/// later gets a number of its own: a drawn sheet `GetNextPageNumber` (`SCH_DRAWING_TOOLS::DrawSheet`), an imported one and the sheets inside it
/// the lowest free one (`SCH_SHEET_LIST::AddNewSheetInstances`, which finds no instance of a path the file did not know, so a page the file gave
/// a sheet inside it is not kept), a pasted one and the sheets inside it the first free one (`SCH_EDITOR_CONTROL::Paste`, `PageNumberExists`) --
/// and a page once given stays when other sheets come and go. Here a sheet path with no stored page shows its virtual page number, so:
///  * a path of `before` (the walk before the edit) that had no stored page and still has none, but whose place in the walk moved (a sheet came
///    or went before it), keeps the page it showed: it is stored;
///  * a path new to the walk gets the next free page number ([`next_page_number`] over the pages the other paths show), in walk order.
///
/// A walk with no path of `before` left but the root (the schematic replaced, Reorganize into Module Sheets) is a schematic loaded anew and is
/// left to its virtual page numbers, as `SetInitialPageNumbers` would number it. The three KiCad rules for a new path differ only for a page that
/// is a whole number written otherwise (`"01"`): one rule, `GetNextPageNumber`'s, is used here. Returns whether anything was stored.
pub fn keep_pages(design: &mut Design, before: &[SheetEntry]) -> bool {
    let after = walk(design);
    let old: std::collections::BTreeMap<&[String], &SheetEntry> = before.iter().map(|e| (e.path.as_slice(), e)).collect();
    if !after.iter().skip(1).any(|e| old.contains_key(e.path.as_slice())) {
        return false;
    }
    let mut changed = false;
    for e in after.iter().skip(1) {
        let Some(b) = old.get(e.path.as_slice()) else { continue };
        if b.page.is_empty() && e.page.is_empty() && b.virtual_page != e.virtual_page {
            changed |= set_page(design, &e.path, &b.effective_page());
        }
    }
    let after = if changed { walk(design) } else { after };
    let mut used: Vec<String> = after.iter().filter(|e| e.path.is_empty() || old.contains_key(e.path.as_slice())).map(SheetEntry::effective_page).collect();
    for e in after.iter().skip(1).filter(|e| !old.contains_key(e.path.as_slice())) {
        let page = next_page_number(used.iter().map(String::as_str));
        changed |= set_page(design, &e.path, &page);
        used.push(page);
    }
    changed
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
/// its screen, or that no path ends in (an orphan, as before), is left as it is; of two references then filed under one path for one symbol, the
/// first stays (the one the old lookup found). Returns how many keys were changed.
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
        // an old file could hold two references under one id (an import keyed by the last id of two whole paths): the first is the one the
        // lookup found, and the later one, which could never win, is dropped
        let mut seen: std::collections::BTreeSet<(Vec<String>, crate::ir::Point, u32)> = std::collections::BTreeSet::new();
        out.retain(|o| seen.insert((o.sheet_path.clone(), o.at, o.unit)));
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

    fn at_x(mut p: SheetInstance, x_mm: f64) -> SheetInstance {
        p.at = Point { x: Nm::from_mm(x_mm), y: Nm::ZERO };
        p
    }

    fn pages(d: &Design) -> Vec<(String, String)> {
        walk(d).into_iter().map(|e| (e.path_text(), e.effective_page())).collect()
    }

    #[test]
    fn a_screen_s_sheets_are_walked_by_place_left_to_right_then_top_to_bottom() {
        let mut low = at_x(placement("low", "Low", "a.kicad_sch", ""), 10.0);
        low.at.y = Nm::from_mm(50.0);
        let d = design(
            section(vec![at_x(placement("c", "C", "a.kicad_sch", ""), 90.0), low, at_x(placement("b", "B", "a.kicad_sch", ""), 10.0), at_x(placement("a", "A", "a.kicad_sch", ""), 0.0)]),
            vec![("a.kicad_sch", section(vec![]))],
        );
        assert_eq!(walk(&d).iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["", "A", "B", "Low", "C"], "x first, then y (SCH_SCREEN::GetSheets)");
    }

    #[test]
    fn the_next_page_number_is_the_first_whole_number_no_page_is() {
        assert_eq!(next_page_number(["1", "2", "4", "x", "-3"]), "3");
        assert_eq!(next_page_number([]), "1");
        assert_eq!(next_page_number(["01", "2"]), "3", "GetNextPageNumber reads a page as a number");
    }

    /// root, then A, B, C left to right, each with a screen of its own; no page stored.
    fn three_sheets() -> Design {
        design(
            section(vec![at_x(placement("a", "A", "a.kicad_sch", ""), 0.0), at_x(placement("b", "B", "b.kicad_sch", ""), 10.0), at_x(placement("c", "C", "c.kicad_sch", ""), 20.0)]),
            vec![("a.kicad_sch", section(vec![])), ("b.kicad_sch", section(vec![])), ("c.kicad_sch", section(vec![])), ("d.kicad_sch", section(vec![]))],
        )
    }

    #[test]
    fn a_new_sheet_gets_the_next_free_page_and_the_others_keep_theirs() {
        let mut d = three_sheets();
        let s = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(pages(&d), s(&[("", "1"), ("a", "2"), ("b", "3"), ("c", "4")]), "no page stored: SetInitialPageNumbers' numbers");
        // a sheet drawn inside A comes second in the walk: B and C would move on, KiCad keeps the pages they were given on load
        let before = walk(&d);
        d.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap().sheets.push(placement("d", "D", "d.kicad_sch", "7"));
        assert!(keep_pages(&mut d, &before));
        assert_eq!(pages(&d), s(&[("", "1"), ("a", "2"), ("a/d", "5"), ("b", "3"), ("c", "4")]), "D: GetNextPageNumber, the page the copy carried is not kept");
        // a sheet deleted before them: the pages stay
        let before = walk(&d);
        d.schematic.as_mut().unwrap().sheets.retain(|p| p.id != "a");
        assert!(!keep_pages(&mut d, &before), "B and C hold their pages already");
        assert_eq!(pages(&d), s(&[("", "1"), ("b", "3"), ("c", "4")]));
        // a sheet deleted from a schematic with no page stored: the ones after it keep the pages they showed
        let mut d = three_sheets();
        let before = walk(&d);
        d.schematic.as_mut().unwrap().sheets.retain(|p| p.id != "a");
        keep_pages(&mut d, &before);
        assert_eq!(pages(&d), s(&[("", "1"), ("b", "3"), ("c", "4")]));
        // nothing moved: nothing is stored
        let mut d = three_sheets();
        let before = walk(&d);
        assert!(!keep_pages(&mut d, &before));
        assert!(d.schematic.as_ref().unwrap().sheets.iter().all(|p| p.page.is_empty()));
    }

    #[test]
    fn a_schematic_replaced_whole_is_numbered_as_loaded() {
        let mut d = three_sheets();
        let before = walk(&d);
        d.schematic = Some(section(vec![at_x(placement("x", "X", "a.kicad_sch", ""), 0.0)]));
        assert!(!keep_pages(&mut d, &before));
        assert!(d.schematic.as_ref().unwrap().sheets[0].page.is_empty());
    }

    #[test]
    fn a_sheet_on_a_screen_shown_twice_has_a_page_per_path() {
        // P1 and P2 both show s.kicad_sch, which draws N
        let mut d = design(
            section(vec![at_x(placement("p1", "P1", "s.kicad_sch", ""), 0.0), at_x(placement("p2", "P2", "s.kicad_sch", ""), 10.0)]),
            vec![("s.kicad_sch", section(vec![placement("n", "N", "n.kicad_sch", "")])), ("n.kicad_sch", section(vec![]))],
        );
        let shown = |d: &Design| pages(d).into_iter().map(|(_, p)| p).collect::<Vec<_>>();
        assert_eq!(shown(&d), vec!["1", "2", "3", "4", "5"]);
        let n = |p: &str| vec![p.to_string(), "n".to_string()];
        assert!(set_page(&mut d, &n("p2"), "9"));
        assert_eq!(shown(&d), vec!["1", "2", "3", "4", "9"], "P1's N keeps its page");
        let screen = &d.sheet_contents.as_ref().unwrap()["s.kicad_sch"];
        assert_eq!((screen.sheets[0].page.as_str(), screen.extras.sheet_pages["n"]["p2"].as_str()), ("", "9"));
        // a page set on a sheet shown once is the sheet's own
        assert!(set_page(&mut d, &["p1".to_string()], "A"));
        assert_eq!(d.schematic.as_ref().unwrap().sheets[0].page, "A");
        assert!(!set_page(&mut d, &["p1".to_string(), "nope".to_string()], "2"), "no such sheet");
        // the whole sheet, every path
        placement_for_every_path(&mut d, "n").unwrap().page = "4".into();
        assert_eq!(shown(&d), vec!["1", "A", "4", "4", "4"], "N is page 4 on both paths (P2 is page 4 by its place)");
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

    /// An old import of a screen shown twice inside a screen shown twice kept two references under the one last id (`issue10926_1`): the first, which
    /// the old lookup found, is the one kept for each path; the second could never win and is not carried on.
    #[test]
    fn two_old_references_under_one_id_keep_the_first_once_per_path() {
        let mut d = nested(vec![]);
        let old = serde_json::json!([{ "at": { "x": 0, "y": 0 }, "parent_sheet_instance_id": "ib", "reference": "U1" }, { "at": { "x": 0, "y": 0 }, "parent_sheet_instance_id": "ib", "reference": "U4" }]);
        d.sheet_contents.as_mut().unwrap().get_mut("i.kicad_sch").unwrap().instance_overrides = serde_json::from_value(old).unwrap();
        let read: Design = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        let i = &read.sheet_contents.as_ref().unwrap()["i.kicad_sch"];
        assert_eq!(i.instance_overrides.iter().map(|o| (o.sheet_path.join("/"), o.reference.as_str())).collect::<Vec<_>>(), vec![("oa/ib".to_string(), "U1"), ("ob/ib".to_string(), "U1")]);
    }
}

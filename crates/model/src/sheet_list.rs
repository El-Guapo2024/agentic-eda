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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Point, Provenance};
    use std::collections::BTreeMap;

    fn section(sheets: Vec<SheetInstance>) -> SchematicSection {
        SchematicSection { sheets, ..Default::default() }
    }

    fn placement(id: &str, name: &str, file: &str, page: &str) -> SheetInstance {
        SheetInstance { id: id.into(), name: name.into(), file: file.into(), at: Point { x: 0, y: 0 }, size: (1000, 1000), pins: vec![], page: page.into() }
    }

    fn design(root: SchematicSection, screens: Vec<(&str, SchematicSection)>) -> Design {
        let contents: BTreeMap<String, SchematicSection> = screens.into_iter().map(|(f, s)| (f.to_string(), s)).collect();
        Design {
            schema: 1,
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
}

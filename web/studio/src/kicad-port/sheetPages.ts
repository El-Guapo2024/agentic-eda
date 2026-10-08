// Sheet page numbers and the order the hierarchy is walked in: `SCH_SHEET::ComparePageNum`, `SCH_SHEET_LIST::SortByPageNumbers` and
// `SCH_NAVIGATE_TOOL::Next / Previous` (eeschema/sch_sheet.cpp, sch_sheet_path.cpp, tools/sch_navigate_tool.cpp at 8303b2ad).
//
// KiCad numbers every sheet in the hierarchy ("virtual page number": its place in the depth-first walk) and lets the user override a sheet's
// page number with any letters-and-digits text. The sheet list is then sorted by page number -- numbers first, in numeric order, then the
// other texts in natural order -- and Next Sheet / Previous Sheet step along that sorted list. A sheet whose page was never set takes its
// virtual page number as its page (`SheetInstance::page` is empty then).

/** One sheet of the hierarchy as `GET /api/sch/hierarchy` lists it: the placement ids from the root down (`[]` is the root). */
export interface HierarchySheet {
  path: string[];
  name: string;
  file: string;
  /** The page number the user gave it; empty when none. */
  page: string;
}

const isDigit = (c: string): boolean => c >= "0" && c <= "9";

/** `wxString::ToLong`: the whole string is one integer (a sign and leading blanks are allowed), else null. */
export function toLong(s: string): number | null {
  return /^\s*[+-]?[0-9]+$/.test(s) ? Number.parseInt(s, 10) : null;
}

/** `StrNumCmp` (common/string_utils.cpp): a natural compare -- runs of digits compare as numbers, everything else by character. */
export function strNumCmp(s1: string, s2: string): number {
  let i = 0;
  let j = 0;
  while (i < s1.length && j < s2.length) {
    let c1 = s1[i]!;
    let c2 = s2[j]!;
    if (isDigit(c1) && isDigit(c2)) {
      // both characters are digits: compare the two numbers
      let nb1 = 0;
      let nb2 = 0;
      do {
        nb1 = nb1 * 10 + (s1.charCodeAt(i) - 48);
        i++;
      } while (i < s1.length && isDigit(s1[i]!));
      do {
        nb2 = nb2 * 10 + (s2.charCodeAt(j) - 48);
        j++;
      } while (j < s2.length && isDigit(s2[j]!));
      if (nb1 < nb2) return -1;
      if (nb1 > nb2) return 1;
      c1 = i < s1.length ? s1[i]! : "\0";
      c2 = j < s2.length ? s2[j]! : "\0";
    }
    // any numerical comparisons to here are identical
    if (c1 < c2) return -1;
    if (c1 > c2) return 1;
    if (i < s1.length) i++;
    if (j < s2.length) j++;
  }
  if (i >= s1.length && j < s2.length) return -1;
  if (i < s1.length && j >= s2.length) return 1;
  return 0;
}

/**
 * `SCH_SHEET::ComparePageNum`: equal texts are equal; two integers compare as numbers; an integer comes before any other text; two other
 * texts compare naturally. (The C++ answers 1 for two integers that are equal but written differently, "01" and "1"; they are equal
 * here, so the sheet order breaks the tie.)
 */
export function comparePageNum(a: string, b: string): number {
  if (a === b) return 0;
  const na = toLong(a);
  const nb = toLong(b);
  if (na !== null && nb !== null) return na < nb ? -1 : na > nb ? 1 : 0;
  if (na !== null) return -1;
  if (nb !== null) return 1;
  return Math.sign(strNumCmp(a, b));
}

/** A sheet's page as it counts: the one the user set, else its place in the walk (`GetPageNumberAsInt` falls back to the virtual page number). */
export function effectivePage(sheet: HierarchySheet, walkIndex: number): string {
  return sheet.page !== "" ? sheet.page : String(walkIndex + 1);
}

/** `SCH_SHEET_LIST::SortByPageNumbers`: `sheets` in walk order (the root first, depth first) -> in page order; the walk order breaks ties. */
export function sortByPageNumbers(sheets: readonly HierarchySheet[]): HierarchySheet[] {
  return sheets
    .map((sheet, i) => ({ sheet, i, page: effectivePage(sheet, i) }))
    .sort((a, b) => comparePageNum(a.page, b.page) || a.i - b.i)
    .map((x) => x.sheet);
}

export function samePath(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((id, i) => id === b[i]);
}

/**
 * `SCH_NAVIGATE_TOOL::Next` / `Previous`: the sheet one step along the page order from `current` (`delta` +1 or -1), or `null` at the end of
 * it (`CanGoNext` / `CanGoPrevious` are false: the C++ rings the bell) or when `current` is not in the list.
 */
export function neighbourSheet(sheets: readonly HierarchySheet[], current: readonly string[], delta: 1 | -1): HierarchySheet | null {
  const sorted = sortByPageNumbers(sheets);
  const at = sorted.findIndex((s) => samePath(s.path, current));
  if (at < 0) return null;
  return sorted[at + delta] ?? null;
}

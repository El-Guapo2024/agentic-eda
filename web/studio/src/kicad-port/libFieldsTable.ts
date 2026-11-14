// The Symbol Editor's Bulk Edit Symbol Fields table (`DIALOG_LIB_FIELDS_TABLE`, eeschema/dialogs/dialog_lib_fields_table.cpp, and its grid model
// `LIB_FIELDS_EDITOR_GRID_DATA_MODEL`, eeschema/lib_fields_data_model.cpp, at KiCad 8303b2ad): the view controls of its side panel (`m_viewControlsGrid`: each column's Field,
// Show and Group By, `UpdateFieldList`'s columns and which start shown), Add / Remove / Rename Field and their refusals, and the rows the grid shows -- the symbols filtered
// (`WildCompareString` over every column), grouped by the Group By columns while "Group symbols" is on (`RebuildRows`, `groupMatch`), sorted (`Sort`, `cmp`), a group's cells
// one value or "-- mixed values --" (`GetValue`), and a group expanded into its symbols (`ExpandRow`). UI-LEFT-3 Y3-5.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import { wildCompare } from "./libeval";

/** `LIB_FIELDS_EDITOR_GRID_DATA_MODEL::SYMBOL_NAME`. */
export const SYMBOL_NAME = "Symbol Name";
/** `INDETERMINATE_STATE` (include/widgets/ui_common.h): a group's cell whose symbols differ. */
export const INDETERMINATE_STATE = "-- mixed values --";

/** A column of the grid and its row of the view controls (`LIB_DATA_MODEL_COL`, `VIEW_CONTROLS_GRID_DATA_MODEL`'s row). */
export interface LibFieldsColumn {
  /** The canonical field name (`m_fieldName`): a field's name, `Symbol Name`, `Keywords`, or an attribute's (`${EXCLUDE_FROM_BOM}` ..., `Power`, `LocalPower`). */
  name: string;
  /** What the header and the view controls' Field column show. */
  label: string;
  show: boolean;
  groupBy: boolean;
  /** `m_userAdded`: made by Add Field, so every symbol gets the field on OK even when its cell is empty. */
  userAdded: boolean;
  /** `m_isCheckbox`: an attribute shown as a check box ("1" / "0"). */
  checkbox: boolean;
}

/** The mandatory fields' columns in `UpdateFieldList`'s order with the Show box each starts with: Reference and Description hidden. */
export const MANDATORY_COLUMNS: readonly { name: string; show: boolean }[] = [
  { name: "Reference", show: false },
  { name: "Value", show: true },
  { name: "Footprint", show: true },
  { name: "Datasheet", show: true },
  { name: "Description", show: false },
];

/** `UpdateFieldList`'s attribute columns, shown as check boxes. */
export const ATTRIBUTE_COLUMNS: readonly { name: string; label: string }[] = [
  { name: "${EXCLUDE_FROM_BOM}", label: "Exclude From BOM" },
  { name: "${EXCLUDE_FROM_SIM}", label: "Exclude From Simulation" },
  { name: "${EXCLUDE_FROM_BOARD}", label: "Exclude From Board" },
  { name: "Power", label: "Power Symbol" },
  { name: "LocalPower", label: "Local Power Symbol" },
];

/** `wxString::CmpNoCase( .. ) == 0`. */
const sameNoCase = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** `GetFieldNameCol`: the column of a field name, in any case; -1 when there is none. */
export function fieldNameCol(cols: readonly LibFieldsColumn[], name: string): number {
  return cols.findIndex((c) => sameNoCase(c.name, name));
}

/** `AddField`: a column (and its view-controls row) unless one of the name, in any case, is there ("make sure we don't add them twice"). */
export function addColumn(cols: readonly LibFieldsColumn[], col: Omit<LibFieldsColumn, "userAdded" | "checkbox"> & Partial<Pick<LibFieldsColumn, "userAdded" | "checkbox">>): LibFieldsColumn[] {
  if (fieldNameCol(cols, col.name) !== -1) return [...cols];
  return [...cols, { userAdded: false, checkbox: false, ...col }];
}

/**
 * `UpdateFieldList`: Symbol Name, the mandatory fields (Reference and Description hidden), Keywords, the attributes, then the user fields any listed symbol has (its private
 * ones left out), one column per name in any case -- named as most symbols spell it (the first of the spellings, in order, on a tie) -- sorted without regard to case.
 * `userFields` holds each symbol's user fields.
 */
export function initialColumns(userFields: readonly (readonly { name: string; private?: boolean }[])[]): LibFieldsColumn[] {
  let cols: LibFieldsColumn[] = [];
  cols = addColumn(cols, { name: SYMBOL_NAME, label: "Symbol Name", show: true, groupBy: false });
  for (const m of MANDATORY_COLUMNS) cols = addColumn(cols, { name: m.name, label: m.name, show: m.show, groupBy: false });
  cols = addColumn(cols, { name: "Keywords", label: "Keywords", show: true, groupBy: false });
  for (const a of ATTRIBUTE_COLUMNS) cols = addColumn(cols, { name: a.name, label: a.label, show: true, groupBy: false, checkbox: true });
  // `std::map<wxString, std::map<wxString, int>, caseInsensitiveLess> userFieldGroups`
  const groups = new Map<string, Map<string, number>>();
  for (const fields of userFields) {
    for (const f of fields) {
      if (f.private) continue;
      const key = f.name.toLowerCase();
      const exact = groups.get(key) ?? new Map<string, number>();
      exact.set(f.name, (exact.get(f.name) ?? 0) + 1);
      groups.set(key, exact);
    }
  }
  const byKey = [...groups.entries()].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
  for (const [, exact] of byKey) {
    let canonical = "";
    let best = -1;
    for (const [name, count] of [...exact.entries()].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0))) {
      if (count > best) {
        best = count;
        canonical = name;
      }
    }
    cols = addColumn(cols, { name: canonical, label: canonical, show: true, groupBy: false });
  }
  return cols;
}

/** `OnAddField`'s refusals: "Field must have a name.", and a name a column has in any case "Field name '%s' already in use."; null when it may be added. */
export function addFieldProblem(cols: readonly LibFieldsColumn[], name: string): string | null {
  if (name === "") return "Field must have a name.";
  if (fieldNameCol(cols, name) !== -1) return `Field name '${name}' already in use.`;
  return null;
}

/** `OnRenameField`'s refusal: a name a column has (in any case, its own too) is "Field name '%s' already exists."; null when it may be renamed (the same name is no change). */
export function renameFieldProblem(cols: readonly LibFieldsColumn[], from: string, to: string): string | null {
  if (to === from) return null;
  if (fieldNameCol(cols, to) !== -1) return `Field name '${to}' already exists.`;
  return null;
}

/** `OnRemoveField`'s question. */
export const removeFieldQuestion = (label: string) => `Are you sure you want to remove the field '${label}'?`;

/** A symbol as the grid reads it: its id (lib_id), its name, and the current value of each column (`m_dataStore[ uuid ][ field ].m_currentData`). */
export interface LibFieldsSymbol {
  id: string;
  name: string;
  value: (col: string) => string;
}

/** A row of the grid: one symbol, a group of several (collapsed or expanded), or a symbol of an expanded group (`GROUP_SINGLETON`, `GROUP_COLLAPSED`, `GROUP_EXPANDED`, `CHILD_ITEM`). */
export interface LibFieldsRow {
  kind: "single" | "group" | "child";
  ids: string[];
  expanded?: boolean;
}

/** `GetValue( group, col )`: the Symbol Name column lists the group's names (sorted, without empties, `,`-joined); another column is its symbols' one value, or `INDETERMINATE_STATE`. */
export function groupValue(symbols: readonly LibFieldsSymbol[], col: string): string {
  if (col === SYMBOL_NAME) {
    const names = [...new Set(symbols.map((s) => s.value(col)))].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
    return names.filter((n) => n !== "").join(",");
  }
  let value: string | null = null;
  for (const s of symbols) {
    const v = s.value(col);
    if (value === null) value = v;
    else if (v !== value) return INDETERMINATE_STATE;
  }
  return value ?? INDETERMINATE_STATE;
}

/** `groupMatch`: two symbols are one group when every Group By column is the same, and there is at least one. */
function groupMatch(a: LibFieldsSymbol, b: LibFieldsSymbol, cols: readonly LibFieldsColumn[]): boolean {
  let found = false;
  for (const c of cols) {
    if (!c.groupBy) continue;
    if (a.value(c.name) !== b.value(c.name)) return false;
    found = true;
  }
  return found;
}

/** `StrNumCmp( a, b, true )`: digits compared as numbers, case ignored. */
function strNumCmp(a: string, b: string): number {
  return a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });
}

/** The grid's sort: a column and its direction (`SetSorting`); Symbol Name ascending at first (`SetupAllColumnProperties`). */
export interface LibFieldsSort {
  col: string;
  ascending: boolean;
}

/**
 * `cmp`: the rows by the sort column's value (trimmed), compared as strings; two groups of several symbols with the same value by their second symbol's name; empty groups
 * last. A plain comparison either way, as `local_cmp` is.
 */
function rowCmp(a: LibFieldsSymbol[], b: LibFieldsSymbol[], sort: LibFieldsSort): number {
  if (a.length === 0) return 1;
  if (b.length === 0) return -1;
  const lhs = groupValue(a, sort.col).trim();
  const rhs = groupValue(b, sort.col).trim();
  const cmp = (x: string, y: string) => (x < y ? -1 : x > y ? 1 : 0) * (sort.ascending ? 1 : -1);
  if (lhs === rhs && a.length > 1 && b.length > 1) return cmp(a[1]!.name, b[1]!.name);
  return cmp(lhs, rhs);
}

/**
 * `RebuildRows` then `Sort`: the symbols whose value of some column matches the filter (`WildCompareString( m_filter, value, false )`, an empty filter keeping all), in one row
 * each, or -- with grouping on -- gathered into the row of the first symbol they match on every Group By column; each row's symbols in name order, the rows in the sort's,
 * and a group listed in `expanded` (by its first symbol) followed by its symbols (`ExpandRow`, sorted the same way).
 */
export function buildRows(symbols: readonly LibFieldsSymbol[], cols: readonly LibFieldsColumn[], opts: { grouping: boolean; filter: string; sort: LibFieldsSort; expanded?: ReadonlySet<string> }): LibFieldsRow[] {
  const groups: LibFieldsSymbol[][] = [];
  for (const s of symbols) {
    if (opts.filter !== "" && !cols.some((c) => wildCompare(opts.filter, s.value(c.name), false))) continue;
    const into = opts.grouping ? groups.find((g) => groupMatch(s, g[0]!, cols)) : undefined;
    if (into) into.push(s);
    else groups.push([s]);
  }
  for (const g of groups) g.sort((a, b) => strNumCmp(a.name, b.name));
  const order = groups.map((g, i) => ({ g, i })).sort((x, y) => rowCmp(x.g, y.g, opts.sort) || x.i - y.i);
  const rows: LibFieldsRow[] = [];
  for (const { g } of order) {
    if (g.length === 1) {
      rows.push({ kind: "single", ids: [g[0]!.id] });
      continue;
    }
    const expanded = !!opts.expanded?.has(g[0]!.id);
    rows.push({ kind: "group", ids: g.map((s) => s.id), expanded });
    if (expanded) {
      const children = g.map((s, i) => ({ s, i })).sort((x, y) => rowCmp([x.s], [y.s], opts.sort) || x.i - y.i);
      for (const { s } of children) rows.push({ kind: "child", ids: [s.id] });
    }
  }
  return rows;
}

/** `IsExpanderColumn`: the first column shown holds a group's expander. */
export function expanderColumn(cols: readonly LibFieldsColumn[]): string | null {
  return cols.find((c) => c.show)?.name ?? null;
}

/** `OnColSort`: a click on the column sorted by turns its direction round; on another column it sorts by that one, ascending. */
export function sortedBy(sort: LibFieldsSort, col: string): LibFieldsSort {
  return sort.col === col ? { col, ascending: !sort.ascending } : { col, ascending: true };
}

/** A cell of one symbol as the grid's data store keeps it (`LIB_DATA_ELEMENT`): what it was, what it is, whether the field was / is missing, whether it changed. */
export interface LibFieldsElement {
  original: string;
  current: string;
  originallyEmpty: boolean;
  currentlyEmpty: boolean;
  modified: boolean;
}

/** How a cell is drawn (`GetAttr`): striped (`STRIPED_CELL_RENDERER`, an empty cell), its background ("window" the grid's own, "frame" a changed row's, else an RGB), bold when changed. */
export interface LibFieldsCellLook {
  striped: boolean;
  background: "window" | "frame" | string;
  bold: boolean;
}

/**
 * `LIB_FIELDS_EDITOR_GRID_DATA_MODEL::GetAttr` of a cell whose symbols' elements are `els` (one per symbol of the row), on a row where some cell changed (`rowModified`),
 * in a column that can be striped (`isStripeableField`: not a check box). An empty cell -- every symbol's field missing, or missing at first and untouched -- is striped
 * on the grid's background, coloured by what a change did: a field that was not there and still is not keeps the background, one emptied from nothing (180, 220, 180),
 * one cleared of text (220, 180, 180), one emptied by typing (180, 200, 180), else (200, 180, 180). Any other cell of a changed row has the frame's colour, (192, 255,
 * 192) when a symbol's text was changed to nothing. A changed cell is bold.
 */
export function cellLook(els: readonly LibFieldsElement[], rowModified: boolean, stripeable: boolean): LibFieldsCellLook {
  const cellModified = els.some((e) => e.modified);
  const cellEmpty = els.every((e) => e.currentlyEmpty || (e.originallyEmpty && !e.modified));
  const blankModified = els.some((e) => e.current === "" && e.modified);
  let background: LibFieldsCellLook["background"] = "window";
  if (cellEmpty && stripeable) {
    for (const e of els) {
      if (!e.modified) continue;
      if (e.currentlyEmpty) background = e.originallyEmpty ? "window" : e.original === "" ? "rgb(180, 220, 180)" : "rgb(220, 180, 180)";
      else if (e.current === "") background = "rgb(180, 200, 180)";
      else background = "rgb(200, 180, 180)";
    }
    return { striped: true, background, bold: cellModified };
  }
  if (rowModified) background = "frame";
  if (blankModified) background = "rgb(192, 255, 192)";
  return { striped: false, background, bold: cellModified };
}

/** `STRIPED_CELL_RENDERER::drawStripedBackground`'s stripe colour: light red on a dark background (luminance under 128), dark red on a light one. */
export function stripeColor(backgroundLuminance: number): string {
  return backgroundLuminance < 128 ? "rgb(220, 180, 180)" : "rgb(100, 10, 10)";
}

/** `MoveColumn( aCol, aNewPos )`: the column at `from` taken out and put back at `to` (`std::rotate`). */
export function movedColumn<T>(list: readonly T[], from: number, to: number): T[] {
  const out = [...list];
  if (from < 0 || from >= out.length || to < 0 || to >= out.length || from === to) return out;
  const [x] = out.splice(from, 1);
  out.splice(to, 0, x!);
  return out;
}

/**
 * `SetupAllColumnProperties`: a shown column's width -- the one the settings keep for its field name (`m_LibFieldEditor.field_widths`, when above 0), else its data's
 * width with the margin, between 100 px and a third of the dialog's default width.
 */
export function columnWidth(saved: Readonly<Record<string, number>>, name: string, dataWidthPx: number, dialogWidthPx = 1100): number {
  const kept = saved[name];
  if (kept !== undefined && kept > 0) return kept;
  const COLUMN_MARGIN = 15;
  return Math.min(Math.max(dataWidthPx + COLUMN_MARGIN, 100), Math.max(100, dialogWidthPx / 3));
}

/**
 * `GRID_TRICKS::paste_text`: the cells a paste writes. With a selection of more than one cell the clipboard fills the selection, repeated as often as it fits; else
 * the clipboard's block from the cursor on (rows and columns past the grid's dropped). Hidden columns are not in `cols` (the grid's shown ones). Returns
 * [row, col, text] in the grid's row and shown-column indices.
 */
export function pasteTargets(cells: readonly (readonly string[])[], cursor: { row: number; col: number }, selection: { row0: number; col0: number; row1: number; col1: number } | null, rowCount: number, colCount: number): [number, number, string][] {
  if (cells.length === 0) return [];
  const out: [number, number, string][] = [];
  const multi = selection && (selection.row1 > selection.row0 || selection.col1 > selection.col0);
  if (multi) {
    for (let r = selection.row0; r <= selection.row1; r++) {
      const line = cells[(r - selection.row0) % cells.length]!;
      for (let c = selection.col0; c <= selection.col1; c++) out.push([r, c, line[(c - selection.col0) % line.length] ?? ""]);
    }
    return out;
  }
  for (let i = 0; i < cells.length && cursor.row + i < rowCount; i++) {
    const line = cells[i]!;
    for (let j = 0; j < line.length && cursor.col + j < colCount; j++) out.push([cursor.row + i, cursor.col + j, line[j]!]);
  }
  return out;
}

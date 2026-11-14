// `DIALOG_LIB_FIELDS_TABLE` (eeschema/dialogs/dialog_lib_fields_table.cpp), `eeschema.SymbolLibraryControl.showLibraryFieldsTable` "Bulk Edit Symbol Fields..." and
// `showRelatedLibraryFieldsTable` "Bulk Edit Related Symbol Fields...": the symbols of the library -- or, with the scope "Related Symbols Only", the target's root and everything
// derived from it (`setScope`) -- as rows, their fields as columns, edited in place and saved together: "Symbol Fields Table ('%s' Library)".
//
// The side panel's view controls (`m_viewControlsGrid`, UI-LEFT-3 Y3-5): a row per column with Field, Show and Group By -- `UpdateFieldList`'s columns, Reference and Description
// hidden at first -- and Add Field / Remove Field / Rename Field under it (`OnAddField`, `OnRemoveField`, `OnRenameField`, their questions and refusals in KiCad's boxes); the
// sidebar folds away (`OnSidebarToggle`). Over the grid: the filter (`WildCompareString` over every column), the scope, "Group symbols" (on at first) and Refresh Grouping; a
// click on a header sorts by it (`OnColSort`, Symbol Name ascending at first); a group's cells show its one value or "-- mixed values --", an edit of one sets it for all its
// symbols (`SetValue`), and its first shown cell expands it into its symbols (`ExpandCollapseRow`). The rules are kicad-port/libFieldsTable.ts.
//
// The grid draws as KiCad's (`GetAttr`, `STRIPED_CELL_RENDERER`): an empty cell striped, the colours of what changed, a changed cell bold; a header dragged onto another moves
// its column (`OnColMove`, the view controls keep their order), its right edge sets the width, kept by field name when the dialog closes (`m_LibFieldEditor.field_widths`,
// with the folded sidebar); a press on a cell puts the cursor there and Shift+press makes a block, which Ctrl+C / Ctrl+X / Ctrl+V / Delete / Ctrl+A and the menu's Cut,
// Copy, Paste, Delete and Select All act on (`GRID_TRICKS`: tab-separated cells, a line a row; a paste fills a block or runs from the cursor). A Reference cell takes typing,
// shown changed, and OK drops it (`Reference is not editable from this dialog`).
//
// OK and Apply write every changed symbol in one undo step (`edit_symbol_properties` and `edit_library_symbol_fields` per symbol, a renamed field's looks with it); Cancel
// with changes asks "Save changes?" (`HandleUnsavedChanges`). As `ApplyData` does: a removed field column takes the field from every listed symbol, a renamed one renames it
// (any case), a column Add Field made gives every symbol the field, the attributes are `setAttributeValue`'s (Power off is `SetNormal`, Local Power off `SetGlobalPower`), an
// empty Value is not written ("Value field cannot be empty") and the Reference is not (`Reference is not editable from this dialog`). A derived symbol's cells are its own: an empty one inherits from the parent. Only the project library's own symbols are rows: one that comes
// from a library file or the built-in table is read-only until it is opened.
import { useEffect, useMemo, useRef, useState } from "react";
import type { Cmd, LibrarySymbol, SymbolPropertiesFields } from "../../api/types";
import { fetchSymbolEditorNames } from "../../api/client";
import { fetchAnySymbol } from "../../api/libraryClient";
import { PROJECT_LIBRARY, splitLibName } from "../../kicad-port/libraryNames";
import { allFields, bareName, defaultField, isMandatoryField, relatedSymbols, wireField, type FieldRow } from "../../kicad-port/symFields";
import {
  addColumn,
  addFieldProblem,
  buildRows,
  expanderColumn,
  groupValue,
  INDETERMINATE_STATE,
  initialColumns,
  removeFieldQuestion,
  renameFieldProblem,
  sortedBy,
  SYMBOL_NAME,
  type LibFieldsColumn,
  type LibFieldsSort,
  type LibFieldsSymbol,
  type LibFieldsElement,
  cellLook,
  stripeColor,
  movedColumn,
  columnWidth,
  pasteTargets,
} from "../../kicad-port/libFieldsTable";
import { cellsAsText, textAsCells } from "../../kicad-port/fieldsTableDialog";
import { renamedFieldLooksCmds } from "../../kicad-port/symLooks";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { notifyLibraryChanged } from "../library/useLibraryNames";
import { askChoice, askConfirm, askText, showError } from "../library/libraryDialogs";
import { SAVE_CHANGES } from "../../kicad-port/foreignImport";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { FootprintChooserDialog } from "../FootprintChooserDialog";
import { datasheetTarget } from "../../kicad-port/itemText";

/** What the table edits of a symbol: the properties `edit_symbol_properties` takes and the symbol's fields. */
interface Edit {
  props: SymbolPropertiesFields;
  fields: FieldRow[];
  /** The Reference cell's text as typed (`Reference is not editable from this dialog`: kept in the grid, not written); absent: the symbol's prefix. */
  reference?: string;
  /** The cells Clear cell emptied (`ClearCell`'s `m_currentlyEmpty`), by column. */
  cleared?: string[];
}
type Row = { libId: string; symbol: LibrarySymbol; derived: boolean; original: Edit; edited: Edit; renames: { from: string; to: string }[] };

const FIELD_KEYS: (keyof SymbolPropertiesFields)[] = ["reference_prefix", "description", "keywords", "datasheet", "power", "in_bom", "on_board", "pin_numbers_hidden", "pin_names_hidden", "pin_name_offset_mm", "unit_count", "has_alternate_body_style", "footprint_filters", "exclude_from_sim", "power_local"];

function editOf(s: LibrarySymbol): Edit {
  const out: Record<string, unknown> = {};
  for (const k of FIELD_KEYS) out[k] = s[k];
  out.exclude_from_sim = !!s.exclude_from_sim;
  out.power_local = !!s.power_local;
  return { props: out as unknown as SymbolPropertiesFields, fields: allFields(s) };
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
const propsChanged = (r: Row) => FIELD_KEYS.some((k) => !same(r.original.props[k], r.edited.props[k]));
const fieldsChanged = (r: Row) => !same(r.original.fields, r.edited.fields);
/** A change OK writes nothing for: a typed Reference, a cleared cell already empty. */
const looksChanged = (r: Row) => r.original.reference !== r.edited.reference || !same(r.original.cleared ?? [], r.edited.cleared ?? []);
const changed = (r: Row) => propsChanged(r) || fieldsChanged(r) || looksChanged(r);
const findField = (e: Edit, name: string) => e.fields.find((f) => f.name.toLowerCase() === name.toLowerCase());
/** A column that stands for a user field (not the name, the keywords, an attribute or a mandatory field). */
const isUserColumn = (c: LibFieldsColumn) => !c.checkbox && c.name !== SYMBOL_NAME && c.name !== "Keywords" && !isMandatoryField(c.name);

/** `updateDataStoreSymbolField` / `getAttributeValue`: what a cell holds. */
function cellValue(r: Row, col: string): string {
  const e = r.edited;
  switch (col) {
    case SYMBOL_NAME:
      return splitLibName(r.libId).item;
    case "${EXCLUDE_FROM_BOM}":
      return e.props.in_bom ? "0" : "1";
    case "${EXCLUDE_FROM_SIM}":
      return e.props.exclude_from_sim ? "1" : "0";
    case "${EXCLUDE_FROM_BOARD}":
      return e.props.on_board ? "0" : "1";
    case "Power":
      return e.props.power ? "1" : "0";
    case "LocalPower":
      return e.props.power && e.props.power_local ? "1" : "0";
    case "Keywords":
      return e.props.keywords;
    case "Reference":
      return e.reference ?? e.props.reference_prefix;
    case "Datasheet":
      return e.props.datasheet;
    case "Description":
      return e.props.description;
    default:
      return findField(e, col)?.text ?? "";
  }
}

/** `SetValue` of a cell (`setAttributeValue` for an attribute): the cell is no longer a cleared one; the Reference takes the typing, which OK does not write. */
function withCell(e0: Edit, col: string, text: string): Edit {
  const e = e0.cleared?.includes(col) ? { ...e0, cleared: e0.cleared.filter((c) => c !== col) } : e0;
  const props = { ...e.props };
  switch (col) {
    case SYMBOL_NAME:
      return e;
    case "Reference":
      return { ...e, reference: text };
    case "${EXCLUDE_FROM_BOM}":
      return { ...e, props: { ...props, in_bom: text !== "1" } };
    case "${EXCLUDE_FROM_SIM}":
      return { ...e, props: { ...props, exclude_from_sim: text === "1" } };
    case "${EXCLUDE_FROM_BOARD}":
      return { ...e, props: { ...props, on_board: text !== "1" } };
    // `setAttributeValue( "Power" )`: off is `SetNormal`, on `SetGlobalPower`
    case "Power":
      return { ...e, props: { ...props, power: text === "1", power_local: false } };
    // `setAttributeValue( "LocalPower" )`: on is `SetLocalPower`, off `SetGlobalPower` ("turning off local power still leaves the global flag set")
    case "LocalPower":
      return { ...e, props: { ...props, power: true, power_local: text === "1" } };
    case "Keywords":
      return { ...e, props: { ...props, keywords: text } };
  }
  if (col === "Datasheet") props.datasheet = text;
  else if (col === "Description") props.description = text;
  // "Value field cannot be empty": an emptied Value is left as it was
  if (col === "Value" && text === "") return { ...e, props };
  const i = e.fields.findIndex((f) => f.name.toLowerCase() === col.toLowerCase());
  const fields = i >= 0 ? e.fields.map((f, j) => (j === i ? { ...f, text } : f)) : [...e.fields, { ...defaultField(col), text }];
  return { ...e, props, fields };
}

/** `ClearCell`: the text gone and, but for the Keywords, the cell marked empty. */
function clearedCell(e: Edit, col: string): Edit {
  const out = col === "Reference" ? { ...e, reference: "" } : withCell(e, col, "");
  return col === "Keywords" || out.cleared?.includes(col) ? out : { ...out, cleared: [...(out.cleared ?? []), col] };
}

/** The data store's element of a symbol's cell (`LIB_DATA_ELEMENT`): a user field the symbol lacks is empty; Clear cell makes one. */
function elementOf(r: Row, c: LibFieldsColumn): LibFieldsElement {
  const original = cellValue({ ...r, edited: r.original }, c.name);
  const current = cellValue(r, c.name);
  const user = isUserColumn(c);
  const originallyEmpty = user && !findField(r.original, c.name);
  const currentlyEmpty = (user && !findField(r.edited, c.name)) || !!r.edited.cleared?.includes(c.name);
  return { original, current, originallyEmpty, currentlyEmpty, modified: current !== original || (!!r.edited.cleared?.includes(c.name) && currentlyEmpty !== originallyEmpty) };
}

/** `SYMBOL_EDITOR_SETTINGS::m_LibFieldEditor`: the column widths by field name and the folded sidebar, kept in this browser as KiCad keeps them in its settings. */
const SETTINGS_KEY = "eda.symedit.libFieldEditor";
function loadSettings(): { field_widths: Record<string, number>; sidebar_collapsed: boolean } {
  try {
    const v = JSON.parse(globalThis.localStorage?.getItem(SETTINGS_KEY) ?? "{}") as { field_widths?: Record<string, number>; sidebar_collapsed?: boolean };
    return { field_widths: v.field_widths ?? {}, sidebar_collapsed: !!v.sidebar_collapsed };
  } catch {
    return { field_widths: {}, sidebar_collapsed: false };
  }
}
function saveSettings(v: { field_widths: Record<string, number>; sidebar_collapsed: boolean }): void {
  try {
    globalThis.localStorage?.setItem(SETTINGS_KEY, JSON.stringify(v));
  } catch {
    // no storage: kept for the session
  }
}

/** `GRID_TRICKS`' clipboard when the browser's cannot be read (a page without the permission). */
let gridClipboard = "";

/** The relative luminance (0..255) of a CSS colour `rgb(..)` / `#rrggbb`, for the stripes' colour; 255 when it cannot be read. */
function luminanceOf(css: string): number {
  const m = /rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/.exec(css) ?? null;
  const h = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(css.trim());
  const [r, g, b] = m ? [Number(m[1]), Number(m[2]), Number(m[3])] : h ? [parseInt(h[1]!, 16), parseInt(h[2]!, 16), parseInt(h[3]!, 16)] : [255, 255, 255];
  return 0.299 * r + 0.587 * g + 0.114 * b;
}

/** Renames in the order made, a rename of a rename folded into one, and a rename back to the name it had dropped. */
function foldRenames(list: readonly { from: string; to: string }[]): { from: string; to: string }[] {
  let out: { from: string; to: string }[] = [];
  for (const x of list) {
    const prior = out.find((a) => a.to === x.from);
    out = prior ? out.map((a) => (a === prior ? { from: a.from, to: x.to } : a)) : [...out, x];
  }
  return out.filter((a) => a.from !== a.to);
}


export function LibraryFieldsTableDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const open = state.fieldsTableOpen;
  const [scope, setScope] = useState<"library" | "related">("library");
  const [rows, setRows] = useState<Row[]>([]);
  const [cols, setCols] = useState<LibFieldsColumn[]>([]);
  const [filter, setFilter] = useState("");
  const [grouping, setGrouping] = useState(true);
  const [sort, setSort] = useState<LibFieldsSort>({ col: SYMBOL_NAME, ascending: true });
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [settings, setSettings] = useState(loadSettings);
  const sidebar = !settings.sidebar_collapsed;
  const setSidebar = (f: (v: boolean) => boolean) => setSettings((st) => ({ ...st, sidebar_collapsed: !f(!st.sidebar_collapsed) }));
  /** The grid's own column order (`MoveColumn`; the view controls keep theirs) and the widths dragged this time. */
  const [order, setOrder] = useState<string[]>([]);
  const [widths, setWidths] = useState<Record<string, number>>({});
  /** The grid cursor and the selected block (`GetGridCursorRow/Col`, the selection), in shown rows and shown columns. */
  const [cursor, setCursor] = useState<{ row: number; col: number } | null>(null);
  const [anchor, setAnchor] = useState<{ row: number; col: number } | null>(null);
  const dragCol = useRef<string | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const [viewRow, setViewRow] = useState<number | null>(null);
  const [regroup, setRegroup] = useState(0);
  const [busy, setBusy] = useState(false);
  /** The grid's context menu (`LIB_FIELDS_EDITOR_GRID_TRICKS::showPopupMenu`) at the pointer, and the footprint chooser it may open on a row. */
  const [menu, setMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const [choosing, setChoosing] = useState<{ ids: string[]; current: string } | null>(null);
  // The library the table is about: the target's own (`GetTargetLibId().GetLibNickname()`), else the project library.
  const target = state.treeSelection[0] ?? state.libId ?? "";
  const lib = useMemo(() => splitLibName(target).lib || PROJECT_LIBRARY, [target, open]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (open) setScope(state.fieldsTableScope);
  }, [open, state.fieldsTableScope]);

  useEffect(() => {
    if (!open) return;
    let stopped = false;
    setFilter("");
    setExpanded(new Set());
    setViewRow(null);
    void (async () => {
      try {
        const names = await fetchSymbolEditorNames();
        const derived = names.derived ?? {};
        let ids = (names.project ?? []).filter((id) => (splitLibName(id).lib || PROJECT_LIBRARY) === lib);
        // `SCOPE_RELATED_SYMBOLS`: the root of the target and everything derived from it
        if (scope === "related") {
          const related = new Set(relatedSymbols(derived, target));
          ids = ids.filter((id) => related.has(id));
        }
        const loaded: Row[] = [];
        for (const id of ids) {
          const { symbol } = await fetchAnySymbol(id);
          const e = editOf(symbol);
          loaded.push({ libId: id, symbol, derived: !!symbol.extends, original: e, edited: e, renames: [] });
        }
        if (stopped) return;
        setRows(loaded);
        // `UpdateFieldList`: the view controls' rows for the symbols listed
        const initial = initialColumns(loaded.map((r) => r.edited.fields.filter((f) => !isMandatoryField(f.name))));
        setCols(initial);
        setOrder(initial.map((c) => c.name));
        setWidths({});
        setCursor(null);
        setAnchor(null);
        // `loadSymbols`: "No symbols found in library '%s'." / "No related symbols found in library '%s'." (`wxMessageBox`)
        if (loaded.length === 0) void showError(scope === "related" ? `No related symbols found in library '${lib}'.` : `No symbols found in library '${lib}'.`, undefined, "Message");
      } catch (e) {
        if (!stopped) void showError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      stopped = true;
    };
  }, [open, lib, scope, target]);

  // Create Derived Symbol's new symbol (`createActualDerivedSymbol`): a row of its own, as it was made, and the table goes on with its edits
  useEffect(() => {
    const id = state.fieldsTableDerived;
    if (!open || !id) return;
    dispatch({ type: "SET_FIELDS_TABLE_DERIVED", libId: null });
    void fetchAnySymbol(id)
      .then(({ symbol }) => {
        const e = editOf(symbol);
        setRows((rs) => (rs.some((r) => r.libId === id) ? rs : [...rs, { libId: id, symbol, derived: true, original: e, edited: e, renames: [] }]));
      })
      .catch(() => undefined);
  }, [open, state.fieldsTableDerived, dispatch]);

  const symbols: LibFieldsSymbol[] = useMemo(() => rows.map((r) => ({ id: r.libId, name: splitLibName(r.libId).item, value: (c: string) => cellValue(r, c) })), [rows]);
  // `RebuildRows`: the grouping is worked out again when a grouping control changes, the filter is typed, the sort changes or Refresh Grouping is pressed (`OnRegroupSymbols`)
  const shownRows = useMemo(() => buildRows(symbols, cols, { grouping, filter, sort, expanded }), [symbols, cols, grouping, filter, sort, expanded, regroup]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!open) return null;
  const byId = new Map(rows.map((r) => [r.libId, r]));
  const symById = new Map(symbols.map((s) => [s.id, s]));
  const colByName = new Map(cols.map((c) => [c.name, c]));
  const shownCols = [...order.filter((n) => colByName.has(n)), ...cols.map((c) => c.name).filter((n) => !order.includes(n))].map((n) => colByName.get(n)!).filter((c) => c.show);
  const expander = expanderColumn(cols);
  const dirty = rows.filter(changed);
  /** The grid's own background (`wxSYS_COLOUR_WINDOW`): the dialog's, for the stripes' colour. */
  const windowBg = (dialogRef.current && getComputedStyle(dialogRef.current).backgroundColor) || "#ffffff";

  /** The dialog's destructor: the shown columns' widths and the sidebar kept in the settings. */
  const closeNow = () => {
    const field_widths = { ...settings.field_widths };
    for (const c of shownCols) field_widths[c.name] = widthOf(c);
    saveSettings({ ...settings, field_widths });
    setSettings((st) => ({ ...st, field_widths }));
    dispatch({ type: "SET_FIELDS_TABLE_OPEN", open: false });
  };
  /** A column's width: dragged, else kept, else from its data (`SetupAllColumnProperties`). */
  const widthOf = (c: LibFieldsColumn) => widths[c.name] ?? columnWidth(settings.field_widths, c.name, Math.max(c.label.length, ...rows.map((r) => cellValue(r, c.name).length)) * 7);
  /** `SetValue` of the row `ids` stands for: a group's edit is every symbol's. */
  const setCell = (ids: readonly string[], col: string, text: string) => setRows((rs) => rs.map((r) => (ids.includes(r.libId) ? { ...r, edited: withCell(r.edited, col, text) } : r)));
  const setCol = (i: number, patch: Partial<LibFieldsColumn>) => setCols((cs) => cs.map((c, j) => (j === i ? { ...c, ...patch } : c)));
  const clearCell = (ids: readonly string[], col: string) => setRows((rs) => rs.map((r) => (ids.includes(r.libId) ? { ...r, edited: clearedCell(r.edited, col) } : r)));

  // `GRID_TRICKS`: the selected block of cells, Cut / Copy / Paste / Delete / Select All
  const sel = cursor && anchor ? { row0: Math.min(cursor.row, anchor.row), row1: Math.max(cursor.row, anchor.row), col0: Math.min(cursor.col, anchor.col), col1: Math.max(cursor.col, anchor.col) } : null;
  const inSel = (r: number, c: number) => !!sel && r >= sel.row0 && r <= sel.row1 && c >= sel.col0 && c <= sel.col1;
  const shownValue = (rowIdx: number, colIdx: number) => {
    const row = shownRows[rowIdx];
    const c = shownCols[colIdx];
    if (!row || !c) return "";
    const members = row.ids.map((id) => symById.get(id)).filter((x): x is LibFieldsSymbol => !!x);
    return members.length === 1 ? members[0]!.value(c.name) : groupValue(members, c.name);
  };
  /** `isReadOnly`: the Symbol Name (`SetValue` ignores it). */
  const writable = (colIdx: number) => !!shownCols[colIdx] && shownCols[colIdx]!.name !== SYMBOL_NAME;
  const cutCopy = async (doDelete: boolean) => {
    if (!sel) return;
    const block: string[][] = [];
    for (let r = sel.row0; r <= sel.row1; r++) {
      const line: string[] = [];
      for (let c = sel.col0; c <= sel.col1; c++) line.push(shownValue(r, c));
      block.push(line);
    }
    gridClipboard = cellsAsText(block);
    try {
      await navigator.clipboard?.writeText(gridClipboard);
    } catch {
      // the grid's own copy stays
    }
    // `cutcopy( .., doDelete )`: text entries only (not the check boxes)
    if (doDelete) for (let r = sel.row0; r <= sel.row1; r++) for (let c = sel.col0; c <= sel.col1; c++) if (writable(c) && !shownCols[c]!.checkbox) setCell(shownRows[r]!.ids, shownCols[c]!.name, "");
  };
  const paste = async () => {
    if (!cursor) return;
    let text = gridClipboard;
    try {
      const read = await navigator.clipboard?.readText();
      if (read) text = read;
    } catch {
      // the grid's own copy
    }
    for (const [r, c, t] of pasteTargets(textAsCells(text), cursor, sel, shownRows.length, shownCols.length)) {
      const col = shownCols[c];
      if (!col || !writable(c)) continue;
      // a check box takes "1" (`SetValueAsBool( row, col, cellTxt == "1" )`)
      setCell(shownRows[r]!.ids, col.name, col.checkbox ? (t === "1" ? "1" : "0") : t);
    }
  };
  const selectAll = () => {
    if (shownRows.length === 0 || shownCols.length === 0) return;
    setAnchor({ row: 0, col: 0 });
    setCursor({ row: shownRows.length - 1, col: shownCols.length - 1 });
  };
  /** `GRID_TRICKS::onKeyDown` on a block of cells (one cell's text is its input's own to copy and paste). */
  const onGridKey = (e: React.KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    const block = !!sel && (sel.row1 > sel.row0 || sel.col1 > sel.col0);
    if (mod && e.key.toLowerCase() === "a") {
      e.preventDefault();
      return selectAll();
    }
    if (!block) return;
    if (mod && e.key.toLowerCase() === "c") (e.preventDefault(), void cutCopy(false));
    else if (mod && e.key.toLowerCase() === "x") (e.preventDefault(), void cutCopy(true));
    else if (mod && e.key.toLowerCase() === "v") (e.preventDefault(), void paste());
    else if (!mod && e.key === "Delete") {
      e.preventDefault();
      for (let r = sel.row0; r <= sel.row1; r++) for (let c = sel.col0; c <= sel.col1; c++) if (writable(c) && !shownCols[c]!.checkbox) setCell(shownRows[r]!.ids, shownCols[c]!.name, "");
    }
  };
  /** A press on a cell: the cursor there, Shift extending the block (`wxGrid`'s selection). */
  const pressCell = (e: React.MouseEvent, rowIdx: number, colIdx: number) => {
    if (e.button !== 0) return;
    setCursor({ row: rowIdx, col: colIdx });
    if (!e.shiftKey || !anchor) setAnchor({ row: rowIdx, col: colIdx });
  };
  /** `OnColMove`: a header dropped on another moves its column there (`MoveColumn`); the view controls keep their order. */
  const dropOn = (name: string) => {
    const from = dragCol.current;
    dragCol.current = null;
    if (!from || from === name) return;
    const full = [...order.filter((n) => colByName.has(n)), ...cols.map((c) => c.name).filter((n) => !order.includes(n))];
    setOrder(movedColumn(full, full.indexOf(from), full.indexOf(name)));
  };
  /** `OnTableColSize`: the header's right edge dragged sets the column's width. */
  const startResize = (e: React.MouseEvent, c: LibFieldsColumn) => {
    e.preventDefault();
    e.stopPropagation();
    const x0 = e.clientX;
    const w0 = widthOf(c);
    const move = (ev: MouseEvent) => setWidths((w) => ({ ...w, [c.name]: Math.max(30, Math.round(w0 + ev.clientX - x0)) }));
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  /** `OnAddField`: "New field name:" -- refused in KiCad's Error box; a column made so gives every symbol the field (`m_userAdded`). */
  const addField = async () => {
    const name = await askText({ title: "Add Field", label: "New field name:" });
    if (name === null) return;
    const problem = addFieldProblem(cols, name);
    if (problem) return void (await showError(problem));
    setCols((cs) => addColumn(cs, { name, label: name, show: true, groupBy: false, userAdded: true }));
    setOrder((o) => (o.some((n) => n.toLowerCase() === name.toLowerCase()) ? o : [...o, name]));
    setRows((rs) => rs.map((r) => (findField(r.edited, name) ? r : { ...r, edited: { ...r.edited, fields: [...r.edited.fields, { ...defaultField(name), text: "" }] } })));
  };
  /** `OnRemoveField`: "Are you sure you want to remove the field '%s'?" (Yes / No); a user field leaves every listed symbol on OK (`ApplyData`'s `stillTracked`). */
  const removeField = async () => {
    const col = viewRow !== null ? cols[viewRow] : undefined;
    if (!col) return;
    if (!(await askConfirm({ title: "Confirmation", message: removeFieldQuestion(col.label), okLabel: "Yes", cancelLabel: "No" }))) return;
    setCols((cs) => cs.filter((c) => c !== col));
    setOrder((o) => o.filter((n) => n !== col.name));
    if (isUserColumn(col)) setRows((rs) => rs.map((r) => ({ ...r, edited: { ...r.edited, fields: r.edited.fields.filter((f) => isMandatoryField(f.name) || f.name.toLowerCase() !== col.name.toLowerCase()) } })));
    if (sort.col === col.name) setSort({ col: SYMBOL_NAME, ascending: true });
    setViewRow(null);
  };
  /** `OnRenameField`: "New field name:" with the name; the field of that name (any case) is renamed on every listed symbol, and its looks go with it. */
  const renameField = async () => {
    const at = viewRow;
    const col = at !== null ? cols[at] : undefined;
    if (at === null || !col) return;
    const to = await askText({ title: "Rename Field", label: "New field name:", initial: col.name });
    if (to === null || to === col.name) return;
    const problem = renameFieldProblem(cols, col.name, to);
    if (problem) return void (await showError(problem));
    setCol(at, { name: to, label: to });
    setOrder((o) => o.map((n) => (n === col.name ? to : n)));
    if (sort.col === col.name) setSort({ ...sort, col: to });
    if (!isUserColumn(col)) return;
    setRows((rs) =>
      rs.map((r) => {
        const f = findField(r.edited, col.name);
        if (!f) return r;
        return { ...r, edited: { ...r.edited, fields: r.edited.fields.map((x) => (x === f ? { ...x, name: to } : x)) }, renames: [...r.renames, { from: f.name, to }] };
      })
    );
  };

  /** `TransferDataFromWindow` (OK and Apply): every changed symbol, one undo step. */
  const apply = async (): Promise<boolean> => {
    if (dirty.length === 0) return true;
    setBusy(true);
    const cmds: Cmd[] = [];
    for (const r of dirty) {
      if (propsChanged(r)) cmds.push({ op: "edit_symbol_properties", lib_id: r.libId, ...r.edited.props });
      if (fieldsChanged(r)) {
        cmds.push({ op: "edit_library_symbol_fields", lib_id: r.libId, fields: r.edited.fields.map(wireField) });
        // a renamed field keeps its face, colour and pen (UI-LEFT-3 Y3-3)
        cmds.push(...renamedFieldLooksCmds(r.libId, r.symbol, foldRenames(r.renames)));
      }
    }
    // a typed Reference and an emptied empty cell write nothing (`Reference is not editable from this dialog`)
    const ok = cmds.length === 0 ? true : await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    setBusy(false);
    notifyLibraryChanged();
    if (!ok) return false;
    // `ClearModify()`: what was written is the table's new start
    setRows((rs) => rs.map((r) => ({ ...r, original: r.edited, renames: [] })));
    return true;
  };
  const ok = async () => {
    if (await apply()) closeNow();
  };
  /** `OnCancel` / `OnClose`: with changes, `HandleUnsavedChanges( this, _( "Save changes?" ), .. )` -- Save applies, Discard Changes closes, Cancel stays. */
  const cancel = async () => {
    if (dirty.length > 0) {
      const answer = await askChoice({ title: SAVE_CHANGES.title, message: `Save changes?\n\n${SAVE_CHANGES.detail}`, okLabel: SAVE_CHANGES.save, altLabel: SAVE_CHANGES.discard, cancelLabel: SAVE_CHANGES.cancel });
      if (answer === null) return;
      if (answer === "ok" && !(await apply())) return;
    }
    closeNow();
  };

  /**
   * `LIB_FIELDS_EDITOR_GRID_TRICKS::showPopupMenu` on a cell: Revert symbol (`RevertRow`, for an edited cell), Clear cell (`ClearCell`), Create Derived Symbol (a row of one
   * symbol: the Derive dialog on it, `CreateDerivedSymbolImmediate`), and Select Footprint... on a Footprint cell or Show Datasheet on a Datasheet one.
   */
  const openMenu = (e: React.MouseEvent, ids: readonly string[], c: LibFieldsColumn, single: boolean, at?: { row: number; col: number }) => {
    e.preventDefault();
    // `OnTableItemContextMenu`: the cursor goes to the cell under the pointer (the block stays when the cell is in it)
    if (at && !inSel(at.row, at.col)) {
      setCursor(at);
      setAnchor(at);
    }
    const members = ids.map((id) => byId.get(id)).filter((r): r is Row => !!r);
    const first = members[0];
    if (!first) return;
    const edited = cellValue({ ...first, edited: first.original }, c.name) !== cellValue(first, c.name);
    const clear = cellValue(first, c.name) === "" && (isUserColumn(c) ? !findField(first.edited, c.name) || findField(first.edited, c.name)!.text === "" : true);
    const value = ids.length === 1 ? cellValue(first, c.name) : groupValue(members.map((r) => symById.get(r.libId)!), c.name);
    const entries: MenuEntry[] = [
      { label: "Revert symbol", hint: "Revert the symbol to its last saved state", disabled: !edited, onSelect: () => setRows((rs) => rs.map((r) => (ids.includes(r.libId) ? { ...r, edited: r.original, renames: [] } : r))) },
      { label: "Clear cell", hint: "Clear the cell value", disabled: clear || c.checkbox || c.name === SYMBOL_NAME, onSelect: () => clearCell(ids, c.name) },
      { label: "", onSelect: () => undefined, separator: true },
      { label: "Create Derived Symbol", hint: "Create a new symbol derived from the selected one", disabled: !single, onSelect: () => dispatch({ type: "SET_DERIVE_REQUEST", parent: first.libId, fromTable: true }) },
    ];
    if (c.name === "Footprint") entries.push({ label: "Select Footprint...", hint: "Browse for footprint", onSelect: () => setChoosing({ ids: [...ids], current: value === INDETERMINATE_STATE ? "" : value }) });
    else if (c.name === "Datasheet") {
      entries.push({
        label: "Show Datasheet",
        hint: "Show datasheet in browser",
        onSelect: () => {
          const target = datasheetTarget(value);
          if (target.kind === "url") window.open(target.url, "_blank", "noopener,noreferrer");
        },
      });
    }
    // `GRID_TRICKS::showPopupMenu`: Cut and Delete for writable text cells in the block, Paste, Select All
    const anyWritable = !!sel && Array.from({ length: sel.col1 - sel.col0 + 1 }, (_, i) => sel.col0 + i).some((ci) => writable(ci) && !shownCols[ci]!.checkbox);
    entries.push(
      { label: "", onSelect: () => undefined, separator: true },
      { label: "Cut", accel: "Ctrl+X", hint: "Clear selected cells placing original contents on clipboard", disabled: !anyWritable, onSelect: () => void cutCopy(true) },
      { label: "Copy", accel: "Ctrl+C", hint: "Copy selected cells to clipboard", onSelect: () => void cutCopy(false) },
      { label: "Paste", accel: "Ctrl+V", hint: "Paste clipboard cells to matrix at current cell", onSelect: () => void paste() },
      { label: "Delete", accel: "Del", hint: "Clear contents of selected cells", disabled: !anyWritable, onSelect: () => { if (sel) for (let r = sel.row0; r <= sel.row1; r++) for (let ci = sel.col0; ci <= sel.col1; ci++) if (writable(ci) && !shownCols[ci]!.checkbox) setCell(shownRows[r]!.ids, shownCols[ci]!.name, ""); } },
      { label: "Select All", accel: "Ctrl+A", hint: "Select all cells", onSelect: selectAll }
    );
    setMenu({ x: e.clientX, y: e.clientY, entries });
  };

  const cell = (ids: readonly string[], c: LibFieldsColumn, testName: string | null, single: boolean, rowIdx: number, colIdx: number) => {
    const members = ids.map((id) => symById.get(id)).filter((s): s is LibFieldsSymbol => !!s);
    const value = members.length === 1 ? members[0]!.value(c.name) : groupValue(members, c.name);
    const mixed = value === INDETERMINATE_STATE;
    // `GetAttr`: the stripes of an empty cell, the colours of what changed, bold for a changed cell
    const rs = ids.map((id) => byId.get(id)).filter((r): r is Row => !!r);
    const look = cellLook(rs.map((r) => elementOf(r, c)), rs.some(changed), !c.checkbox);
    const bg = look.background === "window" ? undefined : look.background === "frame" ? "var(--chrome-bg-raised)" : look.background;
    const stripes = look.striped && value === "" ? `repeating-linear-gradient(45deg, transparent 0 13px, ${stripeColor(luminanceOf(bg ?? windowBg))} 13px 14px)` : undefined;
    const selected = inSel(rowIdx, colIdx);
    const tdStyle: React.CSSProperties = { background: bg, backgroundImage: stripes, outline: selected ? "1px solid var(--chrome-accent, #4aa3ff)" : undefined, outlineOffset: -1 };
    const common = {
      onContextMenu: (e: React.MouseEvent) => openMenu(e, ids, c, single, { row: rowIdx, col: colIdx }),
      onMouseDown: (e: React.MouseEvent) => pressCell(e, rowIdx, colIdx),
      "data-look": `${look.striped ? "striped" : "plain"} ${look.background}${look.bold ? " bold" : ""}`,
    };
    if (c.checkbox) {
      return (
        <td key={c.name} style={{ ...tdStyle, textAlign: "center" }} {...common}>
          <input
            type="checkbox"
            checked={value === "1"}
            ref={(el) => {
              if (el) el.indeterminate = mixed;
            }}
            onChange={(e) => setCell(ids, c.name, e.target.checked ? "1" : "0")}
            aria-label={`${c.label} of ${ids.join(", ")}`}
            data-testid={testName ? `cell-${testName}-${c.name === "${EXCLUDE_FROM_SIM}" ? "exclude-sim" : c.name === "LocalPower" ? "local-power" : c.name}` : undefined}
          />
        </td>
      );
    }
    const readOnly = c.name === SYMBOL_NAME;
    return (
      <td key={c.name} style={{ ...tdStyle, whiteSpace: "nowrap" }} {...common} data-testid={testName ? `td-${testName}-${c.name}` : undefined}>
        <input
          value={mixed ? "" : value}
          placeholder={mixed ? INDETERMINATE_STATE : undefined}
          readOnly={readOnly}
          style={{ width: "100%", boxSizing: "border-box", fontWeight: look.bold ? "bold" : undefined, ...(readOnly || stripes ? { background: "transparent" } : {}), ...(readOnly ? { border: "none" } : {}) }}
          onChange={(e) => setCell(ids, c.name, e.target.value)}
          aria-label={`${c.label} of ${ids.join(", ")}`}
          data-testid={testName ? `cell-${testName}-${c.name}` : undefined}
        />
      </td>
    );
  };

  return (
    <div className="dialog-backdrop" onClick={() => void cancel()}>
      <div ref={dialogRef} className="dialog" style={{ width: 1100, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Symbol Fields Table" data-testid="fields-table">
        <div className="dialog-header">
          <span>Symbol Fields Table ('{lib}' Library)</span>
        </div>
        <div className="dialog-body" style={{ padding: 8, display: "flex", gap: 8 }}>
          {sidebar && (
            // the left panel: `m_viewControlsGrid` (`ShowHideColumns( "0 2 3" )`: Field, Show, Group By) and its three buttons
            <div style={{ width: 230, flex: "0 0 auto", display: "flex", flexDirection: "column", gap: 6 }} data-testid="fields-view-controls">
              <div style={{ maxHeight: "55vh", overflow: "auto", border: "1px solid var(--chrome-border)" }}>
                <table className="setup-table" style={{ margin: 0, width: "100%" }}>
                  <thead>
                    <tr>
                      <th>Field</th>
                      <th>Show</th>
                      <th>Group By</th>
                    </tr>
                  </thead>
                  <tbody>
                    {cols.map((c, i) => (
                      <tr key={c.name} onClick={() => setViewRow(i)} style={viewRow === i ? { background: "var(--chrome-bg-raised)", outline: "1px solid var(--chrome-accent, #4aa3ff)" } : undefined} data-testid={`view-row-${c.name}`}>
                        <td style={{ whiteSpace: "nowrap" }}>{c.label}</td>
                        <td style={{ textAlign: "center" }}>
                          <input type="checkbox" checked={c.show} onChange={(e) => setCol(i, { show: e.target.checked })} aria-label={`Show ${c.label}`} data-testid={`view-show-${c.name}`} />
                        </td>
                        <td style={{ textAlign: "center" }}>
                          <input type="checkbox" checked={c.groupBy} onChange={(e) => setCol(i, { groupBy: e.target.checked })} aria-label={`Group by ${c.label}`} data-testid={`view-group-${c.name}`} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <div style={{ display: "flex", gap: 4 }}>
                <button onClick={() => void addField()} title="Add Field" data-testid="fields-add-field">
                  +
                </button>
                <button onClick={() => void renameField()} disabled={viewRow === null} title="Rename Field" data-testid="fields-rename-field">
                  ✎
                </button>
                <button onClick={() => void removeField()} disabled={viewRow === null} title="Remove Field" data-testid="fields-remove-field">
                  🗑
                </button>
              </div>
            </div>
          )}
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", gap: 8, marginBottom: 6, alignItems: "center", flexWrap: "wrap" }}>
              <input value={filter} placeholder="Filter" onChange={(e) => setFilter(e.target.value)} style={{ width: 200 }} aria-label="Filter" data-testid="fields-filter" />
              <label>
                Scope:{" "}
                <select value={scope} onChange={(e) => setScope(e.target.value as "library" | "related")} data-testid="fields-scope" aria-label="Scope">
                  <option value="library">Whole Library</option>
                  <option value="related">Related Symbols Only</option>
                </select>
              </label>
              <label title="Group symbols together based on common properties">
                <input type="checkbox" checked={grouping} onChange={(e) => setGrouping(e.target.checked)} data-testid="fields-group-symbols" /> Group symbols
              </label>
              <button onClick={() => setRegroup((n) => n + 1)} title="Refresh Grouping" data-testid="fields-regroup">
                ⟳
              </button>
            </div>
            <div style={{ maxHeight: "55vh", overflow: "auto", border: "1px solid var(--chrome-border)" }}>
              <table className="setup-table" style={{ margin: 0, tableLayout: "fixed", width: shownCols.reduce((w, c) => w + widthOf(c), 0) }} onKeyDown={onGridKey} data-testid="fields-grid">
                <thead>
                  <tr>
                    {shownCols.map((c) => (
                      <th
                        key={c.name}
                        draggable
                        onDragStart={() => (dragCol.current = c.name)}
                        onDragOver={(e) => e.preventDefault()}
                        onDrop={(e) => (e.preventDefault(), dropOn(c.name))}
                        onClick={() => setSort((s) => sortedBy(s, c.name))}
                        style={{ cursor: "pointer", whiteSpace: "nowrap", width: widthOf(c), position: "relative", overflow: "hidden", textOverflow: "ellipsis" }}
                        data-testid={`fields-head-${c.name}`}
                      >
                        {c.label}
                        {sort.col === c.name ? (sort.ascending ? " ▲" : " ▼") : ""}
                        <span onMouseDown={(e) => startResize(e, c)} onClick={(e) => e.stopPropagation()} style={{ position: "absolute", right: 0, top: 0, bottom: 0, width: 5, cursor: "col-resize" }} data-testid={`fields-resize-${c.name}`} />
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {shownRows.map((row, rowIdx) => {
                    const first = row.ids[0]!;
                    const one = row.kind !== "group" ? byId.get(first) : undefined;
                    const testName = one ? bareName(one.libId) : null;
                    const groupName = row.ids.map((id) => bareName(id)).join(",");
                    return (
                      <tr
                        key={`${row.kind}:${row.ids.join("|")}`}
                        data-testid={one ? `fields-row-${testName}` : `fields-group-${groupName}`}
                      >
                        {shownCols.map((c, colIdx) => {
                          if (c.name !== expander || row.kind !== "group") return cell(row.ids, c, testName, row.kind !== "group", rowIdx, colIdx);
                          // `OnTableCellClick` on the expander column: `ExpandCollapseRow`
                          const toggle = () => setExpanded((s) => (s.has(first) ? new Set([...s].filter((x) => x !== first)) : new Set([...s, first])));
                          return (
                            <td key={c.name} style={{ whiteSpace: "nowrap" }}>
                              <button className="link-button" onClick={toggle} aria-label={row.expanded ? "Collapse group" : "Expand group"} data-testid={`fields-expand-${groupName}`}>
                                {row.expanded ? "▾" : "▸"}
                              </button>{" "}
                              {groupValue(row.ids.map((id) => symById.get(id)!).filter(Boolean), c.name)}
                            </td>
                          );
                        })}
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </div>
        </div>
        <div className="dialog-footer">
          {/* `m_sidebarButton`: the left panel folds away and back (`OnSidebarToggle`) */}
          <button onClick={() => setSidebar((v) => !v)} title={sidebar ? "Hide the view controls" : "Show the view controls"} data-testid="fields-sidebar">
            {sidebar ? "◀" : "▶"}
          </button>
          <span style={{ marginRight: "auto", color: "var(--chrome-text-dim)", fontSize: 12 }}>{dirty.length > 0 ? `${dirty.length} symbol(s) changed` : ""}</span>
          <button className="primary" disabled={busy} onClick={() => void ok()} data-testid="fields-ok">
            OK
          </button>
          <button disabled={busy} onClick={() => void apply()} data-testid="fields-apply">
            Apply
          </button>
          <button onClick={() => void cancel()} data-testid="fields-cancel">
            Cancel
          </button>
        </div>
        {menu && <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onClose={() => setMenu(null)} />}
        {choosing && (
          // `FRAME_FOOTPRINT_CHOOSER` on the cell's footprint: the one chosen goes in the cell
          <FootprintChooserDialog
            preselect={choosing.current.includes(":") ? choosing.current : null}
            onCancel={() => setChoosing(null)}
            onChoose={(pick) => {
              if (pick.kind === "footprint") setCell(choosing.ids, "Footprint", pick.name);
              setChoosing(null);
            }}
          />
        )}
      </div>
    </div>
  );
}

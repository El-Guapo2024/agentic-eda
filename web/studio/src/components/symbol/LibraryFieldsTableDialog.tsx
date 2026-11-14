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
// OK and Apply write every changed symbol in one undo step (`edit_symbol_properties` and `edit_library_symbol_fields` per symbol, a renamed field's looks with it); Cancel
// with changes asks "Save changes?" (`HandleUnsavedChanges`). As `ApplyData` does: a removed field column takes the field from every listed symbol, a renamed one renames it
// (any case), a column Add Field made gives every symbol the field, the attributes are `setAttributeValue`'s (Power off is `SetNormal`, Local Power off `SetGlobalPower`), an
// empty Value is not written ("Value field cannot be empty") and the Reference is not (`Reference is not editable from this dialog`: its cells are read-only here, where KiCad's
// take the typing and drop it). A derived symbol's cells are its own: an empty one inherits from the parent. Only the project library's own symbols are rows: one that comes
// from a library file or the built-in table is read-only until it is opened.
import { useEffect, useMemo, useState } from "react";
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
} from "../../kicad-port/libFieldsTable";
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
const changed = (r: Row) => propsChanged(r) || fieldsChanged(r);
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
      return e.props.reference_prefix;
    case "Datasheet":
      return e.props.datasheet;
    case "Description":
      return e.props.description;
    default:
      return findField(e, col)?.text ?? "";
  }
}

/** `SetValue` of a cell (`setAttributeValue` for an attribute); the Reference is not written from this dialog. */
function withCell(e: Edit, col: string, text: string): Edit {
  const props = { ...e.props };
  switch (col) {
    case SYMBOL_NAME:
    case "Reference":
      return e;
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
  return { props, fields };
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

const WIDTH: Record<string, number> = { Reference: 54, Value: 110, Footprint: 150, Datasheet: 150, Description: 190, Keywords: 120 };

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
  const [sidebar, setSidebar] = useState(true);
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
        setCols(initialColumns(loaded.map((r) => r.edited.fields.filter((f) => !isMandatoryField(f.name)))));
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
  const shownCols = cols.filter((c) => c.show);
  const expander = expanderColumn(cols);
  const dirty = rows.filter(changed);

  const closeNow = () => dispatch({ type: "SET_FIELDS_TABLE_OPEN", open: false });
  /** `SetValue` of the row `ids` stands for: a group's edit is every symbol's. */
  const setCell = (ids: readonly string[], col: string, text: string) => setRows((rs) => rs.map((r) => (ids.includes(r.libId) ? { ...r, edited: withCell(r.edited, col, text) } : r)));
  const setCol = (i: number, patch: Partial<LibFieldsColumn>) => setCols((cs) => cs.map((c, j) => (j === i ? { ...c, ...patch } : c)));

  /** `OnAddField`: "New field name:" -- refused in KiCad's Error box; a column made so gives every symbol the field (`m_userAdded`). */
  const addField = async () => {
    const name = await askText({ title: "Add Field", label: "New field name:" });
    if (name === null) return;
    const problem = addFieldProblem(cols, name);
    if (problem) return void (await showError(problem));
    setCols((cs) => addColumn(cs, { name, label: name, show: true, groupBy: false, userAdded: true }));
    setRows((rs) => rs.map((r) => (findField(r.edited, name) ? r : { ...r, edited: { ...r.edited, fields: [...r.edited.fields, { ...defaultField(name), text: "" }] } })));
  };
  /** `OnRemoveField`: "Are you sure you want to remove the field '%s'?" (Yes / No); a user field leaves every listed symbol on OK (`ApplyData`'s `stillTracked`). */
  const removeField = async () => {
    const col = viewRow !== null ? cols[viewRow] : undefined;
    if (!col) return;
    if (!(await askConfirm({ title: "Confirmation", message: removeFieldQuestion(col.label), okLabel: "Yes", cancelLabel: "No" }))) return;
    setCols((cs) => cs.filter((c) => c !== col));
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
    const ok = await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
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
  const openMenu = (e: React.MouseEvent, ids: readonly string[], c: LibFieldsColumn, single: boolean) => {
    e.preventDefault();
    const members = ids.map((id) => byId.get(id)).filter((r): r is Row => !!r);
    const first = members[0];
    if (!first) return;
    const edited = cellValue({ ...first, edited: first.original }, c.name) !== cellValue(first, c.name);
    const clear = cellValue(first, c.name) === "" && (isUserColumn(c) ? !findField(first.edited, c.name) || findField(first.edited, c.name)!.text === "" : true);
    const value = ids.length === 1 ? cellValue(first, c.name) : groupValue(members.map((r) => symById.get(r.libId)!), c.name);
    const entries: MenuEntry[] = [
      { label: "Revert symbol", hint: "Revert the symbol to its last saved state", disabled: !edited, onSelect: () => setRows((rs) => rs.map((r) => (ids.includes(r.libId) ? { ...r, edited: r.original, renames: [] } : r))) },
      { label: "Clear cell", hint: "Clear the cell value", disabled: clear || c.checkbox || c.name === SYMBOL_NAME || c.name === "Reference", onSelect: () => setCell(ids, c.name, "") },
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
    setMenu({ x: e.clientX, y: e.clientY, entries });
  };

  const cell = (ids: readonly string[], c: LibFieldsColumn, testName: string | null, single = true) => {
    const members = ids.map((id) => symById.get(id)).filter((s): s is LibFieldsSymbol => !!s);
    const value = members.length === 1 ? members[0]!.value(c.name) : groupValue(members, c.name);
    const mixed = value === INDETERMINATE_STATE;
    if (c.checkbox) {
      return (
        <td key={c.name} style={{ textAlign: "center" }} onContextMenu={(e) => openMenu(e, ids, c, single)}>
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
    const readOnly = c.name === SYMBOL_NAME || c.name === "Reference";
    return (
      <td key={c.name} style={{ whiteSpace: "nowrap" }} onContextMenu={(e) => openMenu(e, ids, c, single)} data-testid={testName ? `td-${testName}-${c.name}` : undefined}>
        <input
          value={mixed ? "" : value}
          placeholder={mixed ? INDETERMINATE_STATE : undefined}
          readOnly={readOnly}
          style={{ width: WIDTH[c.name] ?? 110, ...(readOnly ? { background: "transparent", border: "none" } : {}) }}
          onChange={(e) => setCell(ids, c.name, e.target.value)}
          aria-label={`${c.label} of ${ids.join(", ")}`}
          data-testid={testName ? `cell-${testName}-${c.name}` : undefined}
        />
      </td>
    );
  };

  return (
    <div className="dialog-backdrop" onClick={() => void cancel()}>
      <div className="dialog" style={{ width: 1100, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Symbol Fields Table" data-testid="fields-table">
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
              <table className="setup-table" style={{ margin: 0, width: "100%" }}>
                <thead>
                  <tr>
                    {shownCols.map((c) => (
                      <th key={c.name} onClick={() => setSort((s) => sortedBy(s, c.name))} style={{ cursor: "pointer", whiteSpace: "nowrap" }} data-testid={`fields-head-${c.name}`}>
                        {c.label}
                        {sort.col === c.name ? (sort.ascending ? " ▲" : " ▼") : ""}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {shownRows.map((row) => {
                    const first = row.ids[0]!;
                    const one = row.kind !== "group" ? byId.get(first) : undefined;
                    const testName = one ? bareName(one.libId) : null;
                    const groupName = row.ids.map((id) => bareName(id)).join(",");
                    return (
                      <tr
                        key={`${row.kind}:${row.ids.join("|")}`}
                        data-testid={one ? `fields-row-${testName}` : `fields-group-${groupName}`}
                        style={row.ids.some((id) => byId.get(id) && changed(byId.get(id)!)) ? { background: "var(--chrome-bg-raised)" } : undefined}
                      >
                        {shownCols.map((c) => {
                          if (c.name !== expander || row.kind !== "group") return cell(row.ids, c, testName, row.kind !== "group");
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

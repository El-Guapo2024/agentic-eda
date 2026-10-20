// `DIALOG_LIB_FIELDS_TABLE` (eeschema/dialogs/dialog_lib_fields_table.cpp), `eeschema.SymbolLibraryControl.showLibraryFieldsTable` "Bulk Edit Symbol Fields..." and
// `showRelatedLibraryFieldsTable` "Bulk Edit Related Symbol Fields...": the symbols of the library -- or, with the scope "Related Symbols", the target's root and everything
// derived from it (`setScope`) -- as rows, their fields as columns, edited in place and saved together: "Symbol Fields Table ('%s' Library)". The columns are the ones
// `UpdateFieldList` starts with as far as a library symbol keeps them: Symbol Name (never edited here), Reference (the symbol's prefix), Value, Footprint, Datasheet,
// Description, every user field any listed symbol has (sorted by name; "Add Field" makes a column of a new one), Keywords, the Exclude From BOM / Board check boxes and Power Symbol.
// A derived symbol's cells are its own: an empty one inherits from the parent. (Exclude From Simulation and Local Power are not part of the library model.)
// Only the project library's own symbols are rows: one that comes from a library file or the built-in table is read-only until it is opened.
import { useEffect, useMemo, useState } from "react";
import type { Cmd, LibrarySymbol, SymbolPropertiesFields } from "../../api/types";
import { fetchSymbolEditorNames } from "../../api/client";
import { fetchAnySymbol } from "../../api/libraryClient";
import { PROJECT_LIBRARY, splitLibName } from "../../kicad-port/libraryNames";
import { allFields, bareName, defaultField, fieldNameProblem, isMandatoryField, relatedSymbols, wireField, type FieldRow } from "../../kicad-port/symFields";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { notifyLibraryChanged } from "../library/useLibraryNames";

/** What the table edits of a symbol: the properties `edit_symbol_properties` takes and the symbol's fields. */
interface Edit {
  props: SymbolPropertiesFields;
  fields: FieldRow[];
}
type Row = { libId: string; derived: boolean; original: Edit; edited: Edit };

const FIELD_KEYS: (keyof SymbolPropertiesFields)[] = ["reference_prefix", "description", "keywords", "datasheet", "power", "in_bom", "on_board", "pin_numbers_hidden", "pin_names_hidden", "pin_name_offset_mm", "unit_count", "has_alternate_body_style", "footprint_filters"];

function editOf(s: LibrarySymbol): Edit {
  const out: Record<string, unknown> = {};
  for (const k of FIELD_KEYS) out[k] = s[k];
  return { props: out as unknown as SymbolPropertiesFields, fields: allFields(s) };
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
const propsChanged = (r: Row) => FIELD_KEYS.some((k) => !same(r.original.props[k], r.edited.props[k]));
const fieldsChanged = (r: Row) => !same(r.original.fields, r.edited.fields);
const changed = (r: Row) => propsChanged(r) || fieldsChanged(r);

/** The text a cell shows: a mandatory text lives in the properties, the rest in the fields. */
function cellText(e: Edit, name: string): string {
  if (name === "Reference") return e.props.reference_prefix;
  if (name === "Datasheet") return e.props.datasheet;
  if (name === "Description") return e.props.description;
  return e.fields.find((f) => f.name === name)?.text ?? "";
}

/** A cell edited: the properties and the field list both say it, so the verbs agree. */
function withCell(e: Edit, name: string, text: string): Edit {
  const props = { ...e.props };
  if (name === "Reference") props.reference_prefix = text;
  else if (name === "Datasheet") props.datasheet = text;
  else if (name === "Description") props.description = text;
  const i = e.fields.findIndex((f) => f.name === name);
  const fields = i >= 0 ? e.fields.map((f, j) => (j === i ? { ...f, text } : f)) : [...e.fields, { ...defaultField(name), text }];
  return { props, fields };
}

const MANDATORY_COLUMNS = ["Reference", "Value", "Footprint", "Datasheet", "Description"] as const;
const WIDTH: Record<string, number> = { Reference: 54, Value: 110, Footprint: 150, Datasheet: 150, Description: 190 };

export function LibraryFieldsTableDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const open = state.fieldsTableOpen;
  const [scope, setScope] = useState<"library" | "related">("library");
  const [rows, setRows] = useState<Row[]>([]);
  const [filter, setFilter] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [added, setAdded] = useState<string[]>([]);
  const [newColumn, setNewColumn] = useState("");
  // The library the table is about: the target's own (`GetTargetLibId().GetLibNickname()`), else the project library.
  const target = state.treeSelection[0] ?? state.libId ?? "";
  const lib = useMemo(() => splitLibName(target).lib || PROJECT_LIBRARY, [target, open]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (open) setScope(state.fieldsTableScope);
  }, [open, state.fieldsTableScope]);

  useEffect(() => {
    if (!open) return;
    let stopped = false;
    setError(null);
    setFilter("");
    setAdded([]);
    setNewColumn("");
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
          loaded.push({ libId: id, derived: !!symbol.extends, original: e, edited: e });
        }
        if (!stopped) setRows(loaded);
      } catch (e) {
        if (!stopped) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      stopped = true;
    };
  }, [open, lib, scope, target]);

  // the user fields any listed symbol has, and the ones "Add Field" made columns of (`UpdateFieldList`: a sorted set)
  const userColumns = useMemo(() => {
    const names = new Set<string>(added);
    for (const r of rows) for (const f of r.edited.fields) if (!isMandatoryField(f.name)) names.add(f.name);
    return [...names].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base", numeric: true }));
  }, [rows, added]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_FIELDS_TABLE_OPEN", open: false });
  const setProps = (libId: string, patch: Partial<SymbolPropertiesFields>) => setRows((rs) => rs.map((r) => (r.libId === libId ? { ...r, edited: { ...r.edited, props: { ...r.edited.props, ...patch } } } : r)));
  const setCell = (libId: string, name: string, text: string) => setRows((rs) => rs.map((r) => (r.libId === libId ? { ...r, edited: withCell(r.edited, name, text) } : r)));
  const q = filter.trim().toLowerCase();
  const shown = rows.filter((r) => !q || r.libId.toLowerCase().includes(q) || r.edited.props.description.toLowerCase().includes(q) || r.edited.props.keywords.toLowerCase().includes(q));
  const dirty = rows.filter(changed);
  const columns = [...MANDATORY_COLUMNS, ...userColumns];

  /** "Add Field": a column for a new user field (`DIALOG_LIB_FIELDS_TABLE::OnAddField`); the field exists on a symbol once a cell of it has text. */
  const addColumn = () => {
    const name = newColumn.trim();
    const problem = fieldNameProblem(name, userColumns);
    if (problem) return setError(problem);
    setAdded((a) => [...a, name]);
    setNewColumn("");
    setError(null);
  };

  /** OK: every edited row is written as one undo step (`edit_symbol_properties` and `edit_library_symbol_fields` per symbol, in a batch). */
  const apply = async () => {
    if (dirty.length === 0) return close();
    setBusy(true);
    const cmds: Cmd[] = [];
    for (const r of dirty) {
      if (propsChanged(r)) cmds.push({ op: "edit_symbol_properties", lib_id: r.libId, ...r.edited.props });
      if (fieldsChanged(r)) cmds.push({ op: "edit_library_symbol_fields", lib_id: r.libId, fields: r.edited.fields.map(wireField) });
    }
    const ok = await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    setBusy(false);
    notifyLibraryChanged();
    if (ok) close();
    else setError("The edit was refused; see the message below the editor.");
  };

  const emptyMessage = rows.length === 0 ? (scope === "related" ? `No related symbols found in library '${lib}'.` : `No symbols of the project in library '${lib}' yet: a symbol that comes from a library file is read-only until it is opened.`) : "No match.";

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 1000, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Symbol Fields Table" data-testid="fields-table">
        <div className="dialog-header">
          <span>Symbol Fields Table ('{lib}' Library)</span>
        </div>
        <div className="dialog-body" style={{ padding: 8 }}>
          <div style={{ display: "flex", gap: 8, marginBottom: 6, alignItems: "center", flexWrap: "wrap" }}>
            <input value={filter} placeholder="Filter" onChange={(e) => setFilter(e.target.value)} style={{ width: 200 }} />
            <label>
              Scope:{" "}
              <select value={scope} onChange={(e) => setScope(e.target.value as "library" | "related")} data-testid="fields-scope" aria-label="Scope">
                <option value="library">Library</option>
                <option value="related">Related Symbols</option>
              </select>
            </label>
            <span style={{ marginLeft: "auto" }}>
              <input value={newColumn} placeholder="New field name" onChange={(e) => setNewColumn(e.target.value)} style={{ width: 130 }} aria-label="New field name" data-testid="fields-new-name" />{" "}
              <button onClick={addColumn} data-testid="fields-add-field">
                Add Field
              </button>
            </span>
          </div>
          <div style={{ maxHeight: "55vh", overflow: "auto", border: "1px solid var(--chrome-border)" }}>
            <table className="setup-table" style={{ margin: 0, width: "100%" }}>
              <thead>
                <tr>
                  <th>Symbol Name</th>
                  {columns.map((c) => (
                    <th key={c}>{c}</th>
                  ))}
                  <th>Keywords</th>
                  <th>Exclude From BOM</th>
                  <th>Exclude From Board</th>
                  <th>Power Symbol</th>
                </tr>
              </thead>
              <tbody>
                {shown.length === 0 && (
                  <tr>
                    <td colSpan={columns.length + 5} style={{ color: "var(--chrome-text-dim)" }}>
                      {emptyMessage}
                    </td>
                  </tr>
                )}
                {shown.map((r) => (
                  <tr key={r.libId} data-testid={`fields-row-${bareName(r.libId)}`} style={changed(r) ? { background: "var(--chrome-bg-raised)" } : undefined}>
                    <td style={{ whiteSpace: "nowrap", fontStyle: r.derived ? "italic" : undefined }} title={r.derived ? "A derived symbol: an empty cell inherits from its parent" : undefined}>
                      {splitLibName(r.libId).item}
                    </td>
                    {columns.map((c) => (
                      <td key={c}>
                        <input
                          value={cellText(r.edited, c)}
                          style={{ width: WIDTH[c] ?? 110 }}
                          onChange={(e) => setCell(r.libId, c, e.target.value)}
                          aria-label={`${c} of ${r.libId}`}
                          data-testid={`cell-${bareName(r.libId)}-${c}`}
                        />
                      </td>
                    ))}
                    <td>
                      <input value={r.edited.props.keywords} style={{ width: 120 }} onChange={(e) => setProps(r.libId, { keywords: e.target.value })} aria-label={`Keywords of ${r.libId}`} />
                    </td>
                    <td style={{ textAlign: "center" }}>
                      <input type="checkbox" checked={!r.edited.props.in_bom} onChange={(e) => setProps(r.libId, { in_bom: !e.target.checked })} aria-label={`Exclude ${r.libId} from BOM`} />
                    </td>
                    <td style={{ textAlign: "center" }}>
                      <input type="checkbox" checked={!r.edited.props.on_board} onChange={(e) => setProps(r.libId, { on_board: !e.target.checked })} aria-label={`Exclude ${r.libId} from board`} />
                    </td>
                    <td style={{ textAlign: "center" }}>
                      <input type="checkbox" checked={r.edited.props.power} onChange={(e) => setProps(r.libId, { power: e.target.checked })} aria-label={`${r.libId} is a power symbol`} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="fields-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <span style={{ marginRight: "auto", color: "var(--chrome-text-dim)", fontSize: 12 }}>{dirty.length > 0 ? `${dirty.length} symbol(s) changed` : ""}</span>
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy} onClick={() => void apply()} data-testid="fields-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

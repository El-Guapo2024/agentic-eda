// `DIALOG_LIB_FIELDS_TABLE` (eeschema/dialogs/dialog_lib_fields_table.cpp), `eeschema.SymbolLibraryControl.showLibraryFieldsTable`
// "Bulk Edit Symbol Fields...": every symbol of the library as one row, its fields as columns, edited in place and saved together --
// "Symbol Fields Table ('%s' Library)". The columns are the ones `UpdateFieldList` starts with, as far as a library symbol here keeps them:
// Symbol Name (never edited here), Reference (the symbol's prefix), Datasheet, Description, Keywords, the Exclude From BOM / Board check
// boxes and Power Symbol. (Value, Footprint, Exclude From Simulation, Local Power and user fields are not part of the library model.)
// Only the project library's own symbols are rows: one that comes from a library file or the built-in table is read-only until it is opened.
import { useEffect, useMemo, useState } from "react";
import type { Cmd, LibrarySymbol, SymbolPropertiesFields } from "../../api/types";
import { fetchSymbolEditorNames } from "../../api/client";
import { fetchAnySymbol } from "../../api/libraryClient";
import { PROJECT_LIBRARY, splitLibName } from "../../kicad-port/libraryNames";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { notifyLibraryChanged } from "../library/useLibraryNames";

type Row = { libId: string; original: SymbolPropertiesFields; edited: SymbolPropertiesFields };

const FIELD_KEYS: (keyof SymbolPropertiesFields)[] = ["reference_prefix", "description", "keywords", "datasheet", "power", "in_bom", "on_board", "pin_numbers_hidden", "pin_names_hidden", "pin_name_offset_mm", "unit_count", "has_alternate_body_style", "footprint_filters"];

function propertiesOf(s: LibrarySymbol): SymbolPropertiesFields {
  const out: Record<string, unknown> = {};
  for (const k of FIELD_KEYS) out[k] = s[k];
  return out as unknown as SymbolPropertiesFields;
}

const changed = (r: Row) => FIELD_KEYS.some((k) => JSON.stringify(r.original[k]) !== JSON.stringify(r.edited[k]));

export function LibraryFieldsTableDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const open = state.fieldsTableOpen;
  const [rows, setRows] = useState<Row[]>([]);
  const [filter, setFilter] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // The library the table is about: the target's own (`GetTargetLibId().GetLibNickname()`), else the project library.
  const lib = useMemo(() => splitLibName(state.treeSelection[0] ?? state.libId ?? "").lib || PROJECT_LIBRARY, [state.treeSelection, state.libId, open]);

  useEffect(() => {
    if (!open) return;
    let stopped = false;
    setError(null);
    setFilter("");
    void (async () => {
      try {
        const names = await fetchSymbolEditorNames();
        const ids = (names.project ?? []).filter((id) => (splitLibName(id).lib || PROJECT_LIBRARY) === lib);
        const loaded: Row[] = [];
        for (const id of ids) {
          const { symbol } = await fetchAnySymbol(id);
          const p = propertiesOf(symbol);
          loaded.push({ libId: id, original: p, edited: p });
        }
        if (!stopped) setRows(loaded);
      } catch (e) {
        if (!stopped) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      stopped = true;
    };
  }, [open, lib]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_FIELDS_TABLE_OPEN", open: false });
  const set = (libId: string, patch: Partial<SymbolPropertiesFields>) => setRows((rs) => rs.map((r) => (r.libId === libId ? { ...r, edited: { ...r.edited, ...patch } } : r)));
  const q = filter.trim().toLowerCase();
  const shown = rows.filter((r) => !q || r.libId.toLowerCase().includes(q) || r.edited.description.toLowerCase().includes(q) || r.edited.keywords.toLowerCase().includes(q));
  const dirty = rows.filter(changed);

  /** OK: every edited row is written as one undo step (`edit_symbol_properties` per symbol in a batch). */
  const apply = async () => {
    if (dirty.length === 0) return close();
    setBusy(true);
    const cmds: Cmd[] = dirty.map((r): Cmd => ({ op: "edit_symbol_properties", lib_id: r.libId, ...r.edited }));
    const ok = await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    setBusy(false);
    notifyLibraryChanged();
    if (ok) close();
    else setError("The edit was refused; see the message below the editor.");
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 880, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Symbol Fields Table">
        <div className="dialog-header">
          <span>Symbol Fields Table ('{lib}' Library)</span>
        </div>
        <div className="dialog-body" style={{ padding: 8 }}>
          <input value={filter} placeholder="Filter" onChange={(e) => setFilter(e.target.value)} style={{ width: 240, marginBottom: 6 }} />
          <div style={{ maxHeight: "55vh", overflow: "auto", border: "1px solid var(--chrome-border)" }}>
            <table className="setup-table" style={{ margin: 0, width: "100%" }}>
              <thead>
                <tr>
                  <th>Symbol Name</th>
                  <th>Reference</th>
                  <th>Datasheet</th>
                  <th>Description</th>
                  <th>Keywords</th>
                  <th>Exclude From BOM</th>
                  <th>Exclude From Board</th>
                  <th>Power Symbol</th>
                </tr>
              </thead>
              <tbody>
                {shown.length === 0 && (
                  <tr>
                    <td colSpan={8} style={{ color: "var(--chrome-text-dim)" }}>
                      {rows.length === 0 ? `No symbols of the project in library '${lib}' yet: a symbol that comes from a library file is read-only until it is opened.` : "No match."}
                    </td>
                  </tr>
                )}
                {shown.map((r) => (
                  <tr key={r.libId} style={changed(r) ? { background: "var(--chrome-bg-raised)" } : undefined}>
                    <td style={{ whiteSpace: "nowrap" }}>{splitLibName(r.libId).item}</td>
                    <td>
                      <input value={r.edited.reference_prefix} style={{ width: 54 }} onChange={(e) => set(r.libId, { reference_prefix: e.target.value })} aria-label={`Reference of ${r.libId}`} />
                    </td>
                    <td>
                      <input value={r.edited.datasheet} style={{ width: 150 }} onChange={(e) => set(r.libId, { datasheet: e.target.value })} aria-label={`Datasheet of ${r.libId}`} />
                    </td>
                    <td>
                      <input value={r.edited.description} style={{ width: 190 }} onChange={(e) => set(r.libId, { description: e.target.value })} aria-label={`Description of ${r.libId}`} />
                    </td>
                    <td>
                      <input value={r.edited.keywords} style={{ width: 120 }} onChange={(e) => set(r.libId, { keywords: e.target.value })} aria-label={`Keywords of ${r.libId}`} />
                    </td>
                    <td style={{ textAlign: "center" }}>
                      <input type="checkbox" checked={!r.edited.in_bom} onChange={(e) => set(r.libId, { in_bom: !e.target.checked })} aria-label={`Exclude ${r.libId} from BOM`} />
                    </td>
                    <td style={{ textAlign: "center" }}>
                      <input type="checkbox" checked={!r.edited.on_board} onChange={(e) => set(r.libId, { on_board: !e.target.checked })} aria-label={`Exclude ${r.libId} from board`} />
                    </td>
                    <td style={{ textAlign: "center" }}>
                      <input type="checkbox" checked={r.edited.power} onChange={(e) => set(r.libId, { power: e.target.checked })} aria-label={`${r.libId} is a power symbol`} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {error && <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }}>{error}</p>}
        </div>
        <div className="dialog-footer">
          <span style={{ marginRight: "auto", color: "var(--chrome-text-dim)", fontSize: 12 }}>{dirty.length > 0 ? `${dirty.length} symbol(s) changed` : ""}</span>
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy} onClick={() => void apply()}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

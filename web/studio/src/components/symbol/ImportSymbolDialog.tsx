// `DIALOG_IMPORT_SYMBOL_SELECT` (eeschema/dialogs/dialog_import_symbol_select.cpp) as `SYMBOL_EDIT_FRAME::ImportSymbol` runs it: "Import Symbols from
// <file>" lists the symbols of the chosen `.kicad_sym` with a check box each and a filter; "Import" stays disabled until one is checked.
// A symbol the destination library already has is then asked about in "Resolve Import Conflicts" -- each row Overwrite (the default) or Skip,
// with Skip All / Overwrite All. The rest of the work (`performImport`) is `ImportSymbol`'s loop. (The dialog's preview pane is not part of
// this port; the symbol opens on the canvas once imported.)
import { useEffect, useMemo, useState } from "react";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { fetchSymbolEditorNames } from "../../api/client";
import { joinLibName, splitLibName } from "../../kicad-port/libraryNames";
import { performImport } from "../../actions/symbolLibraryOps";

type Resolution = "skip" | "overwrite";

export function ImportSymbolDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const req = state.importRequest;
  const [filter, setFilter] = useState("");
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [conflicts, setConflicts] = useState<string[] | null>(null);
  const [resolutions, setResolutions] = useState<Record<string, Resolution>>({});

  useEffect(() => {
    setFilter("");
    setChecked(new Set());
    setConflicts(null);
    setResolutions({});
  }, [req]);

  const names = useMemo(() => (req ? req.symbols.map((s) => s.lib_id) : []), [req]);
  if (!req) return null;
  const close = () => dispatch({ type: "SET_IMPORT_REQUEST", request: null });
  const q = filter.trim().toLowerCase();
  const shown = names.filter((n) => !q || n.toLowerCase().includes(q));

  const toggle = (n: string) =>
    setChecked((c) => {
      const next = new Set(c);
      if (next.has(n)) next.delete(n);
      else next.add(n);
      return next;
    });

  const run = async (res: Record<string, Resolution>) => {
    const chosen = req.symbols.filter((s) => checked.has(s.lib_id));
    close();
    await performImport({ api, dispatch }, req.lib, chosen, res);
  };

  /** `resolveConflicts`: the chosen symbols the destination library already has. No conflict, no second dialog. */
  const importClicked = async () => {
    let existing: string[] = [];
    try {
      existing = (await fetchSymbolEditorNames()).names;
    } catch {
      /* the verb reports a clash itself */
    }
    const taken = new Set(existing);
    const clashes = names.filter((n) => checked.has(n) && taken.has(joinLibName(req.lib, splitLibName(n).item)));
    if (clashes.length === 0) return void run({});
    setResolutions(Object.fromEntries(clashes.map((n) => [n, "overwrite" as Resolution])));
    setConflicts(clashes);
  };

  if (conflicts) {
    const setAll = (r: Resolution) => setResolutions(Object.fromEntries(conflicts.map((n) => [n, r])));
    return (
      <div className="dialog-backdrop" onClick={close}>
        <div className="dialog" style={{ width: 460 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Resolve Import Conflicts">
          <div className="dialog-header">
            <span>Resolve Import Conflicts</span>
          </div>
          <div className="dialog-body">
            <p style={{ margin: "0 0 8px" }}>The following symbols already exist in the destination library. Choose how to handle each conflict:</p>
            <table className="setup-table" style={{ margin: 0 }}>
              <thead>
                <tr>
                  <th>Symbol</th>
                  <th>Action</th>
                </tr>
              </thead>
              <tbody>
                {conflicts.map((n) => (
                  <tr key={n}>
                    <td>{n}</td>
                    <td>
                      <select value={resolutions[n] ?? "overwrite"} onChange={(e) => setResolutions((r) => ({ ...r, [n]: e.target.value as Resolution }))}>
                        <option value="overwrite">Overwrite</option>
                        <option value="skip">Skip</option>
                      </select>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div style={{ marginTop: 8, display: "flex", gap: 6 }}>
              <button onClick={() => setAll("skip")}>Skip All</button>
              <button onClick={() => setAll("overwrite")}>Overwrite All</button>
            </div>
          </div>
          <div className="dialog-footer">
            <button onClick={close}>Cancel</button>
            <button className="primary" onClick={() => void run(resolutions)}>
              Import
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Import Symbols">
        <div className="dialog-header">
          <span>Import Symbols from {req.fileName}</span>
        </div>
        <div className="dialog-body">
          {req.symbols.length === 0 ? (
            <p style={{ margin: 0 }}>Symbol library '{req.fileName}' is empty.</p>
          ) : (
            <>
              <input value={filter} placeholder="Filter" onChange={(e) => setFilter(e.target.value)} style={{ width: "100%", boxSizing: "border-box", marginBottom: 6 }} />
              <div style={{ border: "1px solid var(--chrome-border)", maxHeight: 240, overflowY: "auto" }}>
                {shown.map((n) => (
                  <label key={n} style={{ display: "flex", gap: 8, padding: "3px 8px", cursor: "pointer" }}>
                    <input type="checkbox" checked={checked.has(n)} onChange={() => toggle(n)} />
                    <span>{n}</span>
                  </label>
                ))}
              </div>
              <p style={{ margin: "8px 0 0", color: "var(--chrome-text-dim)", fontSize: 12 }}>{checked.size} symbols selected</p>
              {req.warnings.length > 0 && <p style={{ margin: "6px 0 0", color: "var(--chrome-text-dim)", fontSize: 11 }}>{req.warnings.join("; ")}</p>}
            </>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={checked.size === 0} onClick={() => void importClicked()}>
            Import
          </button>
        </div>
      </div>
    </div>
  );
}

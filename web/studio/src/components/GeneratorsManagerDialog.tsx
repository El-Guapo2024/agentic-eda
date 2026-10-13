// Port of pcbnew/dialogs/dialog_generators.cpp (`pcbnew.Generator.showManager`, "Generators Manager"): the generators of the board by type, a table for each type with the
// columns `GetRowData` gives (a tuning pattern: its Net and its Tuning, "12.3 mm (tuned)"), "Rebuild Selected" / "Rebuild this type" / "Rebuild All". Selecting rows
// selects the generators on the board. The type is one here -- the tuning pattern, the only generator this studio makes.
import { useEffect, useState } from "react";
import { tunePatterns, tuneRegenerate } from "../api/tuneClient";
import type { TunePatternRow } from "../api/tuneTypes";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { useEscape } from "./pcbDialogKit";

export function GeneratorsManagerDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.pcbx.generatorsManagerOpen;
  const [rows, setRows] = useState<TunePatternRow[]>([]);
  const [picked, setPicked] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const close = () => dispatch({ type: "PCBX", patch: { generatorsManagerOpen: false } });
  useEscape(open ? close : () => undefined);

  // the board changed under it (a pattern added, removed, rebuilt): the table follows (`RebuildModels` on every board event)
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void tunePatterns().then((r) => {
      if (cancelled) return;
      setRows(r.ok ? r.patterns : []);
      setPicked((cur) => cur.filter((id) => r.patterns?.some((p) => p.id === id)));
    });
    return () => {
      cancelled = true;
    };
  }, [open, state.version]);

  if (!open) return null;

  const toggle = (id: string, additive: boolean) => {
    const next = additive ? (picked.includes(id) ? picked.filter((p) => p !== id) : [...picked, id]) : [id];
    setPicked(next);
    // `OnItemSelected`: the generators picked in the table are selected on the board
    dispatch({ type: "SET_SELECTION", refs: next });
  };

  const rebuild = async (ids?: string[]) => {
    setBusy(true);
    const r = await tuneRegenerate(ids);
    setBusy(false);
    dispatch({ type: "TOAST", message: r.ok ? (r.message ?? "Rebuilt.") : (r.message ?? "Nothing was rebuilt."), kind: r.ok ? "info" : "error" });
    await api.refresh();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 560 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Generators Manager</span>
        </div>
        <div className="dialog-body">
          {rows.length === 0 ? (
            <div style={{ fontSize: 12, opacity: 0.8 }}>There are no generators on this board. Tune the length of a track (7, 8 or 9) to make a tuning pattern.</div>
          ) : (
            <>
              <div role="tablist" style={{ display: "flex", gap: 4, marginBottom: 6 }}>
                <span role="tab" aria-selected="true" style={{ padding: "2px 10px", border: "1px solid var(--border, #888)", borderRadius: 3 }}>
                  Tuning Pattern ({rows.length})
                </span>
              </div>
              <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }} data-testid="generators-table">
                <thead>
                  <tr style={{ textAlign: "left" }}>
                    <th style={{ padding: "2px 6px" }}>Net</th>
                    <th style={{ padding: "2px 6px" }}>Tuning</th>
                    <th style={{ padding: "2px 6px" }}>Layer</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r) => (
                    <tr key={r.id} data-pattern={r.id} onClick={(e) => toggle(r.id, e.ctrlKey || e.metaKey || e.shiftKey)} style={{ cursor: "pointer", background: picked.includes(r.id) ? "rgba(80,140,255,0.28)" : undefined }}>
                      <td style={{ padding: "2px 6px" }}>{r.nets.length > 0 ? r.nets.join(", ") : ""}</td>
                      <td style={{ padding: "2px 6px", color: r.status === "tuned" ? undefined : "#e05050" }}>{r.info}</td>
                      <td style={{ padding: "2px 6px" }}>{r.layer}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
        </div>
        <div className="dialog-footer">
          <button disabled={busy || picked.length === 0} onClick={() => void rebuild(picked)}>
            Rebuild Selected
          </button>
          <button disabled={busy || rows.length === 0} onClick={() => void rebuild()}>
            Rebuild this type
          </button>
          <button disabled={busy || rows.length === 0} onClick={() => void rebuild()}>
            Rebuild All
          </button>
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}

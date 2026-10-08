// Change Symbols / Update Symbols (`DIALOG_CHANGE_SYMBOLS`): pick which symbols (the selection, all, or by reference / value / library id), then either give
// them a new library symbol (Change) or refresh them from the project's edited library symbols (Update). Like KiCad's, the dialog stays open after Apply and
// lists what happened to each symbol; Close ends it. The rules are kicad-port/schChangeSymbols.ts.
import { useEffect, useState } from "react";
import { fetchSymbolLibrary } from "../api/client";
import type { SchToolDialog } from "../api/schEditTypes";
import type { SymbolLibraryEntry } from "../api/types";
import { matchReferences, planChange, type LibInfo, type MatchBy, type SymbolRow } from "../kicad-port/schChangeSymbols";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";

export function ChangeSymbolsDialog({ dialog }: { dialog: Extract<SchToolDialog, { kind: "change_symbols" }> }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const change = dialog.mode === "change";
  const rows: SymbolRow[] = (state.schematic?.symbols ?? []).map((s) => ({ id: s.id, lib_id: s.lib_id, unit: s.unit, value: s.value }));
  const first = rows.find((r) => dialog.selected.includes(r.id));

  const [by, setBy] = useState<MatchBy>(dialog.selected.length > 0 ? "selection" : change ? "reference" : "all");
  const [reference, setReference] = useState(first?.id ?? "");
  const [value, setValue] = useState(first?.value ?? "");
  const [matchId, setMatchId] = useState(first?.lib_id ?? "");
  const [newId, setNewId] = useState("");
  const [updateReference, setUpdateReference] = useState(false);
  const [updateValue, setUpdateValue] = useState(false);
  const [resetEmpty, setResetEmpty] = useState(false);
  const [entries, setEntries] = useState<SymbolLibraryEntry[]>([]);
  const [report, setReport] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    void fetchSymbolLibrary()
      .then((l) => live && setEntries(l.entries))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const verb = change ? "Change" : "Update";

  const run = async () => {
    setBusy(true);
    try {
      const refs = matchReferences(rows, { by, reference, value, id: matchId }, new Set(dialog.selected));
      if (refs.length === 0) return setReport(["*** No symbols matching criteria found ***"]);
      if (change) {
        const id = newId.trim();
        const entry = entries.find((e) => e.lib_id === id);
        const lib: LibInfo = { found: entry !== undefined, unitCount: entry?.unit_count ?? 1, prefix: entry?.reference_prefix ?? "", value: id.includes(":") ? id.slice(id.indexOf(":") + 1) : id };
        const plan = planChange(refs, rows, lib, { newLibId: id, updateReference, updateValue, resetEmpty });
        if (plan.cmds.length > 0 && !(await api.cmdBatch(plan.cmds))) return setReport([...plan.report, "*** The change was refused; nothing was changed ***"]);
        return setReport(plan.report);
      }
      // Update: the project's edited library symbols take over for every library id among the matched symbols.
      const libIds = [...new Set(refs.flatMap((r) => rows.filter((s) => s.id === r && s.lib_id).map((s) => s.lib_id as string)))];
      const ok = await api.cmd({ op: "sch_edit", verb: "update_library_symbols", lib_ids: libIds });
      setReport(ok ? libIds.map((id) => `${id}: updated from the project library`) : ["*** No edited library symbol of these symbols is waiting to be updated from ***"]);
    } finally {
      setBusy(false);
    }
  };

  const radio = (value: MatchBy, label: string, extra?: React.ReactNode) => (
    <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0" }}>
      <label style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 250 }}>
        <input type="radio" name="match" checked={by === value} onChange={() => setBy(value)} />
        {label}
      </label>
      {extra}
    </div>
  );

  return (
    <SchDialogShell title={change ? "Change Symbols" : "Update Symbols from Library"} width={560} onCancel={close} cancelLabel="Close" onOk={() => void run()} okLabel={verb} canOk={!busy && (!change || newId.trim().length > 0)}>
      <fieldset style={{ border: "1px solid var(--chrome-border)", margin: "0 0 8px", padding: "6px 8px" }}>
        <legend>{change ? "Change" : "Update"}</legend>
        {dialog.selected.length > 0 && radio("selection", `${verb} selected symbol(s)`)}
        {!change && radio("all", "Update all symbols in schematic")}
        {radio("reference", `${verb} symbols matching reference designator:`, <input value={reference} onChange={(e) => setReference(e.target.value)} onFocus={() => setBy("reference")} />)}
        {radio("value", `${verb} symbols matching value:`, <input value={value} onChange={(e) => setValue(e.target.value)} onFocus={() => setBy("value")} />)}
        {radio("id", `${verb} symbols matching library identifier:`, <input value={matchId} onChange={(e) => setMatchId(e.target.value)} onFocus={() => setBy("id")} />)}
      </fieldset>
      {change && (
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
          <span style={{ minWidth: 150 }}>New library identifier:</span>
          <input list="sch-change-symbol-ids" style={{ flex: 1 }} value={newId} onChange={(e) => setNewId(e.target.value)} placeholder="Library:Symbol" autoFocus />
          <datalist id="sch-change-symbol-ids">
            {entries.map((e) => (
              <option key={e.lib_id} value={e.lib_id}>
                {e.description}
              </option>
            ))}
          </datalist>
        </div>
      )}
      <fieldset style={{ border: "1px solid var(--chrome-border)", margin: "0 0 8px", padding: "6px 8px" }}>
        <legend>Update fields</legend>
        <label style={{ display: "block" }}>
          <input type="checkbox" checked={updateReference} onChange={(e) => setUpdateReference(e.target.checked)} /> Reference (the library symbol's prefix, the number kept)
        </label>
        <label style={{ display: "block" }}>
          <input type="checkbox" checked={updateValue} onChange={(e) => setUpdateValue(e.target.checked)} /> Value (the library symbol's)
        </label>
        <label style={{ display: "block" }}>
          <input type="checkbox" checked={resetEmpty} onChange={(e) => setResetEmpty(e.target.checked)} /> Reset fields if empty in new symbol
        </label>
      </fieldset>
      {report.length > 0 && <pre style={{ margin: 0, maxHeight: 160, overflow: "auto", fontSize: 11, whiteSpace: "pre-wrap" }}>{report.join("\n")}</pre>}
    </SchDialogShell>
  );
}

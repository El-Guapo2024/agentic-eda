// Change Symbols / Update Symbols (`DIALOG_CHANGE_SYMBOLS`): pick which symbols (the selection, all, or by reference / value / library id), then either give
// them a new library symbol (Change) or refresh them from the project's edited library symbols (Update). Like KiCad's, the dialog stays open after Apply and
// lists what happened to each symbol; Close ends it. The rules are kicad-port/schChangeSymbols.ts.
//
// The browse buttons beside "matching library identifier" and "New library identifier" (`launchMatchIdSymbolBrowser`, `launchNewIdSymbolBrowser`) open the Symbol
// Chooser over every library of the tables (`FRAME_SYMBOL_CHOOSER`, SymbolChooserDialog.tsx's `SymbolPickerDialog`), opened on the id the field holds; OK puts
// the chosen `Lib:Name` in the field. A new id from a library the design does not use yet is read from that library (`GetLibSymbol( newId )`: its units, its
// Reference prefix and its Value), and the server keeps its definition with the schematic in the same step as the change (crates/cli/src/library_place.rs).
import { useEffect, useState } from "react";
import { fetchSymbolLibrary } from "../api/client";
import { fetchSymbolPlacements } from "../api/schControlClient";
import { fetchSymbolDetails } from "../api/libraryChooserClient";
import type { SchToolDialog } from "../api/schEditTypes";
import type { Cmd, SymbolLibraryEntry } from "../api/types";
import { changeStep, matchOverHierarchy, planChange, referencesOf, resetCmd, type LibInfo, type MatchBy, type SymbolRow } from "../kicad-port/schChangeSymbols";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";
import { SymbolPickerDialog } from "./SymbolChooserDialog";

export function ChangeSymbolsDialog({ dialog }: { dialog: Extract<SchToolDialog, { kind: "change_symbols" }> }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const change = dialog.mode === "change";
  // the sheet in view's symbols, until every sheet's are read (`processMatchingSymbols` walks the whole hierarchy, UI-LEFT S9)
  const inView = state.currentSheetPath.join("/");
  const viewRows: SymbolRow[] = (state.schematic?.symbols ?? []).map((s) => ({ id: s.id, lib_id: s.lib_id, unit: s.unit, value: s.value, reference: s.reference, sheet: inView }));
  const [hierarchyRows, setHierarchyRows] = useState<SymbolRow[] | null>(null);
  const rows: SymbolRow[] = hierarchyRows ?? viewRows;
  const first = viewRows.find((r) => dialog.selected.includes(r.id));

  const [by, setBy] = useState<MatchBy>(dialog.selected.length > 0 ? "selection" : change ? "reference" : "all");
  const [reference, setReference] = useState(first?.reference ?? first?.id ?? "");
  const [value, setValue] = useState(first?.value ?? "");
  const [matchId, setMatchId] = useState(first?.lib_id ?? "");
  const [newId, setNewId] = useState("");
  const [updateReference, setUpdateReference] = useState(false);
  const [updateValue, setUpdateValue] = useState(false);
  const [resetEmpty, setResetEmpty] = useState(false);
  // the reset options (`DIALOG_CHANGE_SYMBOLS`'s constructor: checked to change a symbol, unchecked to update one; UI-LEFT S16)
  const [resetVisibility, setResetVisibility] = useState(change);
  const [resetEffects, setResetEffects] = useState(change);
  const [resetPositions, setResetPositions] = useState(change);
  const [entries, setEntries] = useState<SymbolLibraryEntry[]>([]);
  const [report, setReport] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  /** Which field the Symbol Chooser is browsing for. */
  const [browsing, setBrowsing] = useState<"match" | "new" | null>(null);

  useEffect(() => {
    let live = true;
    void fetchSymbolLibrary()
      .then((l) => live && setEntries(l.entries))
      .catch(() => {});
    void fetchSymbolPlacements()
      .then((p) => live && setHierarchyRows(p.map((r) => ({ id: r.id, lib_id: r.lib_id || null, unit: r.unit, value: r.value, reference: r.reference, sheet: r.sheet, file: r.file }))))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  /** The reset options (`processSymbols`: visibility, sizes and styles, positions), sent in the change's own step (`changeStep`). */
  const resets = { visibility: resetVisibility, effects: resetEffects, positions: resetPositions };
  const verb = change ? "Change" : "Update";

  const run = async () => {
    setBusy(true);
    try {
      const groups = matchOverHierarchy(rows, { by, reference, value, id: matchId }, new Set(dialog.selected), inView);
      if (groups.length === 0) return setReport(["*** No symbols matching criteria found ***"]);
      if (change) {
        const id = newId.trim();
        const entry = entries.find((e) => e.lib_id === id);
        // not one of the design's libraries: the library of the tables that has it (`GetLibSymbol( newId )`)
        const installed = entry ? null : await fetchSymbolDetails(id).catch(() => null);
        const lib: LibInfo = installed
          ? { found: true, unitCount: Math.max(1, installed.units), prefix: installed.reference, value: installed.value || (id.includes(":") ? id.slice(id.indexOf(":") + 1) : id) }
          : { found: entry !== undefined, unitCount: entry?.unit_count ?? 1, prefix: entry?.reference_prefix ?? "", value: id.includes(":") ? id.slice(id.indexOf(":") + 1) : id };
        // each screen's symbols once, from the first sheet path that shows the screen; the report names a symbol by its references on every path
        const plans = groups.map((g) => {
          const screenRows = rows.filter((r) => (r.file ?? "") === g.file && (r.sheet ?? "") === g.sheet);
          return { g, plan: planChange(g.refs, screenRows, lib, { newLibId: id, updateReference, updateValue, resetEmpty, shown: (ref) => referencesOf(rows, g.file, ref) || ref }) };
        });
        const report = plans.flatMap((p) => p.plan.report);
        // the change and the resets of the symbols it processed: one step (`processSymbols`' one commit, "Change Symbols")
        const step = changeStep(plans.map((p) => ({ sheet: p.g.sheet, cmds: p.plan.cmds, processed: p.plan.processed })), resets);
        if (step && !(await api.cmd(step))) return setReport([...report, "*** The change was refused; nothing was changed ***"]);
        return setReport(report);
      }
      // Update: the project's edited library symbols take over for every library id among the matched symbols, and the resets, in one step ("Update Symbols")
      const libIds = [...new Set(groups.flatMap((g) => g.refs.flatMap((r) => rows.filter((s) => (s.file ?? "") === g.file && s.id === r && s.lib_id).map((s) => s.lib_id as string))))];
      const screens = groups.map((g) => ({ sheet: g.sheet, cmds: [] as Cmd[], processed: g.refs }));
      const update = { op: "sch_edit", verb: "update_library_symbols", lib_ids: libIds } as unknown as Cmd;
      const both = changeStep(screens, resets, [update]);
      if (both && (await api.cmd(both))) return setReport([...libIds.map((id) => `${id}: updated from the project library`), ...(resetCmd(["x"], resets, true) ? ["Fields reset from the library symbol"] : [])]);
      // no edited library symbol waiting: the resets alone, if any
      const alone = changeStep(screens, resets);
      const reset = alone ? await api.cmd(alone) : false;
      setReport(reset ? ["Fields reset from the library symbol"] : ["*** No edited library symbol of these symbols is waiting to be updated from ***"]);
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
        {radio(
          "id",
          `${verb} symbols matching library identifier:`,
          <>
            <input value={matchId} onChange={(e) => setMatchId(e.target.value)} onFocus={() => setBy("id")} data-change-match-id />
            <button
              title="Browse the symbol libraries"
              aria-label="Browse the symbol libraries for the identifier to match"
              onClick={() => {
                setBy("id");
                setBrowsing("match");
              }}
              data-change-match-browse
            >
              {"…"}
            </button>
          </>
        )}
      </fieldset>
      {change && (
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
          <span style={{ minWidth: 150 }}>New library identifier:</span>
          <input list="sch-change-symbol-ids" style={{ flex: 1 }} value={newId} onChange={(e) => setNewId(e.target.value)} placeholder="Library:Symbol" autoFocus data-change-new-id />
          <button title="Browse the symbol libraries" aria-label="Browse the symbol libraries for the new identifier" onClick={() => setBrowsing("new")} data-change-new-browse>
            {"…"}
          </button>
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
        <label style={{ display: "block" }}>
          <input type="checkbox" checked={resetVisibility} onChange={(e) => setResetVisibility(e.target.checked)} /> {change ? "Reset" : "Update"} field visibilities
        </label>
        <label style={{ display: "block" }}>
          <input type="checkbox" checked={resetEffects} onChange={(e) => setResetEffects(e.target.checked)} /> {change ? "Reset" : "Update"} field sizes and styles
        </label>
        <label style={{ display: "block" }}>
          <input type="checkbox" checked={resetPositions} onChange={(e) => setResetPositions(e.target.checked)} /> {change ? "Reset" : "Update"} field positions
        </label>
      </fieldset>
      {report.length > 0 && <pre style={{ margin: 0, maxHeight: 160, overflow: "auto", fontSize: 11, whiteSpace: "pre-wrap" }}>{report.join("\n")}</pre>}
      {browsing && (
        <SymbolPickerDialog
          preselect={(browsing === "new" ? newId : matchId).trim() || null}
          onCancel={() => setBrowsing(null)}
          onChoose={(id) => {
            if (browsing === "new") setNewId(id);
            else setMatchId(id);
            setBrowsing(null);
          }}
        />
      )}
    </SchDialogShell>
  );
}

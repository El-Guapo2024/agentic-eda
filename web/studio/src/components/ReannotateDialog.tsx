// Geographical Reannotate (`pcbnew.ReannotateTool.ShowReannotateDialog`): port of DIALOG_BOARD_REANNOTATE (pcbnew/dialogs/dialog_board_reannotate.cpp at 8303b2ad).
//
// The footprints of the board renumbered by where they are: the order (eight directions), the location a footprint is sorted by (the footprint or its Reference text) rounded to a
// grid, the scope (all, front, back, the selection), whether locked footprints and which references are left alone, the start numbers and prefixes of the front and the back. "Reannotate
// PCB" asks the server for the plan (`POST /api/reannotate_plan`), writes the message window from it -- the lists in their order, the prefixes in use, the change log, the
// duplicates -- and, unless it found duplicates or the user does not accept the warning about references with no number, sends `reannotate_board`: one undo step in which the symbols
// take the new references too (KiCad leaves that to Update Schematic from PCB).
import { useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import { postReannotatePlan, type ReannotatePlan } from "../api/fpAssign";
import { DIRECTIONS, badLines, confirmQuestion, defaultForm, doneLines, filterPrefix, formOptions, planLines, renamedSelection, type ReannotateForm } from "../kicad-port/reannotate";
import type { ReportLine } from "../kicad-port/pcbExchange";
import { useGridSettings } from "../state/gridSettings";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatLength } from "../state/units";
import { useEscape } from "./pcbDialogKit";
import { ReportWindow } from "./ReportWindow";

const SCOPES: readonly { id: ReannotateForm["scope"]; label: string }[] = [
  { id: "all", label: "All" },
  { id: "front", label: "Front" },
  { id: "back", label: "Back" },
  { id: "selection", label: "Selection" },
];

export function ReannotateDialog({ selection }: { selection: readonly string[] }) {
  useEscape();
  const state = useStudioState();
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const { grids } = useGridSettings("pcb");
  const units = state.units;
  const fmt = (um: number) => formatLength(um, units);
  // `m_GridChoice`: the editor's grid when it is on the list, else the first
  const [form, setForm] = useState<ReannotateForm>(() => defaultForm(grids.includes(state.gridNm) ? state.gridNm : (grids[0] ?? 1_000_000), selection.length > 0));
  const [lines, setLines] = useState<ReportLine[]>([]);
  const [busy, setBusy] = useState(false);
  /** `IsOK( ... "Reannotate anyway?" )`: the question while it waits for the answer, with the plan and the window's lines it is about. */
  const [ask, setAsk] = useState<{ question: string; plan: ReannotatePlan; head: ReportLine[] } | null>(null);
  const set = <K extends keyof ReannotateForm>(key: K, value: ReannotateForm[K]) => setForm((cur) => ({ ...cur, [key]: value }));

  const apply = async (plan: ReannotatePlan, head: ReportLine[]) => {
    const ok = await api.cmdBatch([{ op: "reannotate_board", options: formOptions(form), selection: [...selection] }]);
    // the server says why in a toast (the Strict switch refuses a change that adds a gate failure)
    if (!ok) {
      setLines([...head, { severity: "error", text: api.getState().toast?.message ?? "The board could not be reannotated." }]);
      return;
    }
    dispatch({ type: "SET_SELECTION", refs: renamedSelection([...state.selection], plan) });
    setLines([...head, ...doneLines()]);
  };

  const run = async () => {
    setBusy(true);
    setAsk(null);
    try {
      const options = formOptions(form);
      let plan: ReannotatePlan;
      try {
        plan = await postReannotatePlan(options, selection);
      } catch (e) {
        setLines([{ severity: "error", text: e instanceof Error ? e.message : String(e) }]);
        return;
      }
      const head = planLines(plan, options.location, fmt);
      if (plan.errors.length > 0) {
        setLines(head);
        return;
      }
      const question = confirmQuestion(plan.bad);
      if (question) {
        const shown = [...head, ...badLines(plan.bad, fmt)];
        setLines(shown);
        setAsk({ question, plan, head: shown });
        return;
      }
      await apply(plan, head);
    } finally {
      setBusy(false);
    }
  };

  const answer = async (yes: boolean) => {
    const pending = ask;
    setAsk(null);
    if (!pending || !yes) return;
    setBusy(true);
    try {
      await apply(pending.plan, pending.head);
    } finally {
      setBusy(false);
    }
  };

  const text = (label: string, value: string, onChange: (v: string) => void, testId: string, opts: { digits?: boolean; prefix?: boolean; title?: string } = {}) => (
    <label style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 4 }} title={opts.title}>
      <span style={{ flex: "0 0 125px", fontSize: 12 }}>{label}</span>
      <input
        style={{ flex: 1, minWidth: 0 }}
        value={value}
        inputMode={opts.digits ? "numeric" : undefined}
        data-testid={testId}
        onChange={(e) => onChange(opts.digits ? e.target.value.replace(/\D/g, "") : opts.prefix ? filterPrefix(e.target.value) : e.target.value)}
      />
    </label>
  );

  return (
    <div className="dialog-backdrop" onClick={closeSweepDialog}>
      <div className="dialog" style={{ width: 640 }} onClick={(e) => e.stopPropagation()} data-testid="reannotate-dialog">
        <div className="dialog-header">
          <span>Geographical Reannotation</span>
        </div>
        <div className="dialog-body">
          <fieldset style={{ border: "1px solid var(--border, #444)", padding: "4px 8px", marginBottom: 8 }}>
            <legend style={{ fontSize: 12 }}>Footprint Order</legend>
            <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)", columnGap: 12 }}>
              {DIRECTIONS.map((label, i) => (
                <label className="filter-row" key={label}>
                  <input type="radio" name="reannotate-direction" checked={form.direction === i} onChange={() => set("direction", i)} data-testid={`reannotate-direction-${i}`} />
                  {label}
                </label>
              ))}
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 6 }}>
              <span>Based on location of:</span>
              <select value={form.location} onChange={(e) => set("location", e.target.value as ReannotateForm["location"])} data-testid="reannotate-location">
                <option value="footprint">Footprint</option>
                <option value="reference">Reference</option>
              </select>
              <span style={{ marginLeft: 16 }} title="Component position will be rounded to this grid before sorting. This helps with misaligned parts.">
                Round locations to:
              </span>
              <select value={form.gridNm} title="Component position will be rounded to this grid before sorting. This helps with misaligned parts." onChange={(e) => set("gridNm", Number(e.target.value))} data-testid="reannotate-grid">
                {(grids.includes(form.gridNm) ? grids : [...grids, form.gridNm]).map((um) => (
                  <option key={um} value={um}>
                    {formatLength(um, units)}
                  </option>
                ))}
              </select>
            </div>
          </fieldset>

          <fieldset style={{ border: "1px solid var(--border, #444)", padding: "4px 8px", marginBottom: 8 }}>
            <legend style={{ fontSize: 12 }}>Reannotation Scope</legend>
            <div style={{ display: "flex", gap: 16 }}>
              {SCOPES.map((s) => (
                <label className="filter-row" key={s.id}>
                  <input type="radio" name="reannotate-scope" checked={form.scope === s.id} onChange={() => set("scope", s.id)} data-testid={`reannotate-scope-${s.id}`} />
                  {s.label}
                </label>
              ))}
            </div>
            <label className="filter-row" title="Locked footprints will not be reannotated" style={{ display: "block" }}>
              <input type="checkbox" checked={form.excludeLocked} onChange={(e) => set("excludeLocked", e.target.checked)} data-testid="reannotate-exclude-locked" /> Exclude locked footprints
            </label>
            {text("Exclude references:", form.exclude, (v) => set("exclude", v), "reannotate-exclude", { title: "Do not re-annotate this type of reference (R means R*)" })}
          </fieldset>

          <fieldset style={{ border: "1px solid var(--border, #444)", padding: "4px 8px", marginBottom: 8 }}>
            <legend style={{ fontSize: 12 }}>Reference Designators</legend>
            <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)", columnGap: 12 }}>
              <div>
                {text("Front reference start:", form.frontStart, (v) => set("frontStart", v), "reannotate-front-start", { digits: true, title: "Starting reference designation for front." })}
                {text("Front prefix:", form.frontPrefix, (v) => set("frontPrefix", v), "reannotate-front-prefix", { prefix: true, title: "Optional prefix for component side reference designations (e.g. F_)" })}
                <label className="filter-row" title="If checked will remove the front side prefix in the front prefix box if present" style={{ display: "block" }}>
                  <input type="checkbox" checked={form.removeFrontPrefix} onChange={(e) => set("removeFrontPrefix", e.target.checked)} data-testid="reannotate-remove-front" /> Remove front prefix
                </label>
              </div>
              <div>
                {text("Back reference start:", form.backStart, (v) => set("backStart", v), "reannotate-back-start", { digits: true, title: "Blank continues from front or enter a number greater than the highest reference designation on the front." })}
                {text("Back prefix:", form.backPrefix, (v) => set("backPrefix", v), "reannotate-back-prefix", { prefix: true, title: "Optional prefix for solder side reference designations (e.g. B_)" })}
                <label className="filter-row" title="If checked will remove the Back side prefix in the back prefix box if present" style={{ display: "block" }}>
                  <input type="checkbox" checked={form.removeBackPrefix} onChange={(e) => set("removeBackPrefix", e.target.checked)} data-testid="reannotate-remove-back" /> Remove back prefix
                </label>
              </div>
            </div>
          </fieldset>

          <ReportWindow lines={lines} testId="reannotate-messages" height={150} />
          {ask && (
            <div style={{ marginTop: 8, padding: 8, border: "1px solid var(--border, #444)" }} data-testid="reannotate-confirm">
              <div style={{ whiteSpace: "pre-line", marginBottom: 6 }}>{ask.question}</div>
              <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                <button onClick={() => void answer(false)} data-testid="reannotate-confirm-no">
                  Cancel
                </button>
                <button className="primary" onClick={() => void answer(true)} data-testid="reannotate-confirm-yes">
                  OK
                </button>
              </div>
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={closeSweepDialog} data-testid="reannotate-close">
            Close
          </button>
          <button className="primary" disabled={busy || ask !== null} onClick={() => void run()} data-testid="reannotate-run">
            Reannotate PCB
          </button>
        </div>
      </div>
    </div>
  );
}

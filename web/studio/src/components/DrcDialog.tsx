// pcbnew.DRCTool.runDRC ("Design Rules Checker", Inspect menu). Read
// pcbnew/dialogs/dialog_drc_base.cpp for the real dialog's shape: a
// top options row (refill zones / test schematic parity), a notebook with
// "Violations (%s)" / "Unconnected Items (%s)" / "Schematic Parity (%s)" /
// "Ignored Tests (%s)" tabs, a "Show: All / Errors [n] / Warnings [n] /
// Exclusions [n]" filter row, then OK-Cancel.
//
// The violations ARE kicad-cli's: GET /api/drc exports the current design
// to a derived .kicad_pcb, runs `kicad-cli pcb drc` on it, and points each
// reported item back at our own id (crates/cli/src/kicad_engine.rs). There
// is no other DRC engine and no engine switch. It takes seconds (about 4 s
// on a 30-part board), so it runs on demand -- when the dialog opens on a
// board kicad-cli has not judged yet, and on "Run DRC" -- with a visible
// running state, never on every change. KiCad's own `type` names are shown
// directly as each violation's category.
//
// "Lint" is a tab of its own: crates/lint, our own checks that KiCad does
// not have (placement quality, net-class track width). In-process and
// cheap, so it follows the board live while the dialog is open; it never
// mixes into kicad-cli's lists.
//
// "Schematic Parity" and "Ignored Tests" are real KiCad tabs with nothing
// behind them yet -- kept as clickable, honestly-empty tabs rather than
// omitted. Exclusions/Save/Delete Marker have no backing concept and are left
// out rather than wired to a no-op.
import { useEffect, useMemo, useState } from "react";
import type { DrcViolation } from "../api/types";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { boundsOfPoints, fitTransform } from "./canvas/view";

type DrcTab = "violations" | "unconnected" | "lint" | "parity" | "ignored";

const STUB_TABS: Array<{ id: DrcTab; label: string }> = [
  { id: "parity", label: "Schematic Parity" },
  { id: "ignored", label: "Ignored Tests" },
];

/** Our id for a violation's item -> the id the canvas selects by: a pad (`REF.PAD`) selects its part, a track segment (`id#n`) its track; the Edge.Cuts outline (`outline`) is nothing to select. */
function baseRef(id: string): string {
  return id.split("#")[0]!.split(".")[0]!;
}

export function DrcDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.drcDialogOpen;
  const [tab, setTab] = useState<DrcTab>("violations");
  const [showErrors, setShowErrors] = useState(true);
  const [showWarnings, setShowWarnings] = useState(true);

  const violations = state.drc?.violations ?? [];
  const errors = useMemo(() => violations.filter((v) => v.severity === "error"), [violations]);
  const warnings = useMemo(() => violations.filter((v) => v.severity === "warning"), [violations]);
  const visible = violations.filter((v) => (v.severity === "error" ? showErrors : showWarnings));
  const unconnected = state.drc?.unconnected_items ?? [];
  const lint = state.lint?.pcb.violations ?? [];
  const running = state.drcRunning;
  const stale = state.drc !== null && !running && state.drcVersion !== state.version;

  // Opening the dialog on a board kicad-cli has not judged yet runs it (the
  // running state below shows meanwhile); a board it already judged keeps its
  // report until "Run DRC". `version === null` re-evaluates this once the
  // first /api/version answer is in.
  useEffect(() => {
    if (open && state.version !== null && !state.drcRunning && (state.drc === null || state.drcVersion !== state.version)) void api.runDrc();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, state.version === null]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_DRC_OPEN", open: false });

  /**
   * KiCad's own "click a violation to select and zoom to it" (the real
   * dialog cross-probes to the board the same way). Selects/hots every
   * item the violation names, then re-frames the PCB canvas on their
   * combined position -- read directly off the live container's own
   * size (this dialog has no canvas ref of its own: Canvas.tsx owns the
   * element, this just measures the one already on screen, the same
   * class name SchematicView.tsx's own view-fit measures by).
   */
  const jumpTo = (v: DrcViolation, index: number, lintRow: boolean) => {
    dispatch(lintRow ? { type: "SET_DRC_LINT_SELECTED", index } : { type: "SET_DRC_SELECTED", index });
    const refs = v.items.flatMap((it) => (it.id && it.id !== "outline" ? [baseRef(it.id)] : []));
    dispatch({ type: "SET_SELECTION", refs });
    dispatch({ type: "SET_HOT", refs });

    const container = document.querySelector(".pcb-canvas-container");
    const rect = container?.getBoundingClientRect();
    if (!rect || rect.width < 50 || rect.height < 50) return;
    const bounds = boundsOfPoints(v.items.map((it) => it.pos));
    if (!bounds) return;
    // A single-point (or tightly-clustered) violation's own bounds are
    // ~0x0 -- pad them out to a sane minimum (2mm) so fitTransform
    // frames a sensible close-up instead of zooming to a single point.
    const padUm = 2_000;
    const padded = { minX: bounds.minX - padUm, minY: bounds.minY - padUm, maxX: bounds.maxX + padUm, maxY: bounds.maxY + padUm };
    dispatch({ type: "SET_VIEW", view: fitTransform(padded, rect.width, rect.height, 60) });
    dispatch({ type: "SET_TAB", tab: "pcb" });
  };

  const row = (v: DrcViolation, index: number, lintRow: boolean, selected: boolean) => (
    <div key={index} className={`problem-row${v.severity === "warning" ? " warn" : ""}${lintRow ? " lint" : ""}${selected ? " selected" : ""}`} onClick={() => jumpTo(v, index, lintRow)}>
      <b>{v.type.replace(/_/g, " ")}</b> <span>{v.description}</span>
      {v.items.length > 0 && <small>{v.items.map((it) => it.description).join(", ")}</small>}
      {v.fix && (
        <small style={{ display: "block", color: "var(--chrome-accent, #4ea1ff)" }}>
          Fix: move {v.fix.mover} toward {v.fix.toward} ({v.fix.suggested_command})
        </small>
      )}
    </div>
  );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 620 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Design Rules Checker</span>
          <span style={{ display: "flex", gap: 12, fontWeight: 400, fontSize: 11 }}>
            <span style={{ color: "var(--chrome-danger)" }}>{errors.length} error(s)</span>
            <span style={{ color: "var(--chrome-warn)" }}>{warnings.length} warning(s)</span>
          </span>
        </div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <div style={{ display: "flex", gap: 12, alignItems: "center", marginBottom: 10, fontSize: 11 }}>
            <button onClick={() => void api.runDrc()} disabled={running}>
              {running ? "Running…" : "Run DRC"}
            </button>
            {state.drc?.engine && <span style={{ color: "var(--chrome-text-dim)" }}>{state.drc.engine}</span>}
            {stale && <span style={{ color: "var(--chrome-warn)" }}>The board changed since this run -- Run DRC again to refresh.</span>}
            {state.drcError && <span style={{ color: "var(--chrome-danger)" }}>{state.drcError}</span>}
          </div>
          {running && (
            <div className="run-banner" role="status">
              <span className="bar" />
              <span>kicad-cli is checking the board (a few seconds). Edits wait until it is done.</span>
            </div>
          )}
          <div style={{ display: "flex", gap: 18, marginBottom: 10 }}>
            <label className="toggle" title="KiCad's own refill. Off by default: kicad-cli 10.99 skips its courtyard checks on a run that refills, so it judges the fills shown on screen instead.">
              <input type="checkbox" checked={state.drcRefillZones} onChange={(e) => dispatch({ type: "SET_DRC_REFILL", refill: e.target.checked })} />
              Refill all zones before performing DRC
            </label>
            <label className="toggle" style={{ opacity: 0.45 }}>
              <input type="checkbox" readOnly disabled />
              Test for parity between PCB and schematic
            </label>
          </div>

          <div className="dock-tabs" style={{ marginBottom: 10 }}>
            <div className={`dock-tab${tab === "violations" ? " active" : ""}`} onClick={() => setTab("violations")}>
              Violations ({violations.length})
            </div>
            <div className={`dock-tab${tab === "unconnected" ? " active" : ""}`} onClick={() => setTab("unconnected")}>
              Unconnected Items ({unconnected.length})
            </div>
            <div className={`dock-tab${tab === "lint" ? " active" : ""}`} onClick={() => setTab("lint")} title="Our own checks, the ones KiCad does not have">
              Lint ({lint.length})
            </div>
            {STUB_TABS.map((t) => (
              <div key={t.id} className={`dock-tab${tab === t.id ? " active" : ""}`} onClick={() => setTab(t.id)}>
                {t.label}
              </div>
            ))}
          </div>

          <div style={{ opacity: running ? 0.5 : 1 }}>
            {tab === "unconnected" &&
              (!state.drc ? (
                <div className="panel-empty">{running ? "Running DRC…" : "Run DRC to list unconnected items."}</div>
              ) : unconnected.length === 0 ? (
                <div className="panel-empty">No unconnected items.</div>
              ) : (
                unconnected.map((v, i) => (
                  <div key={i} className="problem-row" onClick={() => jumpTo(v, -1, false)}>
                    <b>{v.type.replace(/_/g, " ")}</b> <span>{v.description}</span>
                    {v.items.length > 0 && <small>{v.items.map((it) => it.description).join(", ")}</small>}
                  </div>
                ))
              ))}

            {tab === "lint" && (
              <>
                <div className="panel-empty" style={{ textAlign: "left", marginBottom: 6 }}>
                  Our own checks (placement quality, net-class track width). KiCad has no equivalent; its rules are in Violations.
                </div>
                {!state.lint && <div className="panel-empty">Loading…</div>}
                {state.lint && lint.length === 0 && <div className="panel-empty">No findings.</div>}
                {lint.map((v, i) => row(v, i, true, state.drcLintSelected === i))}
              </>
            )}

            {(tab === "parity" || tab === "ignored") && <div className="panel-empty">Not available: no backend data for this tab yet.</div>}

            {tab === "violations" && (
              <>
                <div style={{ display: "flex", alignItems: "center", gap: 14, marginBottom: 8, fontSize: 11 }}>
                  <span style={{ color: "var(--chrome-text-dim)" }}>Show:</span>
                  <label className="toggle">
                    <input type="checkbox" checked={showErrors} onChange={(e) => setShowErrors(e.target.checked)} />
                    Errors ({errors.length})
                  </label>
                  <label className="toggle">
                    <input type="checkbox" checked={showWarnings} onChange={(e) => setShowWarnings(e.target.checked)} />
                    Warnings ({warnings.length})
                  </label>
                </div>

                {!state.drc && <div className="panel-empty">{running ? "Running DRC…" : "Run DRC to check the board."}</div>}
                {state.drc && violations.length === 0 && <div className="panel-empty">No violations.</div>}
                {state.drc && violations.length > 0 && visible.length === 0 && <div className="panel-empty">Nothing matches the current filter.</div>}
                {visible.map((v) => {
                  const index = violations.indexOf(v);
                  return row(v, index, false, state.drcSelected === index);
                })}
              </>
            )}
          </div>
        </div>
        <div className="dialog-footer">
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

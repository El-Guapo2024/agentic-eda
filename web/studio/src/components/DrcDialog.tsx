// pcbnew.DRCTool.runDRC ("Design Rules Checker", Inspect menu). Read
// pcbnew/dialogs/dialog_drc_base.cpp for the real dialog's shape: a
// top options row (refill zones / test schematic parity + a settings
// menu), a notebook with "Violations (%s)" / "Unconnected Items (%s)" /
// "Schematic Parity (%s)" / "Ignored Tests (%s)" tabs, a "Show: All /
// Errors [n] / Warnings [n] / Exclusions [n]" filter row with a
// Save... button, then Delete Marker / Delete All Markers / OK-Cancel.
//
// Violations now come from GET /api/drc -- crates/drc, the ported real
// KiCad DRC engine (clearance/courtyard/silk/track-width/placement-
// quality providers), not a client-side read of this app's own
// placement/routing gate checks the way this dialog used to work (see
// git history for that version). KiCad's own `type` names (ErrorType's
// snake_case, matching kicad-cli's own DRC report) are shown directly as
// each violation's category rather than this app inventing its own.
//
// "Unconnected Items", "Schematic Parity" and "Ignored Tests" are real
// KiCad tabs with nothing behind them yet -- kept as clickable, honestly
// -empty tabs rather than omitted, matching how a disabled menu item
// still shows with "(not ported yet)" instead of vanishing. Exclusions/
// Save/Delete Marker have no backing concept (nothing persists a marker
// to exclude or delete) and are left out rather than wired to a no-op.
import { useMemo, useState } from "react";
import type { DrcViolation } from "../api/types";
import { useStudioDispatch, useStudioState } from "../state/store";
import { boundsOfPoints, fitTransform } from "./canvas/view";

type DrcTab = "violations" | "unconnected" | "parity" | "ignored";

const STUB_TABS: Array<{ id: DrcTab; label: string }> = [
  { id: "unconnected", label: "Unconnected Items" },
  { id: "parity", label: "Schematic Parity" },
  { id: "ignored", label: "Ignored Tests" },
];

/** `<ref>` or `<ref>.<pad>` (crates/drc's `DrcRefItem.id` convention, see types.ts) -> the bare part reference, for SET_HOT/SET_SELECTION (a track/via/zone id has no "." and passes through as-is -- this app's canvas selection already accepts those ids directly, same as a part ref). */
function baseRef(id: string): string {
  return id.split(".")[0]!;
}

export function DrcDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [tab, setTab] = useState<DrcTab>("violations");
  const [showErrors, setShowErrors] = useState(true);
  const [showWarnings, setShowWarnings] = useState(true);

  const violations = state.drc?.violations ?? [];
  const errors = useMemo(() => violations.filter((v) => v.severity === "error"), [violations]);
  const warnings = useMemo(() => violations.filter((v) => v.severity === "warning"), [violations]);
  const visible = violations.filter((v) => (v.severity === "error" ? showErrors : showWarnings));

  if (!state.drcDialogOpen) return null;
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
  const jumpTo = (v: DrcViolation, index: number) => {
    dispatch({ type: "SET_DRC_SELECTED", index });
    const refs = v.items.map((it) => baseRef(it.id));
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
          <div style={{ display: "flex", gap: 18, marginBottom: 10, opacity: 0.45 }} title="eda_drc runs fresh on every open/board-change -- there's no separate 'run DRC' step or progress phase to show.">
            <label className="toggle">
              <input type="checkbox" checked readOnly disabled />
              Refill all zones before performing DRC
            </label>
            <label className="toggle">
              <input type="checkbox" readOnly disabled />
              Test for parity between PCB and schematic
            </label>
          </div>

          <div className="dock-tabs" style={{ marginBottom: 10 }}>
            <div className={`dock-tab${tab === "violations" ? " active" : ""}`} onClick={() => setTab("violations")}>
              Violations ({violations.length})
            </div>
            {STUB_TABS.map((t) => (
              <div key={t.id} className={`dock-tab${tab === t.id ? " active" : ""}`} onClick={() => setTab(t.id)}>
                {t.label}
              </div>
            ))}
          </div>

          {tab !== "violations" && <div className="panel-empty">Not available: no backend data for this tab yet.</div>}

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

              {!state.drc && <div className="panel-empty">Running DRC…</div>}
              {state.drc && violations.length === 0 && <div className="panel-empty">No violations.</div>}
              {state.drc && violations.length > 0 && visible.length === 0 && <div className="panel-empty">Nothing matches the current filter.</div>}
              {visible.map((v) => {
                const index = violations.indexOf(v);
                return (
                  <div key={index} className={`problem-row${v.severity === "warning" ? " warn" : ""}`} onClick={() => jumpTo(v, index)}>
                    <b>{v.type.replace(/_/g, " ")}</b> <span>{v.description}</span>
                    {v.items.length > 0 && <small>{v.items.map((it) => it.description).join(", ")}</small>}
                    {v.fix && (
                      <small style={{ display: "block", color: "var(--chrome-accent, #4ea1ff)" }}>
                        Fix: move {v.fix.mover} toward {v.fix.toward} ({v.fix.suggested_command})
                      </small>
                    )}
                  </div>
                );
              })}
            </>
          )}
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

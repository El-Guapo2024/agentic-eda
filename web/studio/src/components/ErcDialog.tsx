// eeschema.InspectionTool.runERC ("Electrical Rules Checker", Inspect
// menu). Read eeschema/dialogs/dialog_erc_base.cpp for the real dialog's
// shape: a "Run ERC" button, an Errors/Warnings/Exclusions "Show" filter
// row, a flat RC_TREE_MODEL list (one row per marker, in sheet order --
// not grouped under a severity/sheet header), and a cross-probe that pans/
// selects the offending item on click. Cloned from DrcDialog.tsx's own
// structure (same task note that suggested it).
//
// Violations come from GET /api/erc -- crates/kicad's `check_erc`/
// `check_erc_excluding` (gap #4 in GAPS.md: "ERC engine built, zero UI
// exposure" when first written; exclusions followed in a later session).
// Unlike DrcDialog's violations, `check_erc`'s own `CheckResult` shape
// carries no ready-made canvas position -- `location` is one of several
// id-ish strings, not a point (see types.ts's `ErcViolation.location` doc)
// -- so `ercMarkerPosition` (components/schematic/ercMarkerPosition.ts,
// also used by SchematicView.tsx's own canvas markers) resolves it back
// to a point/refs here instead of reading one straight off the violation
// the way DrcDialog's `jumpTo` can.
//
// "Exclude this violation"/un-exclude (dialog_erc.cpp's own right-click
// menu item, here a plain per-row button since this clone has no
// context menu) persists to `design.schematic.erc_exclusions` via
// `add_erc_exclusion`/`delete_erc_exclusion` -- the dialog doesn't
// manually re-fetch afterward; store.tsx's existing poll loop already
// re-runs GET /api/erc on the next version change while this dialog is
// open, same as every other Cmd this app sends.
import { useMemo, useState } from "react";
import { fetchErc } from "../api/client";
import type { ErcViolation } from "../api/types";
import { useStudioDispatch, useStudioState, useStudioApi } from "../state/store";
import { ercMarkerPosition } from "./schematic/ercMarkerPosition";
import { fitTransform } from "./canvas/view";

export function ErcDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [showErrors, setShowErrors] = useState(true);
  const [showWarnings, setShowWarnings] = useState(true);
  const [showExcluded, setShowExcluded] = useState(true);

  const violations = state.erc?.violations ?? [];
  const errors = useMemo(() => violations.filter((v) => v.severity === "error"), [violations]);
  const warnings = useMemo(() => violations.filter((v) => v.severity === "warning"), [violations]);
  const excluded = useMemo(() => violations.filter((v) => v.severity === "excluded"), [violations]);
  const visible = violations.filter((v) => (v.severity === "error" ? showErrors : v.severity === "warning" ? showWarnings : showExcluded));
  const [running, setRunning] = useState(false);
  const [runError, setRunError] = useState<string | null>(null);
  // "Run ERC" (dialog_erc.cpp OnRunERCClick): kicad-cli runs on demand; the
  // live engine re-runs by itself whenever the design changes.
  const runErc = async () => {
    setRunning(true);
    setRunError(null);
    try {
      dispatch({ type: "ERC_OK", erc: await fetchErc(state.ercEngine) });
    } catch (e) {
      setRunError(String(e));
    } finally {
      setRunning(false);
    }
  };

  if (!state.ercDialogOpen) return null;
  const close = () => dispatch({ type: "SET_ERC_DIALOG_OPEN", open: false });

  /**
   * KiCad's own "click a violation to select and zoom to it". Resolves
   * `v.location` to a point/refs (`ercMarkerPosition`), selects/hots any
   * named symbol, then re-frames the schematic canvas on that point --
   * read directly off the live container's own size, the same
   * `.pcb-canvas-container` class name SchematicView.tsx's own container
   * uses (shared layout class, not PCB-specific -- see that file). A
   * location `ercMarkerPosition` can't resolve (a dangling ref/net, or a
   * genuinely unrecognized shape) still selects the row and switches tabs,
   * it just can't additionally re-frame the view -- a known, smaller gap
   * than DRC's own, since the dangling case is rare in practice.
   */
  const jumpTo = (v: ErcViolation, index: number) => {
    dispatch({ type: "SET_ERC_SELECTED", index });
    const resolved = ercMarkerPosition(v.location, state.schematic);
    if (resolved) {
      if (resolved.refs.length > 0) {
        dispatch({ type: "SET_SELECTION", refs: resolved.refs });
        dispatch({ type: "SET_HOT", refs: resolved.refs });
      }
      const container = document.querySelector(".pcb-canvas-container");
      const rect = container?.getBoundingClientRect();
      if (rect && rect.width >= 50 && rect.height >= 50) {
        // A single point has no natural bounds -- pad it out to a sane
        // minimum (2mm) so fitTransform frames a sensible close-up
        // instead of zooming to a point, same padding DrcDialog's own
        // jumpTo uses for the same reason.
        const padUm = 2_000;
        const [x, y] = resolved.at;
        const bounds = { minX: x - padUm, minY: y - padUm, maxX: x + padUm, maxY: y + padUm };
        dispatch({ type: "SET_SCHEMATIC_VIEW", view: fitTransform(bounds, rect.width, rect.height, 60) });
      }
    }
    dispatch({ type: "SET_TAB", tab: "schematic" });
  };

  const toggleExclusion = (v: ErcViolation) => {
    if (!v.location) return;
    void api.cmd(v.severity === "excluded" ? { op: "delete_erc_exclusion", check: v.check, location: v.location } : { op: "add_erc_exclusion", check: v.check, location: v.location });
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 620 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Electrical Rules Checker</span>
          <span style={{ display: "flex", gap: 12, fontWeight: 400, fontSize: 11 }}>
            <span style={{ color: "var(--chrome-danger)" }}>{errors.length} error(s)</span>
            <span style={{ color: "var(--chrome-warn)" }}>{warnings.length} warning(s)</span>
            <span style={{ color: "var(--chrome-text-dim)" }}>{excluded.length} excluded</span>
          </span>
        </div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <div style={{ display: "flex", gap: 12, alignItems: "center", marginBottom: 10, fontSize: 11 }}>
            <span style={{ color: "var(--chrome-text-dim)" }}>Engine:</span>
            <select value={state.ercEngine} onChange={(e) => dispatch({ type: "SET_ERC_ENGINE", engine: e.target.value as "eda" | "kicad" })}>
              <option value="eda">Live (re-runs on every change)</option>
              <option value="kicad">KiCad (kicad-cli)</option>
            </select>
            <button onClick={runErc} disabled={running}>
              {running ? "Running…" : "Run ERC"}
            </button>
            {state.erc?.engine && <span style={{ color: "var(--chrome-text-dim)" }}>{state.erc.engine}</span>}
            {runError && <span style={{ color: "var(--chrome-danger)" }}>{runError}</span>}
          </div>
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
            <label className="toggle">
              <input type="checkbox" checked={showExcluded} onChange={(e) => setShowExcluded(e.target.checked)} />
              Exclusions ({excluded.length})
            </label>
          </div>

          {!state.erc && <div className="panel-empty">Running ERC…</div>}
          {state.erc && violations.length === 0 && <div className="panel-empty">No violations.</div>}
          {state.erc && violations.length > 0 && visible.length === 0 && <div className="panel-empty">Nothing matches the current filter.</div>}
          {visible.map((v) => {
            const index = violations.indexOf(v);
            const isExcluded = v.severity === "excluded";
            return (
              <div key={index} className={`problem-row${v.severity === "warning" ? " warn" : ""}${isExcluded ? " excluded" : ""}`} onClick={() => jumpTo(v, index)}>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 8 }}>
                  <span>
                    <b>{v.check.replace(/_/g, " ")}</b> {v.location && <span>{v.location}</span>}
                  </span>
                  {v.location && (
                    <button
                      style={{ fontSize: 10, padding: "1px 6px", flexShrink: 0 }}
                      title={isExcluded ? "Un-exclude: report this violation again" : "Exclude: stop reporting this exact violation"}
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleExclusion(v);
                      }}
                    >
                      {isExcluded ? "Un-exclude" : "Exclude"}
                    </button>
                  )}
                </div>
                {v.hint && <small>{v.hint}</small>}
              </div>
            );
          })}
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

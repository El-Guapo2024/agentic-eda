// eeschema.InspectionTool.runERC ("Electrical Rules Checker", Inspect
// menu). Read eeschema/dialogs/dialog_erc_base.cpp for the real dialog's
// shape: a "Run ERC" button, an Errors/Warnings/Exclusions "Show" filter
// row, a flat RC_TREE_MODEL list (one row per marker, in sheet order --
// not grouped under a severity/sheet header), and a cross-probe that pans/
// selects the offending item on click. Cloned from DrcDialog.tsx's own
// structure.
//
// The violations ARE kicad-cli's: GET /api/erc exports the current schematic
// to a derived .kicad_sch, runs `kicad-cli sch erc` on it (with the design's
// own ERC pin map in the project file), and points each reported item back
// at our own id (crates/cli/src/kicad_engine.rs). There is no other ERC
// engine and no engine switch. It takes seconds, so it runs on demand --
// when the dialog opens on a schematic kicad-cli has not judged yet, and on
// "Run ERC" -- with a visible running state, never on every change.
//
// Each violation's `location` is our id for its first item (a symbol, a
// pin "REF.PIN", a power symbol, a wire, a label, a no-connect, a text), not
// a point: `ercMarkerPosition` (components/schematic/ercMarkerPosition.ts,
// also used by SchematicView.tsx's own canvas markers) resolves it back to a
// point/refs here instead of reading one straight off the violation the way
// DrcDialog's `jumpTo` can.
//
// "Lint" is a tab of its own: crates/lint's schematic readability checks
// (grid, wire length and overlap, label placement, sheet density), which
// KiCad's ERC does not have. In-process and cheap, so it follows the
// schematic live while the dialog is open; it never mixes into kicad-cli's
// list.
//
// "Exclude this violation"/un-exclude (dialog_erc.cpp's own right-click
// menu item, here a plain per-row button since this clone has no
// context menu) persists to `design.schematic.erc_exclusions` via
// `add_erc_exclusion`/`delete_erc_exclusion`; the report on screen is
// patched in place (no new kicad-cli run for a waiver).
import { useEffect, useMemo, useState } from "react";
import { fetchVersion } from "../api/client";
import type { ErcViolation } from "../api/types";
import { useStudioDispatch, useStudioState, useStudioApi } from "../state/store";
import { ercMarkerPosition } from "./schematic/ercMarkerPosition";
import { fitTransform } from "./canvas/view";

type ErcTab = "erc" | "lint";

export function ErcDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.ercDialogOpen;
  const [tab, setTab] = useState<ErcTab>("erc");
  const [showErrors, setShowErrors] = useState(true);
  const [showWarnings, setShowWarnings] = useState(true);
  const [showExcluded, setShowExcluded] = useState(true);

  const violations = state.erc?.violations ?? [];
  const errors = useMemo(() => violations.filter((v) => v.severity === "error"), [violations]);
  const warnings = useMemo(() => violations.filter((v) => v.severity === "warning"), [violations]);
  const excluded = useMemo(() => violations.filter((v) => v.severity === "excluded"), [violations]);
  const visible = violations.filter((v) => (v.severity === "error" ? showErrors : v.severity === "warning" ? showWarnings : showExcluded));
  const lint = state.lint?.schematic.violations ?? [];
  const running = state.ercRunning;
  const stale = state.erc !== null && !running && state.ercVersion !== state.version;

  // Opening the dialog on a schematic kicad-cli has not judged yet runs it
  // (the running state below shows meanwhile); one it already judged keeps
  // its report until "Run ERC".
  useEffect(() => {
    if (open && state.version !== null && !state.ercRunning && (state.erc === null || state.ercVersion !== state.version)) void api.runErc();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, state.version === null]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_ERC_DIALOG_OPEN", open: false });

  /**
   * KiCad's own "click a violation to select and zoom to it". Resolves
   * `v.location` to a point/refs (`ercMarkerPosition`), selects/hots any
   * named symbol, then re-frames the schematic canvas on that point --
   * read directly off the live container's own size, the same
   * `.pcb-canvas-container` class name SchematicView.tsx's own container
   * uses (shared layout class, not PCB-specific -- see that file). A
   * location `ercMarkerPosition` can't resolve (an item the schematic
   * no longer has, or a genuinely unrecognized shape) still selects the
   * row and switches tabs, it just can't additionally re-frame the view.
   */
  const jumpTo = (v: ErcViolation, index: number, lintRow: boolean) => {
    dispatch(lintRow ? { type: "SET_ERC_LINT_SELECTED", index } : { type: "SET_ERC_SELECTED", index });
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

  const toggleExclusion = async (v: ErcViolation) => {
    if (!v.location) return;
    const wasExcluded = v.severity === "excluded";
    const ok = await api.cmd(wasExcluded ? { op: "delete_erc_exclusion", check: v.check, location: v.location } : { op: "add_erc_exclusion", check: v.check, location: v.location });
    if (!ok) return;
    // The report on screen is patched in place; `version` is the board
    // version after the Cmd, so the waiver does not make the run look out of date.
    const version = await fetchVersion().catch(() => null);
    dispatch({ type: "ERC_MARK_EXCLUDED", check: v.check, location: v.location, excluded: !wasExcluded, version });
  };

  const row = (v: ErcViolation, index: number, lintRow: boolean, selected: boolean) => {
    const isExcluded = v.severity === "excluded";
    return (
      <div key={index} className={`problem-row${v.severity === "warning" ? " warn" : ""}${isExcluded ? " excluded" : ""}${lintRow ? " lint" : ""}${selected ? " selected" : ""}`} onClick={() => jumpTo(v, index, lintRow)}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 8 }}>
          <span>
            <b>{v.check.replace(/_/g, " ")}</b> {v.location && <span>{v.location}</span>}
          </span>
          {!lintRow && v.location && (
            <button
              style={{ fontSize: 10, padding: "1px 6px", flexShrink: 0 }}
              title={isExcluded ? "Un-exclude: report this violation again" : "Exclude: stop reporting this exact violation"}
              onClick={(e) => {
                e.stopPropagation();
                void toggleExclusion(v);
              }}
            >
              {isExcluded ? "Un-exclude" : "Exclude"}
            </button>
          )}
        </div>
        {v.hint && <small>{v.hint}</small>}
      </div>
    );
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
            <button onClick={() => void api.runErc()} disabled={running}>
              {running ? "Running…" : "Run ERC"}
            </button>
            {state.erc?.engine && <span style={{ color: "var(--chrome-text-dim)" }}>{state.erc.engine}</span>}
            {stale && <span style={{ color: "var(--chrome-warn)" }}>The schematic changed since this run -- Run ERC again to refresh.</span>}
            {state.ercError && <span style={{ color: "var(--chrome-danger)" }}>{state.ercError}</span>}
          </div>
          {running && (
            <div className="run-banner" role="status">
              <span className="bar" />
              <span>kicad-cli is checking the schematic (a few seconds). Edits wait until it is done.</span>
            </div>
          )}

          <div className="dock-tabs" style={{ marginBottom: 10 }}>
            <div className={`dock-tab${tab === "erc" ? " active" : ""}`} onClick={() => setTab("erc")}>
              Violations ({violations.length - excluded.length})
            </div>
            <div className={`dock-tab${tab === "lint" ? " active" : ""}`} onClick={() => setTab("lint")} title="Our own readability checks, the ones KiCad does not have">
              Lint ({lint.length})
            </div>
          </div>

          <div style={{ opacity: running ? 0.5 : 1 }}>
            {tab === "erc" && (
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
                  <label className="toggle">
                    <input type="checkbox" checked={showExcluded} onChange={(e) => setShowExcluded(e.target.checked)} />
                    Exclusions ({excluded.length})
                  </label>
                </div>

                {!state.erc && <div className="panel-empty">{running ? "Running ERC…" : "Run ERC to check the schematic."}</div>}
                {state.erc && violations.length === 0 && <div className="panel-empty">No violations.</div>}
                {state.erc && violations.length > 0 && visible.length === 0 && <div className="panel-empty">Nothing matches the current filter.</div>}
                {visible.map((v) => {
                  const index = violations.indexOf(v);
                  return row(v, index, false, state.ercSelected === index);
                })}
              </>
            )}

            {tab === "lint" && (
              <>
                <div className="panel-empty" style={{ textAlign: "left", marginBottom: 6 }}>
                  Our own readability checks (grid, wire length and overlap, label placement, sheet density). KiCad's ERC has no equivalent; its rules are in Violations.
                </div>
                {!state.lint && <div className="panel-empty">Loading…</div>}
                {state.lint && lint.length === 0 && <div className="panel-empty">No findings.</div>}
                {lint.map((v, i) => row(v, i, true, state.ercLintSelected === i))}
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

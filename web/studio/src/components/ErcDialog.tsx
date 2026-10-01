// eeschema.InspectionTool.runERC ("Electrical Rules Checker", Inspect
// menu). Read eeschema/dialogs/dialog_erc_base.cpp for the real dialog's
// shape: a "Run ERC" button, an Errors/Warnings/Exclusions "Show" filter
// row, a flat RC_TREE_MODEL list (one row per marker, in sheet order --
// not grouped under a severity/sheet header), and a cross-probe that pans/
// selects the offending item on click. Cloned from DrcDialog.tsx's own
// structure (same task note that suggested it); see that file for the
// parts of the real dialog neither clone bothers with (Save/Delete
// Marker/Exclusions -- nothing persists a marker to exclude here either).
//
// Violations come from GET /api/erc -- crates/kicad's `check_erc` (gap #4
// in GAPS.md: "ERC engine built, zero UI exposure"), not a client-side
// check. Unlike DrcDialog's violations, `check_erc`'s own `CheckResult`
// shape carries no canvas position -- `location` is a "REF" or "REF.PIN"
// string, not a point -- so clicking a row selects the named symbol and
// switches to the Schematic tab, but does not additionally re-frame the
// view the way DrcDialog's jumpTo zooms to a violation's exact point (a
// known, smaller gap than DRC's own, since the next schematic-tab-fit
// effect `SchematicView.tsx` runs on load still gets the item on screen
// most of the time -- see PARITY-sch.md).
import { useMemo, useState } from "react";
import type { ErcViolation } from "../api/types";
import { useStudioDispatch, useStudioState } from "../state/store";

/** "U1" or "U1.3" -> "U1" (the symbol reference alone, selectable the same way a PCB part ref is) -- "design"/a net name/anything else with no part behind it just selects nothing, harmlessly. */
function baseRef(location: string): string {
  return location.split(".")[0]!;
}

export function ErcDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [showErrors, setShowErrors] = useState(true);
  const [showWarnings, setShowWarnings] = useState(true);

  const violations = state.erc?.violations ?? [];
  const errors = useMemo(() => violations.filter((v) => v.severity === "error"), [violations]);
  const warnings = useMemo(() => violations.filter((v) => v.severity === "warning"), [violations]);
  const visible = violations.filter((v) => (v.severity === "error" ? showErrors : showWarnings));

  if (!state.ercDialogOpen) return null;
  const close = () => dispatch({ type: "SET_ERC_DIALOG_OPEN", open: false });

  const jumpTo = (v: ErcViolation, index: number) => {
    dispatch({ type: "SET_ERC_SELECTED", index });
    if (v.location) {
      const ref = baseRef(v.location);
      dispatch({ type: "SET_SELECTION", refs: [ref] });
      dispatch({ type: "SET_HOT", refs: [ref] });
    }
    dispatch({ type: "SET_TAB", tab: "schematic" });
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 620 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Electrical Rules Checker</span>
          <span style={{ display: "flex", gap: 12, fontWeight: 400, fontSize: 11 }}>
            <span style={{ color: "var(--chrome-danger)" }}>{errors.length} error(s)</span>
            <span style={{ color: "var(--chrome-warn)" }}>{warnings.length} warning(s)</span>
          </span>
        </div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
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

          {!state.erc && <div className="panel-empty">Running ERC…</div>}
          {state.erc && violations.length === 0 && <div className="panel-empty">No violations.</div>}
          {state.erc && violations.length > 0 && visible.length === 0 && <div className="panel-empty">Nothing matches the current filter.</div>}
          {visible.map((v) => {
            const index = violations.indexOf(v);
            return (
              <div key={index} className={`problem-row${v.severity === "warning" ? " warn" : ""}`} onClick={() => jumpTo(v, index)}>
                <b>{v.check.replace(/_/g, " ")}</b> {v.location && <span>{v.location}</span>}
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

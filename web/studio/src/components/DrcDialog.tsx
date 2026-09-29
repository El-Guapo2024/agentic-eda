// Inspect -> DRC. Modeled on KiCad's DRC dialog (a run button, a list of
// violations you can click to locate) but fed entirely by this app's own
// checks -- eda_gates' placement/routing checks, already returned by GET
// /api/state's `checks` field, not KiCad's DRC engine. There's no
// separate "run" step: the backend recomputes checks on every command,
// so this dialog just reads the latest ones.
import React from "react";
import { useStudioDispatch, useStudioState } from "../state/store";

export function DrcDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  if (!state.drcDialogOpen) return null;
  const checks = state.board?.checks ?? [];
  const close = () => dispatch({ type: "SET_DRC_OPEN", open: false });

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Design Rules Checker</span>
          <span>{checks.length} violation(s)</span>
        </div>
        <div className="dialog-body">
          {checks.length === 0 && <div className="panel-empty">No violations.</div>}
          {checks.map((c, i) => (
            <div
              key={i}
              className={`problem-row${c.fail ? "" : " warn"}`}
              onClick={() => {
                const refs = (c.at ?? "").split(/[^A-Za-z0-9_]+/).filter((t) => state.board?.parts.some((p) => p.ref === t));
                dispatch({ type: "SET_HOT", refs });
              }}
            >
              <b>{c.check.replace(/_/g, " ")}</b> <span>{c.at ?? ""}</span>
              <small>{c.hint ?? ""}</small>
            </div>
          ))}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}

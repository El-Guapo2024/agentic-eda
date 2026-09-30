// pcbnew.DRCTool.runDRC ("Design Rules Checker", Inspect menu). Read
// pcbnew/dialogs/dialog_drc_base.cpp for the real dialog's shape: a
// top options row (refill zones / test schematic parity + a settings
// menu), a notebook with "Violations (%s)" / "Unconnected Items (%s)" /
// "Schematic Parity (%s)" / "Ignored Tests (%s)" tabs, a "Show: All /
// Errors [n] / Warnings [n] / Exclusions [n]" filter row with a
// Save... button, then Delete Marker / Delete All Markers / OK-Cancel.
//
// This app has one real data source -- state.board.checks, the
// placement/routing rules already computed server-side and refreshed by
// the same /api/state poll that drives everything else -- so most of
// that maps to an honest simplification rather than a 1:1 port:
//  - no DRC engine runs here, so there's no "running" phase, no
//    per-test progress, and the refill-zones/schematic-parity options
//    are shown (matching the real layout) but disabled, same convention
//    as the toolbar's unbacked controls.
//  - "Unconnected Items", "Schematic Parity" and "Ignored Tests" are
//    real KiCad tabs with nothing behind them yet -- kept as clickable,
//    honestly-empty tabs rather than omitted, matching how a disabled
//    menu item still shows with "(not ported yet)" instead of vanishing.
//  - "Exclusions"/Save/Delete Marker have no backing concept (nothing
//    persists a marker to exclude or delete) and are left out rather
//    than wired to a no-op.
import { useMemo, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";

type DrcTab = "violations" | "unconnected" | "parity" | "ignored";

const STUB_TABS: Array<{ id: DrcTab; label: string }> = [
  { id: "unconnected", label: "Unconnected Items" },
  { id: "parity", label: "Schematic Parity" },
  { id: "ignored", label: "Ignored Tests" },
];

export function DrcDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [tab, setTab] = useState<DrcTab>("violations");
  const [showErrors, setShowErrors] = useState(true);
  const [showWarnings, setShowWarnings] = useState(true);

  const checks = state.board?.checks ?? [];
  const errors = useMemo(() => checks.filter((c) => c.fail), [checks]);
  const warnings = useMemo(() => checks.filter((c) => !c.fail), [checks]);
  const visible = checks.filter((c) => (c.fail ? showErrors : showWarnings));

  if (!state.drcDialogOpen) return null;
  const close = () => dispatch({ type: "SET_DRC_OPEN", open: false });
  const jumpTo = (at: string | null | undefined) => {
    const refs = (at ?? "").split(/[^A-Za-z0-9_]+/).filter((t) => state.board?.parts.some((p) => p.ref === t));
    dispatch({ type: "SET_HOT", refs });
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
          <div style={{ display: "flex", gap: 18, marginBottom: 10, opacity: 0.45 }} title="No DRC engine runs in this app -- checks are live from board state, not a batch test.">
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
              Violations ({checks.length})
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

              {checks.length === 0 && <div className="panel-empty">No violations.</div>}
              {checks.length > 0 && visible.length === 0 && <div className="panel-empty">Nothing matches the current filter.</div>}
              {visible.map((c, i) => (
                <div key={i} className={`problem-row${c.fail ? "" : " warn"}`} onClick={() => jumpTo(c.at)}>
                  <b>{c.check.replace(/_/g, " ")}</b> <span>{c.at ?? ""}</span>
                  <small>{c.hint ?? ""}</small>
                </div>
              ))}
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

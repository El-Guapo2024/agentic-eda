// Port of pcbnew/dialogs/dialog_pns_settings.cpp ("Interactive Router
// Settings...", Ctrl+<). Edits `state.routerSettings`, which `X`/`D`'s own
// session start (`startInteractiveRoute`/`startInlineDrag`) reads fresh --
// see that state field's own doc comment in state/store.tsx for why a
// change here takes effect on the *next* route/drag rather than live,
// mid-session, the way upstream's dialog can (this app starts a brand new
// backend session per route/drag; there is no persistent one to push a
// setting change into).
//
// Scoped to exactly what `eda_pns::RoutingSettings` actually implements
// (see that struct's own doc comment, `crates/pns/src/settings.rs`):
// `Mode` and `RemoveLoops` are the only two fields any routing code
// actually reads. Upstream's dialog has several more fields (Shove Vias,
// Jump Over Obstacles/"back pressure", Smart Pads, Smooth Dragged
// Segments, Optimize Entire Dragged Track, Auto Posture, Allow DRC
// Violations) that either don't exist on this port's settings struct at
// all or exist but are never read by any routing code (`smart_pads`,
// same as this file's own "not implemented" pattern) -- left out
// entirely rather than shown doing nothing, same convention
// PARITY-pcb.md's Board Setup/Zone dialogs already use for a field with
// no real backing. Free Angle Mode is the one exception: the task that
// asked for this dialog named it explicitly, so it's shown (disabled,
// with the reason) rather than omitted -- this port's router only ever
// builds 45-degree traces (`crates/pns/src/direction45.rs`), a decision
// made long before this dialog existed, not something a checkbox here
// could turn on.
import { useEffect, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import type { RouteMode } from "../api/types";

const MODE_OPTIONS: { value: RouteMode; label: string }[] = [
  // Labels verbatim from router_tool.cpp's own status-bar summary (`case
  // PNS::PNS_MODE::RM_MarkObstacles: mode = _("Highlight collisions")`, etc.)
  { value: "mark_obstacles", label: "Highlight collisions" },
  { value: "shove", label: "Shove" },
  { value: "walkaround", label: "Walk around" },
];

export function RouterSettingsDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const open = state.routerSettingsDialogOpen;

  const [mode, setMode] = useState<RouteMode>(state.routerSettings.mode);
  const [removeLoops, setRemoveLoops] = useState(state.routerSettings.removeLoops);

  useEffect(() => {
    if (!open) return;
    setMode(state.routerSettings.mode);
    setRemoveLoops(state.routerSettings.removeLoops);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;

  const close = () => dispatch({ type: "SET_ROUTER_SETTINGS_DIALOG_OPEN", open: false });
  const submit = () => {
    dispatch({ type: "SET_ROUTER_SETTINGS", settings: { mode, removeLoops } });
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Interactive Router Settings...</span>
        </div>
        <div className="dialog-body">
          <div style={{ marginBottom: 10 }}>
            <div style={{ fontWeight: 600, marginBottom: 4 }}>Mode</div>
            {MODE_OPTIONS.map((opt) => (
              <label key={opt.value} className="filter-row">
                <input type="radio" name="router-mode" checked={mode === opt.value} onChange={() => setMode(opt.value)} />
                {opt.label}
              </label>
            ))}
          </div>
          <label className="filter-row">
            <input type="checkbox" checked={removeLoops} onChange={(e) => setRemoveLoops(e.target.checked)} />
            Remove redundant tracks
          </label>
          <label className="filter-row" style={{ opacity: 0.5 }} title="Not implemented -- this router only ever builds 45-degree traces.">
            <input type="checkbox" checked={false} disabled />
            Free angle mode (not implemented)
          </label>
          <div style={{ fontSize: 11, opacity: 0.7, marginTop: 6 }}>Takes effect the next time you start a route or drag (X / D).</div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

// Port of pcbnew/dialogs/dialog_pns_settings.cpp ("Interactive Router Settings...", Ctrl+<).
//
// The rows, their order and their tool tips are the wx dialog's own (dialog_pns_settings_base.cpp at 8303b2ad), enabled the
// way `DIALOG_PNS_SETTINGS::onModeChange` does: "Free angle mode" and "Allow DRC violations" under Highlight collisions,
// "Shove vias" and "Jump over obstacles" under Shove. "Suggest track finish" is hidden in KiCad ("options that are not
// implemented") and so is here.
//
// Edits `state.routerSettings` (kicad-port/routerSettings.ts), which `X`/`D` send with a new session and, on OK, this dialog
// sends to the session that is running (`routeSetSettings`), so a change applies from the next move as upstream's does.
//
// Three rows of KiCad's have nothing behind them in this router -- "Smooth dragged segments", "Optimize entire track being
// dragged" and "Use mouse path to set track posture" -- and are shown disabled, unchecked, with the reason in their tool tip
// (kicad-port/routerSettings.ts `UNSUPPORTED_ROUTER_OPTIONS`), not left out and not shown doing nothing. The optimizer effort
// is the one row this dialog has that KiCad's does not: KiCad keeps `OptimizerEffort` in the settings file only
// (`tools.pns.effort`).
import { useEffect, useState } from "react";
import { routeMove, routeSetSettings } from "../api/client";
import { useStudioDispatch, useStudioState } from "../state/store";
import type { RouteMode } from "../api/types";
import { drawStateFromPreview } from "../kicad-port/routeTool";
import { modeOptionEnabled, UNSUPPORTED_ROUTER_OPTIONS, type OptimizerEffort, type RouterSettings } from "../kicad-port/routerSettings";

const EFFORT_OPTIONS: { value: OptimizerEffort; label: string }[] = [
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "full", label: "Full" },
];

function Check({ label, checked, onChange, title, disabled, indent }: { label: string; checked: boolean; onChange?: (v: boolean) => void; title?: string; disabled?: boolean; indent?: boolean }) {
  return (
    <label className="filter-row" title={title} style={{ opacity: disabled ? 0.5 : 1, marginLeft: indent ? 18 : 0 }}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange?.(e.target.checked)} />
      {label}
    </label>
  );
}

export function RouterSettingsDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const open = state.routerSettingsDialogOpen;

  const [s, setS] = useState<RouterSettings>(state.routerSettings);
  const set = (patch: Partial<RouterSettings>) => setS((cur) => ({ ...cur, ...patch }));

  useEffect(() => {
    if (!open) return;
    setS(state.routerSettings);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;

  const close = () => dispatch({ type: "SET_ROUTER_SETTINGS_DIALOG_OPEN", open: false });
  const submit = () => {
    dispatch({ type: "SET_ROUTER_SETTINGS", settings: s });
    // A route, drag or diff pair in progress takes the new settings from its next move.
    const draw = state.drawState;
    const cur = state.cursorUm;
    if (draw?.kind === "route" || draw?.kind === "drag" || draw?.kind === "diffpair") {
      const applied = routeSetSettings(s);
      if (draw.kind === "route" && cur) void applied.then(() => routeMove(cur.x, cur.y).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(draw, preview) })));
    }
    close();
  };

  const mode = s.mode;
  const radio = (value: RouteMode, label: string) => (
    <label className="filter-row" style={{ fontWeight: 600 }}>
      <input type="radio" name="router-mode" checked={mode === value} onChange={() => set({ mode: value })} />
      {label}
    </label>
  );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 640 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Interactive Router Settings...</span>
        </div>
        <div className="dialog-body">
          <div style={{ display: "flex", gap: 12, alignItems: "flex-start" }}>
            <fieldset style={{ margin: 0, minWidth: 200 }}>
              <legend>Mode</legend>
              {radio("mark_obstacles", "Highlight collisions")}
              <Check indent label="Free angle mode" checked={s.freeAngleMode} disabled={!modeOptionEnabled("freeAngleMode", mode)} onChange={(v) => set({ freeAngleMode: v })} />
              <Check
                indent
                label="Allow DRC violations"
                checked={s.allowDrcViolations}
                disabled={!modeOptionEnabled("allowDrcViolations", mode)}
                onChange={(v) => set({ allowDrcViolations: v })}
                title="(Highlight collisions mode only) - allows one to establish a track even if is violating the DRC rules."
              />
              <div style={{ height: 15 }} />
              {radio("shove", "Shove")}
              <Check
                indent
                label="Shove vias"
                checked={s.shoveVias}
                disabled={!modeOptionEnabled("shoveVias", mode)}
                onChange={(v) => set({ shoveVias: v })}
                title="When disabled, vias are treated as un-movable objects and hugged instead of shoved."
              />
              <Check
                indent
                label="Jump over obstacles"
                checked={s.jumpOverObstacles}
                disabled={!modeOptionEnabled("jumpOverObstacles", mode)}
                onChange={(v) => set({ jumpOverObstacles: v })}
                title={'When enabled, the router tries to move colliding tracks behind solid obstacles (e.g. pads) instead of "reflecting" back the collision'}
              />
              <div style={{ height: 15 }} />
              {radio("walkaround", "Walk around")}
            </fieldset>
            <fieldset style={{ margin: 0, flex: 1 }}>
              <legend>General Options</legend>
              <Check label="Remove redundant tracks" checked={s.removeLoops} onChange={(v) => set({ removeLoops: v })} title="If the new track has the same connection as an already existing track, the old track is removed." />
              <Check label="Optimize pad connections" checked={s.smartPads} onChange={(v) => set({ smartPads: v })} title="When enabled, the router tries to break out pads/vias in a clean way, avoiding acute angles and jagged breakout tracks." />
              <Check label="Smooth dragged segments" checked={false} disabled title={`When enabled, the router attempts to merge several jagged segments into a single straight one (dragging mode).\n${UNSUPPORTED_ROUTER_OPTIONS.smoothDragged}`} />
              <Check
                label="Optimize entire track being dragged"
                checked={false}
                disabled
                title={`When enabled, the entire portion of the track that is visible on the screen will be optimized and re-routed when a segment is dragged.  When disabled, only the area near the segment being dragged will be optimized.\n${UNSUPPORTED_ROUTER_OPTIONS.optimizeEntireDraggedTrack}`}
              />
              <Check label="Use mouse path to set track posture" checked={false} disabled title={`When enabled, the posture of tracks will be guided by how the mouse is moved from the starting location\n${UNSUPPORTED_ROUTER_OPTIONS.autoPosture}`} />
              <Check
                label="Fix all segments on click"
                checked={s.fixAllSegments}
                onChange={(v) => set({ fixAllSegments: v })}
                title="When enabled, all track segments will be fixed in place up to the cursor location.  When disabled, the last segment (closest to the cursor) will remain free and follow the cursor."
              />
              <label className="filter-row" title="Bigger means cleaner traces, but slower routing. KiCad keeps this one in its settings file (tools.pns.effort) and has no row for it in this dialog.">
                Optimizer effort
                <select value={s.optimizerEffort} onChange={(e) => set({ optimizerEffort: e.target.value as OptimizerEffort })}>
                  {EFFORT_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </label>
            </fieldset>
          </div>
          <div style={{ fontSize: 11, opacity: 0.7, marginTop: 8 }}>A route or drag in progress takes the new settings from its next move.</div>
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

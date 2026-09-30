// The status bar. eda_draw_frame.cpp: CreateStatusBar(8) -- 8 fields,
// confirmed by field index for several: 1 = zoom (UpdateStatusBar/
// GetZoomLevelIndicator), 4 = grid (DisplayGridMsg), 5 = units
// (DisplayUnitsMsg), 6 = the active tool's message (DisplayToolMsg), 7 =
// a constraints message (DisplayConstraintsMsg, e.g. grid-snap/ortho
// state while a tool is running). Fields 0/2/3 weren't pinned down to a
// specific setter this session (0 is likely a general hint/help string;
// 2/3 read as cursor position/delta by elimination and by matching the
// task brief's stated field order) -- not a full transcription, but
// field 6 (tool message) is real and now backed by state.activeTool.
import { TOOL_MESSAGES, useStudioDispatch, useStudioState } from "../state/store";
import type { LengthUnit } from "../state/units";
import { formatLength, formatXY, toPolar } from "../state/units";

export function StatusBar() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();

  const cursor = state.cursorUm;
  const origin = state.moveOriginUm;
  const dx = cursor && origin ? cursor.x - origin.x : null;
  const dy = cursor && origin ? cursor.y - origin.y : null;

  return (
    <div className="status-bar">
      <span className="field">Z {state.view.scale > 0 ? (state.view.scale * 25.4).toFixed(2) : "–"}</span>
      <span className="field">{cursor ? (state.polar ? `r ${toPolar(cursor.x, cursor.y, state.units).r}  θ ${toPolar(cursor.x, cursor.y, state.units).theta}` : formatXY(cursor.x, cursor.y, state.units)) : "–"}</span>
      <span className="field">
        {dx !== null && dy !== null
          ? state.polar
            ? `Δr ${toPolar(dx, dy, state.units).r}  Δθ ${toPolar(dx, dy, state.units).theta}`
            : `dx ${formatLength(dx, state.units)}  dy ${formatLength(dy, state.units)}  dist ${formatLength(Math.hypot(dx, dy), state.units)}`
          : "dx –  dy –  dist –"}
      </span>
      <span className="field">grid {formatLength(state.gridUm, state.units)}</span>
      <label className="toggle" title="Polar coordinates">
        <input type="checkbox" checked={state.polar} onChange={() => dispatch({ type: "TOGGLE_POLAR" })} />
        polar
      </label>
      <select value={state.units} onChange={(e) => dispatch({ type: "SET_UNITS", units: e.target.value as LengthUnit })} title="Units">
        <option value="mm">mm</option>
        <option value="mil">mil</option>
        <option value="in">in</option>
      </select>
      <span className="field">{TOOL_MESSAGES[state.activeTool]}</span>
      {state.activeLayer && <span className="field">layer {state.activeLayer}</span>}
      <span className="spacer" />
      <label className="toggle" title="Refuse a move/edit that adds gate failures (not a KiCad feature)">
        <input type="checkbox" checked={state.strict} onChange={(e) => dispatch({ type: "SET_STRICT", strict: e.target.checked })} />
        strict
      </label>
    </div>
  );
}

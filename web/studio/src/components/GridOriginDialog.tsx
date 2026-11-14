// `ACTIONS::gridOrigin` ("Grid Origin...") -> `COMMON_TOOLS::GridOrigin`: `WX_PT_ENTRY_DIALOG( m_frame, "Grid Origin", "X:", "Y:", origin, true )` -- the X and Y of
// the point the grid is anchored at, in the display unit, starting from where it is. OK sets the origin the way a click of the Grid Origin tool would
// (`RunAction( ACTIONS::gridSetOrigin, new VECTOR2D( ... ) )`): the action carries the point as its parameter. The entries are `ABS_X_COORD` / `ABS_Y_COORD`
// (`WX_PT_ENTRY_DIALOG`'s `SetCoordType`): on the board they count from Origins & Axes' origin, each axis either way (UI-LEFT-2 PP9; with the grid origin as
// the display origin, the origin itself shows as 0, 0).
import { useEffect, useState } from "react";
import { useStudioState } from "../state/store";
import { setGridOriginDialogOpen, useCommonDialogs } from "../state/commonDialogs";
import { useFootprintGridOrigin } from "../state/gridOrigin";
import { nmFrom, nmTo } from "../state/units";
import { originFromEntries, originOf } from "../kicad-port/gridOrigin";
import { useActionRunner } from "../actions/useActionRunner";
import { IDENTITY_TRANSFORMS, useOriginTransforms } from "../state/originTransforms";
import { fromDisplay, toDisplay } from "../kicad-port/originTransforms";

export function GridOriginDialog() {
  const open = useCommonDialogs().gridOrigin;
  const state = useStudioState();
  const footprintOrigin = useFootprintGridOrigin();
  const { run } = useActionRunner();
  const unit = state.units;
  const [x, setX] = useState("");
  const [y, setY] = useState("");

  // the schematic editors' grid origin is (0, 0) (`SCH_BASE_FRAME::GetGridOrigin`)
  const origin = state.tab === "footprint" ? footprintOrigin : state.tab === "pcb" ? originOf(state.board?.grid_origin) : { x: 0, y: 0 };
  // the board editor's and the Footprint Editor's `GetOriginTransforms()` (UI-LEFT-3 F3-8); the schematic frames' change nothing here
  const frameXf = useOriginTransforms();
  const xf = state.tab === "pcb" || state.tab === "footprint" ? frameXf : IDENTITY_TRANSFORMS;

  // Seed the entries when the dialog opens: where the origin is now.
  useEffect(() => {
    if (!open) return;
    const digits = unit === "mm" ? 4 : unit === "mil" ? 2 : 5;
    setX(nmTo(toDisplay(xf, origin.x, "abs_x"), unit).toFixed(digits));
    setY(nmTo(toDisplay(xf, origin.y, "abs_y"), unit).toFixed(digits));
    // Only when the dialog opens -- not when the origin changes under it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;
  const close = () => setGridOriginDialogOpen(false);
  const typed = originFromEntries(x, y, (v) => nmFrom(v, unit));
  const entered = typed ? { x: Math.round(fromDisplay(xf, typed.x, "abs_x")), y: Math.round(fromDisplay(xf, typed.y, "abs_y")) } : null;
  const ok = () => {
    if (!entered) return;
    run("common.Control.gridSetOrigin", entered);
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 320 }} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Enter" && ok()} role="dialog" aria-label="Grid Origin">
        <div className="dialog-header">
          <span>Grid Origin</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "30px 1fr 36px" }}>
            <span>X:</span>
            <input aria-label="X" autoFocus value={x} onChange={(e) => setX(e.target.value)} />
            <span>{unit}</span>
            <span>Y:</span>
            <input aria-label="Y" value={y} onChange={(e) => setY(e.target.value)} />
            <span>{unit}</span>
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!entered} onClick={ok}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

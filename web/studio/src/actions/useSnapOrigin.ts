// Keeps the point the snapping of the editor on screen is anchored at current (`components/canvas/gridHelper.ts` `setSnapOrigin`): the board's grid origin on
// the PCB tab, the session's in the Footprint Editor, (0, 0) in the schematic and the symbol editor, which have no grid origin. Every tool that snaps a click
// through `snapPoint` (and the grid helpers' `GridEnv.origin`) reads it there, so none of them has to be told. Call once, near the app's root.
//
// It keeps that editor's grid snapping current the same way (`setGridSnapping`, `GAL::GetGridSnapping`, UI-LEFT-2 CC5): its Display Options' "Snap to grid:"
// (`grid.snap` of the editor's window, state/commonOptions.ts) -- Always, When grid shown (while the editor's grid is drawn: the board's and the schematic's grid
// toggle, the Footprint Editor's with its Appearance row, the Symbol Editor's), Never.
import { useLayoutEffect } from "react";
import { useStudioState } from "../state/store";
import { useFootprintGridOrigin } from "../state/gridOrigin";
import { useFpState } from "../state/footprintEditorStore";
import { useSymState } from "../state/symbolEditorStore";
import { useCommonOptions, windowOptionsOf } from "../state/commonOptions";
import { setGridSnapping, setSnapOrigin } from "../components/canvas/gridHelper";
import { NO_ORIGIN, originOf } from "../kicad-port/gridOrigin";
import { gridSnappingActive } from "../kicad-port/windowOptions";

export function useSnapOrigin(): void {
  const state = useStudioState();
  const fp = useFpState();
  const sym = useSymState();
  const opts = useCommonOptions();
  const footprintOrigin = useFootprintGridOrigin();
  const board = originOf(state.board?.grid_origin);
  const x = state.tab === "pcb" ? board.x : state.tab === "footprint" ? footprintOrigin.x : 0;
  const y = state.tab === "pcb" ? board.y : state.tab === "footprint" ? footprintOrigin.y : 0;
  useLayoutEffect(() => {
    setSnapOrigin(x === 0 && y === 0 ? NO_ORIGIN : { x, y });
  }, [x, y]);
  const gridShown = state.tab === "footprint" ? fp.gridVisible && fp.layerVisible["obj:grid"] !== false : state.tab === "symbol" ? sym.gridVisible : state.gridVisible;
  const snapping = gridSnappingActive(windowOptionsOf(opts, state.tab).gridSnap, gridShown);
  useLayoutEffect(() => {
    setGridSnapping(snapping);
  }, [snapping]);
}

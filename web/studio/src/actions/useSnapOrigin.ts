// Keeps the point the snapping of the editor on screen is anchored at current (`components/canvas/gridHelper.ts` `setSnapOrigin`): the board's grid origin on
// the PCB tab, the session's in the Footprint Editor, (0, 0) in the schematic and the symbol editor, which have no grid origin. Every tool that snaps a click
// through `snapPoint` / `snapWithAnchors` reads it there, so none of them has to be told. Call once, near the app's root.
import { useLayoutEffect } from "react";
import { useStudioState } from "../state/store";
import { useFootprintGridOrigin } from "../state/gridOrigin";
import { setSnapOrigin } from "../components/canvas/gridHelper";
import { NO_ORIGIN, originOf } from "../kicad-port/gridOrigin";

export function useSnapOrigin(): void {
  const state = useStudioState();
  const footprintOrigin = useFootprintGridOrigin();
  const board = originOf(state.board?.grid_origin);
  const x = state.tab === "pcb" ? board.x : state.tab === "footprint" ? footprintOrigin.x : 0;
  const y = state.tab === "pcb" ? board.y : state.tab === "footprint" ? footprintOrigin.y : 0;
  useLayoutEffect(() => {
    setSnapOrigin(x === 0 && y === 0 ? NO_ORIGIN : { x, y });
  }, [x, y]);
}

// What every tab of the Appearance panel reads and writes: the state, the board's nets and classes, and one `op` that dispatches an edit
// (kicad-port/appearanceOps.ts) to the store.
import { useCallback, useMemo } from "react";
import { useStudioDispatch, useStudioState } from "../../../state/store";
import { netsContext } from "../../../kicad-port/appearanceNets";
import type { AppearanceOp } from "../../../kicad-port/appearanceOps";

export function useAppearanceView() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const nets = useMemo(() => netsContext(state.board), [state.board]);
  const op = useCallback((o: AppearanceOp) => dispatch({ type: "APPEARANCE", op: o }), [dispatch]);
  return { state, dispatch, nets, op };
}

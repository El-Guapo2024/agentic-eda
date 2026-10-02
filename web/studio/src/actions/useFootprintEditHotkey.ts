// `pcbnew.EditorControl.EditFpInFpEditor` ("Open in Footprint Editor",
// Ctrl+E on a selected footprint, GAPS.md #8 step 1) and
// `pcbnew.EditorControl.EditLibFpInFpEditor` ("Edit Library Footprint...",
// Ctrl+Shift+E): switch to the Footprint Editor tab and open the selected
// part's own footprint. A small, separate hook (not folded into
// useActionRunner.ts) because it is the one thing in this app that has to
// reach across two otherwise-independent stores -- the PCB tab's selection
// (state/store.tsx) and the Footprint Editor's own open-document state
// (state/footprintEditorStore.tsx) -- which useActionRunner.ts has no way
// to express (every other action there touches exactly one store).
//
// The two action handlers (useActionRunner.ts, "pcbnew parity" block) are
// ordinary registry entries now -- so the hotkey, the menu item and the
// toolbar button all share one path -- and only record the footprint name
// in `state.pcbx.fpEditRequest`; this hook, called once from App.tsx where
// both providers are already in scope, performs it. (It used to be its own
// window keydown listener; that bypassed the menu/toolbar entirely.)
import { useEffect } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { useFpApi } from "../state/footprintEditorStore";

export function useFootprintEditHotkey() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const fpApi = useFpApi();
  const name = state.pcbx.fpEditRequest;

  useEffect(() => {
    if (!name) return;
    dispatch({ type: "PCBX", patch: { fpEditRequest: null } });
    dispatch({ type: "SET_TAB", tab: "footprint" });
    void fpApi.openFootprint(name);
  }, [name, dispatch, fpApi]);
}

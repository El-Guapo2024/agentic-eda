// `pcbnew.EditorControl.editFpInFpEditor` ("Edit Footprint", Ctrl+E on a
// selected footprint, GAPS.md #8 step 1): switches to the Footprint
// Editor tab and opens the selected part's own footprint. A small,
// separate hook (not folded into useGlobalHotkeys.ts's dotted-action
// registry) because it is the one hotkey in this app that has to reach
// across two otherwise-independent stores -- the PCB tab's selection
// (state/store.tsx) and the Footprint Editor's own open-document state
// (state/footprintEditorStore.tsx) -- which useActionRunner.ts has no
// way to express (every other action there touches exactly one store).
// Called once, from App.tsx, where both providers are already in scope.
import { useEffect } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { useFpApi } from "../state/footprintEditorStore";

const TEXT_INPUT_TAGS = new Set(["INPUT", "SELECT", "TEXTAREA"]);

export function useFootprintEditHotkey() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const fpApi = useFpApi();

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const target = e.target as HTMLElement | null;
      if (target && TEXT_INPUT_TAGS.has(target.tagName)) return;
      if (!e.ctrlKey && !e.metaKey) return;
      if (e.key.toLowerCase() !== "e") return;
      if (state.tab !== "pcb" || state.selection.size !== 1) return;
      const ref = [...state.selection][0]!;
      const part = state.board?.parts.find((p) => p.ref === ref);
      if (!part) return;
      const name = part.footprint;
      if (!name) {
        dispatch({ type: "TOAST", message: `${ref} has no footprint/package to open`, kind: "error" });
        return;
      }
      e.preventDefault();
      dispatch({ type: "SET_TAB", tab: "footprint" });
      void fpApi.openFootprint(name);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [state.tab, state.selection, state.board, dispatch, fpApi]);
}

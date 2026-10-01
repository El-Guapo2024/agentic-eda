// `SCH_ACTIONS::editLibSymbolWithLibEdit` ("Edit Library Symbol..."),
// confirmed directly from `eeschema/tools/sch_actions.cpp`/
// `sch_editor_control.cpp::EditWithSymbolEditor` this session: **not**
// plain Ctrl+E (that's `pcbnew.EditorControl.editFpInFpEditor`, the
// Footprint Editor's own "Edit Footprint" from a selected PCB footprint
// -- a different action on a different tab) -- the real schematic-side
// hotkey is Ctrl+Shift+E. Plain `E` on a placed symbol stays bound to
// `SCH_ACTIONS::properties` (Symbol Properties -- Reference/Value/
// Footprint/Datasheet, already wired), exactly as real eeschema has it;
// this hook only adds the second, library-editing action, the same way
// `useFootprintEditHotkey.ts` adds Ctrl+E for the PCB tab without
// touching that tab's own already-bound `E`.
//
// A small, separate hook (not folded into useGlobalHotkeys.ts's dotted-
// action registry) for the same reason `useFootprintEditHotkey.ts` is:
// it has to reach across two otherwise-independent stores -- the
// Schematic tab's own selection (state/store.tsx) and the Symbol
// Editor's own open-document state (state/symbolEditorStore.tsx) --
// which useActionRunner.ts has no way to express. Called once, from
// App.tsx, where both providers are already in scope.
import { useEffect } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { useSymApi } from "../state/symbolEditorStore";

const TEXT_INPUT_TAGS = new Set(["INPUT", "SELECT", "TEXTAREA"]);

export function useSymbolEditHotkey() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const symApi = useSymApi();

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const target = e.target as HTMLElement | null;
      if (target && TEXT_INPUT_TAGS.has(target.tagName)) return;
      if (!e.shiftKey || (!e.ctrlKey && !e.metaKey)) return;
      if (e.key.toLowerCase() !== "e") return;
      if (state.tab !== "schematic" || state.selection.size !== 1) return;
      const id = [...state.selection][0]!;
      const sym = state.schematic?.symbols.find((s) => s.id === id);
      if (!sym) return;
      // A synthetic/unresolvable lib_id (no real library symbol -- see
      // `SchematicSymbol.lib_id`'s own doc) still opens the Symbol
      // Editor, on a fresh name derived from the reference: there is
      // nothing to edit from yet, same "unresolvable name starts blank"
      // precedent `Cmd::OpenSymbolForEdit`'s own doc sets.
      const libId = sym.lib_id && sym.lib_id.trim() ? sym.lib_id : `eda:${sym.id}`;
      e.preventDefault();
      dispatch({ type: "SET_TAB", tab: "symbol" });
      void symApi.openSymbol(libId);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [state.tab, state.selection, state.schematic, dispatch, symApi]);
}

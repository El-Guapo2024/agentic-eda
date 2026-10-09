// What the right-click menus of the Nets and Net Classes tabs do beyond editing the appearance: highlight, and select or unselect the tracks and vias
// (`PCB_ACTIONS::highlightNet`, `selectNet`, `deselectNet`, run by `onNetContextMenu` / `onNetclassContextMenu` with a net code each).
import { useCallback } from "react";
import { useStudioDispatch, useStudioState } from "../../../state/store";
import { netItems } from "../../../kicad-port/pcbSelectionOps";

export function useNetActions() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const board = state.board;

  /** `highlightNet`: the net, or every net of a class (the first one starts the highlight, the others join it). */
  const highlight = useCallback((nets: string[]) => dispatch({ type: "SET_NET_HIGHLIGHT_SET", nets }), [dispatch]);

  /** `SelectAllItemsOnNet` / its inverse for each net: the tracks and vias of the nets are added to the selection, or taken out of it, as the selection filter and locks allow. */
  const select = useCallback(
    (nets: string[], add: boolean) => {
      if (!board) return;
      const locked = new Set(board.locked ?? []);
      const kinds = new Map<string, "track" | "via">();
      for (const t of board.routing?.tracks ?? []) kinds.set(t.id, "track");
      for (const v of board.routing?.vias ?? []) kinds.set(v.id, "via");
      const passes = (id: string): boolean => {
        if (locked.has(id) && !state.selectionFilter.lockedItems) return false;
        const kind = kinds.get(id);
        return kind === "track" ? state.selectionFilter.tracks : kind === "via" ? state.selectionFilter.vias : true;
      };
      const next = new Set(state.selection);
      for (const id of netItems(board, new Set(nets))) {
        if (!passes(id)) continue;
        if (add) next.add(id);
        else next.delete(id);
      }
      dispatch({ type: "SET_SELECTION", refs: [...next] });
    },
    [board, dispatch, state.selection, state.selectionFilter]
  );

  return { highlight, select };
}

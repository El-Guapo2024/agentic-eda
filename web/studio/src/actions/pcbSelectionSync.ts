// The cross-probing / sheet selection rows of the UI-actions sweep (docs/parity/UI-ACTIONS.md):
// Select on Schematic, Items in Same Hierarchical Sheet, Sheet (select on sheet), and the selection
// tool's Sync Selection / Sync Selection With Nets events. Each cites pcbnew/tools/pcb_selection_tool.cpp
// or board_editor_control.cpp at 8303b2ad. Called from `registerPcbEditSweep`.
//
// PCB and schematic share one reference-designator identity and one `state.selection` (see
// components/panels/PropertiesPanel.tsx), so "send the selection to the other editor" is a tab switch
// with the footprint references selected, and "sync" is what the receiving editor does with it.

import type { BoardState } from "../api/types";
import { fetchSchematic } from "../api/client";
import { collectSheetRefs, crossProbeView, sheetOfRef } from "../kicad-port/crossProbe";
import { itemBounds, unionBounds } from "../kicad-port/pcbItems";
import { selectConnections } from "../kicad-port/pcbSelectionOps";
import { fitTransform, type Bounds } from "../kicad-port/view";
import type { SweepCtx } from "./pcbSweepKit";

/** The canvas's pixel size, as `useActionRunner`'s own `canvasRect` reads it. */
function canvasSize(): { width: number; height: number } | null {
  const r = document.querySelector(".pcb-canvas-container")?.getBoundingClientRect();
  return r ? { width: r.width, height: r.height } : null;
}

/** The footprint references among `ids`, a group standing for its members (`crossProbeTypes`: pads, footprints, groups). */
export function partRefsOf(board: BoardState, ids: Iterable<string>): string[] {
  const parts = new Set(board.parts.map((p) => p.ref));
  const groups = new Map((board.drawings?.groups ?? []).map((g) => [g.id, g.member_ids]));
  const out: string[] = [];
  const seen = new Set<string>();
  const add = (id: string) => {
    if (parts.has(id) && !seen.has(id)) {
      seen.add(id);
      out.push(id);
    }
  };
  for (const id of ids) {
    const members = groups.get(id);
    if (members) members.forEach(add);
    else add(id);
  }
  return out;
}

/** The bounding box (um) the given items cover, as `Bounds`. */
function boundsOf(board: BoardState, ids: readonly string[]): Bounds | null {
  const u = unionBounds(ids.map((id) => itemBounds(board, id)));
  return u ? { minX: u[0], minY: u[1], maxX: u[2], maxY: u[3] } : null;
}

export function registerPcbSelectionSync(m: Map<string, () => void>, ctx: SweepCtx): void {
  const { state, dispatch } = ctx;
  const board = state.board;
  const toast = (message: string, kind: "info" | "error" = "info") => dispatch({ type: "TOAST", message, kind });
  const pcbOnly =
    (fn: () => void) =>
    () => {
      if (state.tab === "pcb") fn();
    };

  // ---------------------------------------------------------------- Select on Schematic
  // BOARD_EDITOR_CONTROL::ExplicitCrossProbeToSch -> SendSelectItemsToSch( selection, nullptr, true ): the selected footprints
  // (a group's members too) are selected in the schematic, which comes to the front.
  m.set(
    "pcbnew.InteractiveSelection.SelectOnSchematic",
    pcbOnly(() => {
      if (!board) return;
      const refs = partRefsOf(board, state.selection);
      if (refs.length === 0) return; // `HasTypes( crossProbeTypes )`
      dispatch({ type: "SET_SELECTION", refs });
      dispatch({ type: "SET_TAB", tab: "schematic" });
    })
  );

  // ---------------------------------------------------- Items in Same Hierarchical Sheet
  // PCB_SELECTION_TOOL::selectSameSheet: one selected footprint; every footprint on its sheet is selected, with their connections
  // (`selectAllItemsOnSheet`). The sheet is the one whose page holds the footprint's symbol.
  const sheetsOfDesign = () =>
    collectSheetRefs(async (path) => {
      const s = await fetchSchematic(path);
      return { symbolIds: s.symbols.map((x) => x.id), childIds: s.sheets.map((x) => x.id) };
    });
  const selectOnSheetPath = (sheets: Map<string, Set<string>>, path: string) => {
    if (!board) return;
    const known = new Set(board.parts.map((p) => p.ref));
    const refs = [...(sheets.get(path) ?? [])].filter((r) => known.has(r));
    dispatch({ type: "SET_SELECTION", refs: [...refs, ...selectConnections(board, refs)] });
    return refs;
  };
  m.set(
    "pcbnew.InteractiveSelection.SelectSameSheet",
    pcbOnly(() => {
      if (!board || state.selection.size !== 1) return;
      const ref = [...state.selection][0]!;
      if (!board.parts.some((p) => p.ref === ref)) return; // "this function currently only supports footprints"
      void sheetsOfDesign()
        .then((sheets) => {
          const path = sheetOfRef(sheets, ref);
          if (path === null) {
            toast(`${ref} is not on any schematic sheet.`, "error");
            return;
          }
          selectOnSheetPath(sheets, path);
        })
        .catch(() => toast("Could not read the schematic sheets.", "error"));
    })
  );

  // ------------------------------------------------------------------------------ Sheet
  // PCB_SELECTION_TOOL::selectSheetContents (`selectOnSheetFromEeschema`, sent with the sheet chosen in the schematic): the footprints of
  // that sheet and their connections, then `zoomFitSelection`. Here the sheet is the one the schematic is showing.
  m.set(
    "pcbnew.InteractiveSelection.SelectOnSheet",
    pcbOnly(() => {
      if (!board) return;
      const path = state.currentSheetPath.join("/");
      void sheetsOfDesign()
        .then((sheets) => {
          if (!sheets.has(path)) {
            toast("That sheet is not part of the schematic.", "error");
            return;
          }
          const refs = selectOnSheetPath(sheets, path) ?? [];
          const rect = canvasSize();
          const bounds = rect ? boundsOf(board, [...refs, ...selectConnections(board, refs)]) : null;
          if (rect && bounds && (bounds.maxX !== bounds.minX || bounds.maxY !== bounds.minY)) dispatch({ type: "SET_VIEW", view: fitTransform(bounds, rect.width, rect.height, 0) });
          if (refs.length === 0) toast("No footprints from that sheet are on the board.");
        })
        .catch(() => toast("Could not read the schematic sheets.", "error"));
    })
  );

  // ------------------------------------------------------- Sync Selection (with nets)
  // PCB_SELECTION_TOOL::doSyncSelection: the items sent from the schematic become the selection -- with `selectConnections` for the
  // "with nets" form -- and, with the default cross-probing settings (`center_on_items`, `zoom_to_fit`), the view centres on them.
  const sync = (withNets: boolean) =>
    pcbOnly(() => {
      if (!board || state.selection.size === 0) return;
      const ids = [...state.selection];
      const refs = partRefsOf(board, ids);
      const all = withNets ? [...new Set([...ids, ...selectConnections(board, refs)])] : ids;
      if (withNets) dispatch({ type: "SET_SELECTION", refs: all });
      const rect = canvasSize();
      const bounds = rect ? boundsOf(board, all) : null;
      if (rect && bounds) dispatch({ type: "SET_VIEW", view: crossProbeView(state.view, bounds, rect.width, rect.height) });
    });
  m.set("pcbnew.InteractiveSelection.SyncSelection", sync(false));
  m.set("pcbnew.InteractiveSelection.SyncSelectionWithNets", sync(true));
}

// One handle on "the editor that is on screen", for the common actions (src/actions/commonActions.ts).
//
// KiCad's shared tools -- COMMON_TOOLS, the selection/group/picker tools -- run in every editor frame and reach
// the frame through `EDA_DRAW_FRAME` (its view, its selection, its document extents). The studio's four canvases
// keep that state in three stores (the board and the schematic share one, the footprint editor and the symbol editor
// have their own), so an `EditorAdapter` is the small common face of whichever one the active tab is: the pan/zoom
// view, the cursor, the selection, and the boxes of the items it can select. Nothing here is a new copy of state;
// every field reads or writes the store it belongs to.
import type { Dispatch } from "react";
import type { Action as StudioAction, EditorTab, StudioState, ViewTransform } from "../state/store";
import type { FootprintEditorState, FpAction } from "../state/footprintEditorStore";
import type { SymAction, SymbolEditorState } from "../state/symbolEditorStore";
import { boxOfIds, boxOfPoints, footprintItemBoxes, pcbContentBox, pcbItemBoxes, schematicItemBoxes, symbolItemBoxes, unionBoxes, type Box, type ItemBoxes } from "../kicad-port/itemBoxes";
import { PAGE_HEIGHT_UM, PAGE_WIDTH_UM } from "../components/schematic/drawingSheet";
import { GRID as SCH_GRID_UM } from "../components/schematic/layout";
import { DEFAULT_PCB_GRIDS_UM } from "../kicad-port/grid";

/** `DefaultGridSizeList()` for eeschema and the symbol editor: 100, 50, 25 and 10 mil. */
const EESCHEMA_GRIDS_UM: readonly number[] = [2540, 1270, 635, 254];

/** The four editors that have a canvas (the 3D viewer has its own camera and none of these tools). */
export type CanvasTab = "pcb" | "schematic" | "footprint" | "symbol";

export function isCanvasTab(tab: EditorTab): tab is CanvasTab {
  return tab === "pcb" || tab === "schematic" || tab === "footprint" || tab === "symbol";
}

export interface EditorSnapshot {
  tab: EditorTab;
  studio: StudioState;
  fp: FootprintEditorState;
  sym: SymbolEditorState;
  dispatch: Dispatch<StudioAction>;
  fpDispatch: Dispatch<FpAction>;
  symDispatch: Dispatch<SymAction>;
}

export interface EditorAdapter {
  tab: CanvasTab;
  view: ViewTransform;
  setView(view: ViewTransform): void;
  cursor: { x: number; y: number } | null;
  setCursor(at: { x: number; y: number } | null): void;
  /** The editor's own grid, in um. */
  gridUm: number;
  /** `GRID_SETTINGS::grids`: the grids the editor offers, in list order -- null where the editor's grid is fixed (the schematic's is 50 mil). */
  gridList: readonly number[] | null;
  setGridUm(um: number): void;
  selection: ReadonlySet<string>;
  /** `SELECTION_TOOL`'s `ClearSelection` + `AddItemToSel`: replace the selection. */
  setSelection(ids: string[]): void;
  /** True while only the plain selection tool is running (no draw / move / route session). */
  toolIdle: boolean;
  /** The boxes of every selectable item, by selection id (computed on first use). */
  itemBoxes(): ItemBoxes;
  /** `SELECTION::GetBoundingBox()` of the current selection. */
  selectionBox(): Box | null;
  /** The document's extent (`GetDocumentExtents`): what the editor shows when it has nothing else to frame. */
  contentBox(): Box | null;
  /** `canvas->GetDefaultViewBBox()` -- the page or board area to frame when there is nothing to frame. */
  defaultBox(): Box;
}

/** A 100 mm square around the origin: the footprint and symbol editors' default view box (they have no page). */
const LIBRARY_EDITOR_DEFAULT_BOX: Box = [-50_000, -50_000, 50_000, 50_000];

export function makeEditorAdapter(snap: EditorSnapshot): EditorAdapter | null {
  const { tab, studio, fp, sym } = snap;
  let cached: ItemBoxes | null = null;
  const lazy = (build: () => ItemBoxes) => () => (cached ??= build());

  switch (tab) {
    case "pcb": {
      const board = studio.board;
      const itemBoxes = lazy(() => (board ? pcbItemBoxes(board) : new Map()));
      return {
        tab,
        view: studio.view,
        setView: (view) => snap.dispatch({ type: "SET_VIEW", view }),
        cursor: studio.cursorUm,
        setCursor: (at) => snap.dispatch({ type: "SET_CURSOR", at }),
        gridUm: studio.gridUm,
        gridList: DEFAULT_PCB_GRIDS_UM,
        setGridUm: (um) => snap.dispatch({ type: "SET_GRID_UM", um }),
        selection: studio.selection,
        setSelection: (ids) => snap.dispatch({ type: "SET_SELECTION", refs: ids }),
        toolIdle: studio.activeTool === "select" && !studio.drawState,
        itemBoxes,
        selectionBox: () => boxOfIds(itemBoxes(), studio.selection),
        contentBox: () => (board ? pcbContentBox(board, itemBoxes()) : null),
        defaultBox: () => (board?.outline ? boxOfPoints(board.outline) : null) ?? LIBRARY_EDITOR_DEFAULT_BOX,
      };
    }
    case "schematic": {
      const sch = studio.schematic;
      const itemBoxes = lazy(() => (sch ? schematicItemBoxes(sch) : new Map()));
      return {
        tab,
        view: studio.schematicView,
        setView: (view) => snap.dispatch({ type: "SET_SCHEMATIC_VIEW", view }),
        cursor: studio.cursorUm,
        setCursor: (at) => snap.dispatch({ type: "SET_CURSOR", at }),
        gridUm: SCH_GRID_UM,
        gridList: null,
        setGridUm: () => {},
        selection: studio.selection,
        setSelection: (ids) => snap.dispatch({ type: "SET_SELECTION", refs: ids }),
        toolIdle: studio.activeTool === "select" && !studio.drawState,
        itemBoxes,
        selectionBox: () => boxOfIds(itemBoxes(), studio.selection),
        contentBox: () => unionBoxes(itemBoxes().values()),
        defaultBox: () => [0, 0, PAGE_WIDTH_UM, PAGE_HEIGHT_UM],
      };
    }
    case "footprint": {
      const footprint = fp.footprint;
      const itemBoxes = lazy(() => (footprint ? footprintItemBoxes(footprint) : new Map()));
      return {
        tab,
        view: fp.view,
        setView: (view) => snap.fpDispatch({ type: "SET_VIEW", view }),
        cursor: fp.cursorUm,
        setCursor: (at) => snap.fpDispatch({ type: "SET_CURSOR", at }),
        gridUm: fp.gridUm,
        gridList: DEFAULT_PCB_GRIDS_UM,
        setGridUm: (um) => snap.fpDispatch({ type: "SET_GRID_UM", um }),
        selection: fp.selection,
        setSelection: (ids) => snap.fpDispatch({ type: "SET_SELECTION", refs: ids }),
        toolIdle: fp.activeTool === "select" && !fp.drawState,
        itemBoxes,
        selectionBox: () => boxOfIds(itemBoxes(), fp.selection),
        contentBox: () => unionBoxes(itemBoxes().values()),
        defaultBox: () => LIBRARY_EDITOR_DEFAULT_BOX,
      };
    }
    case "symbol": {
      const symbol = sym.symbol;
      const itemBoxes = lazy(() => (symbol ? symbolItemBoxes(symbol, sym.activeUnit, sym.activeBodyStyle) : new Map()));
      return {
        tab,
        view: sym.view,
        setView: (view) => snap.symDispatch({ type: "SET_VIEW", view }),
        cursor: sym.cursorUm,
        setCursor: (at) => snap.symDispatch({ type: "SET_CURSOR", at }),
        gridUm: sym.gridUm,
        gridList: EESCHEMA_GRIDS_UM,
        setGridUm: (um) => snap.symDispatch({ type: "SET_GRID_UM", um }),
        selection: sym.selection,
        setSelection: (ids) => snap.symDispatch({ type: "SET_SELECTION", refs: ids }),
        toolIdle: sym.activeTool === "select" && !sym.drawState,
        itemBoxes,
        selectionBox: () => boxOfIds(itemBoxes(), sym.selection),
        contentBox: () => unionBoxes(itemBoxes().values()),
        defaultBox: () => LIBRARY_EDITOR_DEFAULT_BOX,
      };
    }
    default:
      return null;
  }
}

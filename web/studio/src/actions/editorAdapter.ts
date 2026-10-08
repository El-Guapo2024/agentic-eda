// One handle on "the editor that is on screen", for the common actions (src/actions/commonActions.ts).
//
// KiCad's shared tools -- COMMON_TOOLS, the selection/group/picker tools -- run in every editor frame and reach
// the frame through `EDA_DRAW_FRAME` (its view, its selection, its document extents). The studio's four canvases
// keep that state in three stores (the board and the schematic share one, the footprint editor and the symbol editor
// have their own), so an `EditorAdapter` is the small common face of whichever one the active tab is: the pan/zoom
// view, the cursor, the selection, the boxes of the items it can select, what is under a point and how to delete it.
// Nothing here is a new copy of state; every field reads or writes the store it belongs to.
import type { Dispatch } from "react";
import type { Action as StudioAction, EditorTab, StudioApi, StudioState, ViewTransform } from "../state/store";
import type { FootprintEditorApi, FootprintEditorState, FpAction } from "../state/footprintEditorStore";
import type { SymAction, SymbolEditorApi, SymbolEditorState } from "../state/symbolEditorStore";
import { boxOfIds, boxOfPoints, footprintItemBoxes, pcbContentBox, pcbItemBoxes, symbolItemBoxes, unionBoxes, type Box, type ItemBoxes } from "../kicad-port/itemBoxes";
import type { Schematic } from "../api/types";
import { PAGE_HEIGHT_UM, PAGE_WIDTH_UM } from "../components/schematic/drawingSheet";
import { GRID as SCH_GRID_UM } from "../components/schematic/layout";
import { DEFAULT_PCB_GRIDS_UM } from "../kicad-port/grid";
import { pickSelectionCandidates } from "../components/canvas/selectionCandidates";
import { hitSymbol, hitWire } from "../components/schematic/schHit";
import { allItems, hitItems, itemBounds } from "../components/schematic/schItems";
import { schSelectable } from "../kicad-port/schSelectionFilter";
import { deleteCmds as schDeleteCmds } from "../kicad-port/schDelete";
import { boardDeleteCmds } from "../kicad-port/deleteCmds";
import { pickFootprintItem, pickSymbolItem } from "../kicad-port/libEditorHit";

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
  /** The stores' own edit APIs; only what deletes or edits needs them (the overlays that just draw leave them out). */
  api?: StudioApi;
  fpApi?: FootprintEditorApi;
  symApi?: SymbolEditorApi;
}

/** One thing under a point: its selection id and what kind of item it is. */
export interface PickedItem {
  id: string;
  kind: string;
}

export interface EditorAdapter {
  tab: CanvasTab;
  view: ViewTransform;
  setView(view: ViewTransform): void;
  /** The board is drawn mirrored left to right (Flip Board View): a screen x is the canvas width minus the unflipped one. */
  flipped: boolean;
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
  /**
   * Everything a click at `(x, y)` would reach, best first (`SELECTION_TOOL::CollectHits` + `GuessSelectionCandidates`): the board
   * narrows what overlaps with its own heuristics, the other editors answer with the one item on top. Empty over empty space.
   */
  candidatesAt(x: number, y: number): PickedItem[];
  /**
   * The editor's own delete of `ids` (`DeleteItems`): one undo step for the board and the schematic, locked items skipped. Resolves to
   * a message when something was refused (a locked item), else null.
   */
  deleteIds(ids: string[]): Promise<string | null>;
  /** `EDA_ITEM::GetItemDescription`: the line the clarification menu lists an item by. */
  describe(item: PickedItem): string;
}

/** Every selectable item of the sheet with its bounds -- the catalog of components/schematic/schItems.ts. */
function schematicItemBoxes(sch: Schematic): ItemBoxes {
  const out: ItemBoxes = new Map();
  for (const ref of allItems(sch)) {
    const b = itemBounds(sch, ref);
    if (b) out.set(ref.id, [b.minX, b.minY, b.maxX, b.maxY]);
  }
  return out;
}

/** A 100 mm square around the origin: the footprint and symbol editors' default view box (they have no page). */
const LIBRARY_EDITOR_DEFAULT_BOX: Box = [-50_000, -50_000, 50_000, 50_000];

/** `GetItemDescription` of a board item, from the lists the board state holds. */
function describeBoardItem(board: StudioState["board"], item: PickedItem): string {
  if (!board) return item.kind;
  const part = board.parts.find((p) => p.ref === item.id);
  if (part) return `Footprint ${part.ref}${part.value ? ` ${part.value}` : ""}`;
  const track = board.routing?.tracks.find((t) => t.id === item.id);
  if (track) return `Track ${track.net} on ${track.layer}`;
  const via = board.routing?.vias.find((v) => v.id === item.id);
  if (via) return `Via ${via.net}`;
  const zone = board.routing?.zones.find((z) => z.id === item.id);
  if (zone) return `${zone.is_rule_area ? "Rule area" : "Zone"} ${zone.net} on ${zone.layer}`;
  const shape = board.drawings?.shapes.find((s) => s.id === item.id);
  if (shape) return `${shape.kind} on ${shape.layer}`;
  const text = board.drawings?.texts.find((t) => t.id === item.id);
  if (text) return `Text "${text.content}" on ${text.layer}`;
  const dim = board.drawings?.dimensions.find((d) => d.id === item.id);
  if (dim) return `${dim.kind} dimension on ${dim.layer}`;
  const group = board.drawings?.groups.find((g) => g.id === item.id);
  if (group) return `Group ${group.name || item.id}`;
  return item.kind;
}

/** `GetItemDescription` of a schematic item. */
function describeSchematicItem(sch: Schematic | null, item: PickedItem): string {
  if (!sch) return item.kind;
  const symbol = sch.symbols.find((s) => s.id === item.id);
  if (symbol) return `Symbol ${symbol.id}${symbol.value ? ` ${symbol.value}` : ""}`;
  const label = sch.labels.find((l) => l.id === item.id);
  if (label) return `${label.scope === "local" ? "Label" : label.scope === "global" ? "Global label" : "Hierarchical label"} ${label.net}`;
  const text = sch.texts.find((t) => t.id === item.id);
  if (text) return `Text "${text.content}"`;
  const power = sch.power_symbols.find((p) => p.id === item.id);
  if (power) return `Power symbol ${power.net}`;
  const wire = sch.wires.find((w) => w.id === item.id);
  if (wire) return `${wire.bus ? "Bus" : "Wire"} ${wire.net}`;
  return item.kind.replace(/_/g, " ");
}

/** `FilterCollectorForHierarchy`: an item of a group that has not been entered is picked as the group. */
function promoteToGroup(id: string, groups: readonly { id: string; member_ids: string[] }[], entered: string | null): string {
  const g = groups.find((x) => x.member_ids.includes(id));
  return g && g.id !== entered ? g.id : id;
}

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
        flipped: studio.bcx.boardFlipped,
        cursor: studio.cursorUm,
        setCursor: (at) => snap.dispatch({ type: "SET_CURSOR", at }),
        gridUm: studio.gridUm,
        gridList: DEFAULT_PCB_GRIDS_UM,
        setGridUm: (um) => snap.dispatch({ type: "SET_GRID_UM", um }),
        selection: studio.selection,
        setSelection: (ids) => snap.dispatch({ type: "SET_SELECTION", refs: ids }),
        toolIdle: studio.activeTool === "select" && !studio.drawState && !studio.armed && !studio.movePreview,
        describe: (item) => describeBoardItem(board, item),
        itemBoxes,
        selectionBox: () => boxOfIds(itemBoxes(), studio.selection),
        contentBox: () => (board ? pcbContentBox(board, itemBoxes()) : null),
        defaultBox: () => (board?.outline ? boxOfPoints(board.outline) : null) ?? LIBRARY_EDITOR_DEFAULT_BOX,
        candidatesAt: (x, y) => {
          if (!board) return [];
          const toleranceUm = Math.max(150, 6 / (studio.view.scale || 1));
          const cands = pickSelectionCandidates(board, x, y, toleranceUm, 1 / (studio.view.scale || 1), studio.selectionFilter, studio.layerVisible, studio.activeLayer, studio.highContrast, studio.selection, false, false);
          const groups = board.drawings?.groups ?? [];
          return cands.map((c) => ({ id: promoteToGroup(c.id, groups, studio.enteredGroupId), kind: c.kind }));
        },
        deleteIds: async (ids) => {
          if (!board || !snap.api) return null;
          // a click on a member of a group that has not been entered takes the whole group (`FilterCollectorForHierarchy`)
          const groups = board.drawings?.groups ?? [];
          const { cmds, skippedLocked } = boardDeleteCmds(
            board,
            ids.map((id) => promoteToGroup(id, groups, studio.enteredGroupId))
          );
          if (cmds.length > 0) await snap.api.cmdBatch(cmds);
          return skippedLocked > 0 && cmds.length === 0 ? "Item locked." : null;
        },
      };
    }
    case "schematic": {
      const sch = studio.schematic;
      const itemBoxes = lazy(() => (sch ? schematicItemBoxes(sch) : new Map()));
      return {
        tab,
        view: studio.schematicView,
        setView: (view) => snap.dispatch({ type: "SET_SCHEMATIC_VIEW", view }),
        flipped: false,
        cursor: studio.cursorUm,
        setCursor: (at) => snap.dispatch({ type: "SET_CURSOR", at }),
        gridUm: SCH_GRID_UM,
        gridList: null,
        setGridUm: () => {},
        selection: studio.selection,
        setSelection: (ids) => snap.dispatch({ type: "SET_SELECTION", refs: ids }),
        toolIdle: studio.activeTool === "select" && !studio.drawState && !studio.movePreview,
        describe: (item) => describeSchematicItem(sch, item),
        itemBoxes,
        selectionBox: () => boxOfIds(itemBoxes(), studio.selection),
        contentBox: () => unionBoxes(itemBoxes().values()),
        defaultBox: () => [0, 0, PAGE_WIDTH_UM, PAGE_HEIGHT_UM],
        candidatesAt: (x, y) => {
          if (!sch) return [];
          const scale = studio.schematicView.scale || 1;
          // The order of a click on the schematic (SchematicView.tsx): the symbol, any other placed item at screen tolerance, then a wire --
          // each only if the selection filter lets it be picked (`SCH_SELECTION_TOOL::itemPassesFilter`).
          const selectable = schSelectable(sch, studio.schSelectionFilter);
          const out: PickedItem[] = [];
          const add = (id: string | null | undefined, kind: string) => {
            if (id && selectable(id) && !out.some((o) => o.id === id)) out.push({ id, kind });
          };
          add(hitSymbol(sch, x, y), "symbol");
          for (const r of hitItems(sch, x, y, 6 / scale)) if (r.kind !== "symbol" && r.kind !== "wire") add(r.id, r.kind);
          add(hitWire(sch, x, y, 400 / scale), "wire");
          return out;
        },
        deleteIds: async (ids) => {
          if (!sch || !snap.api) return null;
          const locked = new Set(sch.locked ?? []);
          const cmds = schDeleteCmds(sch, ids, locked);
          if (cmds.length > 0) await snap.api.cmdBatch(cmds);
          return cmds.length === 0 && ids.some((id) => locked.has(id)) ? "Item locked." : null;
        },
      };
    }
    case "footprint": {
      const footprint = fp.footprint;
      const itemBoxes = lazy(() => (footprint ? footprintItemBoxes(footprint) : new Map()));
      return {
        tab,
        view: fp.view,
        setView: (view) => snap.fpDispatch({ type: "SET_VIEW", view }),
        flipped: false,
        cursor: fp.cursorUm,
        setCursor: (at) => snap.fpDispatch({ type: "SET_CURSOR", at }),
        gridUm: fp.gridUm,
        gridList: DEFAULT_PCB_GRIDS_UM,
        setGridUm: (um) => snap.fpDispatch({ type: "SET_GRID_UM", um }),
        selection: fp.selection,
        setSelection: (ids) => snap.fpDispatch({ type: "SET_SELECTION", refs: ids }),
        toolIdle: fp.activeTool === "select" && !fp.drawState && !fp.movePreview,
        describe: (item) => {
          const pad = footprint?.pads.find((p) => p.id === item.id);
          if (pad) return `Pad ${pad.number}`;
          const text = footprint?.texts.find((t) => t.id === item.id);
          if (text) return `Text "${text.content}" on ${text.layer}`;
          const g = footprint?.graphics.find((x) => x.id === item.id);
          return g ? `${g.kind} on ${g.layer}` : item.kind;
        },
        itemBoxes,
        selectionBox: () => boxOfIds(itemBoxes(), fp.selection),
        contentBox: () => unionBoxes(itemBoxes().values()),
        defaultBox: () => LIBRARY_EDITOR_DEFAULT_BOX,
        candidatesAt: (x, y) => {
          const hit = footprint ? pickFootprintItem(footprint, x, y, fp.view.scale) : null;
          return hit ? [hit] : [];
        },
        deleteIds: async (ids) => {
          const api = snap.fpApi;
          if (!footprint || !api) return null;
          for (const id of ids) {
            if (footprint.pads.some((p) => p.id === id)) await api.deletePad(id);
            else if (footprint.graphics.some((g) => g.id === id)) await api.deleteGraphic(id);
            else if (footprint.texts.some((t) => t.id === id)) await api.deleteText(id);
          }
          return null;
        },
      };
    }
    case "symbol": {
      const symbol = sym.symbol;
      const itemBoxes = lazy(() => (symbol ? symbolItemBoxes(symbol, sym.activeUnit, sym.activeBodyStyle) : new Map()));
      return {
        tab,
        view: sym.view,
        setView: (view) => snap.symDispatch({ type: "SET_VIEW", view }),
        flipped: false,
        cursor: sym.cursorUm,
        setCursor: (at) => snap.symDispatch({ type: "SET_CURSOR", at }),
        gridUm: sym.gridUm,
        gridList: EESCHEMA_GRIDS_UM,
        setGridUm: (um) => snap.symDispatch({ type: "SET_GRID_UM", um }),
        selection: sym.selection,
        setSelection: (ids) => snap.symDispatch({ type: "SET_SELECTION", refs: ids }),
        toolIdle: sym.activeTool === "select" && !sym.drawState && !sym.movePreview,
        describe: (item) => {
          const pin = symbol?.pins.find((p) => p.id === item.id);
          if (pin) return `Pin ${pin.number}${pin.name && pin.name !== "~" ? ` ${pin.name}` : ""}`;
          const g = symbol?.graphics.find((x) => x.id === item.id);
          return g ? g.kind : item.kind;
        },
        itemBoxes,
        selectionBox: () => boxOfIds(itemBoxes(), sym.selection),
        contentBox: () => unionBoxes(itemBoxes().values()),
        defaultBox: () => LIBRARY_EDITOR_DEFAULT_BOX,
        candidatesAt: (x, y) => {
          const hit = symbol ? pickSymbolItem(symbol, x, y, sym.view.scale, sym.activeUnit, sym.activeBodyStyle, sym.showHiddenPins) : null;
          return hit ? [hit] : [];
        },
        deleteIds: async (ids) => {
          const api = snap.symApi;
          if (!symbol || !api) return null;
          for (const id of ids) {
            if (symbol.pins.some((p) => p.id === id)) await api.deletePin(id);
            else if (symbol.graphics.some((g) => g.id === id)) await api.deleteGraphic(id);
          }
          return null;
        },
      };
    }
    default:
      return null;
  }
}

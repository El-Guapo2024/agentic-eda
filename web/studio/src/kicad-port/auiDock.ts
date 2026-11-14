// Where the editors' movable panes are docked -- the board editor's Net Inspector and Search, the schematic editor's Search -- as wxAUI places a pane: a port
// of the `wxAuiPaneInfo` each frame gives them (pcbnew/pcb_edit_frame.cpp and eeschema/sch_edit_frame.cpp at 8303b2ad, the constructors' `AddPane`s) and of what
// wxAUI 3.2.4 lets a person do with a pane that is neither a toolbar nor fixed (src/aui/framemanager.cpp: `DoDrop`, `LayoutAll`, `OnMotion`'s
// `actionClickCaption` branch, `OnFloatingPaneMoving` / `OnFloatingPaneMoved`, `OnLeftDClick`, `DoInsertDockRow`, `DoInsertPane`, `GetMaxLayer`, `GetMaxRow`):
//
//   - A docked pane is at a direction, a layer, a row and a position (`dock_direction`, `dock_layer`, `dock_row`, `dock_pos`). Layer 0 is right round the
//     canvas, inside the vertical toolbars; a higher layer goes round everything inside it. The panes of one direction, layer and row share a dock -- side by
//     side in a top or bottom one, one over the other in a left or right one -- in the order of their positions, each its share of the dock (`dock_proportion`,
//     a sash between two). A top and a left dock's rows go inwards from the frame's edge (row 0 the outermost), so do a bottom and a right dock's (`LayoutAll`
//     adds a right and a bottom dock's rows last first).
//   - Both board panes are added `.Bottom()` (layer 0, row 0) in the order `m_netInspectorPanel`, `m_searchPane`; the schematic's Search `.Bottom()` too. None
//     is given `TopDockable( false )` and the like, `Floatable( false )` or `DockFixed()`.
//   - A caption dragged past the drag threshold floats the pane at once (`paneInfo->Float()`; the pointer keeps its offset into the window, 30 px when the
//     window is narrower than that) and the window follows the pointer. Each move asks `DoDrop` where it would dock and draws the hint there; the release docks
//     it there, or leaves it floating. `DoDrop`:
//       1. near an edge of the frame (from 5 px inside to 35 px outside: `auiLayerInsertOffset`, `auiLayerInsertPixels`) the pane docks along that edge on a
//          layer of its own, one higher than the highest of that side and the two beside it (`GetMaxLayer`, fixed docks not counted), row 0;
//       2. over a docked pane, near its outer edge (`auiInsertRowPixels`, 10 px) a new row is made there, the pane's row and the ones inside it moved inwards
//          (`DoInsertDockRow`); elsewhere on it the pane goes before it (its first half) or after it (`DoInsertPane`), in its row;
//       3. over a toolbar's dock the pane goes "along the edge under the toolbar, but over all other panes": the highest layer of that side and the two beside
//          it, row 0, the rows there moved inwards;
//       4. over the canvas, near one of its edges (`auiNewRowPixels`, 40 px, never more than a fifth of the canvas) a new row of layer 0 on that side, inside
//          the others (`GetMaxRow( .. ) + 1`);
//       5. anywhere else it stays floating.
//   - A new dock is as deep as its panes' `BestSize` (plus the caption for a top or bottom one), a third of the frame at most (`m_dockConstraintX` / `Y`); a pane
//     joining a dock takes its depth; the sash then sizes it. A double click on a caption floats a docked pane and docks a floating one back where it was
//     (`OnLeftDClick`: `Float()` / `Dock()`, which keep the direction, layer, row and position).
//   - Where they are is kept with the frame's layout (`RestoreAuiLayout`, the saved perspective); KiCad's Search also keeps `aui.search_panel_dock_direction`.
//
// **Differs**, as the studio's frame is not wxAUI's: its own columns and bars (the toolbars, Properties, Appearance, the Hierarchy and Design Blocks columns,
// the message panel) stay where they are, so (a) a layer above 0 is a ring round the whole editor inside the main toolbar and the message panel -- KiCad's
// `GetMaxLayer` counts the Properties and Appearance panes' layers 4 and 5, which puts an edge drop at layer 5 or 6, outside those columns too; here the layers
// are counted among the movable panes alone, from 1 -- and (b) a drop on Properties, Appearance, a Design Blocks or Hierarchy column or the message panel (KiCad:
// into that pane's dock) leaves the pane floating.
//
// Pure: no store, no DOM. Unit-tested in auiDock.test.ts.

/** The editors with movable panes, each with its own layout (KiCad keeps one per frame). */
export type DockEditor = "pcb" | "sch";

/** The movable panes. */
export type MovablePaneId = "netInspector" | "search" | "schSearch";

/** Each editor's movable panes in the order its frame adds them: the order of panes of one position. */
export const EDITOR_PANES: Record<DockEditor, readonly MovablePaneId[]> = { pcb: ["netInspector", "search"], sch: ["schSearch"] };

/** The board editor's two (`PCB_EDIT_FRAME`'s order). */
export const PCB_DOCKING_PANES: readonly MovablePaneId[] = EDITOR_PANES.pcb;

/** `wxAUI_DOCK_TOP`, `_BOTTOM`, `_LEFT`, `_RIGHT`. */
export type PaneSide = "top" | "bottom" | "left" | "right";

/** A side of the canvas, or floating (`wxAuiPaneInfo::IsFloating`). */
export type PaneDockAt = PaneSide | "float";

export const PANE_SIDES: readonly PaneSide[] = ["top", "bottom", "left", "right"];

/** A floating pane's window, in CSS px of the page. */
export interface PaneRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Where on its side a pane docks (`dock_layer`, `dock_row`, `dock_pos`): kept while it floats, where `Dock()` puts it back. */
export interface PaneSpot {
  layer: number;
  row: number;
  pos: number;
}

/** One editor's movable panes. */
export interface PaneDock {
  /** Where each pane is. */
  at: Partial<Record<MovablePaneId, PaneDockAt>>;
  /** The side each pane was last docked on (`dock_direction`, which `Float()` keeps). */
  lastDocked: Partial<Record<MovablePaneId, PaneSide>>;
  /** Each pane's layer, row and position on that side. */
  spot: Partial<Record<MovablePaneId, PaneSpot>>;
  /** The depth of the dock each pane is in: the height of a top or bottom one, the width of a left or right one (`wxAuiDockInfo::size`). */
  depth: Partial<Record<MovablePaneId, number>>;
  /** Each pane's share of its dock (`dock_proportion`, 100000 to begin with). */
  proportion: Partial<Record<MovablePaneId, number>>;
  /** Each pane's floating window. */
  floating: Partial<Record<MovablePaneId, PaneRect>>;
}

/** `BestSize`: `FromDIP( wxSize( 300, 200 ) )` the Net Inspector, `FromDIP( wxSize( 180, 100 ) )` both Search panes. */
export const PANE_BEST_SIZE: Record<MovablePaneId, { w: number; h: number }> = { netInspector: { w: 300, h: 200 }, search: { w: 180, h: 100 }, schSearch: { w: 180, h: 100 } };

/** `MinSize`: `FromDIP( wxSize( 240, 60 ) )` the Net Inspector, `FromDIP( wxSize( 180, 60 ) )` both Search panes. */
export const PANE_MIN_SIZE: Record<MovablePaneId, { w: number; h: number }> = { netInspector: { w: 240, h: 60 }, search: { w: 180, h: 60 }, schSearch: { w: 180, h: 60 } };

/** `FloatingSize`: `wxSize( 300, 200 )` the Net Inspector, `FromDIP( wxSize( 480, 200 ) )` both Search panes. */
export const PANE_FLOATING_SIZE: Record<MovablePaneId, { w: number; h: number }> = { netInspector: { w: 300, h: 200 }, search: { w: 480, h: 200 }, schSearch: { w: 480, h: 200 } };

/** The caption bar's height, which a top or bottom dock's depth includes. */
export const CAPTION_PX = 22;

/** `wxAuiPaneInfo::dock_proportion`'s start. */
export const DEFAULT_PROPORTION = 100000;

/** `m_dockConstraintX` / `m_dockConstraintY`: a new dock is a third of the frame at most. */
export const DOCK_CONSTRAINT = 1 / 3;

/** `auiLayerInsertOffset`, `auiLayerInsertPixels`, `auiInsertRowPixels`, `auiNewRowPixels`. */
export const LAYER_INSERT_OFFSET = 5;
export const LAYER_INSERT_PIXELS = 40;
export const INSERT_ROW_PIXELS = 10;
export const NEW_ROW_PIXELS = 40;

/** How far a caption must be dragged before the pane floats (`wxSYS_DRAG_X` / `wxSYS_DRAG_Y`: a few pixels). */
export const DRAG_THRESHOLD_PX = 4;

/** The pointer's offset into a window taken out of its dock when the caption was grabbed further in than the window is wide (`FromDIP( 30 )`). */
export const FLOAT_GRAB_FALLBACK_PX = 30;

/**
 * Where everything starts: every pane `.Bottom()` at layer 0, row 0, in the frame's order, the dock as tall as its panes' `BestSize` height plus the caption
 * (`LayoutAll`'s new dock: 222 px for the board's two, which the studio had as 220 px before, and keeps; 122 px for the schematic's Search), or `bottomHeight`,
 * a height kept from before; a floating window at its `FloatingSize`.
 */
export function defaultPaneDock(editor: DockEditor, bottomHeight = editor === "pcb" ? 220 : PANE_BEST_SIZE.schSearch.h + CAPTION_PX): PaneDock {
  const d: PaneDock = { at: {}, lastDocked: {}, spot: {}, depth: {}, proportion: {}, floating: {} };
  EDITOR_PANES[editor].forEach((id, i) => {
    d.at[id] = "bottom";
    d.lastDocked[id] = "bottom";
    d.spot[id] = { layer: 0, row: 0, pos: i };
    d.depth[id] = bottomHeight;
    d.proportion[id] = DEFAULT_PROPORTION;
    d.floating[id] = { x: 120 + 40 * i, y: 160 + 40 * i, ...PANE_FLOATING_SIZE[id] };
  });
  return d;
}

const isSide = (v: unknown): v is PaneSide => v === "top" || v === "bottom" || v === "left" || v === "right";
const isAt = (v: unknown): v is PaneDockAt => isSide(v) || v === "float";
const num = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const rec = (v: unknown): Record<string, unknown> => (v && typeof v === "object" ? (v as Record<string, unknown>) : {});

/**
 * Read a kept layout back: anything missing or of the wrong type keeps its default (a layout of an older build must not break the frame). A layout of before
 * the rows (`size` by side, no `spot`) puts each pane at layer 0, row 0 of its side, the side's size its depth.
 */
export function parsePaneDock(editor: DockEditor, raw: unknown, bottomHeight?: number): PaneDock {
  const d = defaultPaneDock(editor, bottomHeight);
  if (!raw || typeof raw !== "object") return d;
  const o = rec(raw);
  const at = rec(o.at);
  const last = rec(o.lastDocked);
  const spot = rec(o.spot);
  const depth = rec(o.depth);
  const prop = rec(o.proportion);
  const fl = rec(o.floating);
  const oldSize = rec(o.size);
  const out: PaneDock = { at: { ...d.at }, lastDocked: { ...d.lastDocked }, spot: { ...d.spot }, depth: { ...d.depth }, proportion: { ...d.proportion }, floating: { ...d.floating } };
  for (const id of EDITOR_PANES[editor]) {
    if (isAt(at[id])) out.at[id] = at[id] as PaneDockAt;
    if (isSide(last[id])) out.lastDocked[id] = last[id] as PaneSide;
    const s = rec(spot[id]);
    if (num(s.layer) && num(s.row) && num(s.pos) && s.layer >= 0 && s.row >= 0) out.spot[id] = { layer: Math.round(s.layer), row: Math.round(s.row), pos: s.pos };
    const side = out.at[id] === "float" ? out.lastDocked[id]! : (out.at[id] as PaneSide);
    if (num(depth[id]) && (depth[id] as number) > 0) out.depth[id] = depth[id] as number;
    else if (num(oldSize[side]) && (oldSize[side] as number) > 0) out.depth[id] = oldSize[side] as number;
    if (num(prop[id]) && (prop[id] as number) > 0) out.proportion[id] = prop[id] as number;
    const r = rec(fl[id]);
    if (num(r.x) && num(r.y) && num(r.w) && num(r.h)) out.floating[id] = { x: r.x, y: r.y, w: r.w, h: r.h };
  }
  return out;
}

/** One dock: the panes of one side, layer and row (`wxAuiDockInfo`), in the order of their positions. */
export interface DockRow {
  side: PaneSide;
  layer: number;
  row: number;
  panes: MovablePaneId[];
  /** The dock's depth (`wxAuiDockInfo::size`): its panes' deepest. */
  depth: number;
}

/** The docks `LayoutAll` builds from the docked panes that are `shown`: each side's, by layer, then row (row 0 the outermost). */
export function dockRows(editor: DockEditor, dock: PaneDock, shown: (id: MovablePaneId) => boolean): DockRow[] {
  const panes = EDITOR_PANES[editor];
  const docked = panes.filter((id) => dock.at[id] !== "float" && dock.at[id] !== undefined && shown(id));
  const key = (id: MovablePaneId) => `${dock.at[id]}|${dock.spot[id]!.layer}|${dock.spot[id]!.row}`;
  const groups = new Map<string, MovablePaneId[]>();
  for (const id of docked) groups.set(key(id), [...(groups.get(key(id)) ?? []), id]);
  const out: DockRow[] = [];
  for (const ids of groups.values()) {
    const first = ids[0]!;
    // `PaneSortFunc`: by position; panes of one position in the frame's order
    ids.sort((a, b) => dock.spot[a]!.pos - dock.spot[b]!.pos || panes.indexOf(a) - panes.indexOf(b));
    out.push({ side: dock.at[first] as PaneSide, layer: dock.spot[first]!.layer, row: dock.spot[first]!.row, panes: ids, depth: Math.max(...ids.map((id) => dock.depth[id] ?? 0)) });
  }
  out.sort((a, b) => PANE_SIDES.indexOf(a.side) - PANE_SIDES.indexOf(b.side) || a.layer - b.layer || a.row - b.row);
  return out;
}

/** The docks of one side and layer, in the order they are laid out on the page: a top or left side's from the frame's edge in, a bottom or right side's from the canvas out (top to bottom, left to right). */
export function sideRows(rows: readonly DockRow[], side: PaneSide, layer: number): DockRow[] {
  const mine = rows.filter((r) => r.side === side && r.layer === layer).sort((a, b) => a.row - b.row);
  return side === "bottom" || side === "right" ? mine.reverse() : mine;
}

/** The layers above 0 that hold a dock: the rings round the editor, inner first. */
export function outerLayers(rows: readonly DockRow[]): number[] {
  return [...new Set(rows.filter((r) => r.layer > 0).map((r) => r.layer))].sort((a, b) => a - b);
}

/** The floating panes of those `shown`. */
export function floatingPanes(editor: DockEditor, dock: PaneDock, shown: (id: MovablePaneId) => boolean): MovablePaneId[] {
  return EDITOR_PANES[editor].filter((id) => dock.at[id] === "float" && shown(id));
}

/** A top or bottom dock holds its panes in a row; a left or right one in a column. */
export function sideIsHorizontal(side: PaneSide): boolean {
  return side === "top" || side === "bottom";
}

/** `GetMaxLayer`: the highest layer of the docks on `side` (0 for none). */
function maxLayer(rows: readonly DockRow[], side: PaneSide): number {
  return Math.max(0, ...rows.filter((r) => r.side === side).map((r) => r.layer));
}

/** `GetMaxRow`: the highest row of the panes of `side` at `layer` (0 for none) -- every pane's, a hidden or floating one's too (`dock_direction` stays while it floats). */
function maxRow(editor: DockEditor, dock: PaneDock, side: PaneSide, layer: number): number {
  const dir = (id: MovablePaneId) => (dock.at[id] === "float" ? dock.lastDocked[id] : dock.at[id]);
  return Math.max(0, ...EDITOR_PANES[editor].filter((id) => dir(id) === side && dock.spot[id]!.layer === layer).map((id) => dock.spot[id]!.row));
}

/** The two sides beside `side` that `DoDrop` counts with it: a left or right dock's top and bottom, a top or bottom dock's left and right. */
function besideSides(side: PaneSide): PaneSide[] {
  return sideIsHorizontal(side) ? ["left", "right"] : ["top", "bottom"];
}

/** `DoInsertDockRow`: every docked pane of `side` and `layer` at row `row` or further in moves one row in. */
function insertDockRow(editor: DockEditor, dock: PaneDock, side: PaneSide, layer: number, row: number): PaneDock {
  const spot = { ...dock.spot };
  for (const id of EDITOR_PANES[editor]) {
    const s = spot[id]!;
    if (dock.at[id] === side && s.layer === layer && s.row >= row) spot[id] = { ...s, row: s.row + 1 };
  }
  return { ...dock, spot };
}

/** `DoInsertPane`: every docked pane of that dock at position `pos` or after moves one place on. */
function insertPane(editor: DockEditor, dock: PaneDock, side: PaneSide, layer: number, row: number, pos: number): PaneDock {
  const spot = { ...dock.spot };
  for (const id of EDITOR_PANES[editor]) {
    const s = spot[id]!;
    if (dock.at[id] === side && s.layer === layer && s.row === row && s.pos >= pos) spot[id] = { ...s, pos: s.pos + 1 };
  }
  return { ...dock, spot };
}

/** A rectangle of the page. */
export interface PageRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** A docked pane on the page, for `doDrop`'s hit test. */
export interface DockedPaneRect {
  id: MovablePaneId;
  rect: PageRect;
}

/** What `doDrop` sees of the page (`HitTest`'s parts): the frame, the canvas, the docked panes (not the one dragged), the sashes of the docks, the toolbars. */
export interface DropGeometry {
  /** The frame's client area: the editor from the top of its main toolbar to the bottom of its message panel. */
  frame: PageRect;
  canvas: PageRect | null;
  panes: readonly DockedPaneRect[];
  /** Each dock's sash (`typeDockSizer`), with the panes of its dock. */
  sashes: readonly { rect: PageRect; panes: readonly MovablePaneId[] }[];
  /** The toolbars' docks, with the side each is docked on. */
  toolbars: readonly { side: PaneSide; rect: PageRect }[];
}

const inside = (r: PageRect, x: number, y: number) => x >= r.left && x < r.left + r.width && y >= r.top && y < r.top + r.height;

/** Where a drop put the pane, and what was under the pointer: for the hint. */
export interface DropResult {
  dock: PaneDock;
  side: PaneSide;
  /** "edge" a layer of its own at the frame's edge, "row" a new row by a pane, "pane" before or after a pane in its row, "toolbar" by a toolbar, "canvas" a new row by the canvas. */
  how: "edge" | "row" | "pane" | "toolbar" | "canvas";
  /** The pane the drop was by ("row", "pane"), and whether it went after it. */
  by?: MovablePaneId;
  after?: boolean;
}

/**
 * `wxAuiManager::DoDrop` for pane `id` (floating, being dragged) with the pointer at (`x`, `y`): the layout with the pane docked where a release there docks it,
 * or null where it stays floating. `rows` are the docks on screen (`dockRows` without the dragged pane).
 */
export function doDrop(editor: DockEditor, dock: PaneDock, id: MovablePaneId, g: DropGeometry, x: number, y: number, shown: (id: MovablePaneId) => boolean): DropResult | null {
  const rows = dockRows(editor, dock, (p) => p !== id && shown(p));
  const place = (d: PaneDock, side: PaneSide, spot: PaneSpot, depth: number): PaneDock => {
    const out: PaneDock = { ...d, at: { ...d.at, [id]: side }, lastDocked: { ...d.lastDocked, [id]: side }, spot: { ...d.spot, [id]: spot }, depth: { ...d.depth, [id]: depth } };
    return normalizePositions(editor, out);
  };
  const fresh = (side: PaneSide) => newDockDepth(id, side, g.frame);
  const f = g.frame;
  const right = f.left + f.width;
  const bottom = f.top + f.height;
  const edgeLayer = (side: PaneSide) => Math.max(maxLayer(rows, side), ...besideSides(side).map((s) => maxLayer(rows, s))) + 1;

  // 1. along an edge of the frame: a layer of its own
  let edge: PaneSide | null = null;
  if (x < f.left + LAYER_INSERT_OFFSET && x > f.left + LAYER_INSERT_OFFSET - LAYER_INSERT_PIXELS && y > f.top && y < bottom) edge = "left";
  else if (y < f.top + LAYER_INSERT_OFFSET && y > f.top + LAYER_INSERT_OFFSET - LAYER_INSERT_PIXELS && x > f.left && x < right) edge = "top";
  else if (x >= right - LAYER_INSERT_OFFSET && x < right - LAYER_INSERT_OFFSET + LAYER_INSERT_PIXELS && y > f.top && y < bottom) edge = "right";
  else if (y >= bottom - LAYER_INSERT_OFFSET && y < bottom - LAYER_INSERT_OFFSET + LAYER_INSERT_PIXELS && x > f.left && x < right) edge = "bottom";
  if (edge) return { dock: place(dock, edge, { layer: edgeLayer(edge), row: 0, pos: 0 }, fresh(edge)), side: edge, how: "edge" };

  // 2. a docked pane, or the sash of a dock of one pane
  let hit = g.panes.find((p) => p.id !== id && inside(p.rect, x, y));
  if (!hit) {
    const sash = g.sashes.find((s) => inside(s.rect, x, y));
    if (sash) {
      const others = sash.panes.filter((p) => p !== id);
      if (others.length !== 1) return null;
      hit = g.panes.find((p) => p.id === others[0]);
      if (!hit) return null;
    }
  }
  if (hit) {
    const side = dock.at[hit.id] as PaneSide;
    const s = dock.spot[hit.id]!;
    const r = hit.rect;
    const rowDepth = rows.find((d) => d.panes.includes(hit!.id))?.depth ?? fresh(side);
    const nearOuter =
      side === "top"
        ? y >= r.top && y < r.top + INSERT_ROW_PIXELS
        : side === "bottom"
          ? y > r.top + r.height - INSERT_ROW_PIXELS && y <= r.top + r.height
          : side === "left"
            ? x >= r.left && x < r.left + INSERT_ROW_PIXELS
            : x > r.left + r.width - INSERT_ROW_PIXELS && x <= r.left + r.width;
    if (nearOuter) {
      const d = insertDockRow(editor, dock, side, s.layer, s.row);
      return { dock: place(d, side, { layer: s.layer, row: s.row, pos: 0 }, fresh(side)), side, how: "row", by: hit.id };
    }
    // the dock's own direction: a top or bottom dock's panes are side by side
    const offset = sideIsHorizontal(side) ? x - r.left : y - r.top;
    const size = sideIsHorizontal(side) ? r.width : r.height;
    const after = offset > size / 2;
    const pos = after ? s.pos + 1 : s.pos;
    const d = insertPane(editor, dock, side, s.layer, s.row, pos);
    return { dock: place(d, side, { layer: s.layer, row: s.row, pos }, rowDepth), side, how: "pane", by: hit.id, after };
  }

  // 3. a toolbar's dock: along the edge under the toolbar, over the other panes
  const bar = g.toolbars.find((t) => inside(t.rect, x, y));
  if (bar) {
    const layer = Math.max(1, maxLayer(rows, bar.side), ...besideSides(bar.side).map((s) => maxLayer(rows, s)));
    const d = insertDockRow(editor, dock, bar.side, layer, 0);
    return { dock: place(d, bar.side, { layer, row: 0, pos: 0 }, fresh(bar.side)), side: bar.side, how: "toolbar" };
  }

  // 4. the canvas: near an edge, a new row of layer 0 inside the others on that side
  const c = g.canvas;
  if (c && inside(c, x, y)) {
    const nx = Math.min(NEW_ROW_PIXELS, Math.trunc((c.width * 20) / 100));
    const ny = Math.min(NEW_ROW_PIXELS, Math.trunc((c.height * 20) / 100));
    let side: PaneSide | null = null;
    if (x >= c.left && x < c.left + nx) side = "left";
    else if (y >= c.top && y < c.top + ny) side = "top";
    else if (x >= c.left + c.width - nx && x < c.left + c.width) side = "right";
    else if (y >= c.top + c.height - ny && y < c.top + c.height) side = "bottom";
    if (!side) return null;
    const row = maxRow(editor, dock, side, 0) + 1;
    return { dock: place(insertDockRow(editor, dock, side, 0, row), side, { layer: 0, row, pos: 0 }, fresh(side)), side, how: "canvas" };
  }
  return null;
}

/** The depth a new dock of pane `id` on `side` starts at: its `BestSize` (with the caption for a top or bottom dock), a third of the frame at most, 10 px at least. */
export function newDockDepth(id: MovablePaneId, side: PaneSide, frame: { width: number; height: number }): number {
  const best = PANE_BEST_SIZE[id];
  const size = sideIsHorizontal(side) ? best.h + CAPTION_PX : best.w;
  const max = Math.trunc(DOCK_CONSTRAINT * (sideIsHorizontal(side) ? frame.height : frame.width));
  return Math.max(10, max > 0 ? Math.min(size, max) : size);
}

/** `LayoutAll`'s "reassign the dock_pos to the sequential 0, 1, 2, 3": each dock's positions without gaps. */
function normalizePositions(editor: DockEditor, dock: PaneDock): PaneDock {
  const spot = { ...dock.spot };
  const panes = EDITOR_PANES[editor];
  const groups = new Map<string, MovablePaneId[]>();
  for (const id of panes) {
    const at = dock.at[id];
    if (!at || at === "float") continue;
    const k = `${at}|${spot[id]!.layer}|${spot[id]!.row}`;
    groups.set(k, [...(groups.get(k) ?? []), id]);
  }
  for (const ids of groups.values()) {
    ids.sort((a, b) => spot[a]!.pos - spot[b]!.pos || panes.indexOf(a) - panes.indexOf(b));
    ids.forEach((id, i) => (spot[id] = { ...spot[id]!, pos: i }));
  }
  return { ...dock, spot };
}

/**
 * Float pane `id` with its window at `from` (its place on the page when it is floated by a double click or a drag: wxAUI opens the floating frame where the
 * pane was); it keeps its side, layer, row and position for `Dock()`.
 */
export function floatPane(dock: PaneDock, id: MovablePaneId, from?: { x: number; y: number }): PaneDock {
  const next: PaneDock = { ...dock, at: { ...dock.at, [id]: "float" } };
  if (from) next.floating = { ...dock.floating, [id]: { ...dock.floating[id]!, x: Math.round(from.x), y: Math.round(from.y) } };
  return next;
}

/** `Dock()`: a floating pane back on the side it was docked on last, at its layer, row and position. */
export function dockBack(editor: DockEditor, dock: PaneDock, id: MovablePaneId): PaneDock {
  return normalizePositions(editor, { ...dock, at: { ...dock.at, [id]: dock.lastDocked[id] ?? "bottom" } });
}

/** `wxAuiManager::OnLeftDClick`: a docked pane floats, a floating one docks back where it was. */
export function toggledPaneDock(dock: PaneDock, id: MovablePaneId): PaneDockAt {
  return dock.at[id] === "float" ? (dock.lastDocked[id] ?? "bottom") : "float";
}

/**
 * The caption's buttons (the studio's, as wxAUI's caption has the close button alone): pane `id` docked on `side` -- into the innermost dock of layer 0 there
 * when one is shown, after its panes (so two panes sent to one side share its dock, as both start), else a dock of its own.
 */
export function dockOnSide(editor: DockEditor, dock: PaneDock, id: MovablePaneId, side: PaneSide, shown: (id: MovablePaneId) => boolean, frame?: { width: number; height: number }): PaneDock {
  const rows = dockRows(editor, dock, (p) => p !== id && shown(p)).filter((r) => r.side === side && r.layer === 0);
  const inner = rows.sort((a, b) => b.row - a.row)[0];
  const spot: PaneSpot = inner ? { layer: 0, row: inner.row, pos: Math.max(...inner.panes.map((p) => dock.spot[p]!.pos)) + 1 } : { layer: 0, row: 0, pos: 0 };
  const depth = inner ? inner.depth : (frame ? newDockDepth(id, side, frame) : (dock.depth[id] ?? PANE_BEST_SIZE[id].h + CAPTION_PX));
  return normalizePositions(editor, { ...dock, at: { ...dock.at, [id]: side }, lastDocked: { ...dock.lastDocked, [id]: side }, spot: { ...dock.spot, [id]: spot }, depth: { ...dock.depth, [id]: depth } });
}

/** The deepest a dock may be in a window `windowW` x `windowH`: leave the canvas at least 120 px. */
export function maxDockSize(side: PaneSide, windowW: number, windowH: number): number {
  return Math.max(80, (sideIsHorizontal(side) ? windowH - 200 : windowW - 360) | 0);
}

/** A dock's depth after its sash is dragged: no shallower than its panes' `MinSize` (plus the caption for a top or bottom dock), no deeper than `maxDockSize`. */
export function clampDockSize(side: PaneSide, size: number, panes: readonly MovablePaneId[], windowW: number, windowH: number): number {
  const horizontal = sideIsHorizontal(side);
  const min = Math.max(horizontal ? 60 + CAPTION_PX : 120, ...panes.map((id) => (horizontal ? PANE_MIN_SIZE[id].h + CAPTION_PX : PANE_MIN_SIZE[id].w)));
  return Math.round(Math.max(min, Math.min(maxDockSize(side, windowW, windowH), size)));
}

/** A dock's sash dragged: every pane of the dock takes the new depth. */
export function setRowDepth(dock: PaneDock, panes: readonly MovablePaneId[], depth: number): PaneDock {
  const out = { ...dock.depth };
  for (const id of panes) out[id] = depth;
  return { ...dock, depth: out };
}

/**
 * The sash between two panes of one dock dragged (`typePaneSizer`, `OnLeftUp_Resize`'s proportions): `a` and `b` share what they had in the ratio of their new
 * sizes in px, each kept to its `MinSize` along the dock.
 */
export function resizeBetween(dock: PaneDock, side: PaneSide, a: MovablePaneId, b: MovablePaneId, sizeA: number, sizeB: number, delta: number): PaneDock {
  const along = (id: MovablePaneId) => (sideIsHorizontal(side) ? PANE_MIN_SIZE[id].w : PANE_MIN_SIZE[id].h + CAPTION_PX);
  const total = sizeA + sizeB;
  const na = Math.max(along(a), Math.min(total - along(b), sizeA + delta));
  const nb = total - na;
  if (total <= 0 || nb <= 0) return dock;
  const sum = (dock.proportion[a] ?? DEFAULT_PROPORTION) + (dock.proportion[b] ?? DEFAULT_PROPORTION);
  const pa = Math.max(1, Math.round((sum * na) / total));
  return { ...dock, proportion: { ...dock.proportion, [a]: pa, [b]: Math.max(1, sum - pa) } };
}

/** A floating window kept on the page: no smaller than the pane's `MinSize`, no larger than the window, its caption reachable. */
export function clampPaneRect(id: MovablePaneId, r: PaneRect, windowW: number, windowH: number): PaneRect {
  const min = PANE_MIN_SIZE[id];
  const w = Math.max(min.w, Math.min(r.w, windowW));
  const h = Math.max(min.h + CAPTION_PX, Math.min(r.h, windowH));
  const x = Math.max(0, Math.min(r.x, windowW - w));
  const y = Math.max(0, Math.min(r.y, windowH - h));
  return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) };
}

/** The caption's buttons for a pane at `at`: the other sides and floating, then Close (`CloseButton( true )`), each with what it says. */
export function paneDockChoices(at: PaneDockAt, title: string): { to: PaneDockAt | "close"; label: string }[] {
  const label: Record<PaneDockAt, string> = { top: "Dock at the top", bottom: "Dock at the bottom", left: "Dock on the left", right: "Dock on the right", float: "Float" };
  const order: PaneDockAt[] = ["bottom", "top", "left", "right", "float"];
  return [...order.filter((to) => to !== at).map((to) => ({ to, label: label[to] })), { to: "close", label: `Hide ${title}` }];
}

/**
 * Where the window of a pane taken out of its dock sits under the pointer (`OnMotion`'s `actionClickCaption` branch): the pointer keeps its offset into the
 * caption, unless that is as far in as the floating window is wide, then 30 px in.
 */
export function floatGrab(offset: { x: number; y: number }, floatingW: number): { x: number; y: number } {
  return { x: floatingW <= offset.x ? FLOAT_GRAB_FALLBACK_PX : offset.x, y: offset.y };
}

/**
 * The hint wxAUI draws where the pane would dock (`CalculateHintRect`: the pane's rectangle in the layout `DoDrop` made): for a new dock at the frame's edge or
 * by a toolbar, a band of the frame along that edge; for a new row by the canvas a band of the canvas; for a new row by a pane a band along that pane's outer
 * edge; for a place in a pane's row, the half of the pane it would take.
 */
export function dropHintRect(drop: DropResult, g: DropGeometry, depth: number): PageRect {
  const band = (r: PageRect, side: PaneSide, d: number): PageRect => {
    const dd = Math.min(d, sideIsHorizontal(side) ? r.height / 2 : r.width / 2);
    switch (side) {
      case "top":
        return { left: r.left, top: r.top, width: r.width, height: dd };
      case "bottom":
        return { left: r.left, top: r.top + r.height - dd, width: r.width, height: dd };
      case "left":
        return { left: r.left, top: r.top, width: dd, height: r.height };
      case "right":
        return { left: r.left + r.width - dd, top: r.top, width: dd, height: r.height };
    }
  };
  const by = drop.by ? g.panes.find((p) => p.id === drop.by)?.rect : undefined;
  if (drop.how === "canvas" && g.canvas) return band(g.canvas, drop.side, depth);
  if (drop.how === "row" && by) return band(by, drop.side, depth);
  if (drop.how === "pane" && by) {
    const half = sideIsHorizontal(drop.side) ? { ...by, width: by.width / 2 } : { ...by, height: by.height / 2 };
    if (drop.after) return sideIsHorizontal(drop.side) ? { ...half, left: by.left + by.width / 2 } : { ...half, top: by.top + by.height / 2 };
    return half;
  }
  return band(g.frame, drop.side, depth);
}

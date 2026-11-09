// Where the board editor's Net Inspector and Search panes are docked: a port of the AUI pane info `PCB_EDIT_FRAME` gives them (pcbnew/pcb_edit_frame.cpp at
// 8303b2ad, the constructor's `AddPane`s and `PCB_EDIT_FRAME::SaveSettings`) and of what wxAUI lets a person do with a pane that is neither a toolbar nor fixed:
//
//   - Both panes are added `.Bottom()` at layer 0 -- right under the canvas, inside the vertical toolbars (layer 3) -- in the order `m_netInspectorPanel`, then
//     `m_searchPane`, so two panes docked on one side share that dock: side by side in a top or bottom dock, one over the other in a left or right one.
//   - Neither is given `TopDockable( false )` and the like, `Floatable( false )` or `DockFixed()`: wxAUI lets the person drag either by its caption to any side of the
//     canvas (the centre dock guide's four arrows dock it at layer 0 on that side) or leave it floating as a window of its own (`FloatingSize( 300, 200 )` for the
//     Net Inspector, `FloatingSize( 480, 200 )` for Search); a double click on the caption floats a docked pane and docks a floating one back on the side it came from
//     (`wxAuiManager::OnLeftDClick` with `wxAUI_MGR_ALLOW_FLOATING`).
//   - A dock is as deep as the person drags its sash; the panes' `BestSize` is where a fresh dock starts (300 x 200 for the Net Inspector, 180 x 100 for Search).
//   - Where they are is kept with the frame's layout (`RestoreAuiLayout` / the saved AUI perspective) and, for Search, in its own settings
//     (`aui.search_panel_dock_direction`, `aui.search_panel_height` for a top or bottom dock, `aui.search_panel_width` for a left or right one).
//
// The studio keeps the same in the person's browser (state/pcbPaneDock.ts), as it keeps the rest of the window's arrangement (state/dockLayoutStore.ts).
//
// Pure: no store, no DOM. Unit-tested in pcbPaneDock.test.ts.

/** The two panes of the board editor that dock around the canvas. */
export type PcbPaneId = "netInspector" | "search";

/** `wxAUI_DOCK_TOP`, `_BOTTOM`, `_LEFT`, `_RIGHT` at layer 0. */
export type PaneSide = "top" | "bottom" | "left" | "right";

/** A side of the canvas, or floating (`wxAuiPaneInfo::IsFloating`). */
export type PcbPaneDockAt = PaneSide | "float";

/** The panes in the order `PCB_EDIT_FRAME` adds them: the order they sit in when they share a dock. */
export const PCB_DOCKING_PANES: readonly PcbPaneId[] = ["netInspector", "search"];

export const PANE_SIDES: readonly PaneSide[] = ["top", "bottom", "left", "right"];

/** A floating pane's window, in CSS px of the page. */
export interface PaneRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface PcbPaneDock {
  /** Where each pane is. */
  at: Record<PcbPaneId, PcbPaneDockAt>;
  /** The side each pane was last docked on: where a double click on a floating pane's caption puts it back. */
  lastDocked: Record<PcbPaneId, PaneSide>;
  /** Each dock's depth: the height of a top or bottom dock, the width of a left or right one (the sash's position). */
  size: Record<PaneSide, number>;
  /** Each pane's floating window. */
  floating: Record<PcbPaneId, PaneRect>;
}

/** `BestSize` of each pane (`FromDIP( wxSize( 300, 200 ) )`, `FromDIP( wxSize( 180, 100 ) )`). */
export const PANE_BEST_SIZE: Record<PcbPaneId, { w: number; h: number }> = { netInspector: { w: 300, h: 200 }, search: { w: 180, h: 100 } };

/** `MinSize` of each pane (`FromDIP( wxSize( 240, 60 ) )`, `FromDIP( wxSize( 180, 60 ) )`). */
export const PANE_MIN_SIZE: Record<PcbPaneId, { w: number; h: number }> = { netInspector: { w: 240, h: 60 }, search: { w: 180, h: 60 } };

/** `FloatingSize`: `wxSize( 300, 200 )` for the Net Inspector, `FromDIP( wxSize( 480, 200 ) )` for Search. */
export const PANE_FLOATING_SIZE: Record<PcbPaneId, { w: number; h: number }> = { netInspector: { w: 300, h: 200 }, search: { w: 480, h: 200 } };

/** The caption bar's height, which a dock's depth includes (a pane never goes under its `MinSize` height plus the caption). */
export const CAPTION_PX = 22;

/**
 * Where everything starts: both panes `.Bottom()`, the bottom dock as tall as the Net Inspector's `BestSize` height plus its caption (the studio's 220 px of
 * before), a side dock as wide as its `BestSize` width; a floating window at its `FloatingSize`, `bottomHeight` is a height kept from before this layout existed.
 */
export function defaultPcbPaneDock(bottomHeight = 220): PcbPaneDock {
  return {
    at: { netInspector: "bottom", search: "bottom" },
    lastDocked: { netInspector: "bottom", search: "bottom" },
    size: { top: 220, bottom: bottomHeight, left: PANE_BEST_SIZE.netInspector.w, right: PANE_BEST_SIZE.netInspector.w },
    floating: {
      netInspector: { x: 120, y: 160, ...PANE_FLOATING_SIZE.netInspector },
      search: { x: 160, y: 200, ...PANE_FLOATING_SIZE.search },
    },
  };
}

const isSide = (v: unknown): v is PaneSide => v === "top" || v === "bottom" || v === "left" || v === "right";
const isAt = (v: unknown): v is PcbPaneDockAt => isSide(v) || v === "float";
const num = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);

/** Read a kept layout back: anything missing or of the wrong type keeps its default (a layout of an older build must not break the frame). */
export function parsePcbPaneDock(raw: unknown, bottomHeight?: number): PcbPaneDock {
  const d = defaultPcbPaneDock(bottomHeight);
  if (!raw || typeof raw !== "object") return d;
  const o = raw as Record<string, unknown>;
  const rec = (v: unknown) => (v && typeof v === "object" ? (v as Record<string, unknown>) : {});
  const at = rec(o.at);
  const last = rec(o.lastDocked);
  const size = rec(o.size);
  const fl = rec(o.floating);
  const rect = (v: unknown, fallback: PaneRect): PaneRect => {
    const r = rec(v);
    return num(r.x) && num(r.y) && num(r.w) && num(r.h) ? { x: r.x, y: r.y, w: r.w, h: r.h } : fallback;
  };
  const out: PcbPaneDock = { at: { ...d.at }, lastDocked: { ...d.lastDocked }, size: { ...d.size }, floating: { ...d.floating } };
  for (const id of PCB_DOCKING_PANES) {
    if (isAt(at[id])) out.at[id] = at[id] as PcbPaneDockAt;
    if (isSide(last[id])) out.lastDocked[id] = last[id] as PaneSide;
    out.floating[id] = rect(fl[id], d.floating[id]);
  }
  for (const s of PANE_SIDES) if (num(size[s]) && (size[s] as number) > 0) out.size[s] = size[s] as number;
  return out;
}

/** The panes docked on `side`, in the order they were added (`PCB_DOCKING_PANES`), of those `shown`. */
export function panesOnSide(dock: PcbPaneDock, side: PaneSide, shown: (id: PcbPaneId) => boolean): PcbPaneId[] {
  return PCB_DOCKING_PANES.filter((id) => dock.at[id] === side && shown(id));
}

/** The floating panes of those `shown`. */
export function floatingPanes(dock: PcbPaneDock, shown: (id: PcbPaneId) => boolean): PcbPaneId[] {
  return PCB_DOCKING_PANES.filter((id) => dock.at[id] === "float" && shown(id));
}

/** A top or bottom dock holds its panes in a row; a left or right one in a column. */
export function sideIsHorizontal(side: PaneSide): boolean {
  return side === "top" || side === "bottom";
}

/**
 * Put pane `id` at `to`. Docking it remembers the side (`lastDocked`); floating it with a window `from` (the pane's rectangle on the page when it is floated by a
 * double click or a drag: wxAUI opens the floating frame where the pane was) moves its window there, at its own floating size.
 */
export function movePane(dock: PcbPaneDock, id: PcbPaneId, to: PcbPaneDockAt, from?: { x: number; y: number }): PcbPaneDock {
  const next: PcbPaneDock = { ...dock, at: { ...dock.at, [id]: to } };
  if (to !== "float") next.lastDocked = { ...dock.lastDocked, [id]: to };
  else if (from) next.floating = { ...dock.floating, [id]: { ...dock.floating[id], x: Math.round(from.x), y: Math.round(from.y) } };
  return next;
}

/** `wxAuiManager::OnLeftDClick`: a docked pane floats, a floating one goes back to the side it was docked on last (`wxAuiPaneInfo::Dock()` keeps the direction). */
export function toggledPaneDock(dock: PcbPaneDock, id: PcbPaneId): PcbPaneDockAt {
  return dock.at[id] === "float" ? dock.lastDocked[id] : "float";
}

/** The deepest a dock may be in a window `windowW` x `windowH`: leave the canvas at least 120 px. */
export function maxDockSize(side: PaneSide, windowW: number, windowH: number): number {
  return Math.max(80, (sideIsHorizontal(side) ? windowH - 200 : windowW - 360) | 0);
}

/** A dock's depth after its sash is dragged: no shallower than its panes' `MinSize` (plus the caption for a top or bottom dock), no deeper than `maxDockSize`. */
export function clampDockSize(side: PaneSide, size: number, panes: readonly PcbPaneId[], windowW: number, windowH: number): number {
  const horizontal = sideIsHorizontal(side);
  const min = Math.max(horizontal ? 60 + CAPTION_PX : 120, ...panes.map((id) => (horizontal ? PANE_MIN_SIZE[id].h + CAPTION_PX : PANE_MIN_SIZE[id].w)));
  return Math.round(Math.max(min, Math.min(maxDockSize(side, windowW, windowH), size)));
}

/** A floating window kept on the page: no smaller than the pane's `MinSize`, no larger than the window, its caption reachable. */
export function clampPaneRect(id: PcbPaneId, r: PaneRect, windowW: number, windowH: number): PaneRect {
  const min = PANE_MIN_SIZE[id];
  const w = Math.max(min.w, Math.min(r.w, windowW));
  const h = Math.max(min.h + CAPTION_PX, Math.min(r.h, windowH));
  const x = Math.max(0, Math.min(r.x, windowW - w));
  const y = Math.max(0, Math.min(r.y, windowH - h));
  return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) };
}

/** The caption's buttons for a pane at `at`: the other sides and floating, then Close (`CloseButton( true )`), each with what it says. */
export function paneDockChoices(at: PcbPaneDockAt, title: string): { to: PcbPaneDockAt | "close"; label: string }[] {
  const label: Record<PcbPaneDockAt, string> = { top: "Dock at the top", bottom: "Dock at the bottom", left: "Dock on the left", right: "Dock on the right", float: "Float" };
  const order: PcbPaneDockAt[] = ["bottom", "top", "left", "right", "float"];
  return [...order.filter((to) => to !== at).map((to) => ({ to, label: label[to] })), { to: "close", label: `Hide ${title}` }];
}

/** A rectangle of the page (the canvas's, when a pane is dragged). */
export interface PageRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** The size of each of the centre dock guide's squares (`wxAuiCenterDockingGuide`'s arrows) and the gap between them. */
export const GUIDE_PX = 32;

/**
 * The centre dock guide wxAUI shows over the canvas while a pane is dragged (`wxAuiCenterDockingGuide`): an arrow on each side of the canvas's centre. Each entry
 * is the guide's square on the page and the side a pane dropped on it docks on.
 */
export function dockGuides(canvas: PageRect): { side: PaneSide; rect: PageRect }[] {
  const cx = canvas.left + canvas.width / 2;
  const cy = canvas.top + canvas.height / 2;
  const g = GUIDE_PX;
  const sq = (x: number, y: number): PageRect => ({ left: Math.round(x - g / 2), top: Math.round(y - g / 2), width: g, height: g });
  return [
    { side: "top", rect: sq(cx, cy - g * 1.25) },
    { side: "bottom", rect: sq(cx, cy + g * 1.25) },
    { side: "left", rect: sq(cx - g * 1.25, cy) },
    { side: "right", rect: sq(cx + g * 1.25, cy) },
  ];
}

/** The guide under the pointer at (`x`, `y`), if any: where a drop there docks the pane. */
export function dockGuideAt(canvas: PageRect, x: number, y: number): PaneSide | null {
  for (const { side, rect } of dockGuides(canvas)) if (x >= rect.left && x < rect.left + rect.width && y >= rect.top && y < rect.top + rect.height) return side;
  return null;
}

/**
 * The hint wxAUI draws while the pointer is on a guide (`wxAuiManager::DrawHintRect`): the part of the canvas the pane would take docked on `side`, as deep as that
 * dock is (half the canvas at most).
 */
export function dockHintRect(canvas: PageRect, side: PaneSide, depth: number): PageRect {
  const d = Math.min(depth, sideIsHorizontal(side) ? canvas.height / 2 : canvas.width / 2);
  switch (side) {
    case "top":
      return { left: canvas.left, top: canvas.top, width: canvas.width, height: d };
    case "bottom":
      return { left: canvas.left, top: canvas.top + canvas.height - d, width: canvas.width, height: d };
    case "left":
      return { left: canvas.left, top: canvas.top, width: d, height: canvas.height };
    case "right":
      return { left: canvas.left + canvas.width - d, top: canvas.top, width: d, height: canvas.height };
  }
}

/** How far a caption must be dragged before the pane leaves its dock (`wxAuiManager`'s drag threshold, `wxSYS_DRAG_X` / `wxSYS_DRAG_Y`: a few pixels). */
export const DRAG_THRESHOLD_PX = 4;

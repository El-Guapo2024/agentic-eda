// The editors' movable panes where wxAUI lets them go (kicad-port/auiDock.ts): the board editor's Net Inspector and Search (`PCB_EDIT_FRAME`) and the schematic
// editor's Search (`SCH_EDIT_FRAME`), each frame with its own layout. Every pane starts `.Bottom()` at layer 0, under the canvas and inside the vertical
// toolbars; the person drags a caption and the pane floats at once and follows the pointer, a hint showing where a release would dock it (`DoDrop`): along an
// edge of the editor on a layer of its own (a ring round the editor, inside the main toolbar and the message panel), in a new row by a docked pane or in its row
// before or after it, by a toolbar, or in a new row by an edge of the canvas; let go elsewhere it stays a floating window. A double click on the caption floats
// a docked pane and docks a floating one back where it was (`OnLeftDClick`). The panes of one side, layer and row share a dock -- side by side at the top or
// bottom, one over the other on the left or right -- with a sash between two of them; each dock's sash on the canvas's side sizes it. A floating pane is moved
// by its caption and sized by its corner. Each pane has a close button (`CloseButton( true )`) that its action shows again (`ToggleNetInspector`,
// `ToggleSearch`). The caption's arrow buttons move it as well (not KiCad's: wxAUI's caption has the close button alone).
import { useCallback, useEffect, useRef, useSyncExternalStore, type CSSProperties, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { paneVisible, type DockLayout } from "../../kicad-port/dockLayout";
import {
  DRAG_THRESHOLD_PX,
  PANE_FLOATING_SIZE,
  doDrop,
  dockRows,
  dropHintRect,
  floatGrab,
  floatingPanes,
  newDockDepth,
  outerLayers,
  paneDockChoices,
  sideIsHorizontal,
  sideRows,
  toggledPaneDock,
  type DockEditor,
  type DockRow,
  type DropGeometry,
  type DropResult,
  type MovablePaneId,
  type PageRect,
  type PaneDockAt,
  type PaneRect,
  type PaneSide,
} from "../../kicad-port/auiDock";
import { toggleDockPane, useDockLayout } from "../../state/dockLayoutStore";
import { dockPaneBack, getPaneDock, movePaneTo, paneShown, resizePanesBetween, setDockDepth, setPaneDock, setPaneFloatRect, usePaneDock } from "../../state/paneDock";
import { DockPanel } from "./Dock";
import { NetInspectorPanel } from "./NetInspectorPanel";
import { SearchPanel } from "./SearchPanel";
import { SchSearchPanel } from "./SchSearchPanel";

const TITLE: Record<MovablePaneId, string> = { netInspector: "Net Inspector", search: "Search", schSearch: "Search" };
const GLYPH: Record<PaneDockAt, string> = { top: "⤒", bottom: "⤓", left: "⇤", right: "⇥", float: "⧉" };

const shownIn = (layout: DockLayout) => (id: MovablePaneId) => paneVisible(layout, id);

function paneBody(id: MovablePaneId): ReactNode {
  if (id === "netInspector") return <NetInspectorPanel />;
  if (id === "search") return <SearchPanel />;
  return <SchSearchPanel />;
}

// ---------------------------------------------------------------------------------------------------------------------------------- a caption dragged

/** A pane being dragged by its caption: its window, and where a release would dock it. */
interface PaneDrag {
  editor: DockEditor;
  id: MovablePaneId;
  rect: PaneRect;
  drop: DropResult | null;
  hint: PageRect | null;
}

let drag: PaneDrag | null = null;
const dragListeners = new Set<() => void>();
function setDrag(next: PaneDrag | null): void {
  drag = next;
  for (const l of [...dragListeners]) l();
}
function subscribeDrag(l: () => void): () => void {
  dragListeners.add(l);
  return () => dragListeners.delete(l);
}
function useDrag(): PaneDrag | null {
  return useSyncExternalStore(subscribeDrag, () => drag, () => drag);
}

const pageRect = (el: Element | null | undefined): PageRect | null => {
  const r = el?.getBoundingClientRect();
  return r && (r.width > 0 || r.height > 0) ? { left: r.left, top: r.top, width: r.width, height: r.height } : null;
};

/**
 * What `DoDrop`'s hit test sees on the page: the editor from its main toolbar to its message panel (the frame's client area), the canvas, the docked panes and
 * docks' sashes of `editor`, the toolbars with their sides.
 */
function dropGeometry(editor: DockEditor): DropGeometry {
  const all = pageRect(document.querySelector(".app-frame")) ?? { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight };
  const top = pageRect(document.querySelector(".app-frame .main-toolbar-row")) ?? pageRect(document.querySelector(".aui-frame"));
  const bottom = pageRect(document.querySelector(".app-frame .message-panel-row")) ?? pageRect(document.querySelector(".aui-frame"));
  const frame: PageRect = top && bottom ? { left: all.left, top: top.top, width: all.width, height: bottom.top + bottom.height - top.top } : all;
  const panes = [...document.querySelectorAll<HTMLElement>(`[data-dock-editor="${editor}"] [data-dock-pane]`)].flatMap((el) => {
    const rect = pageRect(el);
    return rect ? [{ id: el.dataset.dockPane as MovablePaneId, rect }] : [];
  });
  const sashes = [...document.querySelectorAll<HTMLElement>(`[data-dock-editor="${editor}"] [data-dock-sash]`)].flatMap((el) => {
    const rect = pageRect(el);
    return rect ? [{ rect, panes: (el.dataset.dockSash ?? "").split(",").filter((p) => p !== "") as MovablePaneId[] }] : [];
  });
  const toolbars: { side: PaneSide; rect: PageRect }[] = [];
  const bar = (sel: string, side: PaneSide) => {
    const rect = pageRect(document.querySelector(sel));
    if (rect) toolbars.push({ side, rect });
  };
  bar(".app-frame .main-toolbar-row", "top");
  bar(".app-frame .aux-toolbar-row", "top");
  bar(".app-body .options-toolbar-col", "left");
  bar(".app-body .drawing-toolbar-col", "right");
  return { frame, canvas: pageRect(document.querySelector(".app-body .canvas-col")), panes, sashes, toolbars };
}

/**
 * The caption's press: past the drag threshold the pane floats where it is (wxAUI floats a docked pane as soon as its caption is dragged) and its window
 * follows the pointer, the hint showing where `DoDrop` would dock it; released, it docks there, or stays floating where it was let go.
 */
function startCaptionDrag(editor: DockEditor, id: MovablePaneId, e: ReactPointerEvent<HTMLElement>): void {
  if (e.button !== 0 || (e.target as HTMLElement).closest("button, .dock-panel-chevron")) return;
  const pane = (e.currentTarget.closest("[data-dock-pane]") as HTMLElement | null) ?? e.currentTarget;
  const box = pane.getBoundingClientRect();
  const start = { x: e.clientX, y: e.clientY };
  let moving = false;
  let grab = { x: e.clientX - box.left, y: e.clientY - box.top };
  let size = { w: 0, h: 0 };
  const at = (ev: PointerEvent): PaneDrag => {
    const rect = { x: ev.clientX - grab.x, y: ev.clientY - grab.y, w: size.w, h: size.h };
    const g = dropGeometry(editor);
    const drop = doDrop(editor, getPaneDock(editor), id, g, ev.clientX, ev.clientY, paneShown);
    const depth = drop ? (drop.dock.depth[id] ?? newDockDepth(id, drop.side, g.frame)) : 0;
    return { editor, id, rect, drop, hint: drop ? dropHintRect(drop, g, depth) : null };
  };
  const move = (ev: PointerEvent) => {
    if (!moving && Math.hypot(ev.clientX - start.x, ev.clientY - start.y) < DRAG_THRESHOLD_PX) return;
    ev.preventDefault();
    if (!moving) {
      moving = true;
      const dock = getPaneDock(editor);
      const kept = dock.floating[id];
      size = kept ? { w: kept.w, h: kept.h } : { ...PANE_FLOATING_SIZE[id] };
      if (dock.at[id] !== "float") {
        // `paneInfo->Float()`: the pane leaves its dock at once, its window at its floating size under the pointer
        grab = floatGrab(grab, size.w);
        movePaneTo(editor, id, "float", { x: ev.clientX - grab.x, y: ev.clientY - grab.y });
      }
    }
    setDrag(at(ev));
  };
  const up = (ev: PointerEvent) => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    if (!moving) return;
    const d = at(ev);
    setDrag(null);
    if (d.drop) setPaneDock(editor, d.drop.dock);
    else setPaneFloatRect(editor, id, d.rect);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
}

/** Where pane `id` is on the page, for a floating window opened where it was. */
function paneOrigin(id: MovablePaneId): { x: number; y: number } | undefined {
  const box = document.querySelector(`[data-dock-pane="${id}"]`)?.getBoundingClientRect();
  return box ? { x: box.left, y: box.top } : undefined;
}

/** Put pane `id` at `to`; floated, its window opens where the pane was. */
function moveTo(editor: DockEditor, id: MovablePaneId, to: PaneDockAt): void {
  movePaneTo(editor, id, to, to === "float" ? paneOrigin(id) : undefined);
}

/** One pane in its dock or window: the captioned panel with the places it may go. */
function MovablePane({ editor, id }: { editor: DockEditor; id: MovablePaneId }) {
  const dock = usePaneDock(editor);
  const at = dock.at[id] ?? "bottom";
  const buttons = paneDockChoices(at, TITLE[id])
    .filter((c): c is { to: PaneDockAt; label: string } => c.to !== "close")
    .map((c) => (
      <button
        key={c.to}
        type="button"
        className="pcb-pane-move"
        data-dock-to={c.to}
        aria-label={c.label}
        title={c.label}
        onClick={(e) => {
          e.stopPropagation();
          moveTo(editor, id, c.to);
        }}
      >
        {GLYPH[c.to]}
      </button>
    ));
  return (
    <DockPanel
      id={id}
      title={TITLE[id]}
      onClose={() => toggleDockPane(id)}
      movable={{
        onPointerDown: (e) => startCaptionDrag(editor, id, e),
        onDoubleClick: (e) => {
          if ((e.target as HTMLElement).closest("button, .dock-panel-chevron")) return;
          // `OnLeftDClick`: a docked pane floats where it is, a floating one docks back where it was
          if (toggledPaneDock(getPaneDock(editor), id) === "float") moveTo(editor, id, "float");
          else dockPaneBack(editor, id);
        },
        hint: at === "float" ? "Drag to move or to dock; double-click to dock" : "Drag to move; double-click to float",
        buttons,
      }}
    >
      {paneBody(id)}
    </DockPanel>
  );
}

// ---------------------------------------------------------------------------------------------------------------------------------------- the docks

/** A pointer drag of a sash: `onDelta` with how far it has gone (along y when `vertical`, else along x). */
function useSashDrag(vertical: boolean, onDelta: (delta: number) => void) {
  const live = useRef<{ at: number; onDelta: (d: number) => void } | null>(null);
  const onMove = useCallback(
    (e: PointerEvent) => {
      const d = live.current;
      if (d) d.onDelta((vertical ? e.clientY : e.clientX) - d.at);
    },
    [vertical]
  );
  const onUp = useCallback(() => {
    live.current = null;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
  }, [onMove]);
  useEffect(() => onUp, [onUp]);
  return (e: ReactPointerEvent<HTMLElement>) => {
    e.preventDefault();
    live.current = { at: vertical ? e.clientY : e.clientX, onDelta };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };
}

/** The sash between two panes of one dock (`typePaneSizer`): their shares of the dock follow it. */
function PaneSizer({ editor, side, a, b }: { editor: DockEditor; side: PaneSide; a: MovablePaneId; b: MovablePaneId }) {
  const across = sideIsHorizontal(side);
  const sizes = useRef({ a: 0, b: 0 });
  const start = useSashDrag(!across, (delta) => resizePanesBetween(editor, side, a, b, sizes.current.a, sizes.current.b, delta));
  return (
    <div
      className={`pane-dock-pane-sizer ${across ? "across" : "down"}`}
      role="separator"
      aria-orientation={across ? "vertical" : "horizontal"}
      aria-label={`Resize ${TITLE[a]} and ${TITLE[b]}`}
      data-testid={`pane-sizer-${a}-${b}`}
      onPointerDown={(e) => {
        const box = (id: MovablePaneId) => document.querySelector(`[data-dock-pane="${id}"]`)?.getBoundingClientRect();
        const ra = box(a);
        const rb = box(b);
        sizes.current = { a: ra ? (across ? ra.width : ra.height) : 0, b: rb ? (across ? rb.width : rb.height) : 0 };
        start(e);
      }}
    />
  );
}

/** One dock (`wxAuiDockInfo`): its panes in a row or a column, the sash on the canvas's side sizing it. */
function DockRowView({ editor, row, testSide }: { editor: DockEditor; row: DockRow; testSide: boolean }) {
  const layout = useDockLayout();
  const dock = usePaneDock(editor);
  const { side } = row;
  const horizontal = sideIsHorizontal(side);
  const depthAtPress = useRef(row.depth);
  const start = useSashDrag(horizontal, (delta) => {
    // the bottom and right docks grow as the sash goes up or left
    const grow = side === "bottom" || side === "right" ? -delta : delta;
    setDockDepth(editor, side, depthAtPress.current + grow, row.panes);
  });
  const allFolded = row.panes.every((id) => layout.folded[id]);
  const style: CSSProperties = horizontal ? { height: allFolded ? undefined : row.depth } : { width: row.depth };
  const sash = (!horizontal || !allFolded) && (
    <div
      className="bottom-dock-sash pcb-pane-dock-sash"
      role="separator"
      aria-orientation={horizontal ? "horizontal" : "vertical"}
      aria-label={`Resize the ${side} panes`}
      data-testid={testSide ? `${editor}-pane-sash-${side}` : undefined}
      data-dock-sash={row.panes.join(",")}
      onPointerDown={(e) => {
        depthAtPress.current = row.depth;
        start(e);
      }}
    />
  );
  const panes: ReactNode[] = [];
  row.panes.forEach((id, i) => {
    if (i > 0) panes.push(<PaneSizer key={`sizer-${id}`} editor={editor} side={side} a={row.panes[i - 1]!} b={id} />);
    panes.push(
      <div
        key={id}
        className="bottom-dock-pane pcb-pane-dock-pane"
        style={{ flexGrow: dock.proportion[id] ?? 1 }}
        data-bottom-pane={side === "bottom" && row.layer === 0 ? id : undefined}
        data-dock-pane={id}
        data-testid={id === "schSearch" ? "sch-search-dock" : undefined}
      >
        <MovablePane editor={editor} id={id} />
      </div>
    );
  });
  return (
    <div className={`pane-dock-row ${side}`} style={style} data-testid="pane-dock-row" data-dock-row={row.row} data-dock-layer={row.layer} data-dock-side={side}>
      {(side === "bottom" || side === "right") && sash}
      <div className="bottom-dock-panes pcb-pane-dock-panes">{panes}</div>
      {(side === "top" || side === "left") && sash}
    </div>
  );
}

/** The docks of one side and layer of `editor`, in the order they are laid out (row 0 at the editor's edge). */
export function PaneDockArea({ editor, side, layer = 0 }: { editor: DockEditor; side: PaneSide; layer?: number }) {
  const layout = useDockLayout();
  const dock = usePaneDock(editor);
  const rows = sideRows(dockRows(editor, dock, shownIn(layout)), side, layer);
  if (rows.length === 0) return null;
  const testid = layer > 0 ? `${editor}-pane-dock-${side}-${layer}` : editor === "pcb" && side === "bottom" ? "bottom-dock" : `${editor}-pane-dock-${side}`;
  return (
    <div className={`bottom-dock pcb-pane-dock pane-dock-area ${side}`} data-testid={testid} data-dock-side={side} data-dock-layer={layer} data-dock-editor={editor}>
      {rows.map((r) => (
        <DockRowView key={`${r.layer}-${r.row}`} editor={editor} row={r} testSide={layer === 0} />
      ))}
    </div>
  );
}

/**
 * The rings of the layers above 0 (`LayoutAll`, a layer at a time from the inside out: its top dock over its left dock, the inside and its right dock, then
 * its bottom dock), round `children` -- the auxiliary toolbar and the editor's body. One grid whatever the number of rings, so the editor inside is not built
 * again when a ring comes or goes.
 */
export function OuterDocks({ editor, children }: { editor: DockEditor | null; children: ReactNode }) {
  const layout = useDockLayout();
  const pcb = usePaneDock("pcb");
  const sch = usePaneDock("sch");
  const dock = editor === "pcb" ? pcb : editor === "sch" ? sch : null;
  const layers = editor && dock ? outerLayers(dockRows(editor, dock, shownIn(layout))) : [];
  const n = layers.length;
  // ring k from the outside is layers[n - 1 - k]: its left dock in grid column k + 1, its right dock in column 2n + 1 - k, its top and bottom docks in those rows
  const cells: ReactNode[] = [];
  layers.forEach((layer, i) => {
    const k = n - 1 - i;
    const near = k + 1;
    const far = 2 * n + 1 - k;
    const area = (side: PaneSide, style: CSSProperties) => (
      <div key={`ring-${layer}-${side}`} className={`aui-ring-dock ${side}`} style={style}>
        <PaneDockArea editor={editor!} side={side} layer={layer} />
      </div>
    );
    cells.push(area("top", { gridRow: near, gridColumn: `${near} / ${far + 1}` }));
    cells.push(area("bottom", { gridRow: far, gridColumn: `${near} / ${far + 1}` }));
    cells.push(area("left", { gridColumn: near, gridRow: `${near + 1} / ${far}` }));
    cells.push(area("right", { gridColumn: far, gridRow: `${near + 1} / ${far}` }));
  });
  const tracks = n > 0 ? `repeat(${n}, auto) minmax(0, 1fr) repeat(${n}, auto)` : "minmax(0, 1fr)";
  return (
    <div className="aui-frame" data-rings={n} style={{ gridTemplateColumns: tracks, gridTemplateRows: tracks }}>
      <div key="inside" className="aui-inside" style={{ gridRow: n + 1, gridColumn: n + 1 }}>
        {children}
      </div>
      {cells}
    </div>
  );
}

/** One floating pane: a window over the editor (`wxAuiFloatingFrame`), moved by its caption, sized by its corner. */
function FloatingPane({ editor, id }: { editor: DockEditor; id: MovablePaneId }) {
  const dock = usePaneDock(editor);
  const live = useDrag();
  const box = useRef<HTMLDivElement>(null);
  const rect = live?.editor === editor && live.id === id ? live.rect : dock.floating[id]!;

  // sized by its corner (CSS `resize`): the size it is left at is the one it opens with next time
  useEffect(() => {
    if (!box.current || typeof ResizeObserver === "undefined") return;
    const el = box.current;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const seen = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        if (drag?.id === id) return;
        const r = getPaneDock(editor).floating[id]!;
        if (Math.abs(el.offsetWidth - r.w) > 1 || Math.abs(el.offsetHeight - r.h) > 1) setPaneFloatRect(editor, id, { ...r, w: el.offsetWidth, h: el.offsetHeight });
      }, 150);
    });
    seen.observe(el);
    return () => {
      clearTimeout(timer);
      seen.disconnect();
    };
  }, [editor, id]);

  return (
    <div
      ref={box}
      className="pcb-pane-floating"
      role="dialog"
      aria-label={TITLE[id]}
      data-testid={`${editor}-pane-floating-${id}`}
      data-dock-floating={id}
      style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h }}
    >
      <div className="pcb-pane-floating-holder" data-testid={id === "schSearch" ? "sch-search-dock" : undefined} data-dock-pane={id}>
        <MovablePane editor={editor} id={id} />
      </div>
    </div>
  );
}

/** The floating panes of `editor`, and while a caption is dragged the hint of where a release would dock the pane (`DrawHintRect`). */
export function FloatingPanes({ editor }: { editor: DockEditor }) {
  const layout = useDockLayout();
  const dock = usePaneDock(editor);
  const live = useDrag();
  const floating = floatingPanes(editor, dock, shownIn(layout));
  const hint = live?.editor === editor ? live.hint : null;
  return (
    <>
      {floating.map((id) => (
        <FloatingPane key={id} editor={editor} id={id} />
      ))}
      {hint && (
        <div
          className="pcb-pane-dock-hint"
          style={{ left: hint.left, top: hint.top, width: hint.width, height: hint.height }}
          data-testid="pcb-pane-dock-hint"
          data-side={live?.drop?.side ?? ""}
          data-how={live?.drop?.how ?? ""}
        />
      )}
    </>
  );
}

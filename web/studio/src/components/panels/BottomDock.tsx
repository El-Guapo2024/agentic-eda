// The board editor's Net Inspector and Search panes where wxAUI lets them go (kicad-port/pcbPaneDock.ts): `PCB_EDIT_FRAME` docks both `.Bottom()` at layer 0, right
// under the canvas and inside the vertical toolbars, side by side in the order they are added; the person may drag either by its caption to another side of the
// canvas -- the centre dock guide's arrows (`wxAuiCenterDockingGuide`) show while it is dragged, a hint shows where it would go, a drop on an arrow docks it
// there, a drop anywhere else leaves it floating -- or double-click the caption to float it and to dock it back (`wxAuiManager::OnLeftDClick`). Two panes on one
// side share that dock: in a row at the top or bottom, one over the other on the left or right. Each dock is as deep as its sash is dragged; a floating pane is
// moved by its caption and sized by its corner. Each pane has a close button (`CloseButton( true )`) that its action shows again (`ToggleNetInspector`,
// `ToggleSearch`). The caption's arrow buttons move it as well (not KiCad's: wxAUI's caption has the close button alone).
import { useCallback, useEffect, useRef, useSyncExternalStore, type PointerEvent as ReactPointerEvent } from "react";
import { paneVisible, type DockLayout } from "../../kicad-port/dockLayout";
import {
  DRAG_THRESHOLD_PX,
  PANE_FLOATING_SIZE,
  dockGuideAt,
  dockGuides,
  dockHintRect,
  floatingPanes,
  paneDockChoices,
  panesOnSide,
  sideIsHorizontal,
  toggledPaneDock,
  type PageRect,
  type PaneRect,
  type PaneSide,
  type PcbPaneDockAt,
  type PcbPaneId,
} from "../../kicad-port/pcbPaneDock";
import { toggleDockPane, useDockLayout } from "../../state/dockLayoutStore";
import { getPcbPaneDock, setPcbDockSize, setPcbPaneDock, setPcbPaneFloatRect, usePcbPaneDock } from "../../state/pcbPaneDock";
import { DockPanel } from "./Dock";
import { NetInspectorPanel } from "./NetInspectorPanel";
import { SearchPanel } from "./SearchPanel";

const TITLE: Record<PcbPaneId, string> = { netInspector: "Net Inspector", search: "Search" };
const GLYPH: Record<PcbPaneDockAt, string> = { top: "⤒", bottom: "⤓", left: "⇤", right: "⇥", float: "⧉" };

const shownIn = (layout: DockLayout) => (id: PcbPaneId) => paneVisible(layout, id);

// ---------------------------------------------------------------------------------------------------------------------------------- a caption dragged

/** A pane being dragged by its caption: where its window is, and the guide under the pointer. */
interface PaneDrag {
  id: PcbPaneId;
  rect: PaneRect;
  guide: PaneSide | null;
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

/** The canvas's rectangle on the page: what the dock guides and the hint are drawn over. */
function canvasRect(): PageRect | null {
  const r = document.querySelector(".app-body .canvas-col")?.getBoundingClientRect();
  return r ? { left: r.left, top: r.top, width: r.width, height: r.height } : null;
}

/**
 * The caption's press: past the drag threshold the pane leaves its dock and follows the pointer as a floating window (wxAUI floats a docked pane as soon as its
 * caption is dragged), the dock guides showing; released on a guide it docks on that side, anywhere else it stays floating where it was let go.
 */
function startCaptionDrag(id: PcbPaneId, e: ReactPointerEvent<HTMLElement>): void {
  if (e.button !== 0 || (e.target as HTMLElement).closest("button, .dock-panel-chevron")) return;
  const pane = (e.currentTarget.closest("[data-dock-pane]") as HTMLElement | null) ?? e.currentTarget;
  const box = pane.getBoundingClientRect();
  const dock = getPcbPaneDock();
  const floating = dock.at[id] === "float";
  const size = floating ? { w: dock.floating[id].w, h: dock.floating[id].h } : PANE_FLOATING_SIZE[id];
  // where the pointer holds the window: the same offset into a floating window, the caption's left part for a pane taken out of its dock
  const grab = floating ? { x: e.clientX - box.left, y: e.clientY - box.top } : { x: Math.min(e.clientX - box.left, size.w / 2), y: Math.min(e.clientY - box.top, 10) };
  const start = { x: e.clientX, y: e.clientY };
  let moving = false;
  const at = (ev: PointerEvent): PaneDrag => {
    const rect = { x: ev.clientX - grab.x, y: ev.clientY - grab.y, w: size.w, h: size.h };
    const c = canvasRect();
    return { id, rect, guide: c ? dockGuideAt(c, ev.clientX, ev.clientY) : null };
  };
  const move = (ev: PointerEvent) => {
    if (!moving && Math.hypot(ev.clientX - start.x, ev.clientY - start.y) < DRAG_THRESHOLD_PX) return;
    moving = true;
    ev.preventDefault();
    setDrag(at(ev));
  };
  const up = (ev: PointerEvent) => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    if (!moving) return;
    const d = at(ev);
    setDrag(null);
    if (d.guide) setPcbPaneDock(id, d.guide);
    else {
      setPcbPaneDock(id, "float", { x: d.rect.x, y: d.rect.y });
      setPcbPaneFloatRect(id, d.rect);
    }
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
}

/** Where pane `id` is on the page, for a floating window opened where it was. */
function paneOrigin(id: PcbPaneId): { x: number; y: number } | undefined {
  const box = document.querySelector(`[data-dock-pane="${id}"]`)?.getBoundingClientRect();
  return box ? { x: box.left, y: box.top } : undefined;
}

/** Put pane `id` at `to`; floated, its window opens where the pane was. */
function moveTo(id: PcbPaneId, to: PcbPaneDockAt): void {
  setPcbPaneDock(id, to, to === "float" ? paneOrigin(id) : undefined);
}

/** One pane in its dock or window: the captioned panel with the places it may go. */
function MovablePane({ id }: { id: PcbPaneId }) {
  const dock = usePcbPaneDock();
  const at = dock.at[id];
  const buttons = paneDockChoices(at, TITLE[id])
    .filter((c): c is { to: PcbPaneDockAt; label: string } => c.to !== "close")
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
          moveTo(id, c.to);
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
        onPointerDown: (e) => startCaptionDrag(id, e),
        onDoubleClick: (e) => {
          if ((e.target as HTMLElement).closest("button, .dock-panel-chevron")) return;
          // `OnLeftDClick`: a docked pane floats where it is, a floating one docks back on the side it came from
          moveTo(id, toggledPaneDock(getPcbPaneDock(), id));
        },
        hint: at === "float" ? "Drag to move or to dock; double-click to dock" : "Drag to move; double-click to float",
        buttons,
      }}
    >
      {id === "netInspector" ? <NetInspectorPanel /> : <SearchPanel />}
    </DockPanel>
  );
}

// ---------------------------------------------------------------------------------------------------------------------------------------- the docks

/** The panes docked on one side of the canvas, the sash on the canvas's side of them. */
export function PcbPaneDockArea({ side }: { side: PaneSide }) {
  const layout = useDockLayout();
  const dock = usePcbPaneDock();
  const panes = panesOnSide(dock, side, shownIn(layout));
  const horizontal = sideIsHorizontal(side);
  const sash = useRef<{ at: number; size: number; panes: PcbPaneId[] } | null>(null);

  const onMove = useCallback(
    (e: PointerEvent) => {
      const d = sash.current;
      if (!d) return;
      const pos = horizontal ? e.clientY : e.clientX;
      // the bottom and right docks grow as the sash goes up or left
      const grow = side === "bottom" || side === "right" ? d.at - pos : pos - d.at;
      setPcbDockSize(side, d.size + grow, d.panes);
    },
    [side, horizontal]
  );
  const onUp = useCallback(() => {
    sash.current = null;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
  }, [onMove]);
  useEffect(() => onUp, [onUp]);

  if (panes.length === 0) return null;
  const allFolded = panes.every((id) => layout.folded[id]);
  const depth = dock.size[side];
  const style = horizontal ? { height: allFolded ? undefined : depth } : { width: depth };
  const sashEl = (!horizontal || !allFolded) && (
    <div
      className="bottom-dock-sash pcb-pane-dock-sash"
      role="separator"
      aria-orientation={horizontal ? "horizontal" : "vertical"}
      aria-label={`Resize the ${side} panes`}
      data-testid={`pcb-pane-sash-${side}`}
      onPointerDown={(e) => {
        e.preventDefault();
        sash.current = { at: horizontal ? e.clientY : e.clientX, size: depth, panes };
        window.addEventListener("pointermove", onMove);
        window.addEventListener("pointerup", onUp);
      }}
    />
  );
  return (
    <div className={`bottom-dock pcb-pane-dock ${side}`} data-testid={side === "bottom" ? "bottom-dock" : `pcb-pane-dock-${side}`} data-dock-side={side} style={style}>
      {(side === "bottom" || side === "right") && sashEl}
      <div className="bottom-dock-panes pcb-pane-dock-panes">
        {panes.map((id) => (
          <div key={id} className="bottom-dock-pane pcb-pane-dock-pane" data-bottom-pane={side === "bottom" ? id : undefined} data-dock-pane={id}>
            <MovablePane id={id} />
          </div>
        ))}
      </div>
      {(side === "top" || side === "left") && sashEl}
    </div>
  );
}

/** The bottom dock (`.Bottom()`): where both panes start. */
export function BottomDock() {
  return <PcbPaneDockArea side="bottom" />;
}

/** One floating pane: a window over the editor (`wxAuiFloatingFrame`), moved by its caption, sized by its corner. */
function FloatingPane({ id }: { id: PcbPaneId }) {
  const dock = usePcbPaneDock();
  const live = useDrag();
  const box = useRef<HTMLDivElement>(null);
  const rect = live?.id === id ? live.rect : dock.floating[id];

  // sized by its corner (CSS `resize`): the size it is left at is the one it opens with next time
  useEffect(() => {
    if (!box.current || typeof ResizeObserver === "undefined") return;
    const el = box.current;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const seen = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        if (drag?.id === id) return;
        const r = getPcbPaneDock().floating[id];
        if (Math.abs(el.offsetWidth - r.w) > 1 || Math.abs(el.offsetHeight - r.h) > 1) setPcbPaneFloatRect(id, { ...r, w: el.offsetWidth, h: el.offsetHeight });
      }, 150);
    });
    seen.observe(el);
    return () => {
      clearTimeout(timer);
      seen.disconnect();
    };
  }, [id]);

  return (
    <div
      ref={box}
      className="pcb-pane-floating"
      role="dialog"
      aria-label={TITLE[id]}
      data-testid={`pcb-pane-floating-${id}`}
      data-dock-pane={id}
      style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h }}
    >
      <MovablePane id={id} />
    </div>
  );
}

/**
 * The floating panes, and while a caption is dragged the pane's window under the pointer, the centre dock guide over the canvas and, over a guide, the hint of
 * where the pane would dock.
 */
export function PcbFloatingPanes() {
  const layout = useDockLayout();
  const dock = usePcbPaneDock();
  const live = useDrag();
  const floating = floatingPanes(dock, shownIn(layout));
  const canvas = live ? canvasRect() : null;
  // a docked pane being dragged out is drawn as an outline under the pointer until it is let go (its panel stays in the dock meanwhile)
  const ghost = live && dock.at[live.id] !== "float" ? live : null;
  const hint = live?.guide && canvas ? dockHintRect(canvas, live.guide, dock.size[live.guide]) : null;
  return (
    <>
      {floating.map((id) => (
        <FloatingPane key={id} id={id} />
      ))}
      {ghost && <div className="pcb-pane-drag-ghost" style={{ left: ghost.rect.x, top: ghost.rect.y, width: ghost.rect.w, height: ghost.rect.h }} data-testid="pcb-pane-drag-ghost" />}
      {hint && <div className="pcb-pane-dock-hint" style={{ left: hint.left, top: hint.top, width: hint.width, height: hint.height }} data-testid="pcb-pane-dock-hint" data-side={live?.guide ?? ""} />}
      {live &&
        canvas &&
        dockGuides(canvas).map((g) => (
          <div
            key={g.side}
            className={`pcb-pane-dock-guide${live.guide === g.side ? " active" : ""}`}
            style={{ left: g.rect.left, top: g.rect.top, width: g.rect.width, height: g.rect.height }}
            data-testid={`pcb-pane-guide-${g.side}`}
            data-side={g.side}
          >
            {GLYPH[g.side]}
          </div>
        ))}
    </>
  );
}

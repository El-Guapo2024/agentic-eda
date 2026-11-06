// The two pieces every docked column is made of: `DockColumn`, a column that folds to a thin handle so the canvas can take the width (an 800 px
// window cannot hold a 240 px column and still have a canvas), and `DockPanel`, one pane with a KiCad-style caption bar (a chevron to roll the pane up,
// a close button that hides it until its toolbar button shows it again -- `CloseButton( true )` in the AUI pane info). The layout rules and the
// default arrangement are kicad-port/dockLayout.ts's; where it is kept is state/dockLayoutStore.ts's.
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent, ReactNode } from "react";
import { paneVisible, type DockColumnId, type DockPaneId } from "../../kicad-port/dockLayout";
import { setDockColumnCollapsed, toggleDockPane, toggleDockPaneFolded, useDockLayout } from "../../state/dockLayoutStore";

/** `column` names the fold state when it is not the side's own: the library editors' tree column sits on the left but folds on its own. */
export function DockColumn({ side, column = side, label, children }: { side: "left" | "right"; column?: DockColumnId; label: string; children: ReactNode }) {
  const layout = useDockLayout();
  const collapsed = column === "left" ? layout.leftCollapsed : column === "right" ? layout.rightCollapsed : layout.treeCollapsed;
  // The chevron points where the column goes when the button is pressed.
  const chevron = side === "left" ? (collapsed ? "›" : "‹") : collapsed ? "‹" : "›";
  return (
    <div className={`dock-column ${side}${collapsed ? " collapsed" : ""}`} data-dock-column={column}>
      {!collapsed && children}
      <button
        type="button"
        className="dock-handle"
        aria-expanded={!collapsed}
        aria-label={collapsed ? `Show ${label}` : `Hide ${label}`}
        title={collapsed ? `Show ${label}` : `Hide ${label}`}
        onClick={() => setDockColumnCollapsed(column, !collapsed)}
      >
        <span className="dock-handle-chevron" aria-hidden>
          {chevron}
        </span>
        {collapsed && (
          <span className="dock-handle-label" aria-hidden>
            {label}
          </span>
        )}
      </button>
    </div>
  );
}

/**
 * A pane that wxAUI lets the person move (the board editor's Net Inspector and Search, components/panels/BottomDock.tsx): its caption is dragged and
 * double-clicked to dock or float it, so only the chevron folds it, and `buttons` (the places it may go) sit before the close button.
 */
export interface MovableCaption {
  onPointerDown: (e: ReactPointerEvent<HTMLElement>) => void;
  onDoubleClick: (e: ReactMouseEvent<HTMLElement>) => void;
  hint: string;
  buttons: ReactNode;
}

/**
 * `visible` / `onClose` are for a pane whose open state lives elsewhere (the Net Navigator's is the schematic control store's, read by its action and its
 * check mark): the pane is drawn while `visible`, and its close button calls `onClose` instead of hiding it in the layout.
 */
export function DockPanel({ id, title, children, visible, onClose, movable }: { id: DockPaneId; title: string; children: ReactNode; visible?: boolean; onClose?: () => void; movable?: MovableCaption }) {
  const layout = useDockLayout();
  if (!(visible ?? paneVisible(layout, id))) return null;
  const folded = layout.folded[id];
  return (
    <section className={`dock-panel${folded ? " folded" : ""}`} data-pane={id}>
      <header
        className={`dock-panel-caption${movable ? " movable" : ""}`}
        role="button"
        tabIndex={0}
        aria-expanded={!folded}
        title={movable ? movable.hint : folded ? `Unfold ${title}` : `Fold ${title}`}
        onClick={movable ? undefined : () => toggleDockPaneFolded(id)}
        onPointerDown={movable?.onPointerDown}
        onDoubleClick={movable?.onDoubleClick}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            toggleDockPaneFolded(id);
          }
        }}
      >
        <span
          className="dock-panel-chevron"
          aria-hidden={!movable}
          role={movable ? "button" : undefined}
          title={movable ? (folded ? `Unfold ${title}` : `Fold ${title}`) : undefined}
          data-testid={movable ? `dock-fold-${id}` : undefined}
          onClick={
            movable
              ? (e) => {
                  e.stopPropagation();
                  toggleDockPaneFolded(id);
                }
              : undefined
          }
        >
          {folded ? "▸" : "▾"}
        </span>
        <span className="dock-panel-title">{title}</span>
        {movable?.buttons}
        {/* The Selection Filter has no switch of its own (it follows the other panes), so it has no close button either. */}
        {id !== "selectionFilter" && (
          <button
            type="button"
            className="dock-panel-close"
            aria-label={`Hide ${title}`}
            title={`Hide ${title}`}
            onClick={(e) => {
              e.stopPropagation();
              if (onClose) onClose();
              else toggleDockPane(id);
            }}
          >
            {"×"}
          </button>
        )}
      </header>
      {!folded && <div className="dock-panel-body">{children}</div>}
    </section>
  );
}

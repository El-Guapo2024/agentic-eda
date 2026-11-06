// The Design Blocks pane as an AUI pane (`DESIGN_BLOCK_PANE` in the frame's AUI manager, caption "Design Blocks", `CloseButton( true )`): the schematic's sits in a column of its
// own on the right of the sheet (`defaultDesignBlocksPaneInfo`: `Right().Layer( 3 )`), the board editor's is the outermost pane on the right (`Right().Layer( 5 )`), next to the
// Appearance dock. Either may be docked on the left instead, or float as a window of its own (`TopDockable( false )`, `BottomDockable( false )`, floatable: first at
// `FloatingPosition( 50, 200 )`, `FloatingSize( 800, 600 )`); the caption's buttons move it, a double click on the caption floats a docked pane and docks a floating one back
// where it was (`wxAuiManager::OnLeftDClick`), and a floating pane is moved by its caption and sized by its corner. View > Design Blocks (Shift+B places a block; the View menu
// entry shows or hides this) and the pane's close button show and hide it; whether it is shown, where it is and the floating window are `state/designBlocks.ts`'s.
import { useEffect, useRef } from "react";
import { useStudioState } from "../../state/store";
import { getDesignBlocksUi, setDesignBlockFloatRect, setDesignBlockPaneDock, setDesignBlockPaneShown, useDesignBlocksUi, type BlockEditor } from "../../state/designBlocks";
import { clampFloatRect, paneCaptionChoices, toggledDock, type FloatRect, type PaneDock } from "../../kicad-port/designBlocks";
import { DesignBlocksPane } from "./DesignBlocksPane";

const GLYPH: Record<PaneDock | "close", string> = { left: "⇤", right: "⇥", float: "⧉", close: "×" };

/** A rectangle kept inside the browser window as it is now. */
function onPage(r: FloatRect): FloatRect {
  return clampFloatRect(r, window.innerWidth, window.innerHeight);
}

export function DesignBlocksDock({ editor }: { editor: BlockEditor }) {
  const state = useStudioState();
  const ui = useDesignBlocksUi();
  const dock = ui.dock[editor];
  const floating = dock === "float";
  const rect = onPage(ui.floating[editor]);
  const box = useRef<HTMLDivElement>(null);

  // A floating pane sized by its corner (CSS `resize`): the size it is left at is the one it opens with next time.
  useEffect(() => {
    if (!floating || !box.current || typeof ResizeObserver === "undefined") return;
    const el = box.current;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const seen = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(() => setDesignBlockFloatRect(editor, onPage({ ...getDesignBlocksUi().floating[editor], w: el.offsetWidth, h: el.offsetHeight })), 150);
    });
    seen.observe(el);
    return () => {
      clearTimeout(timer);
      seen.disconnect();
    };
  }, [floating, editor]);

  const go = (to: PaneDock | "close") => (to === "close" ? setDesignBlockPaneShown(editor, false) : setDesignBlockPaneDock(editor, to));

  /** A floating pane follows the pointer while its caption is held (the floating frame's title bar, `wxAuiFloatingFrame`). */
  const startDrag = (e: React.PointerEvent<HTMLElement>) => {
    if (!floating || e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
    e.preventDefault();
    const from = { x: e.clientX, y: e.clientY, rect };
    const target = e.currentTarget;
    try {
      target.setPointerCapture?.(e.pointerId);
    } catch {
      // a pointer the browser does not know (a synthetic event): the moves still reach the caption
    }
    const at = (ev: PointerEvent) => onPage({ ...from.rect, x: from.rect.x + ev.clientX - from.x, y: from.rect.y + ev.clientY - from.y });
    const move = (ev: PointerEvent) => {
      const next = at(ev);
      if (box.current) {
        box.current.style.left = `${next.x}px`;
        box.current.style.top = `${next.y}px`;
      }
    };
    const up = (ev: PointerEvent) => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      setDesignBlockFloatRect(editor, at(ev));
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };

  const style: React.CSSProperties | undefined = floating ? { position: "fixed", left: rect.x, top: rect.y, width: rect.w, height: rect.h } : undefined;
  return (
    <div
      ref={box}
      className={`dock db-dock${floating ? " db-floating" : ""}`}
      data-testid="db-dock"
      data-editor={editor}
      data-dock={dock}
      style={style}
      role={floating ? "dialog" : undefined}
      aria-label={floating ? "Design Blocks" : undefined}
    >
      <header
        className="dock-panel-caption"
        onPointerDown={startDrag}
        onDoubleClick={(e) => !(e.target as HTMLElement).closest("button") && go(toggledDock(dock, ui.lastDocked[editor]))}
        title={floating ? "Drag to move; double-click to dock" : "Double-click to float"}
      >
        <span className="dock-panel-title">Design Blocks</span>
        {paneCaptionChoices(dock).map((c) => (
          <button key={c.to} type="button" className={c.to === "close" ? "dock-panel-close" : "dock-panel-close db-dock-move"} data-dock-to={c.to} aria-label={c.label} title={c.label} onClick={() => go(c.to)}>
            {GLYPH[c.to]}
          </button>
        ))}
      </header>
      <DesignBlocksPane editor={editor} hasSelection={state.selection.size > 0} />
    </div>
  );
}

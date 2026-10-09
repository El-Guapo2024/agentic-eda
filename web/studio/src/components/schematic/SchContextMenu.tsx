// The editors' right-click menu: the actions the selection tool's context menu offers for the selection (kicad-port/schContextMenu.ts decides which for the
// schematic, kicad-port/pcbContextMenu.ts for the board), drawn with the menu bar's own item rendering so labels, hotkeys and the "(not ported yet)" disabled state
// are the same as everywhere else. A long menu is moved back inside the window, and a submenu that would run off its edge opens on the other side or higher up.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { MenuNode } from "../../kicad/types";
import { MenuNodeView } from "../MenuBar";

/** The margin the menu keeps from the window's edge. */
const EDGE = 4;

/** Moves a submenu that runs off the window's right or bottom edge: to the left of its label, or up. */
function fitSubmenu(label: HTMLElement): void {
  const panel = label.querySelector<HTMLElement>(":scope > .menubar-dropdown");
  if (!panel) return;
  panel.style.left = "";
  panel.style.right = "";
  panel.style.top = "";
  const r = panel.getBoundingClientRect();
  if (r.width === 0 && r.height === 0) return;
  if (r.right > window.innerWidth - EDGE) {
    panel.style.left = "auto";
    panel.style.right = "100%";
  }
  const overflow = r.bottom - (window.innerHeight - EDGE);
  if (overflow > 0) panel.style.top = `${Math.max(-5 - overflow, EDGE - label.getBoundingClientRect().top)}px`;
}

export function SchContextMenu({ x, y, nodes, onClose }: { x: number; y: number; nodes: MenuNode[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState({ left: x, top: y });
  // Where the menu really goes: the click point, moved back inside the window when it would run off the right or bottom edge.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setAt({ left: Math.max(EDGE, Math.min(x, window.innerWidth - r.width - EDGE)), top: Math.max(EDGE, Math.min(y, window.innerHeight - r.height - EDGE)) });
  }, [x, y, nodes]);
  useEffect(() => {
    const onDocDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("pointerdown", onDocDown, true);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onDocDown, true);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);
  // The menu is a React child of the canvas, whose pointer handlers would otherwise see every press and move over the menu as one on the sheet
  // underneath (clearing the selection the menu was opened for, or re-targeting the hover), so none of them bubble out of it.
  const keep = (e: { stopPropagation: () => void }) => e.stopPropagation();
  return (
    // An item's own click handler runs first, then this one closes the menu (a submenu label is not an item and does not close it).
    <div
      ref={ref}
      className="menubar-dropdown"
      style={{ position: "fixed", left: at.left, top: at.top, minWidth: 220, zIndex: 4000 }}
      onPointerDown={keep}
      onPointerMove={keep}
      onPointerUp={keep}
      onWheel={keep}
      onMouseOver={(e) => {
        const sub = (e.target as HTMLElement).closest<HTMLElement>(".menu-node-submenu");
        if (sub) requestAnimationFrame(() => fitSubmenu(sub));
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      onClick={(e) => (e.target as HTMLElement).closest(".menu-node-item") && onClose()}
    >
      {nodes.map((n, i) => (
        <MenuNodeView key={i} node={n} />
      ))}
    </div>
  );
}

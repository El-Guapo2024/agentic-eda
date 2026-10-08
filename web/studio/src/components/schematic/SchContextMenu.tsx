// The schematic canvas's right-click menu: the actions `SCH_SELECTION_TOOL`'s context menu offers for the selection
// (kicad-port/schContextMenu.ts decides which), drawn with the menu bar's own item rendering so labels, hotkeys and the
// "(not ported yet)" disabled state are the same as everywhere else.
import { useEffect, useRef } from "react";
import type { MenuNode } from "../../kicad/types";
import { MenuNodeView } from "../MenuBar";

export function SchContextMenu({ x, y, nodes, onClose }: { x: number; y: number; nodes: MenuNode[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
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
      style={{ position: "fixed", left: x, top: y, minWidth: 220, zIndex: 4000 }}
      onPointerDown={keep}
      onPointerMove={keep}
      onPointerUp={keep}
      onWheel={keep}
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

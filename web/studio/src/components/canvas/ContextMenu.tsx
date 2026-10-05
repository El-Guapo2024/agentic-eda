// The canvas's right-click menu (KiCad's is built per-selection by
// pcb_selection_tool.cpp/edit_tool.cpp, which this session read for
// their move/selection *logic* but not their exact context-menu
// contents) and the Alt-click disambiguation list (pcb_selection_tool.cpp:
// clicking where several items overlap shows a small picker instead of
// guessing) -- both generic enough to build without a verified item
// list: this app's version offers exactly the actions it actually
// implements for the current selection, everything else the same
// "(not ported yet)" a disabled menu item gets elsewhere.
import { useEffect, useLayoutEffect, useRef, useState } from "react";

export interface MenuEntry {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
}

export function ContextMenu({ x, y, entries, onClose }: { x: number; y: number; entries: MenuEntry[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  /** Where the menu really goes: the click point, moved back inside the window when it would run off the right or bottom edge (a long menu then scrolls). */
  const [at, setAt] = useState({ left: x, top: y });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const margin = 4;
    setAt({
      left: Math.max(margin, Math.min(x, window.innerWidth - r.width - margin)),
      top: Math.max(margin, Math.min(y, window.innerHeight - r.height - margin)),
    });
  }, [x, y, entries]);

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

  return (
    // The menu is a child of the canvas container, whose own pointer-down handler closes the menu and acts on the board underneath:
    // a press on an entry must stay here, or the menu is gone before the entry's click arrives.
    <div ref={ref} className="menubar-dropdown" style={{ position: "fixed", left: at.left, top: at.top, minWidth: 180, maxHeight: "calc(100vh - 8px)", overflowY: "auto", zIndex: 4000 }} onPointerDown={(e) => e.stopPropagation()}>
      {entries.map((entry, i) => (
        <div
          key={i}
          className="menu-node-item"
          role="menuitem"
          aria-disabled={entry.disabled}
          onClick={() => {
            if (entry.disabled) return;
            entry.onSelect();
            onClose();
          }}
        >
          <span>{entry.label}</span>
        </div>
      ))}
    </div>
  );
}

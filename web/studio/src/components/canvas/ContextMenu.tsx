// The canvas's right-click menu (KiCad's is built per-selection by
// pcb_selection_tool.cpp/edit_tool.cpp, which this session read for
// their move/selection *logic* but not their exact context-menu
// contents) and the Alt-click disambiguation list (pcb_selection_tool.cpp:
// clicking where several items overlap shows a small picker instead of
// guessing) -- both generic enough to build without a verified item
// list: this app's version offers exactly the actions it actually
// implements for the current selection, everything else the same
// "(not ported yet)" a disabled menu item gets elsewhere.
import { useEffect, useRef } from "react";

export interface MenuEntry {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
}

export function ContextMenu({ x, y, entries, onClose }: { x: number; y: number; entries: MenuEntry[]; onClose: () => void }) {
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

  return (
    <div ref={ref} className="menubar-dropdown" style={{ position: "fixed", left: x, top: y, minWidth: 180, zIndex: 4000 }}>
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

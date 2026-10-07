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
  // A long menu (the Zones entries on a zone) must stay on screen: slid up or left until it fits, and scrolled when the window is too short.
  const [place, setPlace] = useState({ left: x, top: y, maxHeight: undefined as number | undefined });
  useLayoutEffect(() => {
    const box = ref.current?.getBoundingClientRect();
    if (!box) return;
    const margin = 4;
    const maxHeight = window.innerHeight - 2 * margin;
    const height = Math.min(box.height, maxHeight);
    setPlace({
      left: Math.max(margin, Math.min(x, window.innerWidth - box.width - margin)),
      top: Math.max(margin, Math.min(y, window.innerHeight - height - margin)),
      maxHeight: box.height > maxHeight ? maxHeight : undefined,
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
    <div ref={ref} className="menubar-dropdown" style={{ position: "fixed", left: place.left, top: place.top, maxHeight: place.maxHeight, overflowY: place.maxHeight ? "auto" : undefined, minWidth: 180, zIndex: 4000 }}>
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

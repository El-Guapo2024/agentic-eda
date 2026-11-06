// The canvas's right-click menu (KiCad's is built per-selection by
// pcb_selection_tool.cpp/edit_tool.cpp, which this session read for
// their move/selection *logic* but not their exact context-menu
// contents) and the Alt-click disambiguation list (pcb_selection_tool.cpp:
// clicking where several items overlap shows a small picker instead of
// guessing) -- both generic enough to build without a verified item
// list: this app's version offers exactly the actions it actually
// implements for the current selection, everything else the same
// "(not ported yet)" a disabled menu item gets elsewhere.
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { popupMenuKey } from "../../kicad-port/popupMenuKeys";

export interface MenuEntry {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
  /** A line between groups of entries (the entry's other fields are ignored). */
  separator?: boolean;
  /** Help text for the entry (KiCad's status-bar help), shown as a tooltip. */
  hint?: string;
  /** A small icon drawn before the label (the toolbar's group menus show each member's KiCad icon). */
  icon?: ReactNode;
  /** Drawn pressed, like a checked menu item (the group's current member). */
  checked?: boolean;
  /** A submenu (`ACTION_MENU::Add( submenu )`): the entry opens it to its right when the pointer is over it; `onSelect` is not called. */
  submenu?: MenuEntry[];
}

export function ContextMenu({ x, y, entries, onClose, nested = false }: { x: number; y: number; entries: MenuEntry[]; onClose: () => void; /** A submenu: its parent closes it (no listeners of its own). */ nested?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  /** The submenu open, and where (the right edge of its entry). */
  const [sub, setSub] = useState<{ index: number; x: number; y: number } | null>(null);
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
    if (nested) return;
    const onDocDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    // Escape dismisses the menu only (kicad-port/popupMenuKeys.ts): stopped here, in the capture phase, it never reaches the window's Escape, which would close
    // the dialog the menu was opened in or cancel the tool.
    const onKey = (e: KeyboardEvent) => {
      const k = popupMenuKey(e.key);
      if (k.consume) {
        e.preventDefault();
        e.stopPropagation();
      }
      if (k.close) onClose();
    };
    document.addEventListener("pointerdown", onDocDown, true);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onDocDown, true);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [onClose, nested]);

  // The menu is a React child of the canvas, whose `onPointerDown` closes the menu (and clears or re-targets the selection) -- so a press on an entry used to
  // unmount the menu before its `click` could fire, and the entry never ran. Presses and releases inside the menu stay inside it.
  const keep = (e: { stopPropagation: () => void }) => e.stopPropagation();
  return (
    <div ref={ref} className="menubar-dropdown" style={{ position: "fixed", left: at.left, top: at.top, minWidth: 180, maxHeight: "calc(100vh - 8px)", overflowY: "auto", zIndex: 4000 }} onPointerDown={keep} onPointerUp={keep}>
      {entries.map((entry, i) =>
        entry.separator ? (
          <div key={i} className="menu-separator" role="separator" />
        ) : (
        <div
          key={i}
          className="menu-node-item"
          role="menuitem"
          title={entry.hint}
          aria-disabled={entry.disabled}
          onMouseEnter={(e) => {
            if (entry.submenu && !entry.disabled) {
              const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
              setSub({ index: i, x: r.right, y: r.top });
            } else setSub(null);
          }}
          onClick={(e) => {
            if (entry.disabled) return;
            if (entry.submenu) {
              const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
              setSub({ index: i, x: r.right, y: r.top });
              return;
            }
            entry.onSelect();
            onClose();
          }}
        >
          {entry.icon || entry.checked ? (
            <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
              {entry.icon && <span style={{ display: "inline-flex", width: 18, height: 18, alignItems: "center", justifyContent: "center" }}>{entry.icon}</span>}
              <span style={entry.checked ? { fontWeight: 600 } : undefined}>{entry.label}</span>
            </span>
          ) : (
            <span>{entry.label}</span>
          )}
          {entry.submenu && <span style={{ float: "right", marginLeft: 12 }}>{"\u25B8"}</span>}
        </div>
        )
      )}
      {/* Inside this menu's element, so a press in the submenu is not a press outside the menu. */}
      {sub && entries[sub.index]?.submenu && <ContextMenu x={sub.x} y={sub.y} entries={entries[sub.index]!.submenu!} onClose={onClose} nested />}
    </div>
  );
}

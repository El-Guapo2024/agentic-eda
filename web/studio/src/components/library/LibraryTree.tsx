// The library tree of the two library editors (`LIB_TREE` in `SYMBOL_EDIT_FRAME` / `FOOTPRINT_EDIT_FRAME`'s tree
// pane): libraries as groups, their symbols / footprints below, a search box, a selection that the library actions work on
// (`GetTargetLibId` / `GetTargetFPID`) and the context menu `SYMBOL_EDITOR_CONTROL::Init` / `FOOTPRINT_EDITOR_CONTROL::Init`
// build. Generic over what the items are: the two panels (`SymbolLibraryPanel`, `FootprintLibraryPanel`) feed it names and
// the entries of the menu.
import { useMemo, useRef, useState } from "react";
import { libraryGroups, splitLibName } from "../../kicad-port/libraryNames";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";

export interface TreeItem {
  /** The library key: `Lib:Name`, or a bare name. */
  name: string;
  /** In the project library (editable in place); otherwise it comes from a library file or the built-in table. */
  project: boolean;
}

export interface LibraryTreeProps {
  title: string;
  items: readonly TreeItem[];
  selected: readonly string[];
  /** The item open in the editor (drawn bold). */
  current: string | null;
  multi?: boolean;
  onSelect: (names: string[]) => void;
  /** Double click / Enter: `EditSymbol` / `EditFootprint`. */
  onOpen: (name: string) => void;
  /** The context menu for the current selection (empty = no menu). */
  menu: (selected: string[]) => MenuEntry[];
}

export function LibraryTree({ title, items, selected, current, multi, onSelect, onOpen, menu }: LibraryTreeProps) {
  const [filter, setFilter] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [ctx, setCtx] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const anchor = useRef<string | null>(null);

  const q = filter.trim().toLowerCase();
  const visible = useMemo(() => items.filter((it) => !q || it.name.toLowerCase().includes(q)), [items, q]);
  const groups = useMemo(() => libraryGroups(visible), [visible]);
  const flat = useMemo(() => groups.flatMap((g) => (collapsed.has(g.lib) && !q ? [] : g.items.map((i) => i.name))), [groups, collapsed, q]);
  const projectOf = useMemo(() => new Map(items.map((i) => [i.name, i.project])), [items]);

  const click = (name: string, e: React.MouseEvent) => {
    const toggle = multi && (e.metaKey || e.ctrlKey);
    if (multi && e.shiftKey && anchor.current && flat.includes(anchor.current)) {
      const a = flat.indexOf(anchor.current);
      const b = flat.indexOf(name);
      onSelect(flat.slice(Math.min(a, b), Math.max(a, b) + 1));
      return;
    }
    anchor.current = name;
    if (toggle) onSelect(selected.includes(name) ? selected.filter((n) => n !== name) : [...selected, name]);
    else onSelect([name]);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const at = flat.indexOf(selected[selected.length - 1] ?? "");
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = flat[Math.max(0, Math.min(flat.length - 1, at + (e.key === "ArrowDown" ? 1 : -1)))];
      if (next) {
        anchor.current = next;
        onSelect([next]);
      }
    } else if (e.key === "Enter" && selected.length === 1) {
      onOpen(selected[0]!);
    }
  };

  const onContextMenu = (e: React.MouseEvent, name: string | null) => {
    e.preventDefault();
    e.stopPropagation();
    let sel = [...selected];
    if (name && !sel.includes(name)) {
      sel = [name];
      onSelect(sel);
    }
    const entries = menu(sel);
    if (entries.length > 0) setCtx({ x: e.clientX, y: e.clientY, entries });
  };

  return (
    <div
      className="library-tree"
      role="tree"
      aria-label={title}
      tabIndex={0}
      onKeyDown={onKeyDown}
      onContextMenu={(e) => onContextMenu(e, null)}
      style={{ width: 236, flex: "0 0 236px", display: "flex", flexDirection: "column", borderRight: "1px solid var(--chrome-border)", background: "var(--chrome-bg-raised)", minHeight: 0, outline: "none" }}
    >
      <div style={{ padding: "6px 8px", borderBottom: "1px solid var(--chrome-border)", display: "flex", gap: 6, alignItems: "center" }}>
        <strong style={{ fontSize: 12 }}>{title}</strong>
        <span style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>{items.length}</span>
      </div>
      <div style={{ padding: "6px 8px" }}>
        <input value={filter} placeholder="Search" onChange={(e) => setFilter(e.target.value)} style={{ width: "100%", boxSizing: "border-box" }} aria-label={`Search ${title}`} />
      </div>
      <div style={{ overflowY: "auto", flex: 1, minHeight: 0, paddingBottom: 8 }}>
        {groups.length === 0 && <div style={{ padding: "8px 10px", color: "var(--chrome-text-dim)", fontSize: 12 }}>{items.length === 0 ? "Nothing in the library yet." : "No match."}</div>}
        {groups.map((g) => {
          const open = q ? true : !collapsed.has(g.lib);
          return (
            <div key={g.lib}>
              <div
                role="treeitem"
                aria-expanded={open}
                className="library-tree-group"
                style={{ padding: "3px 8px", fontWeight: 600, fontSize: 12, cursor: "pointer", userSelect: "none", display: "flex", gap: 4 }}
                onClick={() => setCollapsed((c) => { const n = new Set(c); if (n.has(g.lib)) n.delete(g.lib); else n.add(g.lib); return n; })}
              >
                <span style={{ width: 10 }}>{open ? "▾" : "▸"}</span>
                <span>{g.lib}</span>
                <span style={{ color: "var(--chrome-text-dim)", fontWeight: 400 }}>{g.items.length}</span>
              </div>
              {open &&
                g.items.map((it) => {
                  const isSel = selected.includes(it.name);
                  const project = projectOf.get(it.name) ?? false;
                  return (
                    <div
                      key={it.name}
                      role="treeitem"
                      aria-selected={isSel}
                      data-name={it.name}
                      title={project ? it.name : `${it.name} -- from a library file or the built-in table; opening it makes an editable copy in the project`}
                      onClick={(e) => click(it.name, e)}
                      onDoubleClick={() => onOpen(it.name)}
                      onContextMenu={(e) => onContextMenu(e, it.name)}
                      style={{
                        padding: "2px 8px 2px 24px",
                        fontSize: 12,
                        cursor: "pointer",
                        userSelect: "none",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        fontWeight: it.name === current ? 700 : 400,
                        fontStyle: project ? "normal" : "italic",
                        color: isSel ? "var(--chrome-selected-text)" : project ? "var(--chrome-text)" : "var(--chrome-text-dim)",
                        background: isSel ? "var(--chrome-selected-bg)" : "transparent",
                      }}
                    >
                      {splitLibName(it.name).item}
                    </div>
                  );
                })}
            </div>
          );
        })}
      </div>
      {ctx && <ContextMenu x={ctx.x} y={ctx.y} entries={ctx.entries} onClose={() => setCtx(null)} />}
    </div>
  );
}

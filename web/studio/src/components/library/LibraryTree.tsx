// The library tree of the two library editors (`LIB_TREE` in `SYMBOL_EDIT_FRAME` / `FOOTPRINT_EDIT_FRAME`'s tree
// pane): libraries as groups, their symbols / footprints below, a search box, a selection that the library actions work on
// (`GetTargetLibId` / `GetTargetFPID`) and the context menu `SYMBOL_EDITOR_CONTROL::Init` / `FOOTPRINT_EDITOR_CONTROL::Init`
// build. Generic over what the items are: the two panels (`SymbolLibraryPanel`, `FootprintLibraryPanel`) feed it names and
// the entries of the menu.
//
// The libraries are the project's own plus KiCad's installed ones (155 footprint and 223 symbol libraries): every installed library is a
// collapsed group from the start, and its items are fetched when the group opens (`onLoadLibrary`), so a tree of thousands of footprints
// costs one request per library the person actually looks into. The search box filters across every library; the first time it is used it
// asks for the whole index (`onSearch`). The merge, ordering and search rules are kicad-port/libraryTreeModel.ts's.
import { useEffect, useMemo, useRef, useState } from "react";
import { splitLibName } from "../../kicad-port/libraryNames";
import { buildTreeGroups, defaultGroupOpen, needsLoading, type InstalledLib, type TreeItem } from "../../kicad-port/libraryTreeModel";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";

export type { TreeItem };

export interface LibraryTreeProps {
  title: string;
  /** The names the editor already lists (the project's own entries, what the board's model resolves, the built-in table). */
  items: readonly TreeItem[];
  /** KiCad's installed libraries, and the item names of the ones loaded so far. */
  installed?: readonly InstalledLib[];
  loaded?: ReadonlyMap<string, readonly string[]>;
  loading?: ReadonlySet<string>;
  /** An installed library's group opened: fetch its items. */
  onLoadLibrary?: (lib: string) => void;
  /** The search box has text: make sure every library's names are available. */
  onSearch?: () => void;
  searchIndex?: "idle" | "loading" | "ready";
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

const NO_LIBS: readonly InstalledLib[] = [];
const NOTHING_LOADED: ReadonlyMap<string, readonly string[]> = new Map();
const NOT_LOADING: ReadonlySet<string> = new Set();

export function LibraryTree({ title, items, installed = NO_LIBS, loaded = NOTHING_LOADED, loading = NOT_LOADING, onLoadLibrary, onSearch, searchIndex = "idle", selected, current, multi, onSelect, onOpen, menu }: LibraryTreeProps) {
  const [filter, setFilter] = useState("");
  /** The groups the person opened or closed by hand; every other group follows `defaultGroupOpen`. */
  const [userOpen, setUserOpen] = useState<Map<string, boolean>>(new Map());
  const [ctx, setCtx] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const anchor = useRef<string | null>(null);

  const q = filter.trim();
  const { groups, truncated } = useMemo(() => buildTreeGroups(items, installed, loaded, q), [items, installed, loaded, q]);
  const isOpen = (g: { lib: string }, def: boolean) => (q ? true : (userOpen.get(g.lib) ?? def));
  const open = (g: (typeof groups)[number]) => isOpen(g, defaultGroupOpen(g));
  const flat = useMemo(() => groups.flatMap((g) => (isOpen(g, defaultGroupOpen(g)) ? g.items.map((i) => i.name) : [])), [groups, userOpen, q]); // eslint-disable-line react-hooks/exhaustive-deps
  const projectOf = useMemo(() => new Map(items.map((i) => [i.name, i.project])), [items]);

  // An open group of an installed library gets its items (the groups KiCad has libraries for open by default only when the editor already lists names in them).
  useEffect(() => {
    if (!onLoadLibrary) return;
    for (const g of groups) if (needsLoading(g, isOpen(g, defaultGroupOpen(g)), loaded)) onLoadLibrary(g.lib);
  }, [groups, userOpen, loaded, onLoadLibrary]); // eslint-disable-line react-hooks/exhaustive-deps

  // The search box asks for every library's names the first time it has text (KiCad's tree searches all of them).
  useEffect(() => {
    if (q && onSearch) onSearch();
  }, [q, onSearch]);

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

  const nothing = items.length === 0 && installed.length === 0;
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
        <span style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>{groups.length}</span>
      </div>
      <div style={{ padding: "6px 8px" }}>
        <input value={filter} placeholder="Search" onChange={(e) => setFilter(e.target.value)} style={{ width: "100%", boxSizing: "border-box" }} aria-label={`Search ${title}`} />
      </div>
      <div style={{ overflowY: "auto", flex: 1, minHeight: 0, paddingBottom: 8 }}>
        {q && searchIndex === "loading" && <div style={{ padding: "2px 10px 6px", color: "var(--chrome-text-dim)", fontSize: 11 }}>Searching every installed library…</div>}
        {groups.length === 0 && <div style={{ padding: "8px 10px", color: "var(--chrome-text-dim)", fontSize: 12 }}>{nothing ? "Nothing in the library yet." : "No match."}</div>}
        {groups.map((g) => {
          const isGroupOpen = open(g);
          const isLoading = loading.has(g.lib);
          const count = g.installed && loaded.has(g.lib) ? Math.max(g.items.length, loaded.get(g.lib)!.length) : (g.count ?? g.items.length);
          return (
            <div key={g.lib}>
              <div
                role="treeitem"
                aria-expanded={isGroupOpen}
                data-library={g.lib}
                className="library-tree-group"
                style={{ padding: "3px 8px", fontWeight: 600, fontSize: 12, cursor: "pointer", userSelect: "none", display: "flex", gap: 4 }}
                onClick={() =>
                  setUserOpen((m) => {
                    const n = new Map(m);
                    n.set(g.lib, !(m.get(g.lib) ?? defaultGroupOpen(g)));
                    return n;
                  })
                }
              >
                <span style={{ width: 10 }}>{isGroupOpen ? "▾" : "▸"}</span>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{g.lib}</span>
                <span style={{ color: "var(--chrome-text-dim)", fontWeight: 400 }}>{isLoading ? "…" : count}</span>
              </div>
              {isGroupOpen &&
                g.items.map((it) => {
                  const isSel = selected.includes(it.name);
                  const project = projectOf.get(it.name) ?? false;
                  return (
                    <div
                      key={it.name}
                      role="treeitem"
                      aria-selected={isSel}
                      data-name={it.name}
                      title={project ? it.name : `${it.name} -- from an installed library or the built-in table; opening it makes an editable copy in the project`}
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
        {truncated && <div style={{ padding: "6px 10px", color: "var(--chrome-text-dim)", fontSize: 11 }}>Showing the first matches only -- type more to narrow the search.</div>}
      </div>
      {ctx && <ContextMenu x={ctx.x} y={ctx.y} entries={ctx.entries} onClose={() => setCtx(null)} />}
    </div>
  );
}

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
//
// What the tree remembers -- which libraries are open, which are pinned, which library rows are selected -- lives in `state/libraryTree.ts`, so the
// shared actions (Expand All, Collapse All, Pin Library, Unpin Library) and the tree act on the same state. A library row is selectable like a
// symbol row (`LIB_TREE_NODE::TYPE::LIBRARY`); a pinned library is listed first with a star in front of its name. Whether the tree is shown is the
// dock layout's (the Libraries column folds to its handle).
import { useEffect, useMemo, useRef, useState } from "react";
import { splitLibName } from "../../kicad-port/libraryNames";
import { buildTreeGroups, defaultGroupOpen, needsLoading, type InstalledLib, type TreeGroup, type TreeItem } from "../../kicad-port/libraryTreeModel";
import { isGroupOpen, libraryLabel, pinnedFirst } from "../../kicad-port/libraryTreeState";
import { collapseAllLibraries, expandAllLibraries, selectLibraries, setLibraryOpen, useLibraryTree, type TreeKind } from "../../state/libraryTree";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";

export type { TreeItem };

export interface LibraryTreeProps {
  /** Which editor's tree this is (the state it shares with the shared actions). */
  kind: TreeKind;
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
  /** The context menu for the current selection of items and of library rows (empty = no menu). */
  menu: (selected: string[], libs: string[]) => MenuEntry[];
}

const NO_LIBS: readonly InstalledLib[] = [];
const NOTHING_LOADED: ReadonlyMap<string, readonly string[]> = new Map();
const NOT_LOADING: ReadonlySet<string> = new Set();

export function LibraryTree({ kind, title, items, installed = NO_LIBS, loaded = NOTHING_LOADED, loading = NOT_LOADING, onLoadLibrary, onSearch, searchIndex = "idle", selected, current, multi, onSelect, onOpen, menu }: LibraryTreeProps) {
  const ui = useLibraryTree(kind);
  const [filter, setFilter] = useState("");
  const [ctx, setCtx] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const anchor = useRef<string | null>(null);

  const q = filter.trim();
  const built = useMemo(() => buildTreeGroups(items, installed, loaded, q), [items, installed, loaded, q]);
  const { truncated } = built;
  const groups = useMemo(() => pinnedFirst(built.groups, ui.pinned), [built.groups, ui.pinned]);
  /** Whether a group is open: a search opens them all (the matches are what it is for); else the fold decides. */
  const isOpen = (g: TreeGroup) => (q ? true : isGroupOpen(ui.fold, g.lib, { hasItems: g.items.length > 0, defaultOpen: defaultGroupOpen(g) }));
  const flat = useMemo(() => groups.flatMap((g) => (isOpen(g) ? g.items.map((i) => i.name) : [])), [groups, ui.fold, q]); // eslint-disable-line react-hooks/exhaustive-deps
  const projectOf = useMemo(() => new Map(items.map((i) => [i.name, i.project])), [items]);

  // An open group of an installed library gets its items (the groups KiCad has libraries for open by default only when the editor already lists names in them).
  useEffect(() => {
    if (!onLoadLibrary) return;
    for (const g of groups) if (needsLoading(g, isOpen(g), loaded)) onLoadLibrary(g.lib);
  }, [groups, ui.fold, loaded, onLoadLibrary]); // eslint-disable-line react-hooks/exhaustive-deps

  // The search box asks for every library's names the first time it has text (KiCad's tree searches all of them).
  useEffect(() => {
    if (q && onSearch) onSearch();
  }, [q, onSearch]);

  const click = (name: string, e: React.MouseEvent) => {
    const toggle = multi && (e.metaKey || e.ctrlKey);
    selectLibraries(kind, []);
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

  /** A library row: it opens or folds, and is the selection (a library and symbols are not selected together). */
  const clickLibrary = (g: TreeGroup) => {
    setLibraryOpen(kind, g.lib, !isOpen(g));
    selectLibraries(kind, [g.lib]);
    if (selected.length > 0) onSelect([]);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const at = flat.indexOf(selected[selected.length - 1] ?? "");
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = flat[Math.max(0, Math.min(flat.length - 1, at + (e.key === "ArrowDown" ? 1 : -1)))];
      if (next) {
        anchor.current = next;
        selectLibraries(kind, []);
        onSelect([next]);
      }
    } else if (e.key === "Enter" && selected.length === 1) {
      onOpen(selected[0]!);
    }
  };

  const onContextMenu = (e: React.MouseEvent, name: string | null, lib: string | null = null) => {
    e.preventDefault();
    e.stopPropagation();
    let sel = [...selected];
    let libs = [...ui.selectedLibs];
    if (name && !sel.includes(name)) {
      sel = [name];
      libs = [];
      selectLibraries(kind, []);
      onSelect(sel);
    } else if (lib && !libs.includes(lib)) {
      libs = [lib];
      sel = [];
      selectLibraries(kind, libs);
      if (selected.length > 0) onSelect([]);
    }
    const entries = menu(sel, libs);
    if (entries.length > 0) setCtx({ x: e.clientX, y: e.clientY, entries });
  };

  // `LIB_TREE`'s configuration button beside the search box: its menu has Expand All and Collapse All (the sort modes are not ported).
  const openConfigMenu = (e: React.MouseEvent<HTMLButtonElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    setCtx({
      x: r.left,
      y: r.bottom,
      entries: [
        { label: "Expand All", onSelect: () => expandAllLibraries(kind) },
        { label: "Collapse All", onSelect: () => collapseAllLibraries(kind) },
      ],
    });
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
      <div style={{ padding: "6px 8px", display: "flex", gap: 4 }}>
        <input
          value={filter}
          placeholder="Search"
          data-library-tree-search={kind}
          onChange={(e) => setFilter(e.target.value)}
          style={{ flex: 1, minWidth: 0, boxSizing: "border-box" }}
          aria-label={`Search ${title}`}
        />
        <button onClick={openConfigMenu} title="Expand All, Collapse All" aria-label="Library tree options" style={{ padding: "0 6px" }}>
          {"⋯"}
        </button>
      </div>
      <div style={{ overflowY: "auto", flex: 1, minHeight: 0, paddingBottom: 8 }}>
        {q && searchIndex === "loading" && <div style={{ padding: "2px 10px 6px", color: "var(--chrome-text-dim)", fontSize: 11 }}>Searching every installed library…</div>}
        {groups.length === 0 && <div style={{ padding: "8px 10px", color: "var(--chrome-text-dim)", fontSize: 12 }}>{nothing ? "Nothing in the library yet." : "No match."}</div>}
        {groups.map((g) => {
          const isGroupOpenNow = isOpen(g);
          const isLoading = loading.has(g.lib);
          const count = g.installed && loaded.has(g.lib) ? Math.max(g.items.length, loaded.get(g.lib)!.length) : (g.count ?? g.items.length);
          const libSelected = ui.selectedLibs.includes(g.lib);
          return (
            <div key={g.lib}>
              <div
                role="treeitem"
                aria-expanded={isGroupOpenNow}
                aria-selected={libSelected}
                data-library={g.lib}
                className="library-tree-group"
                style={{
                  padding: "3px 8px",
                  fontWeight: 600,
                  fontSize: 12,
                  cursor: "pointer",
                  userSelect: "none",
                  display: "flex",
                  gap: 4,
                  color: libSelected ? "var(--chrome-selected-text)" : undefined,
                  background: libSelected ? "var(--chrome-selected-bg)" : "transparent",
                }}
                onClick={() => clickLibrary(g)}
                onContextMenu={(e) => onContextMenu(e, null, g.lib)}
              >
                <span style={{ width: 10 }}>{isGroupOpenNow ? "▾" : "▸"}</span>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{libraryLabel(g.lib, ui.pinned)}</span>
                <span style={{ color: libSelected ? "inherit" : "var(--chrome-text-dim)", fontWeight: 400 }}>{isLoading ? "…" : count}</span>
              </div>
              {isGroupOpenNow &&
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

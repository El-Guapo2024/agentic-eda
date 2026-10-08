// The library tree of the two library editors (`LIB_TREE` in `SYMBOL_EDIT_FRAME` / `FOOTPRINT_EDIT_FRAME`'s tree
// pane): libraries as groups, their symbols / footprints below, a search box, a selection that the library actions work on
// (`GetTargetLibId` / `GetTargetFPID`) and the context menu `SYMBOL_EDITOR_CONTROL::Init` / `FOOTPRINT_EDITOR_CONTROL::Init`
// build. Generic over what the items are: the two panels (`SymbolLibraryPanel`, `FootprintLibraryPanel`) feed it names and
// the entries of the menu.
//
// What the tree remembers -- shown or hidden, which libraries are folded, which are pinned, which library rows are selected -- lives in
// `state/libraryTree.ts`, so the shared actions (Library Tree, Hide Library Tree, Expand All, Collapse All, Pin Library, Unpin Library)
// and the tree act on the same state. A library row is selectable like a symbol row (`LIB_TREE_NODE::TYPE::LIBRARY`); a pinned library
// is listed first with a star in front of its name.
import { useMemo, useRef, useState } from "react";
import { libraryGroups, splitLibName } from "../../kicad-port/libraryNames";
import { isFolded, libraryLabel, pinnedFirst } from "../../kicad-port/libraryTreeState";
import { collapseAllLibraries, expandAllLibraries, selectLibraries, toggleLibraryFold, useLibraryTree, type TreeKind } from "../../state/libraryTree";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";

export interface TreeItem {
  /** The library key: `Lib:Name`, or a bare name. */
  name: string;
  /** In the project library (editable in place); otherwise it comes from a library file or the built-in table. */
  project: boolean;
}

export interface LibraryTreeProps {
  /** Which editor's tree this is (the state it shares with the shared actions). */
  kind: TreeKind;
  title: string;
  items: readonly TreeItem[];
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

export function LibraryTree({ kind, title, items, selected, current, multi, onSelect, onOpen, menu }: LibraryTreeProps) {
  const ui = useLibraryTree(kind);
  const [filter, setFilter] = useState("");
  const [ctx, setCtx] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const anchor = useRef<string | null>(null);

  const q = filter.trim().toLowerCase();
  const visible = useMemo(() => items.filter((it) => !q || it.name.toLowerCase().includes(q)), [items, q]);
  const groups = useMemo(() => pinnedFirst(libraryGroups(visible), ui.pinned), [visible, ui.pinned]);
  const flat = useMemo(() => groups.flatMap((g) => (isFolded(ui.fold, g.lib) && !q ? [] : g.items.map((i) => i.name))), [groups, ui.fold, q]);
  const projectOf = useMemo(() => new Map(items.map((i) => [i.name, i.project])), [items]);

  // `ToggleLibraryTree` hid the pane: nothing is drawn, the state stays.
  if (!ui.shown) return null;

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
  const clickLibrary = (lib: string) => {
    toggleLibraryFold(kind, lib);
    selectLibraries(kind, [lib]);
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
        {groups.length === 0 && <div style={{ padding: "8px 10px", color: "var(--chrome-text-dim)", fontSize: 12 }}>{items.length === 0 ? "Nothing in the library yet." : "No match."}</div>}
        {groups.map((g) => {
          const open = q ? true : !isFolded(ui.fold, g.lib);
          const libSelected = ui.selectedLibs.includes(g.lib);
          return (
            <div key={g.lib}>
              <div
                role="treeitem"
                aria-expanded={open}
                aria-selected={libSelected}
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
                onClick={() => clickLibrary(g.lib)}
                onContextMenu={(e) => onContextMenu(e, null, g.lib)}
              >
                <span style={{ width: 10 }}>{open ? "▾" : "▸"}</span>
                <span>{libraryLabel(g.lib, ui.pinned)}</span>
                <span style={{ color: libSelected ? "inherit" : "var(--chrome-text-dim)", fontWeight: 400 }}>{g.items.length}</span>
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

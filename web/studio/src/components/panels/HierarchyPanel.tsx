// The Hierarchy Navigator (docked left, below Properties, per the coordinator's reading of eeschema's default AUI layout in sch_edit_frame.cpp) --
// the tree of the design's sheets (GAPS.md #6): the root, then every sheet placed on it, then the sheets those place, as
// `HIERARCHY_PANE::buildHierarchyTree` builds it: each sheet's children sorted by page (`SortChildren`, `ComparePageNum`; `hierarchyTreeOrder`),
// every sheet labelled with its page (`formatPageString`: KiCad's "name (page N)"; here the name, then the file and the page). The list is
// `GET /api/sch/hierarchy` (the same one Next / Previous Sheet and Edit Sheet Page Number read) and it is read again whenever the design
// changes, so a Reorganize into Module Sheets, an undo, a new sheet or a page number shows at once.
//
// A click opens the sheet: it runs `eeschema.NavigateTool.changeSheet` with the sheet's path (`SCH_NAVIGATE_TOOL::ChangeSheet`, which the Hierarchy Navigator's
// tree raises in KiCad), so Back / Forward / Leave Sheet keep their history. The sheet in view is highlighted.
//
// r6 C6-L2: the root row is the project's name (`getRootString`), a sheet with no name shows its file's name (the `sheetNameBase.IsEmpty()`
// fallback), a row with sheets under it folds and unfolds. The folded rows are kept per board (KiCad's `m_SchHierarchyCollapsed`, the project's local
// settings).
//
// UI-LEFT S8 (`HIERARCHY_PANE::onRightClick`, `onTreeEditFinished`, `UpdateNetHighlight`, `UpdateHierarchyTree`): a row's right-click menu has "Edit Page
// Number" (Edit Sheet Page Number on that sheet path), "Rename" (the row's name edited in place: Enter or leaving the box renames the sheet -- its Sheetname --
// on the screen that draws it, Esc keeps it), "Delete Top-Level Sheet" (a top-level sheet: "Delete top-level sheet '%s'?\n\nThis cannot be undone.", then
// "Cannot delete the last top-level sheet." for the last one), "New Top-Level Sheet" ("Enter name for new top-level sheet:", "Untitled" to start with), and
// "Expand All" / "Collapse All". A project of several top-level sheets (`virtual_root`) shows each as a root of its own, in the project's order. The sheets that
// hold the highlighted net (Highlight Net) are written in `LAYER_BRIGHTENED`.
//
// **Differs**: a design whose root is its only top-level sheet keeps it so -- New Top-Level Sheet is offered disabled there, and its root row's page is the
// root's 1 (the model stores no page for it); the rows read "name file · page N" (A7-L3); a deleted top-level sheet comes back with Undo (KiCad: "This
// cannot be undone.").
import { useEffect, useState } from "react";
import { useActionRunner } from "../../actions/useActionRunner";
import { fetchHierarchyTree, type HierarchyEntry } from "../../api/schControlClient";
import type { Cmd } from "../../api/types";
import { effectivePage, hasChildSheets, hierarchyPathKey, hierarchyRowName, hierarchyTreeOrder, samePath, topLevelTreeOrder, visibleHierarchyRows } from "../../kicad-port/sheetPages";
import { useStudioApi, useStudioState } from "../../state/store";
import { useSchControlDispatch } from "../../state/schControlStore";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { layerColor } from "../canvas/layers";
import { SchDialogShell } from "../SchDialogShell";

const collapsedStoreKey = (board: string) => `eda.hierarchy.collapsed.${board}`;
function readCollapsed(board: string): Set<string> {
  try {
    const raw = window.localStorage.getItem(collapsedStoreKey(board));
    return new Set(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
}
function writeCollapsed(board: string, keys: ReadonlySet<string>) {
  try {
    window.localStorage.setItem(collapsedStoreKey(board), JSON.stringify([...keys]));
  } catch {
    // a private window: the fold is kept for this session only
  }
}

type Row = HierarchyEntry & { net?: boolean };

/** A command for the screen that draws the sheet at `path` (its parent's), whatever sheet is in view. */
const onParentOf = (path: readonly string[], cmd: Record<string, unknown>): Cmd => ({ op: "on_sheet", sheet: path.slice(0, -1).join("/"), cmd }) as unknown as Cmd;

/** The rows that are top-level sheets: a virtual root's sheets, else the one root. */
function topLevelRows(tree: readonly HierarchyEntry[], virtualRoot: boolean): HierarchyEntry[] {
  return tree.filter((s) => (virtualRoot ? s.path.length === 1 : s.path.length === 0));
}

export function HierarchyPanel() {
  const state = useStudioState();
  const api = useStudioApi();
  const controlDispatch = useSchControlDispatch();
  const { run } = useActionRunner();
  const [sheets, setSheets] = useState<Row[]>([]);
  const [virtualRoot, setVirtualRoot] = useState(false);
  const name = state.board?.name || "schematic";
  const here = state.currentSheetPath;
  const [collapsed, setCollapsedState] = useState<Set<string>>(() => readCollapsed(name));
  const [menu, setMenu] = useState<{ x: number; y: number; row: Row | null } | null>(null);
  const [renaming, setRenaming] = useState<{ path: string[]; text: string } | null>(null);
  const [newTop, setNewTop] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<Row | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const setCollapsed = (next: Set<string>) => {
    setCollapsedState(next);
    writeCollapsed(name, next);
  };
  const net = state.tab === "schematic" ? state.netHighlight : null;

  useEffect(() => {
    let live = true;
    fetchHierarchyTree(net).then(
      (t) => {
        if (!live) return;
        setSheets(t.sheets);
        setVirtualRoot(t.virtualRoot);
      },
      () => live && setSheets([])
    );
    return () => {
      live = false;
    };
  }, [state.version, net]);

  const open = (path: string[]) => run("eeschema.NavigateTool.changeSheet", path);
  // The root first even when the list has not arrived yet, so the panel is never empty.
  const tree: Row[] = sheets.length > 0 ? (virtualRoot ? topLevelTreeOrder(sheets) : hierarchyTreeOrder(sheets)) : [{ path: [], name: "", file: "", page: "1" }];
  const rows = visibleHierarchyRows(tree, collapsed);
  const pageOf = (s: HierarchyEntry) => effectivePage(s, Math.max(0, sheets.indexOf(s)));
  const parents = tree.filter((s) => hasChildSheets(tree, s.path));
  const depthOf = (s: HierarchyEntry) => (virtualRoot ? s.path.length - 1 : s.path.length);
  const labelOf = (s: HierarchyEntry) => hierarchyRowName(s, state.board?.name ?? "");
  const topLevel = (s: HierarchyEntry) => (virtualRoot ? s.path.length === 1 : s.path.length === 0);

  const commitRename = async () => {
    const r = renaming;
    setRenaming(null);
    if (!r) return;
    const row = sheets.find((s) => samePath(s.path, r.path));
    const text = r.text.trim();
    // `onTreeEditFinished`: an empty label or the same name changes nothing
    if (!row || text === "" || text === row.name) return;
    const id = r.path[r.path.length - 1]!;
    await api.cmd(onParentOf(r.path, { op: "sch_edit", verb: "edit_sheet", id, name: text }));
  };

  const menuEntries = (row: Row | null): MenuEntry[] => {
    const out: MenuEntry[] = [];
    if (row) {
      const isRoot = row.path.length === 0;
      out.push({
        label: "Edit Page Number",
        disabled: isRoot,
        hint: isRoot ? "The root sheet is page 1: its page number is not stored, only the sub-sheets' are." : undefined,
        onSelect: () => controlDispatch({ type: "OPEN_DIALOG", dialog: { kind: "page_number", path: row.path } }),
      });
      if (!isRoot) out.push({ label: "Rename", hint: "Change name of this sheet", onSelect: () => setRenaming({ path: row.path, text: row.name }) });
      if (topLevel(row)) out.push({ label: "Delete Top-Level Sheet", hint: "Remove this top-level sheet from the project", onSelect: () => setDeleting(row) });
      out.push({ label: "", separator: true, onSelect: () => undefined });
    }
    out.push({
      label: "New Top-Level Sheet",
      hint: virtualRoot ? "Create a new top-level sheet" : "This design's root sheet is its only top-level sheet: add a sheet to it with Draw Sheet",
      disabled: !virtualRoot,
      onSelect: () => setNewTop("Untitled"),
    });
    out.push({ label: "", separator: true, onSelect: () => undefined });
    out.push({ label: "Expand All", onSelect: () => setCollapsed(new Set()) });
    out.push({ label: "Collapse All", onSelect: () => setCollapsed(new Set(parents.map((s) => hierarchyPathKey(s.path)))) });
    return out;
  };

  const deleteTopLevel = async (row: Row) => {
    setDeleting(null);
    // `RemoveTopLevelSheet` refuses the last one (said after the question, as KiCad says it)
    if (!virtualRoot || topLevelRows(tree, virtualRoot).length <= 1) return setMessage("Cannot delete the last top-level sheet.");
    const id = row.path[0]!;
    // the sheet in view goes with it: the first top-level sheet left is shown (`RemoveTopLevelSheet`: "m_currentSheet = [m_topLevelSheets[0]]")
    if (here[0] === id) {
      const next = topLevelRows(tree, virtualRoot).find((r) => r.path[0] !== id);
      if (next) await open(next.path);
    }
    await api.cmd({ op: "on_sheet", sheet: "", cmd: { op: "sch_edit", verb: "delete_top_level_sheet", id } } as unknown as Cmd);
  };

  return (
    <div
      className="panel-section"
      id="hierarchy-panel"
      tabIndex={-1}
      onContextMenu={(e) => {
        e.preventDefault();
        // a right-click in empty space takes the tree's selection: the sheet in view (`onRightClick( wxTreeItemId() )`)
        setMenu({ x: e.clientX, y: e.clientY, row: tree.find((s) => samePath(s.path, here)) ?? null });
      }}
    >
      {menu && <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)} entries={menuEntries(menu.row)} />}
      {rows.map((s) => {
        const current = samePath(s.path, here);
        const isRoot = s.path.length === 0;
        const label = labelOf(s);
        const file = isRoot ? `${name}.kicad_sch` : s.file;
        const key = hierarchyPathKey(s.path);
        const folds = hasChildSheets(tree, s.path);
        const folded = collapsed.has(key);
        const editing = renaming !== null && samePath(renaming.path, s.path);
        return (
          <div
            key={s.path.join("/")}
            role="button"
            tabIndex={0}
            data-testid="hierarchy-row"
            data-path={s.path.join("/")}
            aria-current={current ? "true" : undefined}
            className={`unplaced-row${current ? " armed" : ""}`}
            style={{ marginLeft: depthOf(s) * 10, cursor: current ? "default" : "pointer" }}
            onClick={() => !current && !editing && open(s.path)}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setMenu({ x: e.clientX, y: e.clientY, row: s });
            }}
            onKeyDown={(e) => {
              if (editing) return;
              if ((e.key === "Enter" || e.key === " ") && !current) {
                e.preventDefault();
                open(s.path);
              }
            }}
            title={current ? `"${label}" is the sheet in view` : isRoot ? "Go to the root sheet" : `Open sheet "${label}" (${file})`}
          >
            <span
              data-testid="hierarchy-fold"
              aria-expanded={folds ? !folded : undefined}
              style={{ display: "inline-block", width: 12, textAlign: "center", visibility: folds ? "visible" : "hidden" }}
              onClick={(e) => {
                e.stopPropagation();
                const next = new Set(collapsed);
                if (folded) next.delete(key);
                else next.add(key);
                setCollapsed(next);
              }}
            >
              {folded ? "▸" : "▾"}
            </span>
            {editing ? (
              <input
                autoFocus
                aria-label="Sheet name"
                value={renaming.text}
                onClick={(e) => e.stopPropagation()}
                onChange={(e) => setRenaming({ path: s.path, text: e.target.value })}
                onBlur={() => void commitRename()}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Enter") void commitRename();
                  if (e.key === "Escape") setRenaming(null);
                }}
              />
            ) : (
              // `UpdateNetHighlight`: the sheets that hold the highlighted net in `LAYER_BRIGHTENED`
              <b data-testid="hierarchy-name" data-net={s.net ? "yes" : undefined} style={s.net ? { color: layerColor("LAYER_BRIGHTENED") } : undefined}>
                {label}
              </b>
            )}
            <small>{`${file} · page ${pageOf(s)}`}</small>
          </div>
        );
      })}
      {newTop !== null && (
        <SchDialogShell
          title="New Top-Level Sheet"
          width={360}
          onCancel={() => setNewTop(null)}
          canOk={newTop.trim() !== ""}
          onOk={() => {
            const text = newTop.trim();
            setNewTop(null);
            if (text) void api.cmd({ op: "on_sheet", sheet: "", cmd: { op: "sch_edit", verb: "add_top_level_sheet", name: text } } as unknown as Cmd);
          }}
        >
          <p style={{ marginTop: 0 }}>Enter name for new top-level sheet:</p>
          <input autoFocus aria-label="New top-level sheet name" style={{ width: "100%", boxSizing: "border-box" }} value={newTop} onChange={(e) => setNewTop(e.target.value)} />
        </SchDialogShell>
      )}
      {deleting && (
        <SchDialogShell title="Delete Top-Level Sheet" width={380} cancelLabel="No" okLabel="Yes" onCancel={() => setDeleting(null)} onOk={() => void deleteTopLevel(deleting)}>
          <p style={{ margin: 0, whiteSpace: "pre-wrap" }}>{`Delete top-level sheet '${labelOf(deleting)}'?\n\nThis cannot be undone.`}</p>
        </SchDialogShell>
      )}
      {message && (
        <SchDialogShell title="Delete Top-Level Sheet" width={360} cancelLabel="OK" onCancel={() => setMessage(null)}>
          <p style={{ margin: 0 }} data-testid="hierarchy-message">
            {message}
          </p>
        </SchDialogShell>
      )}
    </div>
  );
}

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
// fallback), a row with sheets under it folds and unfolds, and the right-click menu has "Expand All" / "Collapse All"
// (`HIERARCHY_PANE::onContextMenu`). The folded rows are kept per board (KiCad's `m_SchHierarchyCollapsed`, the project's local settings).
// Not ported from that menu: "Edit Page Number" and "Rename" (Sheet Properties edits both), New / Delete Top-Level Sheet (one root here), and
// `UpdateNetHighlight` (the sheets on the highlighted net in bold).
import { useEffect, useState } from "react";
import { useActionRunner } from "../../actions/useActionRunner";
import { fetchHierarchy, type HierarchyEntry } from "../../api/schControlClient";
import { effectivePage, hasChildSheets, hierarchyPathKey, hierarchyRowName, hierarchyTreeOrder, samePath, visibleHierarchyRows } from "../../kicad-port/sheetPages";
import { useStudioState } from "../../state/store";
import { ContextMenu } from "../canvas/ContextMenu";

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

export function HierarchyPanel() {
  const state = useStudioState();
  const { run } = useActionRunner();
  const [sheets, setSheets] = useState<HierarchyEntry[]>([]);
  const name = state.board?.name || "schematic";
  const here = state.currentSheetPath;
  const [collapsed, setCollapsedState] = useState<Set<string>>(() => readCollapsed(name));
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const setCollapsed = (next: Set<string>) => {
    setCollapsedState(next);
    writeCollapsed(name, next);
  };

  useEffect(() => {
    let live = true;
    fetchHierarchy().then(
      (list) => live && setSheets(list),
      () => live && setSheets([])
    );
    return () => {
      live = false;
    };
  }, [state.version]);

  const open = (path: string[]) => run("eeschema.NavigateTool.changeSheet", path);
  // The root first even when the list has not arrived yet, so the panel is never empty.
  const tree: HierarchyEntry[] = sheets.length > 0 ? hierarchyTreeOrder(sheets) : [{ path: [], name: "", file: "", page: "1" }];
  const rows = visibleHierarchyRows(tree, collapsed);
  const pageOf = (s: HierarchyEntry) => effectivePage(s, Math.max(0, sheets.indexOf(s)));
  const parents = tree.filter((s) => hasChildSheets(tree, s.path));

  return (
    <div
      className="panel-section"
      id="hierarchy-panel"
      tabIndex={-1}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
    >
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          entries={[
            { label: "Expand All", onSelect: () => setCollapsed(new Set()) },
            { label: "Collapse All", onSelect: () => setCollapsed(new Set(parents.map((s) => hierarchyPathKey(s.path)))) },
          ]}
        />
      )}
      {rows.map((s) => {
        const current = samePath(s.path, here);
        const isRoot = s.path.length === 0;
        const label = hierarchyRowName(s, state.board?.name ?? "");
        const file = isRoot ? `${name}.kicad_sch` : s.file;
        const key = hierarchyPathKey(s.path);
        const folds = hasChildSheets(tree, s.path);
        const folded = collapsed.has(key);
        return (
          <div
            key={s.path.join("/")}
            role="button"
            tabIndex={0}
            aria-current={current ? "true" : undefined}
            className={`unplaced-row${current ? " armed" : ""}`}
            style={{ marginLeft: s.path.length * 10, cursor: current ? "default" : "pointer" }}
            onClick={() => !current && open(s.path)}
            onKeyDown={(e) => {
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
            <b>{label}</b>
            <small>{`${file} · page ${pageOf(s)}`}</small>
          </div>
        );
      })}
    </div>
  );
}

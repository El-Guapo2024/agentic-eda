// The Hierarchy Navigator (docked left, below Properties, per the coordinator's reading of eeschema's default AUI layout in sch_edit_frame.cpp) --
// the tree of the design's sheets (GAPS.md #6): the root, then every sheet placed on it, then the sheets those place, in the order
// `SCH_SHEET_LIST::BuildSheetList` builds. The list is `GET /api/sch/hierarchy` (the same one Next / Previous Sheet and Edit Sheet Page Number read) and it is read
// again whenever the design changes, so a Reorganize into Module Sheets, an undo or a new sheet shows at once.
//
// A click opens the sheet: it runs `eeschema.NavigateTool.changeSheet` with the sheet's path (`SCH_NAVIGATE_TOOL::ChangeSheet`, which the Hierarchy Navigator's
// tree raises in KiCad), so Back / Forward / Leave Sheet keep their history. The sheet in view is highlighted.
import { useEffect, useState } from "react";
import { useActionRunner } from "../../actions/useActionRunner";
import { fetchHierarchy, type HierarchyEntry } from "../../api/schControlClient";
import { samePath } from "../../kicad-port/sheetPages";
import { useStudioState } from "../../state/store";

export function HierarchyPanel() {
  const state = useStudioState();
  const { run } = useActionRunner();
  const [sheets, setSheets] = useState<HierarchyEntry[]>([]);
  const name = state.board?.name || "schematic";
  const here = state.currentSheetPath;

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
  const rows: HierarchyEntry[] = sheets.length > 0 ? sheets : [{ path: [], name: "", file: "", page: "" }];

  return (
    <div className="panel-section" id="hierarchy-panel" tabIndex={-1}>
      {rows.map((s) => {
        const current = samePath(s.path, here);
        const isRoot = s.path.length === 0;
        const label = isRoot ? "Root" : s.name;
        const file = isRoot ? `${name}.kicad_sch` : s.file;
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
            <b>{label}</b>
            <small>{s.page ? `${file} · page ${s.page}` : file}</small>
          </div>
        );
      })}
    </div>
  );
}

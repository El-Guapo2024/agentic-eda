// The Hierarchy Navigator (docked left, below Properties, per the
// coordinator's reading of eeschema's default AUI layout in
// sch_edit_frame.cpp) -- a tree of the design's sheets (GAPS.md #6).
//
// `GET /api/schematic?sheet=<id>/<id>/...` always returns exactly the
// sheet currently being viewed (`state.schematic.sheet_path`, the
// root-to-here breadcrumb) plus that sheet's own direct children
// (`state.schematic.sheets`) -- this panel is therefore a breadcrumb
// (click any ancestor, including "Root", to jump straight there) over a
// one-level child list (click a child to descend into it), rather than a
// single eagerly-fetched whole-project tree: real eeschema's own Hierarchy
// Navigator is the same shape (a path box of ancestor sheets, one level of
// children shown at a time), and this avoids a second "give me the whole
// hierarchy regardless of what's on screen" endpoint for a tree that's
// usually only a few sheets deep. `state.currentSheetPath` is empty at the
// root, same as a single-sheet design (everything before this feature
// existed) always behaved -- that case renders exactly as this panel
// always used to (one "Root" row, nothing to expand).
import { useStudioApi, useStudioState } from "../../state/store";

export function HierarchyPanel() {
  const state = useStudioState();
  const api = useStudioApi();
  const name = state.board?.name || "schematic";
  const sch = state.schematic;
  const path = state.currentSheetPath;
  const crumbs = sch?.sheet_path ?? [];
  const children = sch?.sheets ?? [];
  const atRoot = path.length === 0;

  const goTo = (depth: number) => {
    // depth 0 = root ([]), depth N = the first N crumbs' own ids.
    api.navigateToSheet(path.slice(0, depth));
  };

  return (
    <div className="panel-section" id="hierarchy-panel" tabIndex={-1}>
      <h3>Hierarchy</h3>
      <div className={`unplaced-row${atRoot ? " armed" : ""}`} style={{ cursor: atRoot ? "default" : "pointer" }} onClick={() => !atRoot && goTo(0)} title={atRoot ? undefined : "Go to root sheet"}>
        <b>Root</b>
        <small>{name}.kicad_sch</small>
      </div>
      {crumbs.map((c, i) => {
        const isCurrent = i === crumbs.length - 1;
        return (
          <div key={c.id} className={`unplaced-row${isCurrent ? " armed" : ""}`} style={{ marginLeft: (i + 1) * 10, cursor: isCurrent ? "default" : "pointer" }} onClick={() => !isCurrent && goTo(i + 1)} title={isCurrent ? undefined : `Go to sheet "${c.name}"`}>
            <b>{c.name}</b>
          </div>
        );
      })}
      {children.length > 0 && (
        <div style={{ marginLeft: (crumbs.length + 1) * 10, marginTop: 4, opacity: 0.85 }}>
          {children.map((s) => (
            <div key={s.id} className="unplaced-row" style={{ cursor: "pointer" }} onClick={() => api.navigateToSheet([...path, s.id])} title={`Enter sheet "${s.name}" (${s.file})`}>
              <b>{s.name}</b>
              <small>{s.file}</small>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// The Hierarchy Navigator (docked left, below Properties, per the
// coordinator's reading of eeschema's default AUI layout in
// sch_edit_frame.cpp) -- a tree of the design's sheets. This app's
// schematic model (GET /api/schematic) has no sheet/sub-sheet concept at
// all yet (crates/model/src/ir.rs's SchematicSection is flat: symbols,
// wires, labels), so there is exactly one row: the root sheet, always
// selected, never expandable. Real multi-sheet navigation (clicking a
// row to switch sheets) has nothing to switch to yet -- ported as a
// single-sheet tree rather than skipped, so the dock position and look
// are already right once sheets exist.
import { useStudioState } from "../../state/store";

export function HierarchyPanel() {
  const state = useStudioState();
  const name = state.board?.name || "schematic";
  return (
    <div className="panel-section">
      <h3>Hierarchy</h3>
      <div className="unplaced-row armed" style={{ cursor: "default" }}>
        <b>Root</b>
        <small>{name}.kicad_sch</small>
      </div>
    </div>
  );
}

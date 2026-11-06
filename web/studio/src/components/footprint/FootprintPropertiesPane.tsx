// The Footprint Editor's Properties pane (UI-LEFT Y3): `PCB_PROPERTIES_PANEL` in the footprint editor frame (footprint_edit_frame.cpp's `m_propertiesPanel`, docked left
// beside the Libraries tree). The grid of the selected pads, shapes, texts and fields -- or of the footprint itself when nothing is selected -- with KiCad's rows in its
// groups and order as a library editor lists them (kicad-port/fpItemProperties.ts) and the same grid the board's, the schematic's and the Symbol Editor's panes draw
// (components/panels/PropertyGrid.tsx). An edit is one batch of the Footprint Editor's verbs, on its own undo stack.
import { useMemo } from "react";
import { useStudioState } from "../../state/store";
import { useFpApi, useFpState } from "../../state/footprintEditorStore";
import { fpEdit, fpGrid, type FpPropCtx } from "../../kicad-port/fpItemProperties";
import type { PropValue } from "../../kicad-port/propertyManager";
import { PropertyGrid } from "../panels/PropertyGrid";

export function FootprintPropertiesPane() {
  const state = useFpState();
  const api = useFpApi();
  const units = useStudioState().units;
  const fp = state.footprint;
  const ids = useMemo(() => [...state.selection], [state.selection]);
  const ctx = useMemo<FpPropCtx | null>(() => (fp && state.name ? { fp, name: state.name, units } : null), [fp, state.name, units]);
  const model = useMemo(() => (ctx ? fpGrid(ctx, ids) : null), [ctx, ids]);
  if (!ctx || !model) {
    return (
      <div className="panel-section">
        <div className="panel-empty">No objects selected</div>
      </div>
    );
  }
  const commit = async (name: string, value: PropValue): Promise<string | null> => {
    const plan = fpEdit(ctx, ids, name, value);
    if (!plan.ok) return plan.error;
    // `BOARD_COMMIT::Push( "Edit Properties" )`: one undo step for the whole edit
    if (plan.cmds.length > 0) await api.cmd(plan.cmds.length === 1 ? plan.cmds[0]! : { op: "batch", cmds: plan.cmds });
    return null;
  };
  return (
    <div data-testid="fp-properties">
      <PropertyGrid model={model} units={units} onCommit={commit} selectionKey={ids.join("\n")} />
    </div>
  );
}

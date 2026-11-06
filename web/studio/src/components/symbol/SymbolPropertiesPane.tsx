// The Symbol Editor's Properties pane (UI-LEFT Y3): `SCH_PROPERTIES_PANEL` in the symbol editor frame (symbol_edit_frame.cpp's `m_propertiesPanel`, docked left with
// the Libraries tree). The grid of the selected pins, shapes, texts, text boxes and fields -- or of the symbol itself when nothing is selected -- with KiCad's rows in its
// groups and order (kicad-port/symItemProperties.ts) and the same grid the board's and the schematic's panes draw (components/panels/PropertyGrid.tsx). An edit is one
// batch of the Symbol Editor's verbs, on its own undo stack.
import { useMemo } from "react";
import { useStudioState } from "../../state/store";
import { useSymApi, useSymState } from "../../state/symbolEditorStore";
import { symEdit, symGrid, type SymCtx } from "../../kicad-port/symItemProperties";
import type { PropValue } from "../../kicad-port/propertyManager";
import { PropertyGrid } from "../panels/PropertyGrid";

export function SymbolPropertiesPane() {
  const state = useSymState();
  const api = useSymApi();
  const units = useStudioState().units;
  const sym = state.symbol;
  const ids = useMemo(() => [...state.selection], [state.selection]);
  const ctx = useMemo<SymCtx | null>(() => (sym && state.libId ? { sym, own: state.derivedOwn ?? sym, libId: state.libId, units } : null), [sym, state.derivedOwn, state.libId, units]);
  const model = useMemo(() => (ctx ? symGrid(ctx, ids) : null), [ctx, ids]);
  if (!ctx || !model) {
    return (
      <div className="panel-section">
        <div className="panel-empty">No objects selected</div>
      </div>
    );
  }
  const commit = async (name: string, value: PropValue): Promise<string | null> => {
    const plan = symEdit(ctx, ids, name, value);
    if (!plan.ok) return plan.error;
    // `SCH_COMMIT::Push( "Edit Properties" )`: one undo step for the whole edit
    if (plan.cmds.length > 0) await api.cmd(plan.cmds.length === 1 ? plan.cmds[0]! : { op: "batch", cmds: plan.cmds });
    return null;
  };
  return (
    <div data-testid="sym-properties">
      <PropertyGrid model={model} units={units} onCommit={commit} selectionKey={ids.join("\n")} />
    </div>
  );
}

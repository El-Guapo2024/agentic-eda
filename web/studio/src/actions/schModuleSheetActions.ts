// Tools > Reorganize into Module Sheets: the studio's own action (KiCad has no counterpart) that turns a flat schematic into a root sheet with one hierarchical sheet per
// functional module (crates/ops/src/sheets.rs `reorganize_sheets`, crates/model/src/modules.rs). It is one `reorganize_sheets` verb, so one Undo step puts the flat sheet back.
// New derivations are module sheets already; this is for a design drawn flat before that, or one the user flattened.
import type { SchEditContext } from "./schEditActions";
import { schematicActions, type ActionMap } from "./schActionRegistry";

/** The action's name: the studio's own namespace, so the KiCad parity audit (which reads `eeschema.` / `pcbnew.` / `common.` names) does not mistake it for one of KiCad's. */
export const REORGANIZE_SHEETS_ACTION = "studio.Sheets.reorganize";

export function registerSchModuleSheetActions(registry: ActionMap, ctx: SchEditContext): void {
  const { state, api, dispatch } = ctx;
  const m = schematicActions(registry, state.tab);
  m.set(REORGANIZE_SHEETS_ACTION, () => {
    if ((state.schematic?.sheets.length ?? 0) > 0 || state.currentSheetPath.length > 0) {
      return dispatch({ type: "TOAST", message: "This schematic already has sheets; there is nothing to reorganize.", kind: "info" });
    }
    // The verb runs on the root and says why when it cannot (a single module, a multi-unit symbol); on success the root sheet is what the user should look at.
    void api.cmd({ op: "reorganize_sheets" }).then((ok) => {
      if (!ok) return;
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      void api.navigateToSheet([]);
      dispatch({ type: "TOAST", message: "Reorganized into module sheets. Undo puts the flat sheet back.", kind: "info" });
    });
  });
}

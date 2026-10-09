// Cycle Body Style (`eeschema.InteractiveEdit.toggleDeMorgan`, `SCH_EDIT_TOOL::CycleBodyStyle`): the selected symbol is drawn in the next body style of its library symbol
// ("Switch between De Morgan (or other) representations"), one undo step. The rules are kicad-port/schBodyStyle.ts; the verb is `sch_edit` `set_body_style`.
import { cycleBodyStyleCmd } from "../kicad-port/schBodyStyle";
import { schematicActions, type ActionMap } from "./schActionRegistry";
import type { SchEditContext } from "./schEditActions";

export function registerSchBodyStyleActions(registry: ActionMap, ctx: SchEditContext): void {
  const { state, api, requestSelection } = ctx;
  const m = schematicActions(registry, state.tab);
  const sch = state.schematic;
  m.set("eeschema.InteractiveEdit.toggleDeMorgan", () => {
    if (!sch) return;
    const cmd = cycleBodyStyleCmd(sch, requestSelection());
    if (!cmd) return ctx.dispatch({ type: "TOAST", message: "Select a symbol that has more than one body style.", kind: "info" });
    void api.cmd(cmd);
  });
}

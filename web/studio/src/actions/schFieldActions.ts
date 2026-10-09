// The schematic actions that work on fields: Autoplace Fields (`SCH_EDIT_TOOL::AutoplaceFields`) and the Left / Center / Right Justify the editor shares with the PCB
// (`SCH_EDIT_TOOL::JustifyText`: a field, and a label whose text runs horizontally). The verbs are `sch_edit` `autoplace_fields`, `edit_field` and `edit_label`
// (crates/ops/src/sch_fields.rs, sch_props.rs); the rules for what a selection holds are kicad-port/schFieldEdit.ts.
import type { Cmd } from "../api/types";
import { autoplaceCmd, findField, isFieldId } from "../kicad-port/schFieldEdit";
import { inferSpin } from "../components/schematic/labelShape";
import { schematicActions, type ActionMap } from "./schActionRegistry";
import type { SchEditContext } from "./schEditActions";

export type Justify = "left" | "center" | "right";

export function registerSchFieldActions(registry: ActionMap, ctx: SchEditContext): void {
  const { state, api, requestSelection } = ctx;
  const m = schematicActions(registry, state.tab);
  const sch = state.schematic;
  const info = (message: string) => ctx.dispatch({ type: "TOAST", message, kind: "info" });

  // Autoplace Fields (`O`): "Runs the automatic placement algorithm on the symbol's (or sheet's) fields". The selection's symbols, power symbols and sheets, and the item each
  // selected field belongs to, have their fields placed beside them, on a side with no pins and clear of what is drawn around (the manual routine, `AUTOPLACE_MANUAL`); one
  // undo step. Moving, turning or editing a field's place afterwards takes the item out of the autoplaced ones again.
  m.set("eeschema.InteractiveEdit.autoplaceFields", () => {
    if (!sch) return;
    const cmd = autoplaceCmd(sch, requestSelection());
    if (!cmd) return info("Select a symbol, a sheet or one of their fields.");
    void api.cmd(cmd);
  });

  // Left / Center / Right Justify -- `JustifyText`: every selected field is justified as asked (as it reads on the sheet), and so is a label whose text runs horizontally (a
  // label's justification is its spin: left-justified runs on to the right of the anchor, right-justified ends at it; there is no centred label here). One undo step.
  const justify = (which: Justify) => () => {
    if (!sch) return;
    const cmds: Cmd[] = [];
    for (const id of requestSelection()) {
      if (isFieldId(id)) {
        const f = findField(sch, id);
        if (f && f.field.h !== which) cmds.push({ op: "sch_edit", verb: "edit_field", id, h: which });
        continue;
      }
      const label = sch.labels.find((l) => l.id === id);
      if (label && which !== "center") {
        const spin = label.spin ?? inferSpin(sch.wires, label.at);
        if (spin !== "right" && spin !== "left") continue; // `GetTextAngle() == ANGLE_HORIZONTAL`
        const next = which === "left" ? "right" : "left";
        if (spin !== next) cmds.push({ op: "sch_edit", verb: "edit_label", id, spin: next });
      }
    }
    if (cmds.length > 0) void api.cmdBatch(cmds);
  };
  m.set("common.Control.leftJustify", justify("left"));
  m.set("common.Control.centerJustify", justify("center"));
  m.set("common.Control.rightJustify", justify("right"));
}

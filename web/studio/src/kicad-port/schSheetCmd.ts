// Which sheet a schematic command is for.
//
// KiCad edits whichever sheet is open (`SCH_EDIT_FRAME::GetCurrentSheet`); every schematic verb in crates/ops acts on one screen, and
// `Cmd::OnSheet` names which: the `/`-joined `SheetInstance::id`s from the root, the same path `GET /api/schematic?sheet=` takes. The
// studio keeps the sheet it is showing in `state.currentSheetPath`, so every schematic command it sends is wrapped here -- the tools
// and the verbs never need to know which sheet they are on, exactly as in KiCad.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd } from "../api/types";

/** The `op`s `Cmd::domain()` files under the schematic editor (crates/ops/src/lib.rs) -- what an undo on the Schematic tab reverts and what acts on a sheet. */
const SCHEMATIC_OPS: ReadonlySet<string> = new Set([
  "move_symbol",
  "drag_symbol",
  "rotate_symbol",
  "mirror_symbol",
  "mirror_symbol_vertical",
  "delete_symbol",
  "add_wire",
  "delete_wire",
  "add_no_connect",
  "delete_no_connect",
  "add_bus_entry",
  "delete_bus_entry",
  "add_junction",
  "delete_junction",
  "add_sch_line",
  "delete_sch_line",
  "add_sheet",
  "swap_sch_items",
  "sch_edit",
  "add_erc_exclusion",
  "delete_erc_exclusion",
  "add_label",
  "delete_label",
  "add_sch_text",
  "delete_sch_text",
  "add_power_symbol",
  "delete_power_symbol",
  "add_symbol",
  "edit_symbol_fields",
  "rename_symbol",
  "set_symbol_fields",
  "replace_text",
  "set_erc_pin_map_cell",
  "reset_erc_pin_map",
  "annotate",
]);

/** Is this a schematic edit (as opposed to a PCB one, or a library editor's)? A batch is judged by its first command, as `Cmd::domain` does. */
export function isSchematicCmd(cmd: Cmd): boolean {
  if (cmd.op === "batch") return cmd.cmds.length > 0 && isSchematicCmd(cmd.cmds[0]!);
  if (cmd.op === "on_sheet") return isSchematicCmd(cmd.cmd);
  return SCHEMATIC_OPS.has(cmd.op) || cmd.op === "reorganize_sheets";
}

/**
 * `cmd` addressed to the sheet being viewed: wrapped in `on_sheet` when that is not the root and the command is a schematic edit;
 * as it is otherwise (on the root, a PCB command, a command already addressed). Reorganize always runs on the root.
 */
export function onCurrentSheet(cmd: Cmd, sheetPath: readonly string[]): Cmd {
  if (sheetPath.length === 0 || cmd.op === "on_sheet" || cmd.op === "reorganize_sheets" || !isSchematicCmd(cmd)) return cmd;
  return { op: "on_sheet", sheet: sheetPath.join("/"), cmd };
}

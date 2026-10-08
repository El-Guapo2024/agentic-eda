// Which sheet a schematic command is for.
//
// KiCad edits whichever sheet is open (`SCH_EDIT_FRAME::GetCurrentSheet`); every schematic verb in crates/ops acts on one screen, and
// `Cmd::OnSheet` names which: the `/`-joined `SheetInstance::id`s from the root, the same path `GET /api/schematic?sheet=` takes. The
// studio keeps the sheet it is showing in `state.currentSheetPath`, so every command it sends from the Schematic tab is wrapped here -- the
// tools and the verbs never need to know which sheet they are on, exactly as in KiCad. There is no list of "schematic ops" to keep in
// step with the server: `Cmd::OnSheet` runs a command that is not a schematic edit as it is (crates/ops/src/lib.rs `on_sheet`).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd } from "../api/types";

/**
 * `cmd` addressed to the sheet being viewed: wrapped in `on_sheet` when that is not the root; as it is on the root, when it is
 * already addressed, and for Reorganize (which always runs on the root).
 */
export function onCurrentSheet(cmd: Cmd, sheetPath: readonly string[]): Cmd {
  if (sheetPath.length === 0 || cmd.op === "on_sheet" || cmd.op === "reorganize_sheets") return cmd;
  return { op: "on_sheet", sheet: sheetPath.join("/"), cmd };
}

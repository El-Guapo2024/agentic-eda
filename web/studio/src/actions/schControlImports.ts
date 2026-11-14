// The control actions that read a file the person picks: Import Footprint Assignments (a `.cmp` file). Plain async functions over the action context.
import type { SchControlContext } from "./schControlActions";
import type { Cmd } from "../api/types";
import { pickTextFile } from "../api/libraryClient";
import { forcedFootprintVisibility, parseCmpFile } from "../kicad-port/cmpFile";
import { collectSheetRefs } from "../kicad-port/crossProbe";
import { fetchSchematic } from "../api/client";


/**
 * `SCH_EDITOR_CONTROL::ImportFPAssignments` -> `processCmpToFootprintLinkFile`: load a `.cmp` file and give each symbol it names the footprint it lists
 * (`SetFootprintFieldText`, an empty `IdModule` included) -- every symbol of the hierarchy (`Hierarchy().GetSymbols( .., SYMBOL_FILTER_NON_POWER )`), every
 * placed unit of a reference; a reference the file lists twice takes its last block. Before it reads the file KiCad asks "Select the footprint field visibility
 * setting." ("Change Visibility": keep / show all / hide all; Cancel does nothing): shown or hidden, the Footprint field of every placed unit the file names is
 * made so (`GetField( FIELD_T::FOOTPRINT )->SetVisible( aVisibilityState )`, UI-LEFT-3 S3-25). Nothing is said when it is done (`SyncView`, `OnModify`); an
 * unreadable file is "Failed to open symbol-footprint link file '%s'.". **Differs**: the change is one undo step (KiCad's is not undoable: no commit), as every
 * studio edit is a verb.
 */
export async function importFootprintAssignments(ctx: SchControlContext): Promise<void> {
  const toast = (text: string, kind: "info" | "error") => ctx.dispatch({ type: "TOAST", message: text, kind });
  if (!ctx.state.schematic) return;
  let picked: { name: string; text: string } | null;
  try {
    picked = await pickTextFile(".cmp");
  } catch {
    return toast("Failed to open symbol-footprint link file.", "error");
  }
  if (!picked) return;
  // "Select the footprint field visibility setting.": keep, show all, hide all; Cancel stops here
  const choice = await new Promise<number | null>((resolve) => ctx.controlDispatch({ type: "OPEN_DIALOG", dialog: { kind: "fp_visibility", resolve } }));
  if (choice === null || choice < 0) return;
  const forced = forcedFootprintVisibility(choice);
  const links = parseCmpFile(picked.text);
  // the footprint of every symbol on every sheet (the sheets' pages, from the root down); `edit_symbol_fields` finds the symbol's own screen. Each placed unit's
  // Footprint field, by the sheet it is on, for the visibility.
  const current = new Map<string, string>();
  const footprintFields: Array<{ reference: string; sheet: string; id: string; visible: boolean }> = [];
  try {
    await collectSheetRefs(async (path) => {
      const sheet = await fetchSchematic(path);
      for (const sym of sheet.symbols) {
        if (!current.has(sym.id)) current.set(sym.id, sym.footprint ?? "");
        const field = sym.fields?.find((f) => f.name === "Footprint");
        if (field?.id) footprintFields.push({ reference: sym.id, sheet: path.join("/"), id: field.id, visible: field.visible });
      }
      return { symbolIds: sheet.symbols.map((x) => x.id), childIds: sheet.sheets.map((x) => x.id) };
    });
  } catch {
    for (const sym of ctx.state.schematic.symbols) current.set(sym.id, sym.footprint ?? "");
  }
  // the blocks in file order, each overwriting the last for its reference
  const wanted = new Map<string, string>();
  for (const l of links) if (current.has(l.reference)) wanted.set(l.reference, l.footprint);
  const cmds: Cmd[] = [];
  for (const [reference, footprint] of wanted) if ((current.get(reference) ?? "") !== footprint) cmds.push({ op: "edit_symbol_fields", id: reference, footprint });
  // `if( aForceVisibilityState ) GetField( FIELD_T::FOOTPRINT )->SetVisible( aVisibilityState )` of every placed unit the file names (a field already so is left)
  if (forced !== null) {
    const seen = new Set<string>();
    for (const f of footprintFields) {
      // (a screen shown on two paths has the field once)
      if (!wanted.has(f.reference) || f.visible === forced || seen.has(f.id)) continue;
      seen.add(f.id);
      const edit: Cmd = { op: "sch_edit", verb: "edit_field", id: f.id, visible: forced };
      cmds.push(f.sheet === "" ? edit : { op: "on_sheet", sheet: f.sheet, cmd: edit });
    }
  }
  if (cmds.length > 0) await ctx.api.cmdBatch(cmds);
}

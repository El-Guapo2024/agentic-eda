// The control actions that read a file the person picks: Import Footprint Assignments (a `.cmp` file). Plain async functions over the action context.
import type { SchControlContext } from "./schControlActions";
import type { Cmd } from "../api/types";
import { pickTextFile } from "../api/libraryClient";
import { parseCmpFile } from "../kicad-port/cmpFile";
import { collectSheetRefs } from "../kicad-port/crossProbe";
import { fetchSchematic } from "../api/client";


/**
 * `SCH_EDITOR_CONTROL::ImportFPAssignments` -> `processCmpToFootprintLinkFile`: load a `.cmp` file and give each symbol it names the footprint it lists
 * (`SetFootprintFieldText`, an empty `IdModule` included) -- every symbol of the hierarchy (`Hierarchy().GetSymbols( .., SYMBOL_FILTER_NON_POWER )`), every
 * placed unit of a reference; a reference the file lists twice takes its last block. Nothing is said when it is done (`SyncView`, `OnModify`); an unreadable
 * file is "Failed to open symbol-footprint link file '%s'.". **Differs**: the change is one undo step (KiCad's is not undoable: no commit), as every studio edit
 * is a verb; KiCad's question "Select the footprint field visibility setting." (keep / show all / hide all) is not asked: the fields keep their visibility.
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
  const links = parseCmpFile(picked.text);
  // the footprint of every symbol on every sheet (the sheets' pages, from the root down); `edit_symbol_fields` finds the symbol's own screen
  const current = new Map<string, string>();
  try {
    await collectSheetRefs(async (path) => {
      const sheet = await fetchSchematic(path);
      for (const sym of sheet.symbols) if (!current.has(sym.id)) current.set(sym.id, sym.footprint ?? "");
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
  if (cmds.length > 0) await ctx.api.cmdBatch(cmds);
}

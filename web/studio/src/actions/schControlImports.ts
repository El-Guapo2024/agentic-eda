// The control actions that read a file the person picks: Import Footprint Assignments (a `.cmp` file). Plain async functions over the action context.
import type { SchControlContext } from "./schControlActions";
import type { Cmd } from "../api/types";
import { pickTextFile } from "../api/libraryClient";
import { parseCmpFile } from "../kicad-port/cmpFile";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * `SCH_EDITOR_CONTROL::ImportFPAssignments`: load a `.cmp` file and give each symbol it names the footprint it lists -- every placed unit of a
 * reference, one undo step for all of them. (The C++ also asks whether to keep, show or hide the footprint fields; the schematic here keeps no field
 * visibility, so there is nothing to ask.)
 */
export async function importFootprintAssignments(ctx: SchControlContext): Promise<void> {
  const toast = (text: string, kind: "info" | "error") => ctx.dispatch({ type: "TOAST", message: text, kind });
  const sch = ctx.state.schematic;
  if (!sch) return;
  let picked: { name: string; text: string } | null;
  try {
    picked = await pickTextFile(".cmp");
  } catch (e) {
    return toast(`Failed to open symbol-footprint link file: ${message(e)}`, "error");
  }
  if (!picked) return;
  const links = parseCmpFile(picked.text);
  const current = new Map<string, string>();
  for (const s of sch.symbols) current.set(s.id, s.footprint ?? "");
  const cmds: Cmd[] = [];
  const done = new Set<string>();
  for (const l of links) {
    if (!current.has(l.reference) || done.has(l.reference)) continue;
    done.add(l.reference);
    if ((current.get(l.reference) ?? "") !== l.footprint) cmds.push({ op: "edit_symbol_fields", id: l.reference, footprint: l.footprint });
  }
  if (links.length === 0) return toast(`'${picked.name}' lists no component.`, "error");
  if (cmds.length === 0) return toast(done.size === 0 ? `No symbol of this schematic is named in '${picked.name}'.` : "Every footprint is already assigned as that file says.", "info");
  if (await ctx.api.cmdBatch(cmds)) toast(`Assigned ${cmds.length} footprint${cmds.length === 1 ? "" : "s"} from '${picked.name}'.`, "info");
}

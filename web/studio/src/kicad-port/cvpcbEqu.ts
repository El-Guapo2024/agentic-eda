// The pure parts of CvPcb's footprint viewer and of Manage Footprint Association Files (KiCad 25ec2335: cvpcb/display_footprints_frame.cpp `InitDisplay`,
// cvpcb/dialogs/dialog_config_equfiles.cpp; the cleaned 8303b2ad tree has no cvpcb/). The files are read and matched on the server (crates/model/src/cvpcb_equ.rs,
// crates/cli/src/cvpcb_api.rs); the dialogs are components/schControl/AssignFootprintsDialog.tsx and EquFilesDialog.tsx. Unit tested in cvpcbEqu.test.ts.

/** `DISPLAY_FOOTPRINTS_FRAME::InitDisplay`: the footprint picked in the footprint list, else the footprint the selected symbol has (`comp->GetFPID()`), else nothing. */
export function viewerFootprint(picked: string | null, selectedSymbolFootprint: string | null): string {
  if (picked && picked.trim() !== "") return picked;
  return selectedSymbolFootprint?.trim() ?? "";
}

/** The viewer's title (`SetTitle( "Footprint: %s" )`) and status text (`"Lib: %s"`, from the footprint list's entry), for a footprint shown. */
export function viewerCaption(id: string, lib: string | undefined): { title: string; status: string } {
  return { title: id ? `Footprint: ${id}` : "", status: lib ? `Lib: ${lib}` : "" };
}

/** `wxFileName::MakeRelativeTo( base )` of two absolute paths with `/` separators: the path from `base` to `path`, `..` where it climbs; null when either is not absolute. */
export function relativeTo(path: string, base: string): string | null {
  if (!path.startsWith("/") || !base.startsWith("/")) return null;
  const p = path.split("/").filter((s) => s !== "" && s !== ".");
  const b = base.split("/").filter((s) => s !== "" && s !== ".");
  let common = 0;
  while (common < p.length && common < b.length && p[common] === b[common]) common++;
  return [...Array<string>(b.length - common).fill(".."), ...p.slice(common)].join("/");
}

/**
 * `OnAddFiles`: how a file chosen in Add's dialog is written in the list -- `${NAME}/relative` under the first variable of the grid (`KIPRJMOD`, then the footprint
 * library folder's) the file can be made relative to (`fn.MakeRelativeTo( libpath )`, which on Linux and macOS succeeds for any absolute path, so the project's
 * folder takes every file, `..` and all), else the path as chosen; `\` made `/` ("Use unix separators only."). A variable with no value is skipped.
 */
export function equEntryFor(path: string, vars: readonly { name: string; value: string }[]): string {
  const p = path.replace(/\\/g, "/");
  for (const v of vars) {
    if (!v.value) continue;
    const rel = relativeTo(p, v.value.replace(/\\/g, "/"));
    if (rel !== null) return `\${${v.name}}/${rel}`;
  }
  return p;
}

/** `OnAddFiles`'s loop: each file chosen is added unless the list has it already ("File '%s' already exists in list."). */
export function addEquFiles(list: readonly string[], chosen: readonly string[], vars: readonly { name: string; value: string }[]): { list: string[]; errors: string[] } {
  const out = [...list];
  const errors: string[] = [];
  for (const path of chosen) {
    const entry = equEntryFor(path, vars);
    if (out.includes(entry)) errors.push(`File '${entry}' already exists in list.`);
    else out.push(entry);
  }
  return { list: out, errors };
}

/** `OnButtonMoveUp`: every selected entry one place up (in the order of the selection, lowest first); nothing when the first entry is selected. */
export function moveEquUp(list: readonly string[], selected: readonly number[]): { list: string[]; selected: number[] } {
  const sel = [...selected].sort((a, b) => a - b);
  if (sel.length === 0 || sel[0] === 0) return { list: [...list], selected: sel };
  const out = [...list];
  for (const jj of sel) [out[jj - 1], out[jj]] = [out[jj]!, out[jj - 1]!];
  return { list: out, selected: sel.map((j) => j - 1) };
}

/** `OnButtonMoveDown`: every selected entry one place down (highest first); nothing when the last entry is selected. */
export function moveEquDown(list: readonly string[], selected: readonly number[]): { list: string[]; selected: number[] } {
  const sel = [...selected].sort((a, b) => a - b);
  if (sel.length === 0 || sel[sel.length - 1] === list.length - 1) return { list: [...list], selected: sel };
  const out = [...list];
  for (const jj of [...sel].reverse()) [out[jj + 1], out[jj]] = [out[jj]!, out[jj + 1]!];
  return { list: out, selected: sel.map((j) => j + 1) };
}

/** `OnRemoveFiles`. */
export function removeEqu(list: readonly string[], selected: readonly number[]): string[] {
  const drop = new Set(selected);
  return list.filter((_, i) => !drop.has(i));
}

/** The symbols the dialog lists as the server's Automatically Assign Footprints reads them: each with the footprint it has in the dialog (an assignment not yet applied counts). */
export function autoAssociateInput(rows: readonly { ref: string; value: string; footprint: string; libId: string }[], staged: Readonly<Record<string, string>>): { ref: string; value: string; footprint: string; lib_id: string }[] {
  return rows.map((r) => ({ ref: r.ref, value: r.value, footprint: staged[r.ref] ?? r.footprint, lib_id: r.libId }));
}

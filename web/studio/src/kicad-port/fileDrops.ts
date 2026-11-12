// What a file dropped on the window does (`EDA_BASE_FRAME::OnDropFiles` and each frame's `m_acceptedExts`, common/eda_base_frame.cpp at KiCad 8303b2ad): the frame looks the file's
// extension up in its own table and runs the action it names with the file; a file whose extension is not in the table is ignored. The tables:
//   board editor        kicad_pcb, brd -> DdAppendBoard;  dxf, svg -> ddImportGraphics                          (pcbnew/pcb_edit_frame.cpp)
//   Footprint Editor    kicad_mod -> ddImportFootprint                                                          (pcbnew/footprint_edit_frame.cpp)
//   schematic editor    kicad_sch -> ddAppendFile;  png, jpg, jpeg -> ddAddImage;  dxf, svg -> ddImportGraphics (eeschema/sch_edit_frame.cpp)
//   Symbol Editor, 3D viewer: none.
// KiCad hands the action the file's path; a browser hands the page the file's contents, so the studio's actions take `{ name, text }` (actions/importActions.ts `asPickedFile`).
// Pure: no store, no DOM; unit-tested in fileDrops.test.ts.

const ACCEPTED: Readonly<Record<string, Readonly<Record<string, string>>>> = {
  pcb: {
    kicad_pcb: "pcbnew.Control.DdAppendBoard",
    brd: "pcbnew.Control.DdAppendBoard",
    dxf: "pcbnew.InteractiveDrawing.ddImportGraphics",
    svg: "pcbnew.InteractiveDrawing.ddImportGraphics",
  },
  footprint: { kicad_mod: "pcbnew.Control.ddImportFootprint" },
  schematic: {
    kicad_sch: "eeschema.EditorControl.ddAppendFile",
    png: "eeschema.EditorControl.ddAddImage",
    jpg: "eeschema.EditorControl.ddAddImage",
    jpeg: "eeschema.EditorControl.ddAddImage",
    dxf: "eeschema.EditorControl.ddImportGraphics",
    svg: "eeschema.EditorControl.ddImportGraphics",
  },
};

/** The extension of a file name, lower case, without the dot ("" when it has none). */
export function extensionOf(name: string): string {
  const i = name.lastIndexOf(".");
  return i < 0 ? "" : name.slice(i + 1).toLowerCase();
}

/** The action `tab`'s frame runs for a dropped file named `name` (`m_acceptedExts`), or null when the frame does not accept that extension. */
export function dropActionFor(tab: string, name: string): string | null {
  return ACCEPTED[tab]?.[extensionOf(name)] ?? null;
}

/** The extensions `tab` accepts, for what is said when nothing dropped is. */
export function acceptedExtensions(tab: string): string[] {
  return Object.keys(ACCEPTED[tab] ?? {});
}

/**
 * The actions that take every file of the drop that has their extension, not only the first. A schematic dropped on the sheet runs Import Sheet with it (`DdAppendFile`), and the sheets it
 * places are found beside it by their names (`SCH_EDIT_FRAME::LoadSheetFromFile`): KiCad has the folder, a browser gives the page the files, so the files of the drop go together and the server
 * puts them side by side (`POST /api/sch/dropped_sheets`).
 */
const TAKES_EVERY: ReadonlySet<string> = new Set(["eeschema.EditorControl.ddAppendFile"]);

export interface DropRun {
  name: string;
  action: string;
  /** Every file of the drop the action takes, `name` first, for an action that takes them all. */
  names?: string[];
}

export interface DropPlan {
  /**
   * The accepted files the studio can open, in the drop's order, each with the action it runs (`DoWithAcceptedFiles`: `RunAction( *m_acceptedExts.at( ext ), &fn )` for each
   * file in turn). KiCad's frame runs the next file's action once the one before returns; the studio runs it once the one before is done with the frame (its dialog closed,
   * its carried placement dropped or cancelled), as `FileDropHost` waits. The schematics of a drop are one run (`names`): Import Sheet places each of them in turn itself.
   */
  runs: DropRun[];
  /** Files the frame accepts that this studio cannot open by a drop (no handler), with the action's id. */
  unavailable: Array<{ name: string; action: string }>;
  /** Files of an extension the frame does not accept: ignored. */
  ignored: string[];
}

/** Sorts a drop's files by what the frame does with them. `isEnabled` says whether the studio handles an action on this tab. */
export function planDrop(tab: string, names: readonly string[], isEnabled: (action: string) => boolean): DropPlan {
  const plan: DropPlan = { runs: [], unavailable: [], ignored: [] };
  for (const name of names) {
    const action = dropActionFor(tab, name);
    if (!action) plan.ignored.push(name);
    else if (!isEnabled(action)) plan.unavailable.push({ name, action });
    else if (TAKES_EVERY.has(action)) {
      const together = plan.runs.find((r) => r.action === action && r.names);
      if (together) together.names!.push(name);
      else plan.runs.push({ name, action, names: [name] });
    } else plan.runs.push({ name, action });
  }
  return plan;
}

/** What is said about a drop of which nothing could be opened, or null when there is nothing to say (KiCad says nothing of a file its frame does not accept; the studio names it when nothing else of the drop is opened). */
export function dropMessage(tab: string, plan: DropPlan): string | null {
  if (plan.runs.length > 0) return null;
  const parts: string[] = [];
  if (plan.unavailable.length > 0) parts.push(`${plan.unavailable.map((u) => u.name).join(", ")} cannot be opened by a drop in the studio.`);
  if (plan.ignored.length > 0) {
    const ext = acceptedExtensions(tab);
    parts.push(`${plan.ignored.join(", ")} ${plan.ignored.length === 1 ? "is not a file" : "are not files"} this editor opens${ext.length > 0 ? ` (it takes ${ext.map((e) => `.${e}`).join(", ")})` : ""}.`);
  }
  return parts.length > 0 ? parts.join(" ") : null;
}

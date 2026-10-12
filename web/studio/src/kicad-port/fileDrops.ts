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

export interface DropPlan {
  /** The file to open and the action to run it with: the first accepted file the studio can open. */
  open: { name: string; action: string } | null;
  /** Accepted files after the first (KiCad runs each in turn; here one at a time, as an open dialog or a carried placement would meet the next). */
  extra: string[];
  /** Files the frame accepts that this studio cannot open by a drop (no handler), with the action's id. */
  unavailable: Array<{ name: string; action: string }>;
  /** Files of an extension the frame does not accept: ignored. */
  ignored: string[];
}

/** Sorts a drop's files by what the frame does with them. `isEnabled` says whether the studio handles an action on this tab. */
export function planDrop(tab: string, names: readonly string[], isEnabled: (action: string) => boolean): DropPlan {
  const plan: DropPlan = { open: null, extra: [], unavailable: [], ignored: [] };
  for (const name of names) {
    const action = dropActionFor(tab, name);
    if (!action) plan.ignored.push(name);
    else if (!isEnabled(action)) plan.unavailable.push({ name, action });
    else if (!plan.open) plan.open = { name, action };
    else plan.extra.push(name);
  }
  return plan;
}

/** What is said about a drop that was not wholly opened, or null when there is nothing to say. */
export function dropMessage(tab: string, plan: DropPlan): string | null {
  const parts: string[] = [];
  if (!plan.open) {
    if (plan.unavailable.length > 0) parts.push(`${plan.unavailable.map((u) => u.name).join(", ")} cannot be opened by a drop in the studio.`);
    if (plan.ignored.length > 0) {
      const ext = acceptedExtensions(tab);
      parts.push(`${plan.ignored.join(", ")} ${plan.ignored.length === 1 ? "is not a file" : "are not files"} this editor opens${ext.length > 0 ? ` (it takes ${ext.map((e) => `.${e}`).join(", ")})` : ""}.`);
    }
    return parts.length > 0 ? parts.join(" ") : null;
  }
  if (plan.extra.length > 0) parts.push(`Only ${plan.open.name} was opened; ${plan.extra.join(", ")} ${plan.extra.length === 1 ? "was" : "were"} skipped (drop one file at a time).`);
  return parts.length > 0 ? parts.join(" ") : null;
}

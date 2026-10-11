// Import Sheet... and Draw Sheet from File -- the pure parts of `SCH_DRAWING_TOOLS::ImportSheet` / `DrawSheet` (eeschema/tools/sch_drawing_tools.cpp) at 8303b2ad and of
// the file chooser's hook (`FILEDLG_IMPORT_SHEET_CONTENTS`, eeschema/widgets/sch_design_block_pane.cpp):
//
//   Import Sheet...        "Choose Schematic": a .kicad_sch file and four options -- Place repeated copies, Place as group, Place as sheet, Keep annotations.
//   Place as sheet         a click starts the sheet (named after the file, `fn.GetName()`, holding `<name>.kicad_sch`), the second click sizes it and opens
//                          `EditSheetProperties`; Add copies the file in (`LoadSheetFromFile`) and annotates the new sheet's symbols.
//   Place as contents      one click: the file's items come in on the sheet in view, carried to the click, and are annotated the same way.
//   Keep annotations       no annotation: the symbols keep their references (only the ones the design already has are numbered anew).
//
// The studio keeps all four options except Place as group (the schematic has no groups). The numbering is the one the Annotate dialog last held
// (`schSettings.m_AnnotateSortOrder` / `m_AnnotateMethod` / `m_AnnotateStartNum`, which `ImportSheet` reads). The file is read, and its sheets, library
// symbols and checks are the server's (crates/cli/src/sheet_import.rs, crates/ops/src/sch_sheets.rs).
import type { Cmd } from "../api/types";
import type { AnnotateOptions, SheetFileCheck } from "../api/hierarchyTypes";
import { startNumber, type AnnotateSettings } from "./schAnnotate";

/** What the chooser holds, and what the placement tool then does with the file. */
export interface SheetImportPlan {
  /** The schematic file: a path under the board's folder, or an absolute one. */
  path: string;
  /** "Place as sheet": a sheet symbol showing a copy of the file; off, its contents go on the sheet in view. */
  asSheet: boolean;
  /** "Keep annotations". */
  keepAnnotations: boolean;
  /** "Place repeated copies": the tool stays armed after a placement. */
  repeated: boolean;
}

/** The chooser's checkboxes as they start. KiCad stores `place_as_sheet` off; the studio starts with the sheet, which is what a file added to a project is for (the brief of
 * "Draw Sheet from File" is the sheet), and the choice is remembered between openings like KiCad's. */
export function defaultImportOptions(): Omit<SheetImportPlan, "path"> {
  return { asSheet: true, keepAnnotations: false, repeated: false };
}

/** The last component of a path, either separator. */
export function fileOfPath(path: string): string {
  const parts = path.trim().split(/[\\/]/);
  return parts[parts.length - 1] ?? "";
}

/** `wxFileName::GetName()`: the file without its folder and its last extension. */
export function stemOfFile(path: string): string {
  const file = fileOfPath(path);
  const dot = file.lastIndexOf(".");
  return dot > 0 ? file.slice(0, dot) : file;
}

/** The name `DrawSheet` gives a sheet drawn from a file: the file's name without its extension (`SHEET_NAME`). */
export function sheetNameOfFile(path: string): string {
  return stemOfFile(path);
}

/** The file `DrawSheet` gives it: the same stem with `.kicad_sch` (`SHEET_FILENAME`). */
export function sheetFileOfFile(path: string): string {
  return `${stemOfFile(path)}.kicad_sch`;
}

/** Whether a typed path can name a schematic at all (`wxFD_FILE_MUST_EXIST` is the server's to judge; this is only the extension and that something is there). */
export function looksLikeSchematic(path: string): boolean {
  const p = path.trim();
  return p !== "" && /\.kicad_sch$/i.test(p);
}

/** The numbering the placement uses: `ImportSheet` annotates with the Annotate dialog's last sort order, method and start number. */
export function importNumbering(s: Pick<AnnotateSettings, "sort" | "algo" | "start">): { order: "y_then_x" | "x_then_y"; options: AnnotateOptions } {
  return { order: s.sort === "x" ? "x_then_y" : "y_then_x", options: { algo: s.algo, start_number: startNumber(s.start) } };
}

/** `import_sheet` as a sheet: a sheet symbol `name` of `size` at `at` showing a copy of the file under `file`. */
export function importSheetCmd(
  plan: Pick<SheetImportPlan, "path" | "keepAnnotations">,
  sheet: { name: string; file: string; at: [number, number]; size: [number, number] },
  numbering: ReturnType<typeof importNumbering>
): Cmd {
  return {
    op: "import_sheet",
    source: plan.path.trim(),
    as_sheet: true,
    name: sheet.name.trim(),
    file: sheet.file.trim(),
    at: { x: sheet.at[0], y: sheet.at[1] },
    size: sheet.size,
    keep_annotations: plan.keepAnnotations,
    order: numbering.order,
    options: numbering.options,
  };
}

/** `import_sheet` as contents (`placeSheetContents`): the file's items on the sheet in view, carried to `at`. */
export function importContentsCmd(plan: Pick<SheetImportPlan, "path" | "keepAnnotations">, at: [number, number], numbering: ReturnType<typeof importNumbering>): Cmd {
  return {
    op: "import_sheet",
    source: plan.path.trim(),
    as_sheet: false,
    at: { x: at[0], y: at[1] },
    keep_annotations: plan.keepAnnotations,
    order: numbering.order,
    options: numbering.options,
  };
}

/** What the chooser says about a file before it is opened: how much comes in, and the findings of `LoadSheetFromFile`'s checks. */
export function describeCheck(check: SheetFileCheck): { summary: string; errors: string[]; warnings: string[] } {
  const sheets = check.sheets.length;
  const parts = [`${check.symbols} symbol${check.symbols === 1 ? "" : "s"}`];
  if (sheets > 0) parts.push(`${sheets} sheet${sheets === 1 ? "" : "s"} (${check.sheets.join(", ")})`);
  const libs = check.libraries.length;
  if (libs > 0) parts.push(`${libs} librar${libs === 1 ? "y" : "ies"} (${check.libraries.join(", ")})`);
  return {
    summary: parts.join(", "),
    errors: check.findings.filter((f) => f.severity === "error").map((f) => f.text),
    warnings: check.findings.filter((f) => f.severity !== "error").map((f) => f.text),
  };
}

/** The line the status bar shows while the tool waits for its click (the hint of the tool). */
export function importToolHint(plan: Pick<SheetImportPlan, "path" | "asSheet">): string {
  const file = fileOfPath(plan.path);
  return plan.asSheet
    ? `Import Sheet (${file}): click one corner of the sheet, then the opposite one, then name it. Esc to cancel`
    : `Import Sheet Contents (${file}): click where the items go. Esc to cancel`;
}

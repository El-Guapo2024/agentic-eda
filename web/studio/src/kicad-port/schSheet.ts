// Pure parts of the Draw Hierarchical Sheet tool (`eeschema.InteractiveDrawing.drawSheet`, `S`):
// `SCH_DRAWING_TOOLS::DrawSheet` and `sizeSheet` (eeschema/tools/sch_drawing_tools.cpp).
//
// The tool takes the sheet's top-left corner from the first click and sizes it from the cursor
// (`sizeSheet`): the size is the cursor minus the corner, never below the minimum
// (`MIN_SHEET_WIDTH` 500 mil by `MIN_SHEET_HEIGHT` 150 mil -- so dragging up or left gives the
// minimum, not a flipped sheet), and the far corner is then snapped to the grid. The second click
// opens `EditSheetProperties`; a new sheet starts as "Untitled Sheet" / "untitled.kicad_sch".

/** mil -> um. */
const MIL_NM = 25_400;
/** `MIN_SHEET_WIDTH` (500 mil), um. */
export const MIN_SHEET_WIDTH_NM = 500 * MIL_NM;
/** `MIN_SHEET_HEIGHT` (150 mil), um. */
export const MIN_SHEET_HEIGHT_NM = 150 * MIL_NM;

/** The name and file a sheet is created with (`SHEET_NAME` / `SHEET_FILENAME` fields of a fresh sheet). */
export const DEFAULT_SHEET_NAME = "Untitled Sheet";
export const DEFAULT_SHEET_FILE = "untitled.kicad_sch";

/** `sizeSheet( sheet, cursor )`: the sheet's `[width, height]` for a first corner at `start` and the cursor at `cursor`. */
export function sheetSize(start: readonly [number, number], cursor: readonly [number, number], gridNm: number): [number, number] {
  const w = Math.max(cursor[0] - start[0], MIN_SHEET_WIDTH_NM);
  const h = Math.max(cursor[1] - start[1], MIN_SHEET_HEIGHT_NM);
  // `GetNearestGridPosition( pos + size )` then `Resize( grid - pos )`
  const snap = (v: number) => (gridNm > 0 ? Math.round(v / gridNm) * gridNm : v);
  return [snap(start[0] + w) - start[0], snap(start[1] + h) - start[1]];
}

/**
 * A sheet name no sheet on the page uses yet: "Untitled Sheet", "Untitled Sheet 2", ... INACTIVE since UI-LEFT S21: the sheet dialog names every new sheet "Untitled
 * Sheet" as KiCad does (a name another sheet has is allowed, ERC reports it); kept for a caller that wants a free name.
 */
export function uniqueSheetName(taken: readonly string[], base = DEFAULT_SHEET_NAME): string {
  const used = new Set(taken);
  if (!used.has(base)) return base;
  for (let i = 2; ; i++) if (!used.has(`${base} ${i}`)) return `${base} ${i}`;
}

/**
 * The file a sheet named `name` gets by default: the name made into a file name ("Power Supply" -> "power_supply.kicad_sch"), `untitled.kicad_sch` for the default
 * name. INACTIVE since UI-LEFT S21: KiCad's sheet dialog does not make the file follow the name.
 */
export function defaultSheetFile(name: string): string {
  const stem = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return stem ? `${stem}.kicad_sch` : DEFAULT_SHEET_FILE;
}

/** The file a sheet's "File name" field stands for: `EnsureFileExtension( name, "kicad_sch" )`. */
export function sheetFileWithExtension(file: string): string {
  const f = file.trim();
  return f.toLowerCase().endsWith(".kicad_sch") ? f : `${f}.kicad_sch`;
}

/**
 * `SCH_EDIT_FRAME::ChangeSheetFile` for a new sheet (8303b2ad eeschema/sheet.cpp): when the hierarchy already shows the file (`SearchHierarchy`) or the project
 * keeps a screen of that file no sheet places (`loadFromFile = wxFileExists( newAbsoluteFilename )`: the studio's project keeps a deleted sheet's screen in
 * `design.sheet_contents` as a file stays on disk beside a KiCad project; `screens` lists them, UI-LEFT-2 SS15), the question it asks before linking the new sheet
 * to that screen -- "'%s' already exists.\n\nLink '%s' to this file?" (the studio has no absolute path to show: both are the file's name); null when the file
 * is new (the sheet gets an empty page of its own).
 */
export function linkQuestion(file: string, hierarchy: ReadonlyArray<{ file: string }>, screens: readonly string[] = []): string | null {
  const f = sheetFileWithExtension(file);
  if (!hierarchy.some((s) => s.file === f) && !screens.includes(f)) return null;
  return `'${f}' already exists.\n\nLink '${f}' to this file?`;
}

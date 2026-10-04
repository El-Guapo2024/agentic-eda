// Pure parts of the Draw Hierarchical Sheet tool (`eeschema.InteractiveDrawing.drawSheet`, `S`):
// `SCH_DRAWING_TOOLS::DrawSheet` and `sizeSheet` (eeschema/tools/sch_drawing_tools.cpp).
//
// The tool takes the sheet's top-left corner from the first click and sizes it from the cursor
// (`sizeSheet`): the size is the cursor minus the corner, never below the minimum
// (`MIN_SHEET_WIDTH` 500 mil by `MIN_SHEET_HEIGHT` 150 mil -- so dragging up or left gives the
// minimum, not a flipped sheet), and the far corner is then snapped to the grid. The second click
// opens `EditSheetProperties`; a new sheet starts as "Untitled Sheet" / "untitled.kicad_sch".

/** mil -> um. */
const MIL_UM = 25.4;
/** `MIN_SHEET_WIDTH` (500 mil), um. */
export const MIN_SHEET_WIDTH_UM = 500 * MIL_UM;
/** `MIN_SHEET_HEIGHT` (150 mil), um. */
export const MIN_SHEET_HEIGHT_UM = 150 * MIL_UM;

/** The name and file a sheet is created with (`SHEET_NAME` / `SHEET_FILENAME` fields of a fresh sheet). */
export const DEFAULT_SHEET_NAME = "Untitled Sheet";
export const DEFAULT_SHEET_FILE = "untitled.kicad_sch";

/** `sizeSheet( sheet, cursor )`: the sheet's `[width, height]` for a first corner at `start` and the cursor at `cursor`. */
export function sheetSize(start: readonly [number, number], cursor: readonly [number, number], gridUm: number): [number, number] {
  const w = Math.max(cursor[0] - start[0], MIN_SHEET_WIDTH_UM);
  const h = Math.max(cursor[1] - start[1], MIN_SHEET_HEIGHT_UM);
  // `GetNearestGridPosition( pos + size )` then `Resize( grid - pos )`
  const snap = (v: number) => (gridUm > 0 ? Math.round(v / gridUm) * gridUm : v);
  return [snap(start[0] + w) - start[0], snap(start[1] + h) - start[1]];
}

/** A sheet name no sheet on the page uses yet (`EditSheetProperties` refuses a name already in use): "Untitled Sheet", "Untitled Sheet 2", ... */
export function uniqueSheetName(taken: readonly string[], base = DEFAULT_SHEET_NAME): string {
  const used = new Set(taken);
  if (!used.has(base)) return base;
  for (let i = 2; ; i++) if (!used.has(`${base} ${i}`)) return `${base} ${i}`;
}

/** The file a sheet named `name` gets by default: the name made into a file name ("Power Supply" -> "power_supply.kicad_sch"), `untitled.kicad_sch` for the default name. */
export function defaultSheetFile(name: string): string {
  const stem = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return stem ? `${stem}.kicad_sch` : DEFAULT_SHEET_FILE;
}

// useGlobalHotkeys.ts's `hotkeyIndex.get(combo)?.find(isEnabled)`: when a
// physical key is double-booked by one `pcbnew.*` and one `eeschema.*`
// action with the *same* extracted hotkey (R, M, G, X, E, U, V, F all
// collide this way in the real table -- `hotkeyIndex.get` returns both),
// whichever name comes first in actions.json's own order used to win on
// *every* tab, because a registered-but-tab-irrelevant handler (every
// `pcbOnly`/`schematicOnly` wrapper is itself always registered -- only
// its function *body* checks the tab, and only once actually called) was
// indistinguishable from a truly-applicable one. `pcbnew.InteractiveEdit.
// rotateCcw` (R) and `pcbnew.InteractiveMove.move` (M) were already
// losing that race on the PCB tab before this module existed -- real,
// silent regressions from the schematic port's own earlier work, not a
// hypothetical. This is the fix, pulled out of useActionRunner.ts's own
// `isEnabled` so the actual decision is unit-testable without a React
// render: gate on the action name's own module prefix, the established
// "pcbnew.X"/"eeschema.X"/"common.X" naming convention every action in
// this table already follows.
//
// Two of this app's tabs are not the editor their action prefix names:
// KiCad has the Footprint Editor and the Symbol Editor as separate frames,
// whose actions still carry the `pcbnew.`/`eeschema.` prefix (Ctrl+N is
// `pcbnew.ModuleEditor.newFootprint` in the footprint editor and
// `eeschema.SymbolLibraryControl.newSymbol` in the symbol editor, next to
// `common.Control.new` everywhere). Those few actions are listed below by
// name and gated to their own tab.

/** Footprint-editor-frame-only actions (`FOOTPRINT_EDIT_FRAME`'s tools): enabled on the Footprint tab only. */
export const FOOTPRINT_EDITOR_ONLY: ReadonlySet<string> = new Set([
  "pcbnew.ModuleEditor.newFootprint",
  "pcbnew.InteractiveDrawing.setAnchor",
  // A `.kicad_mod` dropped on the Footprint Editor (`FOOTPRINT_EDITOR_CONTROL::DdImportFootprint`; the board editor's frame does not take one).
  "pcbnew.Control.ddImportFootprint",
]);

/** Actions the board editor AND the footprint editor both register (`DRAWING_TOOL` / `EDIT_TOOL` run in both frames): enabled on the PCB and Footprint tabs. */
export const BOARD_AND_FOOTPRINT: ReadonlySet<string> = new Set([
  "pcbnew.InteractiveDrawing.arcPosture",
  "pcbnew.InteractiveDrawing.bezier",
  "pcbnew.InteractiveEdit.duplicateIncrementPads",
  // Create Array (Ctrl+T): `ARRAY_TOOL` is registered by both frames; the footprint editor's has the pad numbering panels.
  "pcbnew.Array.createArray",
  // the drawing tools of the footprint editor's Place menu
  "pcbnew.InteractiveDrawing.line",
  "pcbnew.InteractiveDrawing.arc",
  "pcbnew.InteractiveDrawing.rectangle",
  "pcbnew.InteractiveDrawing.circle",
  "pcbnew.InteractiveDrawing.graphicPolygon",
  "pcbnew.InteractiveDrawing.text",
  // Rotate Counterclockwise / Clockwise (R / Shift+R): the footprint editor's top toolbar rotates the selected pads (actions/editorFrameActions.ts).
  "pcbnew.InteractiveEdit.rotateCcw",
  "pcbnew.InteractiveEdit.rotateCw",
  // Properties (E): `EDIT_TOOL::Properties`, a pad's Pad Properties or, with nothing selected, Footprint Properties (actions/editorFrameActions.ts).
  "pcbnew.InteractiveEdit.properties",
]);

/** Symbol-editor-frame-only actions (`SYMBOL_EDIT_FRAME`'s tools): enabled on the Symbol tab only. */
export const SYMBOL_EDITOR_ONLY: ReadonlySet<string> = new Set([
  "eeschema.SymbolDrawing.placeSymbolPin",
  "eeschema.SymbolLibraryControl.newSymbol",
  "eeschema.SymbolLibraryControl.saveLibraryAs",
  // Next / Previous Symbol Unit are `SYMBOL_EDITOR_CONTROL::ChangeUnit` (`if( !m_isSymbolEditor ) return 0`), the Symbol Checker is the editor's Inspect menu.
  "eeschema.EditorControl.nextUnit",
  "eeschema.EditorControl.previousUnit",
  "eeschema.InspectionTool.checkSymbol",
  // `SYMBOL_EDITOR_EDIT_TOOL`'s stacked-pin tools.
  "eeschema.InteractiveEdit.convertStackedPins",
  "eeschema.InteractiveEdit.explodeStackedPin",
  // View > Show Pin Alternate Icons (`SYMBOL_EDITOR_CONTROL::TogglePinAltIcons`): a pin of a library symbol has alternates, a pin of a placed symbol is read from the engine's table, which has none.
  "eeschema.EditorControl.togglePinAltIcons",
]);

/**
 * `common.*` actions that act on a whole document and exist for the two document editors only: the board editor and
 * the schematic editor.
 */
export const BOARD_AND_SCHEMATIC_ONLY: ReadonlySet<string> = new Set([
  "common.Control.updatePcbFromSchematic",
  // Edit Table... (`ACTIONS::editTable`, `EDIT_TABLE_TOOL_BASE`): one common action that the board's and the sheet's table tools both run, though its name says pcbnew.
  "pcbnew.TableEditor.editTable",
]);

/**
 * Save As (`ACTIONS::saveAs`) is the File menu's entry of the board editor, the schematic editor and the Footprint Editor
 * (`FOOTPRINT_EDITOR_CONTROL::SaveAs`). The Symbol Editor's Ctrl+Shift+S is `saveLibraryAs` (above), so Save As must not be live there.
 */
export const SAVE_AS = "common.Control.saveAs";

/**
 * The tool groups that exist for one library editor only: `FOOTPRINT_EDITOR_CONTROL` (`pcbnew.ModuleEditor.*`) and `PAD_TOOL` (`pcbnew.PadTool.*`,
 * which edits pads, and the studio's board has no pads of its own to edit), and `SYMBOL_EDITOR_CONTROL` (`eeschema.SymbolLibraryControl.*`),
 * `SYMBOL_EDITOR_DRAWING_TOOLS` (`eeschema.SymbolDrawing.*`) and `SYMBOL_EDITOR_PIN_TOOL` (`eeschema.PinEditing.*`).
 */
export const FOOTPRINT_EDITOR_TOOL_PREFIXES: readonly string[] = ["pcbnew.ModuleEditor.", "pcbnew.PadTool."];
export const SYMBOL_EDITOR_TOOL_PREFIXES: readonly string[] = ["eeschema.SymbolLibraryControl.", "eeschema.SymbolDrawing.", "eeschema.PinEditing."];

/**
 * Actions the schematic editor and the symbol editor both register (`symbolProperties` is in `SCH_EDIT_TOOL` and `SYMBOL_EDITOR_EDIT_TOOL`, the shape
 * tools in `SCH_DRAWING_TOOLS` and `SYMBOL_EDITOR_DRAWING_TOOLS`, `pinTable` is the symbol editor's Edit menu entry): enabled on the Schematic and Symbol tabs.
 */
export const SCHEMATIC_AND_SYMBOL_EDITOR: ReadonlySet<string> = new Set([
  "eeschema.InteractiveEdit.symbolProperties",
  "eeschema.InteractiveEdit.pinTable",
  "eeschema.InteractiveDrawing.drawRectangle",
  "eeschema.InteractiveDrawing.drawCircle",
  "eeschema.InteractiveDrawing.drawArc",
  // `SCH_DRAWING_TOOLS::ImportGraphics` and `SYMBOL_EDITOR_DRAWING_TOOLS::ImportGraphics` are both `SCH_ACTIONS::importGraphics` (and its drop).
  "eeschema.EditorControl.importGraphics",
  "eeschema.EditorControl.ddImportGraphics",
  // `SYMBOL_EDITOR_EDIT_TOOL::Properties` and `::Rotate` are bound to the schematic editor's own actions (E, R, Shift+R): actions/editorFrameActions.ts.
  "eeschema.InteractiveEdit.properties",
  "eeschema.InteractiveEdit.rotateCCW",
  "eeschema.InteractiveEdit.rotateCW",
]);

export function isActionEnabledForTab(name: string, tab: string, registered: boolean): boolean {
  if (!registered) return false;
  if (FOOTPRINT_EDITOR_ONLY.has(name)) return tab === "footprint";
  if (SYMBOL_EDITOR_ONLY.has(name)) return tab === "symbol";
  if (FOOTPRINT_EDITOR_TOOL_PREFIXES.some((p) => name.startsWith(p))) return tab === "footprint";
  if (SYMBOL_EDITOR_TOOL_PREFIXES.some((p) => name.startsWith(p))) return tab === "symbol";
  if (SCHEMATIC_AND_SYMBOL_EDITOR.has(name)) return tab === "schematic" || tab === "symbol";
  if (BOARD_AND_SCHEMATIC_ONLY.has(name)) return tab === "pcb" || tab === "schematic";
  if (name === SAVE_AS) return tab === "pcb" || tab === "schematic" || tab === "footprint";
  if (BOARD_AND_FOOTPRINT.has(name)) return tab === "pcb" || tab === "footprint";
  if (name.startsWith("pcbnew.")) return tab === "pcb";
  if (name.startsWith("eeschema.")) return tab === "schematic";
  return true; // common.*, and anything else with no tab of its own
}

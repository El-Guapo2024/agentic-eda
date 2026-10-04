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
export const FOOTPRINT_EDITOR_ONLY: ReadonlySet<string> = new Set(["pcbnew.ModuleEditor.newFootprint", "pcbnew.InteractiveDrawing.setAnchor"]);

/** Actions the board editor AND the footprint editor both register (`DRAWING_TOOL` / `EDIT_TOOL` run in both frames): enabled on the PCB and Footprint tabs. */
export const BOARD_AND_FOOTPRINT: ReadonlySet<string> = new Set(["pcbnew.InteractiveDrawing.arcPosture", "pcbnew.InteractiveDrawing.bezier", "pcbnew.InteractiveEdit.duplicateIncrementPads"]);

/** Symbol-editor-frame-only actions (`SYMBOL_EDIT_FRAME`'s tools): enabled on the Symbol tab only. */
export const SYMBOL_EDITOR_ONLY: ReadonlySet<string> = new Set(["eeschema.SymbolDrawing.placeSymbolPin", "eeschema.SymbolLibraryControl.newSymbol", "eeschema.SymbolLibraryControl.saveLibraryAs"]);

/**
 * `common.*` actions that act on a whole document and exist for the two document editors only: the board editor and
 * the schematic editor. The Symbol Editor's Ctrl+Shift+S is `saveLibraryAs` (above), so Save As must not be live there.
 */
export const BOARD_AND_SCHEMATIC_ONLY: ReadonlySet<string> = new Set(["common.Control.saveAs", "common.Control.updatePcbFromSchematic"]);

export function isActionEnabledForTab(name: string, tab: string, registered: boolean): boolean {
  if (!registered) return false;
  if (FOOTPRINT_EDITOR_ONLY.has(name)) return tab === "footprint";
  if (SYMBOL_EDITOR_ONLY.has(name)) return tab === "symbol";
  if (BOARD_AND_SCHEMATIC_ONLY.has(name)) return tab === "pcb" || tab === "schematic";
  if (BOARD_AND_FOOTPRINT.has(name)) return tab === "pcb" || tab === "footprint";
  if (name.startsWith("pcbnew.")) return tab === "pcb";
  if (name.startsWith("eeschema.")) return tab === "schematic";
  return true; // common.*, and anything else with no tab of its own
}

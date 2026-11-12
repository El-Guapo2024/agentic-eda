// The enable conditions the Footprint Editor's and the Symbol Editor's frames give their actions (`FOOTPRINT_EDIT_FRAME::setupUIConditions`,
// pcbnew/footprint_edit_frame.cpp; `SYMBOL_EDIT_FRAME::setupUIConditions`, eeschema/symbol_editor/symbol_edit_frame.cpp), for the actions whose handlers
// live in actions/libraryEditorActions.ts. The handlers refuse what is disabled here silently, as KiCad's handlers are never reached for a disabled
// action; a menu or toolbar that asks this greys the entry out instead. `undefined`: the action has no condition here (enabled).

export interface FpEnableState {
  /** `haveFootprintCond`: `GetBoard()->GetFirstFootprint() != nullptr`. */
  footprintLoaded: boolean;
}

export interface SymEnableState {
  /** `haveSymbolCond`: `m_symbol`. */
  symbolLoaded: boolean;
  /** `IsSymbolAlias()`: the loaded symbol is derived from another one. */
  derived: boolean;
  /** `m_symbol->IsMultiUnit()`. */
  unitCount: number;
  /** `m_symbol->UnitsLocked()` ("All units are interchangeable" off). */
  unitsLocked: boolean;
}

/** `ENABLE( haveFootprintCond )` and the `CURRENT_EDIT_TOOL` rows of `FOOTPRINT_EDIT_FRAME::setupUIConditions`. */
const FP_NEEDS_FOOTPRINT = new Set([
  "pcbnew.ModuleEditor.exportFootprint",
  "pcbnew.ModuleEditor.padTable",
  "pcbnew.ModuleEditor.checkFootprint",
  "pcbnew.ModuleEditor.repairFootprint",
  "pcbnew.PadTool.placePad",
]);

/** `EDIT_TOOL( .. )` = `Enable( isEditableCond )` rows of `SYMBOL_EDIT_FRAME::setupUIConditions` (`IsSymbolEditable() && !IsSymbolAlias()`). */
const SYM_NEEDS_EDITABLE = new Set([
  "eeschema.SymbolDrawing.drawSymbolLines",
  "eeschema.SymbolDrawing.drawSymbolPolygon",
  "eeschema.SymbolDrawing.drawSymbolTextBox",
  "eeschema.SymbolDrawing.placeSymbolAnchor",
  "eeschema.SymbolDrawing.placeSymbolText",
]);

export function fpActionEnabled(name: string, s: FpEnableState): boolean | undefined {
  if (FP_NEEDS_FOOTPRINT.has(name)) return s.footprintLoaded;
  return undefined;
}

/** `isEditableCond`: a symbol is loaded and it is not an alias (the studio's library symbols are all editable copies: `IsSymbolEditable()`). */
export function symEditable(s: SymEnableState): boolean {
  return s.symbolLoaded && !s.derived;
}

export function symActionEnabled(name: string, s: SymEnableState): boolean | undefined {
  if (SYM_NEEDS_EDITABLE.has(name)) return symEditable(s);
  // `multiUnitModeCond`: `m_symbol && m_symbol->IsMultiUnit() && !m_symbol->UnitsLocked()`
  if (name === "eeschema.SymbolLibraryControl.toggleSyncedPinsMode") return s.symbolLoaded && s.unitCount > 1 && !s.unitsLocked;
  return undefined;
}

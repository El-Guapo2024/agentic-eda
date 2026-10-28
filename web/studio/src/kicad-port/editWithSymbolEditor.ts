// The symbol Ctrl+E / Ctrl+Shift+E open in the Symbol Editor (`SCH_EDITOR_CONTROL::EditWithSymbolEditor`, eeschema/tools/sch_editor_control.cpp at 8303b2ad):
//
//   SCH_SELECTION& selection = selTool->RequestSelection( { SCH_SYMBOL_T } );
//   if( selection.GetSize() >= 1 ) symbol = (SCH_SYMBOL*) selection.Front();
//   ...
//   if( symbol->IsMissingLibSymbol() ) { ShowInfoBarError( _( "Symbols with broken library symbol links cannot be edited." ) ); return 0; }
//
// `RequestSelection` keeps the selected symbols (the first one selected is `Front()`), or, with none selected, takes the one under the cursor
// (a hover selection, cleared after). A symbol whose library symbol is not in the schematic's cache (`IsMissingLibSymbol`) is refused.
// (r6 C6-M3. KiCad's `editWithLibEdit` loads the placed symbol's own copy and saves it back to that symbol and the units of its reference,
// `SaveSymbolToSchematic`; this IR has one library symbol per `lib_id`, so both actions open the shared entry: PARITY-sch's Differs row.)

export const BROKEN_LINK_MESSAGE = "Symbols with broken library symbol links cannot be edited.";

/** `open`'s `unit` / `bodyStyle`: the placed symbol's, which the editor shows (`SYMBOL_EDIT_FRAME::LoadSymbolFromSchematic`: `m_unit = std::max( 1,
 *  aSymbol->GetUnit() ); m_bodyStyle = std::max( 1, aSymbol->GetBodyStyle() );`, and Ctrl+Shift+E's `LoadSymbol( symbol->GetLibId(), symbol->GetUnit(),
 *  symbol->GetBodyStyle() )`, r7 A7-M2). */
export type SymbolEditorTarget = { kind: "none" } | { kind: "broken"; id: string } | { kind: "open"; id: string; libId: string; unit: number; bodyStyle: number };

/**
 * `selection`: the selected ids in selection order; `symbols`: the sheet's placed symbols (power symbols too, they are `SCH_SYMBOL`s);
 * `hasLibSymbol`: whether the schematic's library cache holds a `lib_id`; `hovered`: the symbol under the cursor, if any, by id or as the placed unit itself.
 * A selected reference is the unit under the cursor when that is one of its units, else its first placed unit on the sheet (the studio selects a
 * reference, not one unit of it: **Differs** from `selection.Front()`, the unit itself).
 */
export function symbolEditorTarget(
  selection: readonly string[],
  symbols: ReadonlyArray<{ id: string; lib_id?: string | null; unit?: number; body_style?: number }>,
  hasLibSymbol: (libId: string) => boolean,
  hovered: string | { id: string; lib_id?: string | null; unit?: number; body_style?: number } | null
): SymbolEditorTarget {
  const byId = new Map<string, (typeof symbols)[number]>();
  for (const s of symbols) if (!byId.has(s.id)) byId.set(s.id, s);
  const hoveredId = typeof hovered === "string" || hovered == null ? hovered : hovered.id;
  const selected = selection.filter((id) => byId.has(id));
  // a selection with no symbol in it is not a hover: `RequestSelection` filters it to symbols, and it is empty only when nothing was selected at all
  const id = selected[0] ?? (selection.length === 0 ? hoveredId : null);
  // the unit under the cursor stands for the reference when it is the one in question (the studio selects a reference, not one of its units)
  const sym = hovered != null && typeof hovered !== "string" && hovered.id === id ? hovered : id != null ? byId.get(id) : undefined;
  if (!sym) return { kind: "none" };
  const libId = sym.lib_id?.trim() ?? "";
  if (!libId || !hasLibSymbol(libId)) return { kind: "broken", id: sym.id };
  return { kind: "open", id: sym.id, libId, unit: Math.max(1, sym.unit ?? 1), bodyStyle: Math.max(1, sym.body_style ?? 1) };
}

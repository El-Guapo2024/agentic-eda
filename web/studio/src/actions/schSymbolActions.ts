// Change Symbol(s) and Update Symbol(s) -- `SCH_EDIT_TOOL::ChangeSymbols` (eeschema/tools/sch_edit_tool.cpp at 8303b2ad): both open `DIALOG_CHANGE_SYMBOLS`
// (components/SchChangeSymbolsDialog.tsx) on the selected symbol, in change or update mode.
import { schematicActions, type ActionMap } from "./schActionRegistry";
import type { SchEditContext } from "./schEditActions";

export function registerSchSymbolActions(registry: ActionMap, ctx: SchEditContext): void {
  const { state, dispatch, requestSelection } = ctx;
  const m = schematicActions(registry, state.tab);
  const sch = state.schematic;
  const open = (mode: "change" | "update") => () => {
    if (state.tab !== "schematic" || !sch) return;
    // `RequestSelection( { SCH_SYMBOL_T } )`: the selected symbols, or the one under the cursor.
    const selected = [...new Set(requestSelection().filter((id) => sch.symbols.some((s) => s.id === id)))];
    dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "change_symbols", mode, selected } });
  };
  m.set("eeschema.InteractiveEdit.changeSymbol", open("change"));
  m.set("eeschema.InteractiveEdit.changeSymbols", open("change"));
  m.set("eeschema.InteractiveEdit.updateSymbol", open("update"));
  m.set("eeschema.InteractiveEdit.updateSymbols", open("update"));

  // Edit Text & Graphics Properties... -- SCH_EDIT_TOOL::GlobalEdit -> DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS (components/SchGlobalEditDialog.tsx), on the selection or the whole sheet.
  m.set("eeschema.InteractiveEdit.editTextAndGraphics", () => {
    if (state.tab === "schematic" && sch) dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "edit_text_graphics", selected: [...state.selection] } });
  });
}

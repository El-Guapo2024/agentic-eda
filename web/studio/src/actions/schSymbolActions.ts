// Change Symbol(s) and Update Symbol(s) -- `SCH_EDIT_TOOL::ChangeSymbols` (eeschema/tools/sch_edit_tool.cpp at 8303b2ad): both open `DIALOG_CHANGE_SYMBOLS`
// (components/SchChangeSymbolsDialog.tsx) on the selected symbol, in change or update mode.
import type { SchEditContext } from "./schEditActions";

export function registerSchSymbolActions(m: Map<string, () => void>, ctx: SchEditContext): void {
  const { state, dispatch, requestSelection } = ctx;
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
}

// Change Symbol(s) and Update Symbol(s) -- `SCH_EDIT_TOOL::ChangeSymbols` (eeschema/tools/sch_edit_tool.cpp at 8303b2ad): both open `DIALOG_CHANGE_SYMBOLS`
// (components/SchChangeSymbolsDialog.tsx) on the selected symbol, in change or update mode.
import { schematicActions, type ActionMap } from "./schActionRegistry";
import type { SchEditContext } from "./schEditActions";
import { parseSelectUnitArg, selectUnit, SELECT_UNIT_ACTION } from "../kicad-port/schSymbolMenus";

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
  // KiCad 25ec2335 `SCH_EDIT_TOOL::ChangeSymbols`: "Exchanging the base symbol under an active variant would rewrite all variants while the canvas shows
  // variant-resolved values, so require the default variant."
  const change = () => {
    if (state.tab !== "schematic" || !sch) return;
    if (state.schVariant !== "") {
      window.alert("Change Symbol is not available when a design variant is active. Use Set Variant Symbol or switch to the default variant first.");
      return;
    }
    open("change")();
  };
  m.set("eeschema.InteractiveEdit.changeSymbol", change);
  m.set("eeschema.InteractiveEdit.changeSymbols", change);

  // Set Variant Symbol... / Clear Variant Symbol -- KiCad 25ec2335 `SCH_EDIT_TOOL::SetVariantSymbol` / `ClearVariantSymbol` (UI-LEFT-2 SS16): the first selected symbol
  // (`RequestSelection( { SCH_SYMBOL_T } )`, `selection.Front()`), nothing in the default variant (`if( variantName.IsEmpty() ) return 0;`); Set opens the symbol chooser on
  // the symbol's own library id, Clear does nothing for a symbol with no variant symbol in the variant.
  const firstSymbol = () => {
    if (state.tab !== "schematic" || !sch || state.schVariant === "") return null;
    const id = requestSelection().find((x) => sch.symbols.some((s) => s.id === x));
    return id ? sch.symbols.find((s) => s.id === id)! : null;
  };
  m.set("eeschema.InteractiveEdit.setVariantSymbol", () => {
    const sym = firstSymbol();
    if (sym) dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "variant_symbol", id: sym.id, variant: state.schVariant, libId: sym.lib_id ?? "" } });
  });
  m.set("eeschema.InteractiveEdit.clearVariantSymbol", () => {
    const sym = firstSymbol();
    if (sym?.variants?.[state.schVariant]?.symbol_override) void ctx.api.cmd({ op: "set_variant_symbol", variant: state.schVariant, id: sym.id, lib_id: null });
  });
  m.set("eeschema.InteractiveEdit.updateSymbol", open("update"));
  m.set("eeschema.InteractiveEdit.updateSymbols", open("update"));

  // The Symbol Unit submenu (`SYMBOL_UNIT_MENU`, `ID_POPUP_SCH_SELECT_UNIT` -> `SCH_EDIT_FRAME::SelectUnit( symbol, unit )`): the placed unit the menu was opened on draws the
  // unit chosen ("Change Unit"); a unit the reference has placed on the sheet asks first ("Unit Already Placed"). The argument is "<placed unit>:<unit chosen>".
  m.set(SELECT_UNIT_ACTION, (arg) => {
    const pick = parseSelectUnitArg(arg);
    if (state.tab !== "schematic" || !sch || !pick) return;
    const ref = [...new Set(requestSelection().filter((id) => sch.symbols.some((s) => s.id === id)))];
    if (ref.length !== 1) return;
    // `PathHumanReadable( true, true )` of the sheet, "Root" for the top one
    const sheetName = (sch.sheet_path ?? []).map((p) => p.name).join("/");
    const r = selectUnit(sch, ref[0]!, pick.unit, pick.newUnit, sheetName ? `/${sheetName}` : "");
    if (r.do === "cmd") void ctx.api.cmd(r.cmd);
    else if (r.do === "ask") dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "unit_placed", id: ref[0]!, unit: pick.unit, newUnit: r.newUnit, message: r.message, swapLabel: r.swapLabel, duplicateLabel: r.duplicateLabel } });
  });

  // Edit Text & Graphics Properties... -- SCH_EDIT_TOOL::GlobalEdit -> DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS (components/SchGlobalEditDialog.tsx), on the selection or the whole sheet.
  m.set("eeschema.InteractiveEdit.editTextAndGraphics", () => {
    if (state.tab === "schematic" && sch) dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "edit_text_graphics", selected: [...state.selection] } });
  });
}

// Which properties dialog Properties (E) opens in the two library editors, from what is selected. Pure: the actions (actions/editorFrameActions.ts) open it.
//
//   Footprint Editor   `EDIT_TOOL::Properties` (pcbnew/tools/edit_tool.cpp at 8303b2ad), the same tool as the board editor's: one item selected is that item's
//                      dialog (`OnEditItemRequest`; a pad's is Pad Properties), nothing selected is `PCB_ACTIONS::footprintProperties`, several open nothing.
//                      The studio's footprint editor has a dialog for a pad only; a graphic or a text of the footprint opens nothing yet.
//   Symbol Editor      `SYMBOL_EDITOR_EDIT_TOOL::Properties` (eeschema/tools/symbol_editor_edit_tool.cpp): a pin is Pin Properties, a shape its Shape (or
//                      Text Box) Properties, a field Symbol Properties on that field's row, nothing selected Symbol Properties, several nothing.

export interface Selected {
  id: string;
  kind: string;
}

export type FootprintPropertiesTarget = { dialog: "pad"; id: string } | { dialog: "footprint" };
export type SymbolPropertiesTarget = { dialog: "pin"; id: string } | { dialog: "graphic"; id: string } | { dialog: "symbol"; field?: string };

export function footprintPropertiesTarget(selection: readonly Selected[]): FootprintPropertiesTarget | null {
  if (selection.length === 0) return { dialog: "footprint" };
  if (selection.length === 1 && selection[0]!.kind === "pad") return { dialog: "pad", id: selection[0]!.id };
  return null;
}

export function symbolPropertiesTarget(selection: readonly Selected[]): SymbolPropertiesTarget | null {
  if (selection.length === 0) return { dialog: "symbol" };
  if (selection.length !== 1) return null;
  const [only] = selection as [Selected];
  if (only.kind === "pin") return { dialog: "pin", id: only.id };
  if (only.kind === "graphic") return { dialog: "graphic", id: only.id };
  if (only.kind === "field") return { dialog: "symbol", field: only.id };
  return null;
}

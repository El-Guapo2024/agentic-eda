// The board editor's tools for the item kinds GAPS.md item 24 (package P16) added: Draw Text Box today; Place Reference Image, Place Point, Add Barcode and Rehatch Shapes as
// their steps land. Called from `registerPcbEditSweep`. Each tool is a run of picker sessions (kicad-port/pickerHost.ts: the next canvas click answers the session, Escape
// ends it) -- `DRAWING_TOOL`'s own loop of clicks is a loop of picks here -- and ends in the item's dialog.
//
//   pcbnew.InteractiveDrawing.textbox   DRAWING_TOOL::DrawRectangle with PCB_ACTIONS::drawTextBox, then DIALOG_TEXTBOX_PROPERTIES (OK adds the box, Cancel throws it away)
import { newTextBox } from "../kicad-port/pcbTextBox";
import { activeLayerName } from "../kicad-port/layerPresets";
import { picker } from "./pcbPicker";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";
import { setToolPreview } from "../components/canvas/pcbItemsPaint";
import { openTextBoxDialog } from "../state/pcbItemDialogs";

export function registerPcbItemActions(m: Map<string, () => void>, ctx: SweepCtx): void {
  const { pcbOnly } = createSweepHelpers(ctx);

  m.set("pcbnew.InteractiveDrawing.textbox", pcbOnly(() => startTextBoxTool(ctx)));
}

/**
 * `DRAWING_TOOL::DrawRectangle( drawTextBox )`: click one corner, click the opposite one -- the box follows the pointer in between -- and Text Box Properties opens on the box
 * the layer's text defaults make. Clicking the same spot twice, or Escape, ends the tool; after OK or Cancel the tool waits for the next box.
 */
function startTextBoxTool(ctx: SweepCtx): void {
  const { api, dispatch } = ctx;
  const layer = () => activeLayerName(api.getState().activeLayer, "F.SilkS");
  const end = () => setToolPreview(null);
  const first = (): void => {
    picker.start({
      kind: "point",
      prompt: "Text Box: click one corner of the box",
      onPoint: (a) => {
        setToolPreview({ kind: "rect", from: [a.x, a.y], layer: layer() });
        return {
          kind: "point",
          prompt: "Text Box: click the opposite corner",
          onPoint: (b) => {
            end();
            const board = api.getState().board;
            if (!board || (a.x === b.x && a.y === b.y)) return false;
            openTextBoxDialog({ id: null, box: newTextBox(board, layer(), [a.x, a.y], [b.x, b.y]), onClose: first });
            return false;
          },
          onCancel: end,
        };
      },
      onCancel: end,
    });
  };
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  first();
}

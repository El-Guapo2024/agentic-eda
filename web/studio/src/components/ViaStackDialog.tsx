// The Microvia Stack dialog (KiCad 25ec2335 `DIALOG_MICROVIA_STACK`, pcbnew/dialogs/dialog_microvia_stack.cpp; UI-LEFT-3 X3-1) on a placed stack
// (`PCB_VIA_STACK::ShowPropertiesDialog`: "Dialog edits a scratch copy so the undo snapshot below sees the pre-edit settings"; OK is one `set_via_stack`, "Edit
// Microvia Stack", which re-spaces the hops to the pitch and makes the vias again) or on Place Microvia Stack's settings when the board has no preset (OK starts the
// placement; Cancel leaves the tool). The fields and checks are Board Setup's Microvia Stacks dialog's (components/boardSetup/ViaStacksPage.tsx `MicroviaStackDialog`),
// "Use netclass values" showing the stack's net's class sizes (`GetNetclassViaSize( board, GetNetCode() )`).
import { MicroviaStackDialog } from "./boardSetup/ViaStacksPage";
import { closeViaStackDialog, useViaStackTool } from "../state/viaStackTool";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { copperOf, netAtPosition, netclassViaSize, presetOf, viaStackCmd, withPreset, layerRange } from "../kicad-port/viaStack";
import { layerDisplayName } from "../kicad-port/boardLayers";
import { currentViaSize } from "../kicad-port/routerVia";
import { startViaStackPlacement } from "../actions/viaStackActions";

export function ViaStackDialog() {
  const { dialog } = useViaStackTool();
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const board = state.board;
  if (!dialog || !board) return null;
  const copper = copperOf(board);
  const via = currentViaSize(board, state.currentViaIndex, state.pcbx.useCustomTrackVia, state.pcbx.customTrackVia);
  const net = dialog.stack.net ?? "";
  const size = netclassViaSize(board, net, via);
  const units = state.units === "mil" || state.units === "in" ? state.units : "mm";
  return (
    <MicroviaStackDialog
      key={dialog.id ?? "new"}
      preset={presetOf(dialog.stack)}
      copper={copper}
      layerName={(l) => layerDisplayName(board.board_rules?.layer_setup, l)}
      units={units}
      netclassSize={size}
      onCancel={() => closeViaStackDialog()}
      onOk={(p) => {
        const id = dialog.id;
        const next = withPreset(dialog.stack, { ...p, name: dialog.stack.preset ?? "" });
        closeViaStackDialog();
        if (id == null) return startViaStackPlacement(next, board, via.diameter, dispatch);
        // "If the stack has no net, inherit it from the copper it now sits on." (`FindNetAtPosition` over the span; the backend gives it only to a stack of no net)
        const fills = (state.zoneFill?.zones ?? []).filter((z) => z.filled).map((z) => ({ net: z.net, layer: z.layer, fragments: z.fragments }));
        const found = net === "" ? netAtPosition({ board, fills }, [next.origin.x, next.origin.y], layerRange(copper, next.start_layer, next.end_layer)) : net;
        void api.cmd(viaStackCmd(id, { ...next, net: found }, { current: { via_size_nm: via.diameter, via_drill_nm: via.drill } }));
      }}
    />
  );
}

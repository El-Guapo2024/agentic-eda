// The board editor's info bar over its canvas (`WX_INFOBAR`, components/InfoBar.tsx; `state.pcbInfoBar`): Draw Via's "Via location violates DRC." -- `VIA_PLACER::PlaceItem`'s
// `ShowInfoBarError( _( "Via location violates DRC." ), true, WX_INFOBAR::MESSAGE_TYPE::DRC_VIOLATION )`: the error icon and a close button, no timer (`ShowMessage`,
// not `ShowMessageFor`), replaced by the next message and dismissed when a via goes down clean (`if( infobar->GetMessageType() == DRC_VIOLATION ) infobar->Dismiss()`,
// state/store.tsx `runCmd`). It was a toast at the canvas's foot (UI-LEFT-3 P3-32).
import { useStudioDispatch, useStudioState } from "../state/store";
import { InfoBar } from "./InfoBar";

export function PcbInfoBar() {
  const { pcbInfoBar } = useStudioState();
  const dispatch = useStudioDispatch();
  if (!pcbInfoBar) return null;
  return <InfoBar message={pcbInfoBar.message} kind={pcbInfoBar.kind} testid="pcb-infobar" onClose={() => dispatch({ type: "DISMISS_PCB_INFOBAR" })} />;
}

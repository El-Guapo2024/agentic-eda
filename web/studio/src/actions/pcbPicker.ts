// PCB_PICKER_TOOL, the studio side (pcbnew/tools/pcb_picker_tool.cpp). The state machine is
// kicad-port/pickerHost.ts; this file owns the one picker of the app, the React hook that follows it, and the
// two Promise helpers a dialog uses where the C++ dialogs call `RunAction( selectPointInteractively, ... )` /
// `RunAction( selectItemInteractively, ... )` with themselves as the receiver:
//
//   pickPoint( prompt )  -> the snapped point, or null when cancelled   (PCB_PICKER_TOOL::SelectPointInteractively)
//   pickItem( prompt )   -> the id of the item that was clicked          (PCB_PICKER_TOOL::SelectItemInteractively)
//
// The canvas answers the session with a left click (components/canvas/Canvas.tsx), Escape cancels it
// (`common.Interactive.cancel`), and components/PcbPickerPrompt.tsx shows the prompt next to the pointer.

import { useSyncExternalStore } from "react";
import { PickerHost, type PickerSession, type PickPoint } from "../kicad-port/pickerHost";

export const picker = new PickerHost();

/** The running session, for the components that show it. */
export function usePickerSession(): PickerSession | null {
  return useSyncExternalStore(picker.subscribe, picker.session);
}

/** `SelectPointInteractively`: one point, snapped like the other point tools. Null when the user cancelled. */
export function pickPoint(prompt: string): Promise<PickPoint | null> {
  return new Promise((resolve) => {
    picker.start({
      kind: "point",
      prompt,
      onPoint: (at) => {
        resolve(at);
        return false;
      },
      onCancel: () => resolve(null),
    });
  });
}

/** `SelectItemInteractively`: the item under the click; `accept` is the C++ `m_ItemFilter` (a rejected item keeps the session looking). Null when cancelled. */
export function pickItem(prompt: string, accept?: (id: string) => boolean): Promise<string | null> {
  return new Promise((resolve) => {
    picker.start({
      kind: "item",
      prompt,
      onItem: (id) => {
        if (accept && !accept(id)) return true;
        resolve(id);
        return false;
      },
      onCancel: () => resolve(null),
    });
  });
}

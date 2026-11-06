// The `WheelInput` every canvas hands to `handleWheel` (kicad-port/viewControls.ts), built one way for the four of them: the platform's Ctrl (Cmd on
// macOS, as KiCad reads it there), the pointer relative to the canvas, and whether the event is a touchpad pinch (`isPinchWheel`), which KiCad
// receives as `wxEVT_MAGNIFY` and zooms whatever the wheel settings say.
import { isPinchWheel, type WheelInput } from "../kicad-port/viewControls";
import { controlKeyHeld } from "./controlKeyHeld";
import { isMac } from "../platform";

export function wheelInputOf(e: WheelEvent, rect: { left: number; top: number }): WheelInput {
  const mac = isMac();
  return {
    deltaX: e.deltaX,
    deltaY: e.deltaY,
    shiftKey: e.shiftKey,
    ctrlOrCmd: mac ? e.metaKey : e.ctrlKey,
    mac,
    altKey: e.altKey,
    x: e.clientX - rect.left,
    y: e.clientY - rect.top,
    pinch: isPinchWheel(e.ctrlKey, controlKeyHeld()),
  };
}

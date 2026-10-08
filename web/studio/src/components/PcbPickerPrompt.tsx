// The STATUS_TEXT_POPUP of the picker tool: the running session's prompt ("Select reference point for
// move...") in a small box that follows the pointer (`KIPLATFORM::UI::GetMousePosition() + wxPoint( 20, -50 )`).
// Also where a session ends when another tool takes over (`evt->IsActivate()` in PICKER_TOOL::Main): a change
// of the active tool or tab cancels it.
import { useEffect, useRef, useState } from "react";
import { picker, usePickerSession } from "../actions/pcbPicker";
import type { PickerSession } from "../kicad-port/pickerHost";
import { useStudioState } from "../state/store";

/** Where the pointer last was, for the popup of a session that starts before the pointer moves again. */
let lastPointer = { x: 0, y: 0 };
if (typeof window !== "undefined") {
  window.addEventListener(
    "pointermove",
    (e) => {
      lastPointer = { x: e.clientX, y: e.clientY };
    },
    { passive: true }
  );
}

export function PcbPickerPrompt() {
  const session = usePickerSession();
  const state = useStudioState();
  const [pos, setPos] = useState(lastPointer);
  const started = useRef<{ session: PickerSession; tool: string; tab: string } | null>(null);

  useEffect(() => {
    if (!session) {
      started.current = null;
      return;
    }
    setPos(lastPointer);
    const onMove = (e: PointerEvent) => setPos({ x: e.clientX, y: e.clientY });
    window.addEventListener("pointermove", onMove, { passive: true });
    return () => window.removeEventListener("pointermove", onMove);
  }, [session]);

  // Another tool (or tab) took over: the session is cancelled. The tool at the moment the session was first seen is the baseline.
  useEffect(() => {
    if (!session) return;
    const base = started.current;
    if (!base || base.session !== session) {
      started.current = { session, tool: state.activeTool, tab: state.tab };
      return;
    }
    if (base.tool !== state.activeTool || base.tab !== state.tab) picker.cancel(true);
  }, [session, state.activeTool, state.tab]);

  if (!session) return null;
  return (
    <div
      role="status"
      style={{
        position: "fixed",
        left: pos.x + 20,
        top: Math.max(4, pos.y - 50),
        zIndex: 60,
        pointerEvents: "none",
        padding: "4px 8px",
        fontSize: 12,
        color: "var(--chrome-text)",
        background: "var(--chrome-bg-raised)",
        border: "1px solid var(--chrome-border)",
        borderRadius: 4,
        boxShadow: "0 2px 8px var(--chrome-shadow)",
        whiteSpace: "nowrap",
      }}
    >
      {session.prompt}
    </div>
  );
}

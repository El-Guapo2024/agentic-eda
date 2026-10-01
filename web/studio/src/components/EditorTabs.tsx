// The PCB | Schematic | 3D switcher. One window, three tabs -- unlike
// real KiCad's separate windows per editor (pcbnew.EditorControl.
// showEeschema and common.Control.show3DViewer, wired in
// actions/useActionRunner.ts, jump here the same way; this is just the
// always-visible, explicit way to do the same thing by hand).
import type { EditorTab } from "../state/store";
import { useStudioDispatch, useStudioState } from "../state/store";

const TABS: Array<{ id: EditorTab; label: string }> = [
  { id: "pcb", label: "PCB" },
  { id: "schematic", label: "Schematic" },
  { id: "footprint", label: "Footprint" },
  { id: "symbol", label: "Symbol" },
  { id: "3d", label: "3D" },
];

export function EditorTabs() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  return (
    <div className="editor-tabs">
      {TABS.map((t) => (
        <button key={t.id} className={`editor-tab${state.tab === t.id ? " active" : ""}`} onClick={() => dispatch({ type: "SET_TAB", tab: t.id })}>
          {t.label}
        </button>
      ))}
    </div>
  );
}

// The right-hand dock: Appearance and the selection filter (KiCad), plus
// Activity (not a KiCad panel -- docked here, where KiCad's Search panel
// goes, per the task).
import { useStudioDispatch, useStudioState } from "../../state/store";
import type { RightDockTab } from "../../state/store";
import { AppearancePanel } from "./AppearancePanel";
import { SelectionFilterPanel } from "./SelectionFilterPanel";
import { ActivityPanel } from "./ActivityPanel";

const TABS: Array<{ id: RightDockTab; label: string }> = [
  { id: "appearance", label: "Appearance" },
  { id: "filter", label: "Filter" },
  { id: "activity", label: "Activity" },
];

export function RightDock() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  return (
    <div className="dock">
      <div className="dock-tabs">
        {TABS.map((t) => (
          <div key={t.id} className={`dock-tab${state.rightDockTab === t.id ? " active" : ""}`} onClick={() => dispatch({ type: "SET_RIGHT_DOCK_TAB", tab: t.id })}>
            {t.label}
          </div>
        ))}
      </div>
      {state.rightDockTab === "appearance" && <AppearancePanel />}
      {state.rightDockTab === "filter" && <SelectionFilterPanel />}
      {state.rightDockTab === "activity" && <ActivityPanel />}
    </div>
  );
}

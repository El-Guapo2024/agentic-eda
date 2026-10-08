// The schematic frame's left column (eeschema/sch_edit_frame.cpp: every side pane `.Left().Layer( 3 )`, ordered by `Position`): Net Navigator (0, closed to
// begin with), Schematic Hierarchy (1), Properties (2), Selection Filter (4). The Selection Filter shows while any other pane does
// (`updateSelectionFilterVisbility`) and is a fixed-size pane; there is no dock on the right of the sheet. See kicad-port/dockLayout.ts for the rules and why the
// column can fold. The Net Navigator's open state is the schematic control store's (its action and its check mark read it), so it is shown by that.
import { useEffect } from "react";
import { selectionFilterShown } from "../../kicad-port/dockLayout";
import { setDockColumnCollapsed, useDockLayout } from "../../state/dockLayoutStore";
import { useSchControlDispatch, useSchControlState } from "../../state/schControlStore";
import { NetNavigatorBody } from "../schControl/NetNavigatorPanel";
import { DockPanel } from "./Dock";
import { HierarchyPanel } from "./HierarchyPanel";
import { SchematicProperties } from "./PropertiesPanel";
import { SchSelectionFilterPanel } from "./SchSelectionFilterPanel";

/**
 * Opening the Net Navigator brings the column back when it is folded to its handle (an 800 px window starts that way): the pane would otherwise open
 * out of sight. Mounted by the frame, not by the dock, because a folded column does not render its panes.
 */
export function useOpenDockWithNetNavigator(): void {
  const open = useSchControlState().netNavigatorOpen;
  useEffect(() => {
    if (open) setDockColumnCollapsed("left", false);
  }, [open]);
}

export function SchematicDock() {
  const layout = useDockLayout();
  const control = useSchControlState();
  const controlDispatch = useSchControlDispatch();
  return (
    <>
      <DockPanel id="netNavigator" title="Net Navigator" visible={control.netNavigatorOpen} onClose={() => controlDispatch({ type: "SET_NET_NAVIGATOR", open: false })}>
        <NetNavigatorBody />
      </DockPanel>
      <DockPanel id="hierarchy" title="Schematic Hierarchy">
        <HierarchyPanel />
      </DockPanel>
      <DockPanel id="properties" title="Properties">
        <SchematicProperties />
      </DockPanel>
      <DockPanel id="selectionFilter" title="Selection Filter" visible={selectionFilterShown(layout, control.netNavigatorOpen)}>
        <SchSelectionFilterPanel />
      </DockPanel>
    </>
  );
}

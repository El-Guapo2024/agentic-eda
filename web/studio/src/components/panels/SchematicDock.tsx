// The schematic frame's left column (eeschema/sch_edit_frame.cpp: every side pane `.Left().Layer( 3 )`, ordered by `Position`): Schematic Hierarchy (1),
// Properties (2), Selection Filter (4). The Selection Filter shows while any other pane does (`updateSelectionFilterVisbility`) and is a fixed-size pane;
// there is no dock on the right of the sheet. See kicad-port/dockLayout.ts for the rules and why the column can fold.
import { DockPanel } from "./Dock";
import { HierarchyPanel } from "./HierarchyPanel";
import { SchematicProperties } from "./PropertiesPanel";
import { SchSelectionFilterPanel } from "./SchSelectionFilterPanel";

export function SchematicDock() {
  return (
    <>
      <DockPanel id="hierarchy" title="Schematic Hierarchy">
        <HierarchyPanel />
      </DockPanel>
      <DockPanel id="properties" title="Properties">
        <SchematicProperties />
      </DockPanel>
      <DockPanel id="selectionFilter" title="Selection Filter">
        <SchSelectionFilterPanel />
      </DockPanel>
    </>
  );
}

// The Help menu every KiCad editor frame ends its menu bar with: `EDA_BASE_FRAME::AddStandardHelpMenu` (common/eda_base_frame.cpp), the same six
// actions and the About item after a separator in the PCB Editor, the Schematic Editor, the Footprint Editor, the Symbol Editor and the 3D
// viewer. The extracted menu data (src/kicad/*menus.json) leaves it out -- "the Help menu [is] not captured" by the textual scan, because it is
// built by a helper and not in each frame's `menubar_*.cpp` -- so the menu bar appends this one.
import type { MenuConfig } from "../kicad/types";

export const HELP_MENU: MenuConfig = {
  label: "Help",
  items: [
    { type: "item", action: "common.SuiteControl.help" },
    { type: "item", action: "common.SuiteControl.gettingStarted" },
    { type: "item", action: "common.SuiteControl.listHotKeys" },
    { type: "item", action: "common.SuiteControl.getInvolved" },
    { type: "item", action: "common.SuiteControl.donate" },
    { type: "item", action: "common.SuiteControl.reportBug" },
    { type: "separator" },
    { type: "item", action: "common.SuiteControl.about" },
  ],
};

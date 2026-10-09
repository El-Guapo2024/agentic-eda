// The Checker actions of the Inspect menus: Next Marker, Previous Marker and Exclude Marker.
//
//   pcbnew/tools/drc_tool.cpp                 DRC_TOOL::NextMarker / PrevMarker / ExcludeMarker  -> DIALOG_DRC::NextMarker / PrevMarker / ExcludeMarker
//   eeschema/tools/sch_inspection_tool.cpp    SCH_INSPECTION_TOOL::NextMarker / PrevMarker / ExcludeMarker  -> DIALOG_ERC::...
//   common/rc_item.cpp                        RC_TREE_MODEL::NextMarker / PrevMarker (kicad-port/rcItems.ts `stepListed`)
//
// The tools show the Checker window (when it is hidden) and move its selection by one marker of the page it shows, among the rows its Show boxes list;
// selecting a marker selects the items it names and frames them on the canvas (`selectDrc` / `selectErc`, the same "click a row" the dialogs do).
// Exclude Marker waives the marker the list has selected: on the board it is the Violations page's (`DIALOG_DRC::ExcludeMarker` returns on any other page);
// on the schematic the marker selected in the ERC list. When the Show boxes leave exclusions out, the row goes and the selection moves on to the next
// marker (or the one before it when it was the last), as `RC_TREE_MODEL::DeleteCurrentItem` leaves it -- so Exclude Marker pressed again and again walks the list.
// The work is in actions/checkerOps.ts, shared with the dialogs' buttons and the marker menus.
//
// KiCad gives these three actions no default hotkey (`ACTIONS::nextMarker` / `prevMarker` / `excludeMarker` in common/tool/actions.cpp have none; the
// dialog's own key handler reads whatever the user assigned to Exclude Marker), so none is bound here either; they are in both Inspect menus.
import type { ActionHandler, CommonActionContext } from "./commonActions";
import { excludeMarkerDrc, excludeMarkerErc, stepDrc, stepErc } from "./checkerOps";

export function registerCheckerActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  const cctx = { api: ctx.api, dispatch: ctx.dispatch };

  if (ctx.tab === "pcb") {
    m.set("common.Checker.nextMarker", () => stepDrc(cctx, "next"));
    m.set("common.Checker.prevMarker", () => stepDrc(cctx, "prev"));
    m.set("common.Checker.excludeMarker", () => void excludeMarkerDrc(cctx));
  } else if (ctx.tab === "schematic") {
    m.set("common.Checker.nextMarker", () => stepErc(cctx, "next"));
    m.set("common.Checker.prevMarker", () => stepErc(cctx, "prev"));
    m.set("common.Checker.excludeMarker", () => void excludeMarkerErc(cctx));
  }
}

// The facts `kicad-port/actionChecked.ts` needs, read from the stores: `check( actionName )` says whether a toolbar button is drawn pressed (`true` /
// `false`) or is not a toggle (`undefined`).
import { useCallback } from "react";
import { actionChecked, type CheckedContext } from "../kicad-port/actionChecked";
import { useCommonTool } from "../state/commonTool";
import { useDockLayout } from "../state/dockLayoutStore";
import { useFpState } from "../state/footprintEditorStore";
import { useStudioState } from "../state/store";
import { useSymState } from "../state/symbolEditorStore";

export function useActionChecked(): (name: string) => boolean | undefined {
  const state = useStudioState();
  const fp = useFpState();
  const sym = useSymState();
  const dock = useDockLayout();
  const gridVisible = state.tab === "footprint" ? fp.gridVisible : state.tab === "symbol" ? sym.gridVisible : state.gridVisible;
  const highContrast = state.tab === "footprint" ? fp.highContrast : state.highContrast;
  // The zoom tool: the board's and the schematic's is the studio's active tool, the library editors' is the shared tool store's.
  const commonZoom = useCommonTool().zoomArea !== null;
  const zoomArmed = state.tab === "footprint" || state.tab === "symbol" ? commonZoom : state.activeTool === "zoom_area";
  const ctx: CheckedContext = {
    tab: state.tab,
    units: state.units,
    gridVisible,
    fpTool: fp.activeTool,
    symTool: sym.activeTool,
    sym: { showElectricalTypes: sym.showElectricalTypes, showHiddenPins: sym.showHiddenPins, syncPins: sym.syncPins },
    dock,
    rightDockTab: state.rightDockTab,
    highContrast,
    zoomArmed,
  };
  // The context is rebuilt every render; the callback only changes when a fact it reads does.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useCallback((name: string) => actionChecked(name, ctx), [state.tab, state.units, gridVisible, fp.activeTool, sym.activeTool, sym.showElectricalTypes, sym.showHiddenPins, sym.syncPins, dock, state.rightDockTab, highContrast, zoomArmed]);
}

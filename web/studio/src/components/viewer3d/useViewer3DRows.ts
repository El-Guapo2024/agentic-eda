// Which rows of the 3D viewer's Appearance manager are shown (`BOARD_ADAPTER::GetVisibleLayers`): a "Follow" preset's board rows from what it follows -- the PCB editor's
// layers and objects (`FOLLOW_PCB`) or the board's plot settings (`FOLLOW_PLOT_SETTINGS`, what KiCad's viewer opens on) -- and every other row from the viewer's own
// (`state.viewer3d.layers`, else the row's default). The viewer draws with it and the panel shows it, so both read it from here (kicad-port/viewer3dSettings.ts).
import { useMemo } from "react";
import { useStudioState } from "../../state/store";
import { plotFollowVisibility } from "../../kicad-port/appearance3d";
import { followPcbVisibility, rowShown, type FollowSources } from "../../kicad-port/viewer3dSettings";
import { layerStateKey } from "../../kicad-port/layerPresets";
import { objectKey } from "../../kicad-port/appearance";

export interface Viewer3DRows {
  shown: (id: string) => boolean;
  follow: FollowSources;
}

export function useViewer3DRows(): Viewer3DRows {
  const state = useStudioState();
  const { layers, preset } = state.viewer3d;
  const plot = state.board?.plot_params;
  const lv = state.layerVisible;
  const pcbKey = ["F.Cu", "B.Cu", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "F.Paste", "F.Adhes", "Cmts.User", "Dwgs.User", "Eco1.User", "Eco2.User"].map((l) => (lv[layerStateKey(l)] !== false ? "1" : "0")).join("") + ["footprint_references", "footprint_values", "footprint_text"].map((o) => (lv[objectKey(o as "footprint_text")] !== false ? "1" : "0")).join("");
  const key = JSON.stringify([layers, preset, plot?.layers, plot?.plot_on_all_layers, pcbKey]);
  return useMemo(() => {
    const follow: FollowSources = {
      plot: plotFollowVisibility(plot?.layers, plot?.plot_on_all_layers),
      pcb: followPcbVisibility(
        (l) => lv[layerStateKey(l)] !== false,
        (el) => lv[objectKey(el === "references" ? "footprint_references" : el === "values" ? "footprint_values" : "footprint_text")] !== false
      ),
    };
    return { follow, shown: (id: string) => rowShown(layers, preset, follow, id) };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
}

// `EDIT_TOOL::updateModificationPoint`'s `BestSnapAnchor( refPt, nullptr )` for the studio's state: the snap the planners of Rotate, Mirror and Move take for the
// reference point of several items (kicad-port/modificationAnchor.ts has the rules; this reads the view, the grid and the preferences it needs from the state).
import type { StudioState } from "./store";
import { getGridSettings } from "./gridSettings";
import { getGridOverrides } from "./gridOverrides";
import { getSnapOrigin } from "../components/canvas/gridHelper";
import { toPcbMagnetic } from "../kicad-port/preferences";
import { modificationSnap, type ModificationView } from "../kicad-port/modificationAnchor";
import { snapPoint } from "../components/canvas/gridHelper";
import type { Snap } from "../kicad-port/pcbTransform";

/** What the snapping reads of the studio, as the canvas feeds its own helper. Null without a board. */
export function modificationViewOf(state: StudioState): ModificationView | null {
  if (!state.board) return null;
  const origin = getSnapOrigin();
  return {
    board: state.board,
    scale: state.view.scale,
    gridNm: state.gridNm,
    grids: getGridSettings("pcb").grids,
    overrides: getGridOverrides("pcb"),
    origin: [origin.x, origin.y],
    magnetic: toPcbMagnetic(state.prefs, state.magneticAllLayers),
    layerVisible: state.layerVisible,
    highContrast: state.highContrast,
    activeLayer: state.activeLayer,
  };
}

/** The snap for a reference point: anchors within range, else the grid (the grid alone when there is no board to look at). */
export function modificationSnapFor(state: StudioState): Snap {
  const view = modificationViewOf(state);
  return view ? modificationSnap(view) : (p) => snapPoint(p[0], p[1], state.gridNm);
}

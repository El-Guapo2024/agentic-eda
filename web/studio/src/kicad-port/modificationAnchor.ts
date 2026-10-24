// The reference point of Rotate, Mirror and Move for several items: `EDIT_TOOL::updateModificationPoint` (pcbnew/tools/edit_tool.cpp at 8303b2ad).
//
//     PCB_GRID_HELPER grid( m_toolMgr, frame()->GetMagneticItemsSettings() );
//     VECTOR2I        refPt = aSelection.GetCenter();
//     aSelection.SetReferencePoint( grid.BestSnapAnchor( refPt, nullptr ) );
//
// `BestSnapAnchor` has two halves: the grid (the nearest grid point) and the anchors (the origin of a footprint, the ends and middles of a track or a shape,
// the centre of a via, a pad where the magnetic settings allow it ...). The centre of the selection's box goes to the anchor within the snap range when there
// is one -- 25 screen pixels, never more than the visible grid -- else to the grid point. A helper made for the call remembers nothing (no snap line, no
// hysteresis, no construction geometry), no modifier is held (`SetSnap( true )`, `SetUseGrid( true )`), and with no item to move every layer's items are
// candidates (`aReferenceItem == nullptr` -> `LSET::AllLayersMask()`), none skipped.
//
// The grid half alone was ported before (`snapPoint( centre, grid )`); the helper is kicad-port/pcbGridHelper.ts. Pure: the view and the grid come in as data.
import type { BoardState } from "../api/types";
import { computeVisibleGridSize } from "./grid";
import { gridSizeFor, type GridOverrides } from "./gridOverrides";
import { PcbGridHelper, type GridEnv } from "./pcbGridHelper";
import type { PcbMagnetic } from "./snapAnchors";
import { sceneOf } from "./snapScene";
import type { Snap } from "./pcbTransform";

/** What the snapping depends on: the board, the view and the grid, as the canvas feeds its own helper (components/canvas/pcbSnap.ts). */
export interface ModificationView {
  board: BoardState;
  /** Screen pixels per um. */
  scale: number;
  /** The grid on screen, um. */
  gridNm: number;
  /** The editor's grid list and the overrides over it. */
  grids: readonly number[];
  overrides: GridOverrides | null;
  /** The grid origin (`GAL::GetGridOrigin()`), um. */
  origin: readonly [number, number];
  magnetic: PcbMagnetic;
  /** A layer is hidden only when this says `false`. */
  layerVisible: Record<string, boolean>;
  highContrast: boolean;
  activeLayer: string | null;
}

/** `BestSnapAnchor( refPt, nullptr )` on a helper made for the call: where `refPt` goes, in whole micrometres. */
export function modificationAnchor(view: ModificationView, refPt: readonly [number, number]): [number, number] {
  const scale = view.scale > 0 ? view.scale : 1;
  const env: GridEnv = {
    scale,
    gridNm: view.gridNm,
    visibleGridNm: computeVisibleGridSize(view.gridNm, scale),
    origin: [view.origin[0], view.origin[1]],
    gridSizeOf: (c) => gridSizeFor(c, view.gridNm, view.grids, view.overrides),
  };
  const highContrastLayers = view.highContrast && view.activeLayer ? new Set([view.activeLayer]) : null;
  const helper = new PcbGridHelper(env, sceneOf(view.board), view.magnetic, { layerVisible: (layer) => view.layerVisible[layer] !== false, highContrastLayers });
  const p = helper.bestSnapAnchor([refPt[0], refPt[1]], "all", "current", new Set());
  return [Math.round(p[0]), Math.round(p[1])];
}

/** The `Snap` the planners take (`rotationPivot`, `modificationPoint`, `carryStart`) for this view. */
export function modificationSnap(view: ModificationView): Snap {
  return (p) => modificationAnchor(view, p);
}

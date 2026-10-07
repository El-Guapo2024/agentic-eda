// Ports of the cross-probing view code of pcbnew/tools/pcb_selection_tool.cpp (8303b2ad):
// `ZoomFitCrossProbeBBox` -- how far to zoom when the selection arrives from the other
// editor ("a reasonable amount of the circuit around it", not simply the part filling the
// screen) -- and the `FocusOnLocation( bbox.Centre() )` that follows in `doSyncSelection`.
// Plus the sheet bookkeeping of `selectAllItemsOnSheet` / `selectSameSheet`.
import { MAX_SCALE, MIN_SCALE, type Bounds, type ViewTransform } from "./view";

/** `pcbIUScale.mmToIU( DEFAULT_TEXT_SIZE )` (`#define DEFAULT_TEXT_SIZE 1.0`, include/board_design_settings.h): the height the probed part is compared against. */
const DEFAULT_TEXT_SIZE_UM = 1000;

/** The LUT of ZoomFitCrossProbeBBox: [footprint height / default text height, factor to scale the zoom ratio by]. */
const LUT: ReadonlyArray<readonly [number, number]> = [
  [1, 8],
  [1.5, 5],
  [3, 3],
  [4.5, 2.5],
  [8, 2.0],
  [12, 1.7],
  [16, 1.5],
  [24, 1.3],
  [32, 1.0],
];

/**
 * `ZoomFitCrossProbeBBox`'s new zoom (screen px per um) for a probed box of `bounds` on a `width` x `height` canvas at
 * `scale` now, or the current scale when the box has no width or the zoom would not change enough ("try not to zoom on
 * every cross-probe; it gets very noisy": only a ratio below 0.5 or above 1.0 zooms).
 */
export function crossProbeScale(scale: number, bounds: Bounds, width: number, height: number): number {
  const w = bounds.maxX - bounds.minX;
  const h = bounds.maxY - bounds.minY;
  if (w === 0) return scale;
  // `bbox.Inflate( KiROUND( bbox.GetWidth() * 0.2 ) )`: 20% of the width on every side.
  const inflate = Math.round(w * 0.2);
  const bbX = w + 2 * inflate;
  const bbY = h + 2 * inflate;
  // `view->ToWorld( client size, false )`: the canvas in board units.
  const screenX = Math.max(10, Math.abs(width / scale));
  const screenY = Math.max(10, Math.abs(height / scale));
  let ratio = Math.max(-1.0, Math.abs(bbY / screenY));
  const kicadRatio = Math.max(Math.abs(bbX / screenX), Math.abs(bbY / screenY));
  const compRatio = bbY / DEFAULT_TEXT_SIZE_UM;
  // Bigger components need less scaling than small ones: interpolate the LUT.
  let bent = LUT[LUT.length - 1]![1];
  if (compRatio >= LUT[0]![0]) {
    for (let i = 0; i < LUT.length - 1; i++) {
      const [x0, y0] = LUT[i]!;
      const [x1, y1] = LUT[i + 1]!;
      if (x0 <= compRatio && x1 >= compRatio) {
        bent = y0 + ((y1 - y0) * (compRatio - x0)) / (x1 - x0);
        break;
      }
    }
  } else {
    bent = LUT[0]![1];
  }
  // A part wider than the zoomed screen: the plain fit, which guarantees the width fits.
  if (bbX > screenX * ratio * bent) {
    ratio = kicadRatio;
    bent = 1.0;
  }
  ratio *= bent;
  if (ratio < 0.5 || ratio > 1.0) return Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale / ratio));
  return scale;
}

/**
 * `doSyncSelection`'s view step with the default cross-probing settings (`center_on_items`, `zoom_to_fit`): zoom per
 * `ZoomFitCrossProbeBBox`, then `FocusOnLocation( bbox.Centre() )`. A box without width or height leaves the view alone.
 */
export function crossProbeView(view: ViewTransform, bounds: Bounds, width: number, height: number): ViewTransform {
  if (bounds.maxX - bounds.minX === 0 || bounds.maxY - bounds.minY === 0) return view;
  const scale = crossProbeScale(view.scale, bounds, width, height);
  const cx = (bounds.minX + bounds.maxX) / 2;
  const cy = (bounds.minY + bounds.maxY) / 2;
  return { scale, x: width / 2 - cx * scale, y: height / 2 - cy * scale };
}

// ---------------------------------------------------------------------------- sheets

/** One hierarchical sheet as the schematic API serves it: the symbols on that sheet's own page and the child sheet instances placed on it. */
export interface SheetView {
  symbolIds: readonly string[];
  childIds: readonly string[];
}

/**
 * The reference designators on each sheet of the hierarchy, keyed by the sheet path (`""` is the root; a child is its parent's
 * path plus `/` plus its instance id) -- the studio's version of a footprint's KIID path minus its last element
 * (`footprint->GetPath().AsString().BeforeLast( '/' )`). `fetchSheet( path )` loads one sheet's page.
 */
export async function collectSheetRefs(fetchSheet: (path: readonly string[]) => Promise<SheetView>, maxSheets = 200): Promise<Map<string, Set<string>>> {
  const out = new Map<string, Set<string>>();
  const queue: string[][] = [[]];
  let n = 0;
  while (queue.length > 0 && n < maxSheets) {
    const path = queue.shift()!;
    n++;
    const sheet = await fetchSheet(path);
    out.set(path.join("/"), new Set(sheet.symbolIds));
    for (const id of sheet.childIds) queue.push([...path, id]);
  }
  return out;
}

/** The path of the sheet that holds `ref` (`selectSameSheet`'s `sheetPath`), or null when no sheet does. */
export function sheetOfRef(sheets: ReadonlyMap<string, ReadonlySet<string>>, ref: string): string | null {
  for (const [path, refs] of sheets) if (refs.has(ref)) return path;
  return null;
}

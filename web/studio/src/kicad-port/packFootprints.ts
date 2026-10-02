// Port of `SpreadFootprints` (pcbnew/autorouter/spread_footprints.cpp) as
// `EDIT_TOOL::PackAndMoveFootprints` (pcbnew/tools/edit_tool_move_fct.cpp,
// `pcbnew.InteractiveEdit.packAndMoveFootprints`, hotkey P) calls it:
//
//     SpreadFootprints( &footprintsToPack, footprintsBbox.Normalize().GetOrigin(), false );
//
// i.e. the selected footprints are packed into a compact block whose top-left
// corner is the top-left of the selection's own bounding box, grouped by
// identical size and ordered by reference. `aComponentGap` / `aGroupGap` take
// the header's defaults (1 mm / 1.5 mm), `aGroupBySheet` is false. The packed
// group is then handed to the move tool (`doMoveSelection`), so the whole thing
// follows the cursor until a click drops it.
//
// What is exact here: the size grouping (including `std::map<VECTOR2I>`'s
// squared-length key), the per-size row/column arrangement (`optimalCountPerLine`
// and the cell offsets), the reference ordering (`compareFootprintsbyRef`), the
// 0.01 mm packing grid, the half-gap block inflation and the final placement of
// the arrangement's top-left on the target. What is not: the step that packs the
// *blocks* of different sizes against each other uses `rectpack2D`'s
// `find_best_packing` in KiCad (a third-party header the pinned source snapshot
// does not carry); here a shelf packer finds the smallest square bin that holds
// the blocks, with the same "start at sqrt(total area), grow by 1.2" outer loop.
// With one block size (the common case) there is nothing to pack and the result
// is exactly KiCad's. A footprint's box is its courtyard (the model has no
// separate bounding box without text).

export type Box = readonly [number, number, number, number];

export interface PackItem {
  ref: string;
  /** `FOOTPRINT::GetBoundingBox( false )` stand-in: the courtyard box, um. */
  bbox: Box;
}

/** `SpreadFootprints`' defaults: `aComponentGap = mmToIU( 1 )`, `aGroupGap = mmToIU( 1.5 )` (micrometres here). */
export const COMPONENT_GAP_UM = 1000;
export const GROUP_GAP_UM = 1500;
/** `const int scale = (int) ( 0.01 * pcbIUScale.IU_PER_MM )`: blocks are packed on a 0.01 mm grid. */
export const PACK_SCALE_UM = 10;

/** `UTIL::GetRefDesPrefix`: everything up to the last character that is neither a digit nor `?`. */
export function refDesPrefix(ref: string): string {
  let i = ref.length - 1;
  while (i >= 0 && (ref[i] === "?" || (ref[i]! >= "0" && ref[i]! <= "9"))) i--;
  return ref.slice(0, i + 1);
}

/** `GetTrailingInt`: the number the string ends in (0 when it does not end in a digit). */
export function trailingInt(s: string): number {
  let number = 0;
  let base = 1;
  for (let i = s.length - 1; i >= 0; i--) {
    const c = s[i]!;
    if (c < "0" || c > "9") break;
    number += (c.charCodeAt(0) - 48) * base;
    base *= 10;
  }
  return number;
}

/** `compareFootprintsbyRef`: by reference prefix, then by trailing number. */
export function compareByRef(a: string, b: string): number {
  const pa = refDesPrefix(a);
  const pb = refDesPrefix(b);
  if (pa !== pb) return pa < pb ? -1 : 1;
  return trailingInt(a) - trailingInt(b);
}

const trunc = Math.trunc;

/** `optimalCountPerLine` for `count` footprints of cell size `size` (the two ratio/remainder heuristics of `SpreadFootprints`). */
export function optimalCountPerLine(count: number, size: readonly [number, number]): number {
  const vertical = size[0] >= size[1];
  const blockEstimateArea = size[0] * size[1] * count;
  const initialSide = Math.sqrt(blockEstimateArea);
  let initialCountPerLine = count;
  const singleLineRatio = 5;
  // Wrap the line if the ratio is not satisfied (integer division, as in the C++).
  if (vertical) {
    if (trunc((size[1] * count) / size[0]) > singleLineRatio) initialCountPerLine = trunc(initialSide / size[1]);
  } else if (trunc((size[0] * count) / size[1]) > singleLineRatio) {
    initialCountPerLine = trunc(initialSide / size[0]);
  }
  let optimal = initialCountPerLine;
  let optimalRemainder = count % optimal;
  if (optimalRemainder !== 0) {
    for (let i = Math.max(2, initialCountPerLine - 2); i <= Math.min(count - 2, initialCountPerLine + 2); i++) {
      const r = count % i;
      if (r === 0 || r >= optimalRemainder) {
        optimal = i;
        optimalRemainder = r;
      }
    }
  }
  return optimal;
}

interface Placed {
  ref: string;
  /** The footprint box after the move into its cell: [x0, y0, x1, y1]. */
  box: [number, number, number, number];
}

interface Block {
  items: Placed[];
  /** Union of the cells' boxes, each inflated by half the component gap (`block_bbox`). */
  box: [number, number, number, number];
}

/** One size group arranged in rows or columns (`SpreadFootprints`' first loop body). */
function arrangeBlock(items: readonly PackItem[], size: readonly [number, number], componentGap: number): Block {
  const count = items.length;
  const vertical = size[0] >= size[1];
  const perLine = optimalCountPerLine(count, size);
  const sorted = [...items].sort((a, b) => compareByRef(a.ref, b.ref));
  const placed: Placed[] = [];
  let box: [number, number, number, number] | null = null;
  const half = trunc(componentGap / 2);
  for (let i = 0; i < sorted.length; i++) {
    const fp = sorted[i]!;
    let px = trunc(size[0] / 2);
    let py = trunc(size[1] / 2);
    if (vertical) {
      px += size[0] * trunc(i / perLine);
      py += size[1] * (i % perLine);
    } else {
      px += size[0] * (i % perLine);
      py += size[1] * trunc(i / perLine);
    }
    const w = fp.bbox[2] - fp.bbox[0];
    const h = fp.bbox[3] - fp.bbox[1];
    // `footprint->Move( position - old_fp_bbox.GetOrigin() )`: the box origin lands on `position`.
    const b: [number, number, number, number] = [px, py, px + w, py + h];
    placed.push({ ref: fp.ref, box: b });
    const inflated: [number, number, number, number] = [b[0] - half, b[1] - half, b[2] + half, b[3] + half];
    box = box ? [Math.min(box[0], inflated[0]), Math.min(box[1], inflated[1]), Math.max(box[2], inflated[2]), Math.max(box[3], inflated[3])] : inflated;
  }
  return { items: placed, box: box! };
}

/** Shelf packing of `rects` (units of the packing grid) into a `side` x `side` bin; null when they do not fit. */
function shelfPack(rects: readonly { w: number; h: number }[], side: number): { x: number; y: number }[] | null {
  const order = rects.map((_, i) => i).sort((a, b) => rects[b]!.h - rects[a]!.h || rects[b]!.w - rects[a]!.w || a - b);
  const out: { x: number; y: number }[] = new Array(rects.length);
  let x = 0;
  let y = 0;
  let shelfH = 0;
  for (const i of order) {
    const { w, h } = rects[i]!;
    if (w > side || h > side) return null;
    if (x + w > side) {
      y += shelfH;
      x = 0;
      shelfH = 0;
    }
    if (y + h > side) return null;
    out[i] = { x, y };
    x += w;
    shelfH = Math.max(shelfH, h);
  }
  return out;
}

/** `spreadRectangles`: the smallest square bin (from `sqrt(total area)` up, growing by 1.2 like the C++ loop) that holds every rect; positions in grid units. */
export function packRects(rects: readonly { w: number; h: number }[], areaSide: number): { x: number; y: number }[] {
  if (rects.length === 0) return [];
  const minSide = Math.max(1, ...rects.map((r) => Math.max(r.w, r.h)));
  let hi = Math.max(areaSide, minSide);
  let found = shelfPack(rects, hi);
  for (let i = 0; i < 2000 && !found; i++) {
    hi = Math.ceil(hi * 1.2);
    found = shelfPack(rects, hi);
  }
  if (!found) return rects.map(() => ({ x: 0, y: 0 }));
  // `find_best_packing` looks for the smallest bin: bisect the side down to the first one the shelf packer still fits.
  let lo = minSide;
  let best = found;
  while (lo < hi) {
    const mid = trunc((lo + hi) / 2);
    const attempt = shelfPack(rects, mid);
    if (attempt) {
      best = attempt;
      hi = mid;
    } else {
      lo = mid + 1;
    }
  }
  return best;
}

/** `FOOTPRINT::GetBoundingBox`: "bbox.Inflate( mmToIU( 0.25 ) ); // Give a min size to the bbox" around the anchor. */
export const MIN_BBOX_HALF_UM = 250;

export interface PackPart {
  ref: string;
  placed: boolean;
  at?: readonly [number, number];
  /** Board-space courtyard box of a placed part. */
  courtyard?: Box | null;
  /** Board-space pads (centre + rotated extent). */
  pads?: ReadonlyArray<{ x: number; y: number; w: number; h: number }>;
}

/**
 * `FOOTPRINT::GetBoundingBox( false )` as far as the model reaches: the anchor +- 0.25 mm merged with the
 * courtyard and every pad. (KiCad also merges the silkscreen/fab drawings and zones; the courtyard bounds
 * them in practice and the studio model has no per-footprint graphics list.) `null` for an unplaced part.
 */
export function footprintBBox(part: PackPart): Box | null {
  if (!part.placed || !part.at) return null;
  let x0 = part.at[0] - MIN_BBOX_HALF_UM;
  let y0 = part.at[1] - MIN_BBOX_HALF_UM;
  let x1 = part.at[0] + MIN_BBOX_HALF_UM;
  let y1 = part.at[1] + MIN_BBOX_HALF_UM;
  if (part.courtyard) {
    x0 = Math.min(x0, part.courtyard[0]);
    y0 = Math.min(y0, part.courtyard[1]);
    x1 = Math.max(x1, part.courtyard[2]);
    y1 = Math.max(y1, part.courtyard[3]);
  }
  for (const p of part.pads ?? []) {
    x0 = Math.min(x0, p.x - p.w / 2);
    y0 = Math.min(y0, p.y - p.h / 2);
    x1 = Math.max(x1, p.x + p.w / 2);
    y1 = Math.max(y1, p.y + p.h / 2);
  }
  return [Math.floor(x0), Math.floor(y0), Math.ceil(x1), Math.ceil(y1)];
}

/**
 * The selection step of `EDIT_TOOL::PackAndMoveFootprints`: only footprints survive
 * (`FilterCollectorForHierarchy`/`ForFreePads` + the `dynamic_cast<FOOTPRINT*>` sweep), locked ones are dropped
 * (`FilterCollectorForLockedItems`), then `SpreadFootprints` runs over what is left. Null when nothing is left
 * ("if( footprintsToPack.empty() ) return 0"). `refs` keep their order, which is the order `std::map` sees them in.
 */
export function planPack(parts: readonly PackPart[], refs: readonly string[], locked: ReadonlySet<string>): { refs: string[]; offsets: Record<string, [number, number]> } | null {
  const byRef = new Map(parts.map((p) => [p.ref, p]));
  const seen = new Set<string>();
  const items: PackItem[] = [];
  for (const ref of refs) {
    if (seen.has(ref)) continue;
    seen.add(ref);
    const part = byRef.get(ref);
    if (!part || locked.has(ref)) continue;
    const bbox = footprintBBox(part);
    if (bbox) items.push({ ref, bbox });
  }
  if (items.length === 0) return null;
  return { refs: items.map((i) => i.ref), offsets: packFootprints(items).offsets };
}

export interface PackResult {
  /** `ref` -> the shift of the footprint (um) from where it is now to its packed place. */
  offsets: Record<string, [number, number]>;
  /** Top-left of the packed arrangement (the selection's own top-left, `aTargetBoxPosition`). */
  origin: [number, number];
}

/**
 * `SpreadFootprints( &footprints, footprintsBbox.Normalize().GetOrigin(), false )`
 * over `items` (placed footprints only -- the caller applies the selection filter).
 */
export function packFootprints(items: readonly PackItem[], componentGap = COMPONENT_GAP_UM, groupGap = GROUP_GAP_UM): PackResult {
  void groupGap; // only the sheet-level packing reads it, and with a single group that packing is the identity
  if (items.length === 0) return { offsets: {}, origin: [0, 0] };
  // `footprintsBbox.Normalize().GetOrigin()`
  const origin: [number, number] = [Math.min(...items.map((i) => i.bbox[0])), Math.min(...items.map((i) => i.bbox[1]))];

  // `sizeToFpMap`: footprints grouped by `bbox size + component gap` in a `std::map<VECTOR2I, ...>`. KiCad's
  // `VECTOR2::operator<` compares *squared lengths*, so the map is ordered by length and two sizes of equal
  // length (a part and the same part turned 90 degrees, say) are one key: the first one inserted names the
  // group's cell size. Reproduced as is -- it decides how rotated twins are laid out.
  const groups = new Map<number, { size: [number, number]; items: PackItem[] }>();
  for (const it of items) {
    const size: [number, number] = [it.bbox[2] - it.bbox[0] + componentGap, it.bbox[3] - it.bbox[1] + componentGap];
    const key = size[0] * size[0] + size[1] * size[1];
    const g = groups.get(key);
    if (g) g.items.push(it);
    else groups.set(key, { size, items: [it] });
  }
  const ordered = [...groups.entries()].sort((a, b) => a[0] - b[0]).map(([, g]) => g);

  const blocks = ordered.map((g) => arrangeBlock(g.items, g.size, componentGap));

  // Pack the blocks (0.01 mm grid): `areaSide = sqrt( blocksArea )`.
  const rects = blocks.map((b) => ({ w: trunc((b.box[2] - b.box[0]) / PACK_SCALE_UM), h: trunc((b.box[3] - b.box[1]) / PACK_SCALE_UM) }));
  const blocksArea = blocks.reduce((s, b) => s + (b.box[2] - b.box[0]) * (b.box[3] - b.box[1]), 0);
  const areaSide = trunc(Math.sqrt(blocksArea) / PACK_SCALE_UM);
  const spots = packRects(rects, areaSide);

  // Each footprint moves by `target_pos - block_bbox.GetPosition()`; the sheet box is the union of the moved footprint boxes.
  const finalBoxes: { ref: string; box: [number, number, number, number] }[] = [];
  blocks.forEach((b, bi) => {
    const dx = spots[bi]!.x * PACK_SCALE_UM - b.box[0];
    const dy = spots[bi]!.y * PACK_SCALE_UM - b.box[1];
    for (const p of b.items) finalBoxes.push({ ref: p.ref, box: [p.box[0] + dx, p.box[1] + dy, p.box[2] + dx, p.box[3] + dy] });
  });
  const sheetX0 = Math.min(...finalBoxes.map((f) => f.box[0]));
  const sheetY0 = Math.min(...finalBoxes.map((f) => f.box[1]));

  // The single group's own rect is packed alone (position 0,0): the arrangement's top-left lands on the target.
  const offsets: Record<string, [number, number]> = {};
  const byRef = new Map(items.map((i) => [i.ref, i]));
  for (const f of finalBoxes) {
    const src = byRef.get(f.ref)!;
    const newX = f.box[0] - sheetX0 + origin[0];
    const newY = f.box[1] - sheetY0 + origin[1];
    offsets[f.ref] = [newX - src.bbox[0], newY - src.bbox[1]];
  }
  return { offsets, origin };
}

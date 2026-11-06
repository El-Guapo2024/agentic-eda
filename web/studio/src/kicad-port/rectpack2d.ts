// Port of rectpack2D (TeamHypersomnia, MIT), the rectangle packer KiCad's `spreadRectangles` calls (pcbnew/autorouter/spread_footprints.cpp, through
// `rectpack2D::find_best_packing`). The 8303b2ad checkout used here has no `thirdparty/` folder; the headers are KiCad 25ec2335's
// `thirdparty/rectpack2d/rectpack2d/{rect_structs,empty_space_allocators,empty_spaces,insert_and_split,best_bin_finder,finders_interface}.h`, which
// spread_footprints.cpp of both commits includes unchanged.
//
// What KiCad instantiates, and so what this is:
//   using spaces_type = rectpack2D::empty_spaces<allow_flip = true, rectpack2D::default_empty_spaces>;
//   find_best_packing<spaces_type>( rects, make_finder_input( max_side, discard_step = 1, .., flipping_option::DISABLED ) )
// -- the output rectangles are `rect_xywhf` (with a `flipped` flag), but flipping is off, so a rectangle is only ever tried the way it is.
//
// Line by line, with C++'s integer arithmetic (`int` division truncates; `int` products wrap, as `w * h` does on the compilers KiCad is built with:
// `Math.imul`) and `float` for `pathological_mult` (`Math.fround`). The six orders `find_best_packing` tries come from `std::sort`, which is not stable;
// for up to 16 rectangles libstdc++'s sort is its insertion sort, which keeps equal rectangles in their input order, and so does this (`Array.prototype.sort`
// is stable). Above 16 rectangles libstdc++'s introsort can order equal rectangles otherwise, and the packing can then differ from KiCad's where two
// rectangles compare equal.

/** `rect_wh`. */
export interface RectWH {
  w: number;
  h: number;
}

/** `rect_xywhf` (`flipped` is always false here: `flipping_option::DISABLED`). */
export interface RectXYWHF {
  x: number;
  y: number;
  w: number;
  h: number;
  flipped: boolean;
}

/** `space_rect` (`rect_xywh`). */
interface Space {
  x: number;
  y: number;
  w: number;
  h: number;
}

const area = (r: { w: number; h: number }): number => Math.imul(r.w, r.h);
const perimeter = (r: { w: number; h: number }): number => (2 * r.w + 2 * r.h) | 0;
const maxSide = (r: { w: number; h: number }): number => (r.h > r.w ? r.h : r.w);
const minSide = (r: { w: number; h: number }): number => (r.h < r.w ? r.h : r.w);
/** `float pathological_mult() const { return float( max_side() ) / min_side() * area(); }`. */
const pathologicalMult = (r: { w: number; h: number }): number => Math.fround(Math.fround(Math.fround(maxSide(r)) / minSide(r)) * area(r));

/** `created_splits`: `count` -1 is `failed()`. */
interface CreatedSplits {
  count: number;
  spaces: Space[];
}

const FAILED: CreatedSplits = { count: -1, spaces: [] };

/** `insert_and_split( im, sp )`. */
export function insertAndSplit(im: RectWH, sp: Space): CreatedSplits {
  const freeW = (sp.w - im.w) | 0;
  const freeH = (sp.h - im.h) | 0;
  // "Image is bigger than the candidate empty space."
  if (freeW < 0 || freeH < 0) return FAILED;
  // "the image fits exactly": the space goes, no splits
  if (freeW === 0 && freeH === 0) return { count: 0, spaces: [] };
  // exactly one dimension equal: a single split
  if (freeW > 0 && freeH === 0) return { count: 1, spaces: [{ x: (sp.x + im.w) | 0, y: sp.y, w: (sp.w - im.w) | 0, h: sp.h }] };
  if (freeW === 0 && freeH > 0) return { count: 1, spaces: [{ x: sp.x, y: (sp.y + im.h) | 0, w: sp.w, h: (sp.h - im.h) | 0 }] };
  // strictly smaller: one tiny space and one huge one, split along the axis with more left over
  if (freeW > freeH) {
    const bigger: Space = { x: (sp.x + im.w) | 0, y: sp.y, w: freeW, h: sp.h };
    const lesser: Space = { x: sp.x, y: (sp.y + im.h) | 0, w: im.w, h: freeH };
    return { count: 2, spaces: [bigger, lesser] };
  }
  const bigger: Space = { x: sp.x, y: (sp.y + im.h) | 0, w: sp.w, h: freeH };
  const lesser: Space = { x: (sp.x + im.w) | 0, y: sp.y, w: freeW, h: im.h };
  return { count: 2, spaces: [bigger, lesser] };
}

/** `empty_spaces<true, default_empty_spaces>` with `flipping_mode = DISABLED`. */
export class EmptySpaces {
  private spaces: Space[] = [];
  private aabb: RectWH = { w: 0, h: 0 };

  constructor(r: RectWH) {
    this.reset(r);
  }

  /** `reset( r )`: one empty space, the whole bin. */
  reset(r: RectWH): void {
    this.aabb = { w: 0, h: 0 };
    this.spaces = [{ x: 0, y: 0, w: r.w, h: r.h }];
  }

  /**
   * `insert( image_rectangle )`: the empty spaces are tried from the last to the first; the first that takes the rectangle is removed
   * (`default_empty_spaces::remove`: the last space moved into its slot) and its splits are added at the end.
   */
  insert(im: RectWH): RectXYWHF | null {
    for (let i = this.spaces.length - 1; i >= 0; --i) {
      const candidate = this.spaces[i]!;
      const splits = insertAndSplit(im, candidate);
      if (splits.count === -1) continue;
      // `accept_result`
      this.spaces[i] = this.spaces[this.spaces.length - 1]!;
      this.spaces.pop();
      for (let s = 0; s < splits.count; ++s) this.spaces.push(splits.spaces[s]!);
      const result: RectXYWHF = { x: candidate.x, y: candidate.y, w: im.w, h: im.h, flipped: false };
      // `current_aabb.expand_with( result )`
      this.aabb = { w: Math.max(this.aabb.w, (result.x + result.w) | 0), h: Math.max(this.aabb.h, (result.y + result.h) | 0) };
      return result;
    }
    return null;
  }

  /** `get_rects_aabb()`. */
  rectsAabb(): RectWH {
    return { ...this.aabb };
  }
}

type BinDimension = "both" | "width" | "height";

/**
 * `best_packing_for_ordering_impl`: a binary search on the bin size from `startingBin` down; a number is the area inserted when even `startingBin` cannot
 * hold every rectangle (`total_area_type`), a rectangle the bin found.
 */
function bestPackingForOrderingImpl(root: EmptySpaces, ordering: readonly RectXYWHF[], startingBin: RectWH, discardStepIn: number, tried: BinDimension): number | RectWH {
  const candidate: RectWH = { ...startingBin };
  let triesBeforeDiscarding = 0;
  let discardStep = discardStepIn;
  if (discardStep <= 0) {
    triesBeforeDiscarding = -discardStep;
    discardStep = 1;
  }
  let startingStep = 0;
  if (tried === "both") {
    candidate.w = Math.trunc(candidate.w / 2);
    candidate.h = Math.trunc(candidate.h / 2);
    startingStep = Math.trunc(candidate.w / 2);
  } else if (tried === "width") {
    candidate.w = Math.trunc(candidate.w / 2);
    startingStep = Math.trunc(candidate.w / 2);
  } else {
    candidate.h = Math.trunc(candidate.h / 2);
    startingStep = Math.trunc(candidate.h / 2);
  }
  for (let step = startingStep; ; step = Math.max(1, Math.trunc(step / 2))) {
    root.reset(candidate);
    let totalInsertedArea = 0;
    let allInserted = true;
    for (const r of ordering) {
      if (root.insert({ w: r.w, h: r.h })) totalInsertedArea = (totalInsertedArea + area(r)) | 0;
      else {
        allInserted = false;
        break;
      }
    }
    if (allInserted) {
      // "Attempt was successful. Try with a smaller bin."
      if (step <= discardStep) {
        if (triesBeforeDiscarding > 0) triesBeforeDiscarding--;
        else return { ...candidate };
      }
      if (tried === "both") {
        candidate.w -= step;
        candidate.h -= step;
      } else if (tried === "width") candidate.w -= step;
      else candidate.h -= step;
      root.reset(candidate);
    } else {
      // "Attempt ended with failure. Try with a bigger bin."
      if (tried === "both") {
        candidate.w += step;
        candidate.h += step;
        if (area(candidate) > area(startingBin)) return totalInsertedArea;
      } else if (tried === "width") {
        candidate.w += step;
        if (candidate.w > startingBin.w) return totalInsertedArea;
      } else {
        candidate.h += step;
        if (candidate.h > startingBin.h) return totalInsertedArea;
      }
    }
  }
}

/** `best_packing_for_ordering`: both sides together, then the width alone, then the height alone, each from the best bin so far. */
function bestPackingForOrdering(root: EmptySpaces, ordering: readonly RectXYWHF[], startingBin: RectWH, discardStep: number): number | RectWH {
  const best = bestPackingForOrderingImpl(root, ordering, startingBin, discardStep, "both");
  if (typeof best === "number") return best;
  let bestBin = best;
  for (const dim of ["width", "height"] as const) {
    const trial = bestPackingForOrderingImpl(root, ordering, bestBin, discardStep, dim);
    if (typeof trial !== "number") bestBin = trial;
  }
  return bestBin;
}

/** What `make_finder_input` carries; the callbacks return true to go on packing (`CONTINUE_PACKING`), false to stop (`ABORT_PACKING`). */
export interface FinderInput {
  maxBinSide: number;
  discardStep: number;
  onSuccess: (r: RectXYWHF) => boolean;
  onFailure: (r: RectXYWHF) => boolean;
}

/**
 * `find_best_packing<spaces_type>( subjects, input )` with its six default orders (area, perimeter, longer side, width, height, "pathological multiplier",
 * each descending): the best bin over the orders (the smallest area; a later order wins a tie), then the subjects packed into it in that order, their x and
 * y written into `subjects` in place. A subject of no area takes no part and keeps its position. Null when every order fails to fit even the
 * `maxBinSide` square; otherwise the box round what was packed (`get_rects_aabb`).
 */
export function findBestPacking(subjects: RectXYWHF[], input: FinderInput): RectWH | null {
  const initial = subjects.filter((s) => area(s) > 0);
  const comparators: ((a: RectXYWHF, b: RectXYWHF) => boolean)[] = [
    (a, b) => area(a) > area(b),
    (a, b) => perimeter(a) > perimeter(b),
    (a, b) => Math.max(a.w, a.h) > Math.max(b.w, b.h),
    (a, b) => a.w > b.w,
    (a, b) => a.h > b.h,
    (a, b) => pathologicalMult(a) > pathologicalMult(b),
  ];
  // `std::sort( order.begin(), order.end(), predicate )`: a strict "less" turned into a comparator; equal subjects keep their order (see the header)
  const orders = comparators.map((less) => [...initial].sort((a, b) => (less(a, b) ? -1 : less(b, a) ? 1 : 0)));
  const maxBin: RectWH = { w: input.maxBinSide, h: input.maxBinSide };
  let bestOrder: RectXYWHF[] | null = null;
  let bestTotalInserted = -1;
  let bestBin: RectWH = { ...maxBin };
  const root = new EmptySpaces({ w: 0, h: 0 });
  for (const order of orders) {
    const packing = bestPackingForOrdering(root, order, maxBin, input.discardStep);
    if (typeof packing === "number") {
      // "Track which function inserts the most area in total, just in case that all orders will fail to fit into the largest allowed bin."
      if (bestOrder === null && packing > bestTotalInserted) {
        bestOrder = order;
        bestTotalInserted = packing;
      }
    } else if (area(packing) <= area(bestBin)) {
      bestOrder = order;
      bestBin = packing;
    }
  }
  if (bestOrder === null) return null;
  root.reset(bestBin);
  for (const rect of bestOrder) {
    const ret = root.insert({ w: rect.w, h: rect.h });
    if (ret) {
      rect.x = ret.x;
      rect.y = ret.y;
      rect.w = ret.w;
      rect.h = ret.h;
      rect.flipped = ret.flipped;
      if (!input.onSuccess(rect)) break;
    } else if (!input.onFailure(rect)) break;
  }
  return root.rectsAabb();
}

/**
 * `spreadRectangles( vecSubRects, areaSizeX, areaSizeY )` (spread_footprints.cpp): the sizes in 0.01 mm (`scale`), the bin's side starting at the larger of
 * the two and growing by a fifth (`max_side = (int) ( max_side * 1.2 )`) until every rectangle fits, at most 2000 times. `rects` are in packing units and get
 * their x and y in place; the answer is the box round them, null when no bin was found.
 */
export function spreadRectangles(rects: RectXYWHF[], areaSizeXNm: number, areaSizeYNm: number, scaleNm: number): RectWH | null {
  const areaSizeX = Math.trunc(areaSizeXNm / scaleNm);
  const areaSizeY = Math.trunc(areaSizeYNm / scaleNm);
  let result: RectWH | null = null;
  let max = Math.max(areaSizeX, areaSizeY);
  for (let i = 0; i < 2000; i++) {
    let anyUnsuccessful = false;
    result = findBestPacking(rects, {
      maxBinSide: max,
      discardStep: 1,
      onSuccess: () => true,
      onFailure: () => {
        anyUnsuccessful = true;
        return false;
      },
    });
    if (!result || anyUnsuccessful) {
      max = Math.trunc(max * 1.2);
      continue;
    }
    break;
  }
  return result;
}

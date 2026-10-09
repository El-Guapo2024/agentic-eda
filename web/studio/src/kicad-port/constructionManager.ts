// Construction geometry and snap lines: the state behind "extension snaps" in the board and schematic editors. A port of common/tool/construction_manager.cpp and
// include/tool/construction_manager.h (CONSTRUCTION_MANAGER, SNAP_LINE_MANAGER, SNAP_MANAGER, and the ACTIVATION_HELPER they share), commit 8303b2ad.
//
//   SNAP_LINE_MANAGER   the line of snap directions (horizontal and vertical by default; the move tool adds the diagonals in 45-degree mode) through the last point the
//                       cursor snapped to -- its "snap line origin" -- and, once the cursor is on one of them, the snap line from the origin to where it is.
//                       `GetNearestSnapLinePoint` pulls the cursor onto such a line (a hysteresis of 1.5x the range for the direction already in use, an "escape"
//                       when the cursor swings away from it), and onto the grid along it.
//   CONSTRUCTION_MANAGER the extension geometry of items the cursor has dwelt on: a segment's two rays beyond its ends, the rest of an arc's circle. A batch is proposed;
//                       a persistent one is accepted at once, a temporary one at once while fewer than two are shown and otherwise only once it has been the
//                       standing proposal for `ExtensionSnapTimeoutMs` (ADVANCED_CFG, 500 ms). Items of an accepted batch are "involved": the constructed anchors
//                       (an intersection, an extension crossing a line) that involve only involved items may be snapped to.
//   SNAP_MANAGER        the two together, the reference-only points, and `GetConstructionItems`, which adds the snap line's own lines to the batches.
//
// The C++ accepts a pending proposal from a wxTimer. Here the caller passes the clock (`now`, ms) to each call and calls `tick( now )` when a timer it started fires,
// which accepts the pending proposal once its timeout has passed. Nothing in this file reads a clock or draws.
import type { SnapItem } from "./snapScene";
import { dist, type Geom, type Pt } from "./snapGeom";

/** `ADVANCED_CFG::m_ExtensionSnapTimeoutMs`. */
export const EXTENSION_SNAP_TIMEOUT_MS = 500;
/** `CONSTRUCTION_MANAGER::getMaxTemporaryBatches`: "one previous temporary batch and the current one". */
export const MAX_TEMPORARY_BATCHES = 2;

/** A `CONSTRUCTION_GEOM::DRAWABLE`: a line, ray, circle, arc or segment, or a free point (`VECTOR2I`) that is itself an anchor and is drawn as a cross. */
export type Drawable = Geom | { t: "point"; p: Pt };

export interface ConstructionItem {
  source: "items" | "snapLine";
  /** The board item the geometry extends; null for the snap line's own lines (`FROM_SNAP_LINE`). */
  item: SnapItem | null;
  constructions: { drawable: Drawable; lineWidth: number }[];
}

export type ConstructionBatch = ConstructionItem[];

// ------------------------------------------------------------------------------------------------------------------------------ ACTIVATION_HELPER

/** `ACTIVATION_HELPER<T>`: a proposal becomes accepted when nothing replaces or cancels it for `timeoutMs`, or at once when asked to. */
class ActivationHelper<T> {
  private pendingTag: string | null = null;
  private pendingAt = 0;
  private lastAcceptedTag: string | null = null;
  private lastProposal: T | null = null;

  constructor(private readonly timeoutMs: number) {}

  /** `ProposeActivation`: returns the proposal when it was accepted by this call, else null (the same one as the last accepted, or pending already, or waiting for its timeout). */
  propose(proposal: T, tag: string, acceptImmediately: boolean, now: number): T | null {
    if (this.lastAcceptedTag !== null && tag === this.lastAcceptedTag) return null;
    if (this.pendingTag !== null && tag === this.pendingTag) return null;
    this.pendingTag = tag;
    this.pendingAt = now;
    this.lastProposal = proposal;
    return acceptImmediately ? this.accept() : null;
  }

  cancel(): void {
    this.pendingTag = null;
    this.lastProposal = null;
  }

  /** The timer: accepts the pending proposal when its timeout has passed. */
  tick(now: number): T | null {
    if (this.pendingTag !== null && now - this.pendingAt >= this.timeoutMs) return this.accept();
    return null;
  }

  get isPending(): boolean {
    return this.pendingTag !== null;
  }

  /** When the pending proposal comes due (ms on the caller's clock), or null. */
  get dueAt(): number | null {
    return this.pendingTag !== null ? this.pendingAt + this.timeoutMs : null;
  }

  private accept(): T | null {
    if (this.pendingTag === null) return null;
    this.lastAcceptedTag = this.pendingTag;
    this.pendingTag = null;
    const proposal = this.lastProposal;
    this.lastProposal = null;
    return proposal;
  }
}

// ------------------------------------------------------------------------------------------------------------------------------ CONSTRUCTION_MANAGER

interface PendingBatch {
  batch: ConstructionBatch;
  persistent: boolean;
}

/** `HashConstructionBatchSources`: what the batch is made of, as a string. */
function batchTag(batch: ConstructionBatch, persistent: boolean): string {
  return `${persistent ? "P" : "T"}|${batch.map((c) => `${c.source}:${c.item?.id ?? "-"}`).join(",")}`;
}

export class ConstructionManager {
  private persistent: ConstructionBatch | null = null;
  private temporary: ConstructionBatch[] = [];
  private involved = new Set<SnapItem>();
  private readonly activation: ActivationHelper<PendingBatch>;
  /** Called with true whenever the geometry on show changed. */
  onChange: (() => void) | null = null;

  constructor(timeoutMs = EXTENSION_SNAP_TIMEOUT_MS) {
    this.activation = new ActivationHelper<PendingBatch>(timeoutMs);
  }

  /** `ProposeConstructionItems`. An empty batch is not worth proposing: it would only clear what is shown. */
  proposeConstructionItems(batch: ConstructionBatch, persistent: boolean, now: number): void {
    if (batch.length === 0) return;
    const acceptImmediately = persistent || this.temporary.length < MAX_TEMPORARY_BATCHES;
    const accepted = this.activation.propose({ batch, persistent }, batchTag(batch, persistent), acceptImmediately, now);
    if (accepted) this.accept(accepted);
  }

  /** `CancelProposal`. */
  cancelProposal(): void {
    this.activation.cancel();
  }

  /** The timer fired (or the clock moved on): accepts a proposal that has waited long enough. True if the geometry on show changed. */
  tick(now: number): boolean {
    const accepted = this.activation.tick(now);
    return accepted ? this.accept(accepted) : false;
  }

  /** When a pending proposal comes due, for the caller's timer. */
  get dueAt(): number | null {
    return this.activation.dueAt;
  }

  /** `acceptConstructionItems`. */
  private accept(pending: PendingBatch): boolean {
    if (pending.persistent) {
      // "We only keep one previous persistent batch for the moment."
      this.persistent = pending.batch;
    } else {
      // "If there are no new items involved, don't bother adding the batch."
      if (!pending.batch.some((c) => c.item !== null && !this.involved.has(c.item))) return false;
      while (this.temporary.length >= MAX_TEMPORARY_BATCHES) this.temporary.shift();
      this.temporary.push(pending.batch);
    }
    this.involved.clear();
    for (const batch of [...(this.persistent ? [this.persistent] : []), ...this.temporary]) for (const c of batch) if (c.item) this.involved.add(c.item);
    this.onChange?.();
    return true;
  }

  /** `InvolvesAllGivenRealItems`: null items (construction geometry that belongs to no board item) are always involved. */
  involvesAllGivenRealItems(items: readonly (SnapItem | null)[]): boolean {
    return items.every((item) => item === null || this.involved.has(item));
  }

  /** `GetConstructionItems`. */
  getConstructionItems(): ConstructionBatch[] {
    return [...(this.persistent ? [this.persistent] : []), ...this.temporary];
  }

  /** `HasActiveConstruction`. */
  hasActiveConstruction(): boolean {
    return this.persistent !== null || this.temporary.length > 0;
  }

  /** The drawables on show, each with whether its batch is the persistent one. */
  drawables(): { drawable: Drawable; persistent: boolean; lineWidth: number }[] {
    const out: { drawable: Drawable; persistent: boolean; lineWidth: number }[] = [];
    if (this.persistent) for (const c of this.persistent) for (const d of c.constructions) out.push({ drawable: d.drawable, persistent: true, lineWidth: d.lineWidth });
    for (const batch of this.temporary) for (const c of batch) for (const d of c.constructions) out.push({ drawable: d.drawable, persistent: false, lineWidth: d.lineWidth });
    return out;
  }

  /** `Clear`. */
  clear(): void {
    const had = this.hasActiveConstruction();
    this.persistent = null;
    this.temporary = [];
    this.involved.clear();
    this.cancelProposal();
    if (had) this.onChange?.();
  }
}

// ------------------------------------------------------------------------------------------------------------------------------ SNAP_LINE_MANAGER

/** `normalizeDirection`: reduced by the gcd and made to point right (or, if vertical, down). */
function normalizeDirection(d: Pt): Pt {
  if (d[0] === 0 && d[1] === 0) return [0, 0];
  const gcd = (a: number, b: number): number => (b === 0 ? Math.abs(a) : gcd(b, a % b));
  let dx = d[0];
  let dy = d[1];
  const g = Number.isInteger(dx) && Number.isInteger(dy) ? gcd(dx, dy) : 0;
  if (g > 0) {
    dx /= g;
    dy /= g;
  }
  if (dx < 0 || (dx === 0 && dy < 0)) {
    dx = dx === 0 ? 0 : -dx; // (not -0)
    dy = dy === 0 ? 0 : -dy;
  }
  return [dx, dy];
}

const sameDir = (a: Pt, b: Pt) => a[0] === b[0] && a[1] === b[1];

/** The angle of a vector, atan2 in the board's y-down frame. */
const angleOf = (v: Pt) => Math.atan2(v[1], v[0]);

export class SnapLineManager {
  private origin: Pt | null = null;
  private end: Pt | null = null;
  private dirs: Pt[] = [];
  private active: number | null = null;
  onChange: (() => void) | null = null;

  constructor() {
    this.setDirections([
      [1, 0],
      [0, 1],
    ]);
  }

  get snapLineOrigin(): Pt | null {
    return this.origin;
  }

  get snapLineEnd(): Pt | null {
    return this.end;
  }

  get directions(): readonly Pt[] {
    return this.dirs;
  }

  get activeDirection(): number | null {
    return this.active;
  }

  hasCompleteSnapLine(): boolean {
    return this.origin !== null && this.end !== null;
  }

  private findDirectionIndex(delta: Pt): number | null {
    const n = normalizeDirection(delta);
    if (n[0] === 0 && n[1] === 0) return null;
    const i = this.dirs.findIndex((d) => sameDir(d, n));
    return i >= 0 ? i : null;
  }

  /** `SetDirections`. */
  setDirections(directions: readonly Pt[]): void {
    const unique: Pt[] = [];
    for (const d of directions) {
      const n = normalizeDirection(d);
      if (n[0] === 0 && n[1] === 0) continue;
      if (!unique.some((u) => sameDir(u, n))) unique.push(n);
    }
    const changed = unique.length !== this.dirs.length || unique.some((u, i) => !sameDir(u, this.dirs[i]!));
    if (!changed) return;
    this.dirs = unique;
    this.active = null;
    if (this.origin && this.end && this.findDirectionIndex([this.end[0] - this.origin[0], this.end[1] - this.origin[1]]) === null) this.end = null;
    if (this.dirs.length === 0) {
      this.clearSnapLine();
      return;
    }
    this.onChange?.();
  }

  /** `SetSnapLineOrigin`. */
  setSnapLineOrigin(origin: Pt): void {
    if (this.origin && this.origin[0] === origin[0] && this.origin[1] === origin[1] && !this.end) {
      this.onChange?.();
      return;
    }
    this.origin = origin;
    this.end = null;
    this.active = null;
    this.onChange?.();
  }

  /** `SetSnapLineEnd` (null keeps the origin and unsets the end). */
  setSnapLineEnd(end: Pt | null): void {
    const same = (a: Pt | null, b: Pt | null) => (a === null || b === null ? a === b : a[0] === b[0] && a[1] === b[1]);
    if (this.origin && !same(end, this.end)) {
      this.end = end;
      this.active = end ? this.findDirectionIndex([end[0] - this.origin[0], end[1] - this.origin[1]]) : null;
      this.onChange?.();
    }
  }

  /** `ClearSnapLine`. */
  clearSnapLine(): void {
    this.origin = null;
    this.end = null;
    this.active = null;
    this.onChange?.();
  }

  /**
   * `SetSnappedAnchor`: an anchor was snapped to. On a snap direction from the origin it is the snap line's end; anywhere else it is the new origin; with no origin
   * it starts one.
   */
  setSnappedAnchor(anchor: Pt): void {
    if (this.origin) {
      if (this.findDirectionIndex([anchor[0] - this.origin[0], anchor[1] - this.origin[1]]) !== null) this.setSnapLineEnd(anchor);
      else this.setSnapLineOrigin(anchor);
    } else {
      this.setSnapLineOrigin(anchor);
    }
  }

  /**
   * `GetNearestSnapLinePoint`: where the cursor goes if it is near a snap direction from the origin. `distToNearest` is the distance to the nearest anchor that is not a
   * grid point (null for none): within `snapRange` of one, the anchor wins and this returns null. Otherwise, per direction, the cursor must be within `snapRange` of
   * the line; far out along it (more than twice the range off) it "escapes" if its angle to the line exceeds 4 degrees. The projection is put on the grid where there
   * is one: a horizontal or vertical line takes the nearest grid point's coordinate along it, a diagonal the nearby grid point closest to the line.
   */
  nearestSnapLinePoint(cursor: Pt, nearestGrid: Pt, distToNearest: number | null, snapRange: number, gridSize: Pt = [0, 0], gridOrigin: Pt = [0, 0]): Pt | null {
    if (!this.origin || this.dirs.length === 0) return null;
    const gridBetterThanNearest = distToNearest === null || distToNearest > snapRange;
    const gridActive = gridSize[0] > 0 && gridSize[1] > 0;
    if (!gridBetterThanNearest) return null;
    const escapeRange = 2 * snapRange;
    const longRangeEscapeAngle = (4 * Math.PI) / 180;
    const origin = this.origin;
    const delta: Pt = [cursor[0] - origin[0], cursor[1] - origin[1]];
    let bestPerp = Infinity;
    let best: Pt | null = null;
    for (const direction of this.dirs) {
      const len = Math.hypot(direction[0], direction[1]);
      if (len === 0) continue;
      const unit: Pt = [direction[0] / len, direction[1] / len];
      const along = delta[0] * unit[0] + delta[1] * unit[1];
      const projection: Pt = [origin[0] + unit[0] * along, origin[1] + unit[1] * along];
      const perp = Math.hypot(delta[0] - unit[0] * along, delta[1] - unit[1] * along);
      if (perp > snapRange) continue;
      if (perp >= escapeRange) {
        let diff = angleOf(delta) - angleOf(direction);
        while (diff > Math.PI) diff -= 2 * Math.PI;
        while (diff <= -Math.PI) diff += 2 * Math.PI;
        if (Math.abs(diff) > longRangeEscapeAngle) continue;
      }
      let snapPoint: Pt = projection;
      if (gridActive) {
        if (direction[0] === 0 && direction[1] !== 0) snapPoint = [origin[0], nearestGrid[1]];
        else if (direction[1] === 0 && direction[0] !== 0) snapPoint = [nearestGrid[0], origin[1]];
        else snapPoint = nearestGridPointOnLine(projection, origin, unit, cursor, gridSize, gridOrigin);
      }
      if (perp < bestPerp) {
        bestPerp = perp;
        best = snapPoint;
      }
    }
    return best;
  }
}

/** The nearby grid point (3 x 3 around the projection) closest to the line, with a small preference for the one near the cursor -- the diagonal case in KiCad's snap lines. */
export function nearestGridPointOnLine(projection: Pt, origin: Pt, unit: Pt, cursor: Pt, grid: Pt, gridOrigin: Pt): Pt {
  const rel: Pt = [projection[0] - gridOrigin[0], projection[1] - gridOrigin[1]];
  let bestScore = Infinity;
  let bestPoint: Pt = projection;
  for (let dx = -1; dx <= 1; dx++) {
    for (let dy = -1; dy <= 1; dy++) {
      const gx = Math.round(rel[0] / grid[0]) * grid[0] + dx * grid[0];
      const gy = Math.round(rel[1] / grid[1]) * grid[1] + dy * grid[1];
      const pt: Pt = [gx + gridOrigin[0], gy + gridOrigin[1]];
      const d: Pt = [pt[0] - origin[0], pt[1] - origin[1]];
      const dAlong = d[0] * unit[0] + d[1] * unit[1];
      const onLine: Pt = [origin[0] + unit[0] * dAlong, origin[1] + unit[1] * dAlong];
      const perp = dist(pt, onLine);
      const score = perp + dist(pt, cursor) * 0.1;
      if (score < bestScore) {
        bestScore = score;
        bestPoint = pt;
      }
    }
  }
  return bestPoint;
}

// ------------------------------------------------------------------------------------------------------------------------------ SNAP_MANAGER

export class SnapManager {
  readonly snapLines = new SnapLineManager();
  readonly construction: ConstructionManager;
  private referenceOnly: Pt[] = [];
  /** Called whenever something on show (guides, snap line, construction geometry) may have changed. */
  onChange: (() => void) | null = null;

  constructor(timeoutMs = EXTENSION_SNAP_TIMEOUT_MS) {
    this.construction = new ConstructionManager(timeoutMs);
    this.snapLines.onChange = () => this.onChange?.();
    this.construction.onChange = () => this.onChange?.();
  }

  /** `SetReferenceOnlyPoints`: "points that are not snapped to, but can still be used for connection to the snap line". */
  setReferenceOnlyPoints(points: Pt[]): void {
    this.referenceOnly = points;
  }

  get referenceOnlyPoints(): readonly Pt[] {
    return this.referenceOnly;
  }

  isReferenceOnly(p: Pt): boolean {
    return this.referenceOnly.some((q) => q[0] === p[0] && q[1] === p[1]);
  }

  /**
   * `GetConstructionItems`: the construction manager's batches and, with a snap line origin, one more whose lines run through the origin along each snap direction
   * (the active one drawn double). These are what the extension anchors are intersections of.
   */
  constructionItems(): ConstructionBatch[] {
    const batches = this.construction.getConstructionItems();
    const origin = this.snapLines.snapLineOrigin;
    if (origin) {
      const item: ConstructionItem = { source: "snapLine", item: null, constructions: [] };
      this.snapLines.directions.forEach((d, i) => {
        const far: Pt = [origin[0] + d[0] * 100000, origin[1] + d[1] * 100000];
        item.constructions.push({ drawable: { t: "line", a: origin, b: far }, lineWidth: this.snapLines.activeDirection === i ? 2 : 1 });
      });
      if (item.constructions.length > 0) batches.push([item]);
    }
    return batches;
  }

  /** `Clear`. */
  clear(): void {
    this.snapLines.clearSnapLine();
    this.construction.clear();
  }
}

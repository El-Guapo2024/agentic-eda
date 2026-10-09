// The board editor's snapping: where the cursor goes when an item is placed, drawn, routed or moved. A port of common/tool/grid_helper.cpp (GRID_HELPER, the base: Align,
// the grid origin and size, the auxiliary axes, SnapToConstructionLines) and pcbnew/tools/pcb_grid_helper.cpp (PCB_GRID_HELPER: BestSnapAnchor, BestDragOrigin, AlignToSegment,
// AlignToArc, SnapToPad, AddConstructionItems), commit 8303b2ad.
//
// `bestSnapAnchor( origin, layers, category, skip )` is the function every placing tool calls on every mouse event:
//   1. the items whose box touches a small square round the cursor (`snapRange` wide: 25 px, never more than the visible grid) give their anchors -- see snapAnchors.ts --
//      and the intersections of the ones that have idealised geometry, and the extension geometry of items the cursor dwelt on (constructionManager.ts);
//   2. snap lines first: a point on a horizontal / vertical (or, in 45-degree mode, diagonal) line through the last anchor snapped to, put on the grid along it;
//   3. then the anchor already snapped to, kept while the cursor is within `snapRange` + hysteresis of it (`m_snapItem`; the hysteresis is `SnapHysteresis`, 5 px) -- this
//      is what stops the cursor flickering between two points at the edge of range;
//   4. then the nearest anchor, taken when it is within `snapRange` - hysteresis;
//   5. with the grid off, the nearest point on any line or arc under the cursor; otherwise the nearest grid point of the category's grid (grid overrides).
// Ctrl turns the grid off (`SetUseGrid( false )`), Shift turns anchor snapping off (`SetSnap( false )`): `setUseGrid` / `setSnap`, set from each event's modifiers.
//
// The helper is stateful, like the C++ object a tool makes when it starts: it remembers the anchor snapped to, the snap line and the construction geometry. The editor makes one
// per tool, gives it the board's scene (snapScene.ts) and the view (`GridEnv`), and calls it with the pointer position of each event. It draws nothing: `overlay()` says
// what the canvas should draw (the snap marker, the guides, the construction geometry).
import { ANCHOR, computeItemAnchors, nearestFlagged, type Anchor, type AnchorContext, type DragFilter, type PcbMagnetic } from "./snapAnchors";
import type { ConstructionBatch, ConstructionItem, Drawable } from "./constructionManager";
import { queryScene, type FootprintItem, type PadItem, type SnapItem, type SnapScene } from "./snapScene";
import { arcStart, arcEnd, arcThrough, box, distanceTo, dist, half, intersections, line, nearestOfAny, PT, type ArcG, type Box, type Geom, type Pt } from "./snapGeom";
import type { GridCategory } from "./gridOverrides";
import { GridHelper, SNAP_HYSTERESIS_PX, SNAP_RANGE_PX, kiRound, layersOverlap, samePos, type GridEnv, type LayerSel, type SnapOverlay, type Visibility } from "./gridHelperBase";

export { GridHelper, SNAP_HYSTERESIS_PX, SNAP_RANGE_PX, kiRound };
export type { GridEnv, LayerSel, SnapOverlay, Visibility };

// ---------------------------------------------------------------------------------------------------------------------------- PCB_GRID_HELPER

const NEARABLE: ReadonlySet<SnapItem["type"]> = new Set(["shape", "track"]);

/** The idealised geometry of an item for the anchors' tie-break and the intersections (`GetBoardIntersectable`): shapes (not polygons or Beziers) and tracks. */
function intersectableOf(item: SnapItem): Geom | null {
  if (!NEARABLE.has(item.type)) return null;
  return item.geom ?? null;
}

/** `FindSquareDistanceToItem`: how far the item's idealised geometry is from `pos` (null when it has none). */
function distanceToItem(item: SnapItem, pos: Pt): number | null {
  const g = intersectableOf(item);
  return g ? distanceTo(g, pos) : null;
}

export class PcbGridHelper extends GridHelper {
  constructor(
    env: GridEnv,
    public scene: SnapScene,
    public magnetic: PcbMagnetic,
    public visibility: Visibility,
    /** `ADVANCED_CFG::m_SnapHysteresis`, px. */
    public hysteresisPx = SNAP_HYSTERESIS_PX,
    /** `ADVANCED_CFG::m_EnableExtensionSnaps`. */
    public extensionSnaps = true,
    /** `ADVANCED_CFG::m_ExtensionSnapActivateOnHover`. */
    public activateOnHover = true
  ) {
    super(env);
  }

  /** A new board: what it remembers refers to items that are gone. */
  setScene(scene: SnapScene): void {
    if (scene === this.scene) return;
    this.scene = scene;
    this.fullReset();
  }

  private pointOnLineCandidates: Geom[] = [];

  // ---- the item filter every query uses

  private visibleItem = (item: SnapItem): boolean => {
    const { layerVisible, highContrastLayers } = this.visibility;
    if (!item.layers.some(layerVisible)) return false;
    if (highContrastLayers && !item.layers.some((l) => highContrastLayers.has(l))) return false;
    return true;
  };

  private anchorContext(filter?: DragFilter): AnchorContext {
    return {
      magnetic: this.magnetic,
      visible: this.visibleItem,
      footprintLayerVisible: (fp: FootprintItem) => fp.layers.every(this.visibility.layerVisible),
      grid: this.getGrid(),
      filter,
    };
  }

  /** `queryVisible`. */
  private queryVisible(area: Box, skip: ReadonlySet<string>): SnapItem[] {
    return queryScene(this.scene, area, skip).filter(this.visibleItem);
  }

  // ---- the anchors

  /**
   * `computeAnchors( items, refPos, aFrom, filter, layers, forDrag )`: the anchors of every item (those on the layers asked for), then the points of the construction
   * geometry, the intersections of everything that has idealised geometry (unless picking up), and the geometry kept as "point on an element" candidates.
   */
  private computeAnchors(items: readonly SnapItem[], ref: Pt, from: boolean, filter: DragFilter | undefined, layers: LayerSel | null, forDrag: boolean): void {
    const intersectables: { item: SnapItem | null; geom: Geom }[] = [];
    const computeIntersections = !forDrag;
    const computePointsOnElements = !forDrag;
    const excludeGraphics = filter?.graphics === false;
    const excludeTracks = filter?.tracks === false;
    const ctx = this.anchorContext(filter);
    const snappable = (item: SnapItem): boolean => (layers ? this.magnetic.allLayers || layersOverlap(layers, item.layers) : true);

    for (const item of items) {
      // "Don't even process the item if it doesn't match the layers"
      if (!snappable(item)) continue;
      computeItemAnchors(this.anchors, item, ref, from, ctx);
      if (computeIntersections || computePointsOnElements) {
        let geom: Geom | null = null;
        if (!excludeGraphics && item.type === "shape") geom = intersectableOf(item);
        else if (!excludeTracks && item.type === "track") geom = intersectableOf(item);
        if (geom) intersectables.push({ item, geom });
      }
    }

    // The construction geometry: lines, rays, circles and arcs take part in the intersections; free points are anchors.
    for (const batch of this.snapManager.constructionItems()) {
      for (const c of batch) {
        for (const { drawable } of c.constructions) {
          if (drawable.t === "point") this.anchors.add(drawable.p, ANCHOR.SNAPPABLE | ANCHOR.CONSTRUCTED, [c.item], PT.NONE);
          else if (drawable.t === "line" || drawable.t === "circle" || drawable.t === "half" || drawable.t === "arc") intersectables.push({ item: c.item, geom: drawable });
        }
      }
    }

    if (computeIntersections) {
      for (let i = 0; i < intersectables.length; i++) {
        const a = intersectables[i]!;
        for (let j = i + 1; j < intersectables.length; j++) {
          const b = intersectables[j]!;
          // "An item and its own extension will often have intersections ... but they not useful points to snap to"
          if (a.item === b.item) continue;
          for (const p of intersections(a.geom, b.geom)) this.anchors.add(p, ANCHOR.SNAPPABLE | ANCHOR.CONSTRUCTED, [a.item, b.item], PT.INTERSECTION);
        }
      }
    }

    // The intersectables are also what a point "on an element" is searched in when nothing else snaps.
    this.pointOnLineCandidates = [];
    if (computePointsOnElements) for (const i of intersectables) this.pointOnLineCandidates.push(i.geom);
  }

  /** `nearestAnchor( aPos, aFlags )`: the nearest anchor with the flags; of those at one position, the one whose item's geometry is nearest the cursor. */
  private nearestAnchor(pos: Pt, flags: number): Anchor | null {
    let minDist = Infinity;
    let atMin: Anchor[] = [];
    for (const anchor of this.anchors.list) {
      if ((flags & anchor.flags) !== flags) continue;
      if (atMin.length > 0 && samePos(anchor.pos, atMin[0]!.pos)) {
        atMin.push(anchor);
      } else {
        const d = (anchor.pos[0] - pos[0]) ** 2 + (anchor.pos[1] - pos[1]) ** 2;
        if (d < minDist) {
          minDist = d;
          atMin = [anchor];
        }
      }
    }
    // "Check that any involved real items are 'active' (i.e. the user has moused over a key point previously)": a constructed anchor may be taken only when every
    // real item that makes it is on show in the construction manager.
    if (this.extensionSnaps) atMin = atMin.filter((a) => !(a.flags & ANCHOR.CONSTRUCTED) || this.construction.involvesAllGivenRealItems(a.items));
    let minDistToItem = Infinity;
    let best: Anchor | null = null;
    for (const anchor of atMin) {
      let nearestItem = Infinity;
      for (const item of anchor.items) {
        if (!item) continue;
        const d = distanceToItem(item, pos);
        if (d !== null) nearestItem = Math.min(nearestItem, d * d);
      }
      // "If the item doesn't have any special min-dist handler, just use the distance to the anchor"
      nearestItem = Math.min(nearestItem, minDist);
      if (nearestItem < minDistToItem) {
        minDistToItem = nearestItem;
        best = anchor;
      }
    }
    return best;
  }

  // ---- construction geometry

  /**
   * `AddConstructionItems`: proposes the extension geometry of items to the construction manager. A segment gives the two rays beyond its ends (or its whole line when
   * `extensionOnly` is false), an arc the rest of its circle, a circle or rectangle its centre; with `persistent` the end points go in too, as reference points that are
   * not snapped to themselves. Other items are proposed with nothing to draw -- they still become "involved".
   */
  addConstructionItems(items: readonly SnapItem[], extensionOnly: boolean, persistent: boolean): void {
    if (!this.extensionSnaps) return;
    const batch: ConstructionBatch = [];
    const referenceOnly: Pt[] = [];
    for (const item of items) {
      const draw: Drawable[] = [];
      if (item.type === "shape") {
        const s = item.shape;
        if (s.kind === "segment") {
          if (!extensionOnly) {
            draw.push(line(s.start, s.end));
          } else {
            const vec: Pt = [s.end[0] - s.start[0], s.end[1] - s.start[1]];
            draw.push(half(s.start, [s.start[0] - vec[0], s.start[1] - vec[1]]));
            draw.push(half(s.end, [s.end[0] + vec[0], s.end[1] + vec[1]]));
          }
          if (persistent) {
            draw.push({ t: "point", p: s.start }, { t: "point", p: s.end });
            referenceOnly.push(s.start, s.end);
          }
        } else if (s.kind === "arc") {
          const arc: ArcG | null = item.geom?.t === "arc" ? item.geom : null;
          if (arc) {
            if (!extensionOnly) {
              draw.push({ t: "circle", c: arc.c, r: arc.r });
            } else {
              // "The rest of the circle is the arc through the opposite point to the midpoint"
              const opposite: Pt = [2 * arc.c[0] - s.mid[0], 2 * arc.c[1] - s.mid[1]];
              const rest = arcThrough(s.start, opposite, s.end);
              if (rest) draw.push(rest);
            }
            draw.push({ t: "point", p: arc.c });
          }
          if (persistent) {
            draw.push({ t: "point", p: s.start }, { t: "point", p: s.end });
            referenceOnly.push(s.start, s.end);
          }
        } else if (s.kind === "circle") {
          draw.push({ t: "point", p: s.center });
        } else if (s.kind === "rect") {
          draw.push({ t: "point", p: [(s.start[0] + s.end[0]) / 2, (s.start[1] + s.end[1]) / 2] });
        }
      }
      const entry: ConstructionItem = { source: "items", item, constructions: draw.map((drawable) => ({ drawable, lineWidth: 1 })) };
      batch.push(entry);
    }
    if (referenceOnly.length > 0) this.snapManager.setReferenceOnlyPoints(referenceOnly);
    this.construction.proposeConstructionItems(batch, persistent, this.clock());
  }

  // ---- AlignToSegment / AlignToArc / SnapToPad

  /** `AlignToSegment`: the track's end nearest the pointer, or a point on the segment that is on the grid's line through the aligned cursor; else the aligned cursor. */
  alignToSegment(p: Pt, a: Pt, b: Pt): Pt {
    const epsilonSq = 4e-6; // `c_gridSnapEpsilon_sq = 4` nm^2, in um^2
    const aligned = this.align(p);
    if (!this.enableSnap) return aligned;
    const points: Pt[] = [];
    for (const dir of [
      [1, 0],
      [0, 1],
      [1, 1],
      [1, -1],
    ] as const) {
      const hits = intersections({ t: "line", a, b }, { t: "line", a: aligned, b: [aligned[0] + dir[0], aligned[1] + dir[1]] });
      const hit = hits[0];
      if (hit && distanceTo({ t: "seg", a, b }, hit) ** 2 <= epsilonSq) points.push(hit);
    }
    let nearest = aligned;
    let minD = Infinity;
    for (const pt of [a, b]) {
      const d = (pt[0] - p[0]) ** 2 + (pt[1] - p[1]) ** 2;
      if (d < minD) {
        minD = d;
        nearest = pt;
      }
    }
    for (const pt of points) {
      const d = (pt[0] - aligned[0]) ** 2 + (pt[1] - aligned[1]) ** 2;
      if (d < minD) {
        minD = d;
        nearest = pt;
      }
    }
    return nearest;
  }

  /** `AlignToArc`. */
  alignToArc(p: Pt, arc: ArcG): Pt {
    const aligned = this.align(p);
    if (!this.enableSnap) return aligned;
    const points: Pt[] = [];
    for (const dir of [
      [1, 0],
      [0, 1],
      [1, 1],
      [1, -1],
    ] as const) points.push(...intersections(arc, { t: "line", a: aligned, b: [aligned[0] + dir[0], aligned[1] + dir[1]] }));
    let nearest = aligned;
    let minD = Infinity;
    for (const pt of [arcStart(arc), arcEnd(arc)]) {
      const d = (pt[0] - p[0]) ** 2 + (pt[1] - p[1]) ** 2;
      if (d < minD) {
        minD = d;
        nearest = pt;
      }
    }
    for (const pt of points) {
      const d = (pt[0] - aligned[0]) ** 2 + (pt[1] - aligned[1]) ** 2;
      if (d < minD) {
        minD = d;
        nearest = pt;
      }
    }
    return nearest;
  }

  /** `SnapToPad`: the centre of the pad under the mouse, else the mouse position. */
  snapToPad(mouse: Pt, pads: readonly PadItem[]): Pt {
    this.anchors.clear();
    const ctx = this.anchorContext();
    for (const pad of pads) if (pad.hit(mouse)) computeItemAnchors(this.anchors, pad, mouse, true, { ...ctx, magnetic: { ...this.magnetic, pads: "always" } });
    const origin = nearestFlagged(this.anchors.list, mouse, ANCHOR.ORIGIN);
    return origin ? origin.pos : mouse;
  }

  // ---- BestDragOrigin

  /**
   * `BestDragOrigin`: the point of the items being picked up that the move starts from -- the nearest of their origins (a pad's centre, a footprint's position, a circle's
   * centre) and corners to the mouse, or a point on an outline when both are far (more than 50 px) away.
   */
  bestDragOrigin(mouse: Pt, items: readonly SnapItem[], filter?: DragFilter): Pt {
    this.anchors.clear();
    this.computeAnchors(items, mouse, true, filter, null, true);
    const lineSnapMinCornerDistance = 50 / this.env.scale;
    const outline = this.nearestAnchor(mouse, ANCHOR.OUTLINE);
    const corner = this.nearestAnchor(mouse, ANCHOR.CORNER);
    const origin = this.nearestAnchor(mouse, ANCHOR.ORIGIN);
    let best: Anchor | null = null;
    let minDist = Infinity;
    if (origin) {
      minDist = dist(origin.pos, mouse);
      best = origin;
    }
    if (corner) {
      const d = dist(corner.pos, mouse);
      if (d < minDist) {
        minDist = d;
        best = corner;
      }
    }
    if (outline) {
      const d = dist(outline.pos, mouse);
      if (minDist > lineSnapMinCornerDistance && d < minDist) best = outline;
    }
    return best ? best.pos : mouse;
  }

  // ---- BestSnapAnchor

  /** The snap range for the current view and grid: 25 px, and never more than the visible grid while the grid is on. */
  snapRange(): number {
    const snapScale = SNAP_RANGE_PX / this.env.scale;
    return this.enableGrid ? Math.min(snapScale, this.getVisibleGrid()) : snapScale;
  }

  /**
   * `BestSnapAnchor( aOrigin, aLayers, aGrid, aSkip )`: where the cursor at `origin` goes -- see the head of this file for the order of the rules. `layers` are the layers of
   * the item being drawn or moved: only items on them are snapped to, unless the magnetic setting `allLayers` is on. `skip` holds ids (an item's, or its owner's) to ignore --
   * the items being moved.
   */
  bestSnapAnchor(origin: Pt, layers: LayerSel, category: GridCategory = "current", skip: ReadonlySet<string> = new Set()): Pt {
    const snapRange = this.snapRange();
    const horizon = box(origin[0] - snapRange / 2, origin[1] - snapRange / 2, origin[0] + snapRange / 2, origin[1] + snapRange / 2);

    this.anchors.clear();
    const visibleItems = this.queryVisible(horizon, skip);
    this.computeAnchors(visibleItems, origin, false, undefined, layers, false);

    const nearest = this.nearestAnchor(origin, ANCHOR.SNAPPABLE);
    const nearestGrid = this.align(origin, category);
    const gridSize = this.gridSize(category);

    const hysteresisWorld = this.hysteresisPx / this.env.scale;
    const snapIn = Math.max(0, snapRange - hysteresisWorld);
    const snapOut = snapRange + hysteresisWorld;

    // The distance to the nearest snap point, if any -- the anchor already snapped to counts too, so that leaving it is judged against it.
    let snapDist: number | null = nearest ? dist(nearest.pos, origin) : null;
    if (this.snapItem) {
      const existing = dist(this.snapItem.pos, origin);
      if (snapDist === null || existing < snapDist) snapDist = existing;
    }

    this.constructionVisible = this.enableSnap;
    const lines = this.snapManager.snapLines;
    const proposeConstructionFor = (items: readonly (SnapItem | null)[]): void => {
      const list: SnapItem[] = [];
      for (const item of items) if (item && (this.magnetic.allLayers || layersOverlap(layers, item.layers))) list.push(item);
      // "Temporary construction items are not persistent and don't overlay the items themselves (as the items will not be moved)"
      this.addConstructionItems(list, true, false);
    };

    let snapValid = false;

    if (this.enableSnap) {
      // Existing snap lines need priority over new snaps.
      if (this.enableSnapLine) {
        let snapLineSnap = lines.nearestSnapLinePoint(origin, nearestGrid, snapDist, snapRange, gridSize, this.getOrigin());
        if (!snapLineSnap) snapLineSnap = this.snapToConstructionLines(origin, nearestGrid, gridSize, snapRange);

        if (snapLineSnap && !(this.skipPoint && samePos(this.skipPoint, snapLineSnap))) {
          // "Prefer actual anchors over construction line grid intersections"
          const preferAnchor = nearest !== null && dist(nearest.pos, origin) <= snapIn;
          if (!preferAnchor) {
            lines.setSnapLineEnd(snapLineSnap);
            snapValid = true;
            // "Don't show a snap point if we're snapping to a grid rather than an anchor"
            this.hideSnapPoint();
            // "Only return the snap line end as a snap if it's not a reference point"
            if (!this.snapManager.isReferenceOnly(snapLineSnap)) return snapLineSnap;
          }
        }
      }

      if (this.snapItem) {
        const d = dist(this.snapItem.pos, origin);
        if (d <= snapOut) {
          if (nearest && this.snapManager.isReferenceOnly(nearest.pos) && dist(nearest.pos, origin) <= snapRange) lines.setSnapLineOrigin(nearest.pos);
          lines.setSnappedAnchor(this.snapItem.pos);
          this.updateSnapPoint(this.snapItem.pos, this.snapItem.pointTypes);
          return this.snapItem.pos;
        }
        // "m_snapItem too far, clearing..."
        this.snapItem = null;
      }

      // If there's a snap anchor within range, use it if we can.
      if (nearest && dist(nearest.pos, origin) <= snapIn) {
        const constructed = (nearest.flags & ANCHOR.CONSTRUCTED) !== 0;
        if (this.snapManager.isReferenceOnly(nearest.pos)) {
          // "We can set the snap line origin, but don't mess with the accepted snap point"
          lines.setSnapLineOrigin(nearest.pos);
        } else {
          // "'Intrinsic' points of items can trigger adding construction geometry for _that_ item by proximity. E.g. just mousing over the intersection of an item doesn't
          // add a construction item for the second item)."
          if (!constructed) proposeConstructionFor(nearest.items);
          this.snapItem = nearest;
          lines.setSnappedAnchor(nearest.pos);
          this.updateSnapPoint(nearest.pos, nearest.pointTypes);
          return nearest.pos;
        }
        snapValid = true;
      } else if (this.activateOnHover) {
        // "An exact hit on an item, even if not near a snap point"
        for (const item of visibleItems) {
          if (item.hit(origin)) {
            proposeConstructionFor([item]);
            snapValid = true;
            break;
          }
        }
      }

      // "If we're snapping to a grid, on-element snaps would be too intrusive but they're useful when there isn't a grid to snap to"
      if (!this.enableGrid) {
        const onElement = nearestOfAny(this.pointOnLineCandidates, origin);
        if (onElement && dist(onElement, origin) <= snapRange) {
          this.updateSnapPoint(onElement, PT.ON_ELEMENT);
          // "Clear the snap end, but keep the origin so touching another line doesn't kill a snap line"
          lines.setSnapLineEnd(null);
          return onElement;
        }
      }
    }

    // Completely failed to find any snap point, so snap to the grid.
    this.snapItem = null;
    if (!snapValid) this.construction.cancelProposal();
    lines.setSnapLineEnd(null);
    this.hideSnapPoint();
    return nearestGrid;
  }

  /** `GetSnapped`: the item of the anchor last snapped to. */
  getSnapped(): SnapItem | null {
    return this.snapItem?.items[0] ?? null;
  }
}

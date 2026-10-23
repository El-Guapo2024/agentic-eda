// KiCad 10's grid items: a cartesian or polar sub-grid placed on the board that the cursor, the router and the placement tools snap to where it covers the board, instead of the
// editor's own grid (file version 20260728).
//
// Ported from KiCad 25ec2335 (the commit the box's kicad-cli nightly is built from; the 8303b2ad source the rest of the port reads predates grid items):
//   pcbnew/pcb_grid_item.cpp          PCB_GRID_ITEM: AsGridGeometry, buildOutlineWorld, GetBoundingBox, HitTest, GetOrientationAt, FindActiveGridAt, the property rows
//   libs/kimath/src/geometry/grid_geometry.cpp   GRID_GEOMETRY::Snap, Contains, Area, TakesPrecedenceOver
//   pcbnew/tools/pcb_grid_helper.cpp  PCB_GRID_HELPER::Align (an active cursor grid owns the snap; a nearby one offers its points within the snap range), the grid item's anchors
//   pcbnew/tools/drawing_tool.cpp     DRAWING_TOOL::PlaceSubGrid's sizeToCursor
//   common/gal/graphics_abstraction_layer.cpp / cairo_gal.cpp   GAL::gridLattice, AutoSparsePitch, CAIRO_GAL_BASE::drawGridSource (what the painter draws: gridItemPaint.ts)
//   pcbnew/dialogs/dialog_grid_item_properties.cpp   the dialog's three ways of giving a rectangle and its polar page
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardGridItem, CmdGridItem } from "../api/pcbItemTypes";

type P = readonly [number, number];
type V = [number, number];

/** `GRID_GEOMETRY`: what the snap and the coverage tests work from. `orientation` is radians in KiCad's screen sense (`RotatePoint`), the grid's own. */
export interface GridGeometry {
  kind: "cartesian" | "polar";
  origin: P;
  /** Cartesian (dx, dy); polar (dr, dPhi in radians). */
  pitch: P;
  orientation: number;
  /** Cartesian half-extent (x, y); polar (rMax, phiMax in radians). */
  extent: P;
  priority: number;
}

const TWO_PI = 2 * Math.PI;

/** `RotatePoint( pt, angle )` (trigo.cpp) by an angle in radians: x' = x cos + y sin, y' = y cos - x sin -- counter-clockwise on the y-down screen. */
export function rotateScreen(p: P, rad: number): V {
  const s = Math.sin(rad);
  const c = Math.cos(rad);
  return [p[0] * c + p[1] * s, p[1] * c - p[0] * s];
}

/** `PCB_GRID_ITEM::AsGridGeometry`. */
export function gridGeometry(g: Pick<BoardGridItem, "kind" | "x" | "y" | "extent" | "spacing" | "angle" | "phi_extent" | "phi_spacing" | "priority">): GridGeometry {
  const orientation = (g.angle * Math.PI) / 180;
  if (g.kind === "polar") {
    return { kind: "polar", origin: [g.x, g.y], pitch: [g.spacing[0], (g.phi_spacing * Math.PI) / 180], orientation, extent: [g.extent[0], (g.phi_extent * Math.PI) / 180], priority: g.priority };
  }
  return { kind: "cartesian", origin: [g.x, g.y], pitch: [g.spacing[0], g.spacing[1]], orientation, extent: [g.extent[0], g.extent[1]], priority: g.priority };
}

const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
/** `std::round`: half away from zero. */
const roundAway = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));

/** `GRID_GEOMETRY::Snap`: the grid point nearest `p`, clamped to the grid's own points (the last spoke, the outermost ring or line). */
export function snapToGeometry(g: GridGeometry, p: P): V {
  const local = rotateScreen([p[0] - g.origin[0], p[1] - g.origin[1]], -g.orientation);
  let cand: V;
  if (g.kind === "polar") {
    const [dr, dPhi] = g.pitch;
    const [rMax, phiMax] = g.extent;
    if (dr <= 0 || dPhi <= 0) return [p[0], p[1]];
    const r = Math.hypot(local[0], local[1]);
    const rIdx = clamp(roundAway(r / dr), 0, Math.trunc(rMax / dr));
    let phi = Math.atan2(local[1], local[0]);
    if (phi < 0) phi += TWO_PI;
    const pIdx = clamp(roundAway(phi / dPhi), 0, Math.trunc(phiMax / dPhi));
    const phiQ = pIdx * dPhi;
    const rs = rIdx * dr;
    cand = [rs * Math.cos(phiQ), rs * Math.sin(phiQ)];
  } else {
    const [dx, dy] = g.pitch;
    if (dx <= 0 || dy <= 0) return [p[0], p[1]];
    const nX = Math.trunc(g.extent[0] / dx);
    const nY = Math.trunc(g.extent[1] / dy);
    cand = [clamp(roundAway(local[0] / dx), -nX, nX) * dx, clamp(roundAway(local[1] / dy), -nY, nY) * dy];
  }
  const w = rotateScreen(cand, g.orientation);
  return [w[0] + g.origin[0], w[1] + g.origin[1]];
}

/** `GRID_GEOMETRY::Contains`. */
export function geometryContains(g: GridGeometry, p: P, tol = 0): boolean {
  const local = rotateScreen([p[0] - g.origin[0], p[1] - g.origin[1]], -g.orientation);
  if (g.kind === "polar") {
    if (Math.hypot(local[0], local[1]) > g.extent[0] + tol) return false;
    let phi = Math.atan2(local[1], local[0]);
    while (phi < 0) phi += TWO_PI;
    return phi <= Math.abs(g.extent[1]);
  }
  return local[0] >= -g.extent[0] - tol && local[0] <= g.extent[0] + tol && local[1] >= -g.extent[1] - tol && local[1] <= g.extent[1] + tol;
}

/** `GRID_GEOMETRY::Area`. */
export function geometryArea(g: GridGeometry): number {
  return g.kind === "polar" ? 0.5 * g.extent[0] * g.extent[0] * Math.abs(g.extent[1]) : Math.abs(4 * g.extent[0] * g.extent[1]);
}

/** `GRID_GEOMETRY::TakesPrecedenceOver`: the higher priority, then the smaller area. */
export function takesPrecedenceOver(a: GridGeometry, b: GridGeometry): boolean {
  if (a.priority !== b.priority) return a.priority > b.priority;
  return geometryArea(a) < geometryArea(b);
}

/** `PCB_GRID_ITEM::buildOutlineWorld`: the rectangle's four corners, or the polar grid's arc in 32 segments (with the apex first for a wedge short of a turn), turned and placed. */
export function gridOutline(g: Pick<BoardGridItem, "kind" | "x" | "y" | "extent" | "angle" | "phi_extent">): V[] {
  const rad = (g.angle * Math.PI) / 180;
  const world = (x: number, y: number): V => {
    const r = rotateScreen([x, y], rad);
    return [g.x + roundAway(r[0]), g.y + roundAway(r[1])];
  };
  if (g.kind === "polar") {
    const r = g.extent[0];
    const phiMax = (g.phi_extent * Math.PI) / 180;
    const out: V[] = [];
    if (phiMax + 1e-9 < TWO_PI) out.push(world(0, 0));
    for (let i = 0; i <= 32; i++) {
      const phi = (phiMax * i) / 32;
      out.push(world(roundAway(r * Math.cos(phi)), roundAway(r * Math.sin(phi))));
    }
    return out;
  }
  const [ex, ey] = g.extent;
  return [world(-ex, -ey), world(ex, -ey), world(ex, ey), world(-ex, ey)];
}

/** `PCB_GRID_ITEM::GetBoundingBox`: the outline's box. */
export function gridBounds(g: Pick<BoardGridItem, "kind" | "x" | "y" | "extent" | "angle" | "phi_extent">): [number, number, number, number] {
  let x0 = Infinity,
    y0 = Infinity,
    x1 = -Infinity,
    y1 = -Infinity;
  for (const [x, y] of gridOutline(g)) {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x);
    y1 = Math.max(y1, y);
  }
  return [x0, y0, x1, y1];
}

/** `PCB_GRID_ITEM::HitTest( aPosition )`: anywhere the grid covers, turned as it is (0 for a hit, else how far out). */
export function gridHitDistance(g: BoardGridItem, x: number, y: number): number {
  const geom = gridGeometry(g);
  if (geometryContains(geom, [x, y])) return 0;
  // A rough distance from the outline for the click's slop.
  const pts = gridOutline(g);
  let best = Infinity;
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i]!;
    const b = pts[(i + 1) % pts.length]!;
    const dx = b[0] - a[0];
    const dy = b[1] - a[1];
    const len2 = dx * dx + dy * dy;
    const t = len2 === 0 ? 0 : clamp(((x - a[0]) * dx + (y - a[1]) * dy) / len2, 0, 1);
    best = Math.min(best, Math.hypot(x - (a[0] + t * dx), y - (a[1] + t * dy)));
  }
  return best;
}

/** `GetCoverageArea`: the selection tool's area for the grid (`AsGridGeometry().Area()`). */
export function gridArea(g: BoardGridItem): number {
  return geometryArea(gridGeometry(g));
}

export type GridRole = "cursor" | "routing" | "placement";

/** `FindActiveGridAt`: of the grids that apply to `role` and cover `p` (a selected grid is being edited and is left out, so it does not snap to itself), the one that takes precedence. */
export function findActiveGridAt(grids: readonly BoardGridItem[], p: P, role: GridRole, selected: ReadonlySet<string> = new Set()): BoardGridItem | null {
  let best: BoardGridItem | null = null;
  let bestGeom: GridGeometry | null = null;
  for (const g of grids) {
    if (selected.has(g.id) || !g.affects[role]) continue;
    const geom = gridGeometry(g);
    if (!geometryContains(geom, p)) continue;
    if (!best || takesPrecedenceOver(geom, bestGeom!)) {
      best = g;
      bestGeom = geom;
    }
  }
  return best;
}

/** `SnapToGrid( grid, world )`: the grid's point nearest `p`, rounded to the board's units. */
export function snapToGridItem(g: BoardGridItem, p: P): V {
  const s = snapToGeometry(gridGeometry(g), p);
  return [roundAway(s[0]), roundAway(s[1])];
}

/**
 * `PCB_GRID_HELPER::Align` with grid items: a cursor grid that covers `p` owns the snap; otherwise the editor's grid point `gridAligned` stands, unless a cursor grid whose box
 * (grown by `snapRange`) holds `p` has a point nearer than `snapRange` -- the nearest such wins.
 */
export function alignWithGridItems(grids: readonly BoardGridItem[], p: P, gridAligned: P, snapRange: number, selected: ReadonlySet<string> = new Set()): V {
  const active = findActiveGridAt(grids, p, "cursor", selected);
  if (active) return snapToGridItem(active, p);
  let bestDist = snapRange * snapRange;
  let best: V = [gridAligned[0], gridAligned[1]];
  for (const g of grids) {
    if (!g.affects.cursor || selected.has(g.id)) continue;
    const [x0, y0, x1, y1] = gridBounds(g);
    if (p[0] < x0 - snapRange || p[0] > x1 + snapRange || p[1] < y0 - snapRange || p[1] > y1 + snapRange) continue;
    const c = snapToGridItem(g, p);
    const d = (c[0] - p[0]) ** 2 + (c[1] - p[1]) ** 2;
    if (d < bestDist) {
      bestDist = d;
      best = c;
    }
  }
  return best;
}

/** The grid item's own anchors (`PCB_GRID_HELPER::computeAnchors`, `PCB_GRID_ITEM_T`): its centre, and the corners of a rectangle or the two ends and the middle of a polar grid's arc. */
export function gridAnchors(g: BoardGridItem): { centre: V; corners: V[] } {
  const rad = (g.angle * Math.PI) / 180;
  const at = (x: number, y: number): V => {
    const r = rotateScreen([x, y], rad);
    return [g.x + r[0], g.y + r[1]];
  };
  if (g.kind === "polar") {
    const r = g.extent[0];
    const phi = (g.phi_extent * Math.PI) / 180;
    return { centre: [g.x, g.y], corners: [at(r, 0), at(roundAway(r * Math.cos(phi)), roundAway(r * Math.sin(phi))), at(roundAway(r * Math.cos(phi / 2)), roundAway(r * Math.sin(phi / 2)))] };
  }
  const [ex, ey] = g.extent;
  return { centre: [g.x, g.y], corners: [at(-ex, -ey), at(ex, -ey), at(ex, ey), at(-ex, ey)] };
}

// ---------------------------------------------------------------------------------------------------------------------------- the item

/** `EDA_ANGLE::Normalize`: into [0, 360). */
export function normalizeDeg(a: number): number {
  let v = a;
  while (v < -0) v += 360;
  while (v >= 360) v -= 360;
  return v;
}

/** `PCB_GRID_ITEM::PCB_GRID_ITEM`: a cartesian grid 10 mm square with a 1 mm pitch, a full turn and 10 degrees for a polar one, priority 1, affecting everything. */
export function newGridItem(x: number, y: number): BoardGridItem {
  return { id: "", kind: "cartesian", x, y, extent: [5_000, 5_000], spacing: [1_000, 1_000], angle: 0, phi_extent: 360, phi_spacing: 10, priority: 1, tick_interval: 0, affects: { cursor: true, routing: true, placement: true } };
}

/** `DRAWING_TOOL::PlaceSubGrid`'s `sizeToCursor`: the grid as big as the centre's distance to the cursor each way, turned to face it (`SetOrientation( -EDA_ANGLE( v ) )`). */
export function sizeGridToCursor(g: BoardGridItem, cursor: P): BoardGridItem {
  const vx = cursor[0] - g.x;
  const vy = cursor[1] - g.y;
  const len = roundAway(Math.hypot(vx, vy));
  const angle = vx === 0 && vy === 0 ? 0 : (Math.atan2(vy, vx) * 180) / Math.PI;
  return { ...g, angle: normalizeDeg(-angle), extent: [len, len] };
}

/** The grid as `add_grid_item` / `edit_grid_item` take it. */
export function gridItemCmd(g: BoardGridItem): CmdGridItem {
  return {
    kind: g.kind,
    at: { x: g.x, y: g.y },
    extent: { x: g.extent[0], y: g.extent[1] },
    spacing: { x: g.spacing[0], y: g.spacing[1] },
    angle: g.angle,
    phi_extent_deg: g.phi_extent,
    phi_spacing_deg: g.phi_spacing,
    priority: g.priority,
    tick_interval: g.tick_interval,
    affects: { ...g.affects },
    ...(g.custom_properties?.length ? { custom_properties: g.custom_properties } : {}),
  };
}

/** `GetItemDescription`: "Grid Item". */
export const GRID_ITEM_LABEL = "Grid Item";

// ---------------------------------------------------------------------------------------------------------------------------- the dialog

/** `DIALOG_GRID_ITEM_PROPERTIES`' extent box, in world coordinates on the turned outline (`toWorld`): the start and end corners, the start corner and the size, the centre and the size. */
export function gridDialogCorners(g: Pick<BoardGridItem, "x" | "y" | "extent" | "angle">): { start: V; end: V; size: V } {
  const half = rotateScreen(g.extent, (g.angle * Math.PI) / 180).map(roundAway) as V;
  return { start: [g.x - half[0], g.y - half[1]], end: [g.x + half[0], g.y + half[1]], size: [g.extent[0] * 2, g.extent[1] * 2] };
}

/** `onCornersChange`: the centre between the corners, the half-extent their difference taken back into the grid's frame (`toLocal`), magnitudes (`SetExtent`). */
export function gridFromCorners(g: BoardGridItem, start: P, end: P): BoardGridItem {
  const local = rotateScreen([end[0] - start[0], end[1] - start[1]], (-g.angle * Math.PI) / 180);
  return { ...g, x: Math.trunc((start[0] + end[0]) / 2), y: Math.trunc((start[1] + end[1]) / 2), extent: [Math.abs(Math.trunc(roundAway(local[0]) / 2)), Math.abs(Math.trunc(roundAway(local[1]) / 2))] };
}

/** `onCornerSizeChange`: the start corner and the size; the centre is half the size from the corner along the grid's own axes. */
export function gridFromCornerSize(g: BoardGridItem, start: P, size: P): BoardGridItem {
  const half: V = [Math.trunc(size[0] / 2), Math.trunc(size[1] / 2)];
  const w = rotateScreen(half, (g.angle * Math.PI) / 180);
  return { ...g, x: start[0] + roundAway(w[0]), y: start[1] + roundAway(w[1]), extent: [Math.abs(half[0]), Math.abs(half[1])] };
}

/** `onCenterSizeChange`. */
export function gridFromCentreSize(g: BoardGridItem, centre: P, size: P): BoardGridItem {
  return { ...g, x: centre[0], y: centre[1], extent: [Math.abs(Math.trunc(size[0] / 2)), Math.abs(Math.trunc(size[1] / 2))] };
}

/** The setters' ranges (`SetSpacing*`: at least 1; `SetPhi*Degrees`: at least 0.001 degree; `SetAssignedPriority`: 0 is 1; the orientation normalised). */
export function normalizedGridItem(g: BoardGridItem): BoardGridItem {
  return {
    ...g,
    extent: [Math.abs(g.extent[0]), Math.abs(g.extent[1])],
    spacing: [Math.max(1, g.spacing[0]), Math.max(1, g.spacing[1])],
    phi_extent: Math.max(0.001, Math.abs(g.phi_extent)),
    phi_spacing: Math.max(0.001, Math.abs(g.phi_spacing)),
    priority: g.priority > 0 ? Math.trunc(g.priority) : 1,
    tick_interval: Math.max(0, Math.trunc(g.tick_interval)),
    angle: normalizeDeg(g.angle),
  };
}

// ---------------------------------------------------------------------------------------------------------------------------- drawing

/** `AutoSparsePitch`: the pitch multiplied by the major-tick interval (or 2) until its lines are further apart than `threshold`. */
export function autoSparsePitch(pitch: number, tick: number, threshold: number): number {
  const mult = tick > 1 ? tick : 2;
  let p = pitch;
  while (p > 0 && p <= threshold) p *= mult;
  return p;
}

/** `GAL::GRID_LATTICE` for a bounded grid source: the pitch thinned out until its lines are `minSpacingPx` apart on screen (twice that for small crosses), the cells keeping their shape. */
export interface GridLattice {
  pitch: V;
  extent: V;
  tick: number;
}

/** `GAL::gridLattice` (bounded sources only: a grid item). `scale` is screen pixels per µm; `defaultTick` is `m_gridTick`. */
export function gridLattice(g: GridGeometry, tickInterval: number, scale: number, minSpacingPx: number, defaultTick: number, smallCross = false): GridLattice {
  const tick = tickInterval > 0 ? tickInterval : defaultTick;
  const threshold = (minSpacingPx / scale) * (smallCross ? 2 : 1);
  if (g.kind === "polar") {
    const px = autoSparsePitch(g.pitch[0], tick, threshold);
    const py = g.extent[0] > 0 ? autoSparsePitch(g.pitch[1], tick, threshold / g.extent[0]) : g.pitch[1];
    return { pitch: [px, py], extent: [g.extent[0], g.extent[1]], tick };
  }
  const minPitch = Math.min(g.pitch[0], g.pitch[1]);
  const sparse = autoSparsePitch(minPitch, tick, threshold) / minPitch;
  return { pitch: [g.pitch[0] * sparse, g.pitch[1] * sparse], extent: [g.extent[0], g.extent[1]], tick };
}

/** `GRID_LATTICE::IsMajor`. */
export function isMajor(lattice: GridLattice, index: number): boolean {
  return lattice.tick > 0 && index % lattice.tick === 0;
}

/** `GRID_DIM_ALPHA`, `GRID_EDGE_DARKEN`, `GRID_SELECTED_BRIGHTEN` (graphics_abstraction_layer.h). */
export const GRID_DIM_ALPHA = 0.4;
export const GRID_EDGE_DARKEN = 0.75;
export const GRID_SELECTED_BRIGHTEN = 0.5;

/**
 * A layer's colour for a selected item (`RENDER_SETTINGS::update`'s `m_layerColorsSel`, `m_selectFactor` 0.5): kept for a colour close to black, else brightened by
 * `0.25 + brightness^3` (at most 1), and when that hardly changes the brightness, darkened by 0.2 with the blue raised instead. `#rrggbb[aa]` in and out, the alpha kept.
 */
export function selectedLayerColor(hex: string): string {
  const h = hex.replace("#", "");
  const ch = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255) as [number, number, number];
  const alpha = h.length >= 8 ? h.slice(6, 8) : "";
  const brightness = (c: readonly number[]) => c[0]! * 0.299 + c[1]! * 0.587 + c[2]! * 0.117;
  const out = (c: readonly number[]) => "#" + c.map((v) => Math.round(Math.max(0, Math.min(1, v)) * 255).toString(16).padStart(2, "0")).join("") + alpha;
  if (brightness(ch) < 0.05) return out(ch);
  const factor = Math.min(1, 0.5 * 0.5 + Math.pow(brightness(ch), 3));
  let sel = ch.map((v) => v * (1 - factor) + factor);
  if (Math.abs(brightness(sel) - brightness(ch)) < 0.05) {
    sel = ch.map((v) => v * (1 - 0.5 * 0.4));
    sel[2] = ch[2] * (1 - factor) + factor;
  }
  return out(sel);
}

/** The grid items in the order the painter draws them (`GAL::SetGridSources`' stable sort, winners first) -- drawn back to front, so the winner is on top. */
export function gridsInPrecedence(grids: readonly BoardGridItem[]): BoardGridItem[] {
  return grids
    .map((g, i) => ({ g, i, geom: gridGeometry(g) }))
    .sort((a, b) => (takesPrecedenceOver(a.geom, b.geom) ? -1 : takesPrecedenceOver(b.geom, a.geom) ? 1 : a.i - b.i))
    .map((x) => x.g);
}

// KiCad 10's via stitching: a generator that fills an outline with vias of one net, or guards another net's tracks with a row of them (file version 20260816).
//
// Ported from KiCad 25ec2335 (the commit the box's kicad-cli nightly is built from; the 8303b2ad source the rest of the port reads predates via stitching):
//   pcbnew/generators/pcb_via_stitch.cpp   ViewDraw (the outline and its inward ticks), HitTest (`m_outline.Collide`), cellForPosition / positionForCell, ExcludePosition,
//                                          the property rows (PCB_VIA_STITCH_DESC), defaultPitch
//   pcbnew/dialogs/dialog_via_stitch_properties.cpp   buildStitchPreview (the dialog's sample of the pattern)
//   libs/kimath/src/geometry/poisson_disk.cpp          POISSON_DISK::ToroidalUnitTile, with Boost's mt19937 and uniform distributions it draws from
// Where the vias go on the board is worked out by the server (crates/drc/src/via_stitch.rs, `PCB_VIA_STITCH::Update`), from the zone fills.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardViaStitch } from "../api/pcbItemTypes";
import type { Cmd, PointXY } from "../api/types";

type V = [number, number];

/** `PCB_VIA_STITCH_LAYOUT` and `PCB_VIA_STITCH_MODE` as the model names them. */
export type StitchLayout = "plain" | "staggered" | "poisson";
export type StitchMode = "stitch" | "guard";

/** The choices of the dialog's Pattern and Mode controls (dialog_via_stitch_properties_base.cpp), and of the Properties panel's Layout and Mode rows (`ENUM_MAP`). */
export const PATTERN_CHOICES: { label: string; value: StitchLayout }[] = [
  { label: "Normal Grid", value: "plain" },
  { label: "Staggered Grid", value: "staggered" },
  { label: "Poisson", value: "poisson" },
];
export const LAYOUT_PROPERTY_CHOICES: { label: string; value: StitchLayout }[] = [
  { label: "Plain grid", value: "plain" },
  { label: "Staggered grid", value: "staggered" },
  { label: "Poisson disk", value: "poisson" },
];
export const MODE_CHOICES: { label: string; value: StitchMode }[] = [
  { label: "Stitch", value: "stitch" },
  { label: "Guard", value: "guard" },
];

/** `pcbIUScale.mmToIU( 0.1 )`, `mmToIU( 2.0 )`, `mmToIU( 0.5 )`: the outline's width, the tick spacing and the tick length of `ViewDraw`, µm. */
export const OUTLINE_WIDTH_NM = 100_000;
export const TICK_SPACING_NM = 2_000_000;
export const TICK_LENGTH_NM = 500_000;

export const layoutOf = (v: BoardViaStitch): StitchLayout => v.layout ?? "plain";
export const modeOf = (v: BoardViaStitch): StitchMode => v.mode ?? "stitch";
export const pitchOf = (v: BoardViaStitch): number => v.pitch_nm ?? 0;

/** `usesGridCells`. */
export function usesGridCells(v: BoardViaStitch): boolean {
  return layoutOf(v) !== "poisson" && modeOf(v) !== "guard";
}

/** `defaultPitch`: twice the via, and at least 2 mm. */
export function defaultPitchNm(viaSizeNm: number): number {
  return Math.max(2_000_000, viaSizeNm * 2);
}

/** `cellForPosition`: the grid cell of a position (µm; worked in nanometres, as KiCad does), or the position where the layout has no grid. */
export function cellForPosition(v: BoardViaStitch, at: readonly [number, number]): V {
  if (!usesGridCells(v)) return [at[0], at[1]];
  const pitch = Math.max(1, pitchOf(v) || defaultPitchNm(v.via?.size_nm ?? 600_000));
  const roundDiv = (a: number, b: number) => (a >= 0 ? Math.trunc((a + Math.trunc(b / 2)) / b) : -Math.trunc((-a + Math.trunc(b / 2)) / b));
  const ox = v.origin_offset?.x ?? 0;
  const oy = v.origin_offset?.y ?? 0;
  const row = roundDiv(at[1] - oy, pitch);
  const shift = layoutOf(v) === "staggered" && ((row % 2) + 2) % 2 !== 0 ? Math.trunc(pitch / 2) : 0;
  return [roundDiv(at[0] - ox - shift, pitch), row];
}

/** `ExcludePosition`: the stitching with the via at `at` taken out -- its cell on a grid, its position otherwise. */
export function excludedAt(v: BoardViaStitch, at: readonly [number, number]): BoardViaStitch {
  if (usesGridCells(v)) {
    const c = cellForPosition(v, at);
    const cells = v.excluded_cells ?? [];
    return cells.some(([x, y]) => x === c[0] && y === c[1]) ? v : { ...v, excluded_cells: [...cells, c] };
  }
  const pos = v.excluded_positions ?? [];
  return pos.some((p) => p.x === at[0] && p.y === at[1]) ? v : { ...v, excluded_positions: [...pos, { x: at[0], y: at[1] }] };
}

/** The command that gives the stitching `v`'s settings and makes its vias again (the server places them from the zone fills): `set_via_stitch` without `vias`. */
export function viaStitchCmd(id: string | null, v: BoardViaStitch): Cmd {
  const { id: _drop, ...stitch } = v;
  void _drop;
  return { op: "pcb_item", verb: "set_via_stitch", ...(id ? { id } : {}), stitch };
}

/** `VIA_STITCH_TOOL::ExcludeStitchVia`: the stitchings the selected vias belong to, each with those vias taken out. */
export function stitchesExcluding(stitches: readonly BoardViaStitch[], vias: readonly { id: string; x: number; y: number }[]): BoardViaStitch[] {
  const out: BoardViaStitch[] = [];
  for (const s of stitches) {
    let next = s;
    for (const v of vias) if (s.member_ids?.includes(v.id)) next = excludedAt(next, [v.x, v.y]);
    if (next !== s) out.push(next);
  }
  return out;
}

/** `HasExclusions`. */
export function hasExclusions(v: BoardViaStitch): boolean {
  return (v.excluded_cells?.length ?? 0) > 0 || (v.excluded_positions?.length ?? 0) > 0;
}

/** `ClearAllExclusions`. */
export function withoutExclusions(v: BoardViaStitch): BoardViaStitch {
  return { ...v, excluded_cells: [], excluded_positions: [] };
}

/** `SetLayout` / `SetMode`: a change of either drops the exclusions (`ClearAllExclusions`), which are keyed by what the old one placed. */
export function withLayoutAndMode(v: BoardViaStitch, layout: StitchLayout, mode: StitchMode): BoardViaStitch {
  const changed = layout !== layoutOf(v) || mode !== modeOf(v);
  return { ...(changed ? withoutExclusions(v) : v), layout, mode };
}

/** `GetPosition`: the outline's first corner. */
export function stitchPosition(v: BoardViaStitch): V {
  const p = v.outline[0];
  return p ? [p.x, p.y] : [v.origin?.x ?? 0, v.origin?.y ?? 0];
}

/** `Move`: the outline and the origin translated. */
export function movedStitch(v: BoardViaStitch, dx: number, dy: number): BoardViaStitch {
  const t = (p: PointXY): PointXY => ({ x: p.x + dx, y: p.y + dy });
  return { ...v, outline: v.outline.map(t), origin: t(v.origin ?? { x: 0, y: 0 }) };
}

// ---------------------------------------------------------------------------------------------------------------------------- drawing and picking

/** `ViewDraw`'s ticks: every 2 mm along each edge (from 2 mm in), 0.5 mm long, pointing towards the corners' centroid. */
export function stitchTicks(outline: readonly PointXY[]): [V, V][] {
  const n = outline.length;
  if (n === 0) return [];
  let cx = 0;
  let cy = 0;
  for (const p of outline) {
    cx += p.x;
    cy += p.y;
  }
  cx /= n;
  cy /= n;
  const out: [V, V][] = [];
  for (let i = 0; i < n; i++) {
    const p1 = outline[i]!;
    const p2 = outline[(i + 1) % n]!;
    const ex = p2.x - p1.x;
    const ey = p2.y - p1.y;
    const len = Math.hypot(ex, ey);
    if (len < 1) continue;
    const dx = ex / len;
    const dy = ey / len;
    const mx = (p1.x + p2.x) / 2;
    const my = (p1.y + p2.y) / 2;
    const perp: V = [-dy, dx];
    const toward = perp[0] * (cx - mx) + perp[1] * (cy - my) >= 0;
    const inward: V = toward ? perp : [-perp[0], -perp[1]];
    for (let t = TICK_SPACING_NM; t < len; t += TICK_SPACING_NM) {
      const base: V = [p1.x + dx * t, p1.y + dy * t];
      out.push([base, [base[0] + inward[0] * TICK_LENGTH_NM, base[1] + inward[1] * TICK_LENGTH_NM]]);
    }
  }
  return out;
}

function pointInRing(x: number, y: number, ring: readonly PointXY[]): boolean {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const a = ring[i]!;
    const b = ring[j]!;
    if (a.y > y !== b.y > y && x < ((b.x - a.x) * (y - a.y)) / (b.y - a.y) + a.x) inside = !inside;
  }
  return inside;
}

/** `HitTest`: `m_outline.Collide( aPosition, aAccuracy )` -- inside the outline is a hit; outside, how far from it. */
export function stitchHitDistance(v: BoardViaStitch, x: number, y: number): number {
  const ring = v.outline;
  if (ring.length < 3) return Infinity;
  if (pointInRing(x, y, ring)) return 0;
  let best = Infinity;
  for (let i = 0; i < ring.length; i++) {
    const a = ring[i]!;
    const b = ring[(i + 1) % ring.length]!;
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const len2 = dx * dx + dy * dy;
    const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((x - a.x) * dx + (y - a.y) * dy) / len2));
    best = Math.min(best, Math.hypot(x - (a.x + t * dx), y - (a.y + t * dy)));
  }
  return best;
}

/** `GetBoundingBox`: the outline's box. */
export function stitchBounds(v: BoardViaStitch): [number, number, number, number] {
  let x0 = Infinity,
    y0 = Infinity,
    x1 = -Infinity,
    y1 = -Infinity;
  for (const p of v.outline) {
    x0 = Math.min(x0, p.x);
    y0 = Math.min(y0, p.y);
    x1 = Math.max(x1, p.x);
    y1 = Math.max(y1, p.y);
  }
  return [x0, y0, x1, y1];
}

/** The shoelace area of the outline: what the selection weighs it by among the items under the cursor. */
export function stitchArea(v: BoardViaStitch): number {
  let s = 0;
  const r = v.outline;
  for (let i = 0; i < r.length; i++) {
    const a = r[i]!;
    const b = r[(i + 1) % r.length]!;
    s += a.x * b.y - b.x * a.y;
  }
  return Math.abs(s) / 2;
}

/** `GetItemDescription` / `GetFriendlyName`: "Via Stitching". */
export const VIA_STITCH_LABEL = "Via Stitching";

// ---------------------------------------------------------------------------------------------------------------------------- the Poisson tile and the preview

/** `boost::random::mt19937`. */
export class Mt19937 {
  private mt = new Uint32Array(624);
  private index = 624;
  constructor(seed: number) {
    this.mt[0] = seed >>> 0;
    for (let i = 1; i < 624; i++) {
      const prev = this.mt[i - 1]! ^ (this.mt[i - 1]! >>> 30);
      this.mt[i] = (Math.imul(1812433253, prev) + i) >>> 0;
    }
  }
  private twist(): void {
    for (let i = 0; i < 624; i++) {
      const y = (this.mt[i]! & 0x80000000) | (this.mt[(i + 1) % 624]! & 0x7fffffff);
      let v = this.mt[(i + 397) % 624]! ^ (y >>> 1);
      if (y & 1) v ^= 0x9908b0df;
      this.mt[i] = v >>> 0;
    }
    this.index = 0;
  }
  next(): number {
    if (this.index >= 624) this.twist();
    let y = this.mt[this.index++]!;
    y ^= y >>> 11;
    y ^= (y << 7) & 0x9d2c5680;
    y ^= (y << 15) & 0xefc60000;
    y ^= y >>> 18;
    return y >>> 0;
  }
  /** `uniform_real_distribution<double>( 0, 1 )`. */
  uniform01(): number {
    for (;;) {
      const r = this.next() / 4294967296;
      if (r < 1) return r;
    }
  }
  /** `uniform_int_distribution<size_t>( 0, max )`. */
  uniformIndex(max: number): number {
    if (max === 0) return 0;
    const brange = 0xffffffff;
    let bucket = Math.floor(brange / (max + 1));
    if (brange % (max + 1) === max) bucket++;
    for (;;) {
      const r = Math.floor(this.next() / bucket);
      if (r <= max) return r;
    }
  }
}

/** `POISSON_DISK::ToroidalUnitTile`. */
export function toroidalUnitTile(minDist: number, seed: number): V[] {
  const samples: V[] = [];
  if (minDist <= 0 || minDist > 0.5) return samples;
  const gridN = Math.max(3, Math.ceil(Math.SQRT2 / minDist));
  const cellSize = 1 / gridN;
  const grid = new Array<number>(gridN * gridN).fill(-1);
  const active: number[] = [];
  const cellOf = (p: V): V => {
    const gx = Math.floor(p[0] / cellSize);
    const gy = Math.floor(p[1] / cellSize);
    return [((gx % gridN) + gridN) % gridN, ((gy % gridN) + gridN) % gridN];
  };
  const torDistSq = (dx: number, dy: number): number => {
    if (dx > 0.5) dx -= 1;
    if (dx < -0.5) dx += 1;
    if (dy > 0.5) dy -= 1;
    if (dy < -0.5) dy += 1;
    return dx * dx + dy * dy;
  };
  const rng = new Mt19937(seed);
  const x = rng.uniform01();
  const y = rng.uniform01();
  const p: V = [x, y];
  const [gx, gy] = cellOf(p);
  samples.push(p);
  grid[gy * gridN + gx] = 0;
  active.push(0);
  while (active.length > 0) {
    const idx = rng.uniformIndex(active.length - 1);
    const base = samples[active[idx]!]!;
    let accepted = false;
    for (let k = 0; k < 30; k++) {
      const theta = rng.uniform01() * 2 * Math.PI;
      const rho = minDist + rng.uniform01() * minDist;
      const cand: V = [base[0] + rho * Math.cos(theta), base[1] + rho * Math.sin(theta)];
      cand[0] -= Math.floor(cand[0]);
      cand[1] -= Math.floor(cand[1]);
      const [cx, cy] = cellOf(cand);
      let tooClose = false;
      for (let dy = -2; dy <= 2 && !tooClose; dy++) {
        const ny = (((cy + dy) % gridN) + gridN) % gridN;
        for (let dx = -2; dx <= 2 && !tooClose; dx++) {
          const nx = (((cx + dx) % gridN) + gridN) % gridN;
          const n = grid[ny * gridN + nx]!;
          if (n < 0) continue;
          const s = samples[n]!;
          if (torDistSq(s[0] - cand[0], s[1] - cand[1]) < minDist * minDist) tooClose = true;
        }
      }
      if (tooClose) continue;
      samples.push(cand);
      grid[cy * gridN + cx] = samples.length - 1;
      active.push(samples.length - 1);
      accepted = true;
      break;
    }
    if (!accepted) {
      active[idx] = active[active.length - 1]!;
      active.pop();
    }
  }
  return samples;
}

/** `POISSON_TILE_PITCHES`, `POISSON_TILE_SEED`. */
export const POISSON_TILE_PITCHES = 8;
export const POISSON_TILE_SEED = 0x542c21fc;

let baked: V[] | null = null;
/** `bakedPoissonTile`. */
export function bakedPoissonTile(): V[] {
  baked ??= toroidalUnitTile(1 / POISSON_TILE_PITCHES, POISSON_TILE_SEED);
  return baked;
}

/** `PREVIEW_TILE_PITCHES`. */
export const PREVIEW_TILE_PITCHES = 6;

/** `DIALOG_VIA_STITCH_PROPERTIES::buildStitchPreview`: the sample of the pattern the dialog draws, and its extent (µm). */
export function stitchPreview(pitch: number, layout: StitchLayout, seed: number): { samples: V[]; extent: number } {
  const samples: V[] = [];
  if (pitch <= 0) return { samples, extent: 0 };
  if (layout === "poisson") {
    const tile = bakedPoissonTile();
    const tileSize = pitch * POISSON_TILE_PITCHES;
    const extent = tileSize * 2;
    const rng = new Mt19937(seed >>> 0);
    const offX = rng.uniform01() * tileSize;
    const offY = rng.uniform01() * tileSize;
    const firstX = Math.floor((0 - offX) / tileSize);
    const firstY = Math.floor((0 - offY) / tileSize);
    const lastX = Math.floor((extent - offX) / tileSize);
    const lastY = Math.floor((extent - offY) / tileSize);
    for (let ty = firstY; ty <= lastY; ty++) {
      for (let tx = firstX; tx <= lastX; tx++) {
        for (const s of tile) {
          const pt: V = [Math.round(offX + (tx + s[0]) * tileSize), Math.round(offY + (ty + s[1]) * tileSize)];
          if (pt[0] < 0 || pt[0] > extent || pt[1] < 0 || pt[1] > extent) continue;
          samples.push(pt);
        }
      }
    }
    return { samples, extent };
  }
  for (let row = 0; row < PREVIEW_TILE_PITCHES; row++) {
    const shift = layout === "staggered" && row % 2 ? Math.trunc(pitch / 2) : 0;
    for (let col = 0; col < PREVIEW_TILE_PITCHES; col++) samples.push([col * pitch + shift, row * pitch]);
  }
  return { samples, extent: pitch * PREVIEW_TILE_PITCHES };
}

/**
 * `PCB_SELECTION_TOOL::itemPassesFilter`: a generator with members is filtered as its first member (`if( itemType == PCB_GENERATOR_T ) { if( GetItems().empty() ) ..
 * m_filter.otherItems .. else itemType = ( *GetItems().begin() )->Type(); }`): a via stitching that holds vias is gated by the Vias row, an empty one by Other items.
 */
export function stitchFilteredAsVia(board: { drawings?: { via_stitches?: { id: string; member_ids?: string[] }[] } | null } | null | undefined, id: string): boolean {
  return (board?.drawings?.via_stitches?.find((v) => v.id === id)?.member_ids ?? []).length > 0;
}

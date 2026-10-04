// Port of `JUNCTION_HELPERS::AnalyzePoint( items, pos, aBreakCrossings = true )`
// (eeschema/junction_helpers.cpp) as `SCH_SCREEN::IsExplicitJunctionAllowed` uses it -- the gate of the
// Place Junction tool (`eeschema.InteractiveDrawing.placeJunction`, J): "Junction location contains
// no joinable wires and/or pins." when it fails.
//
// A point is a junction ("of some sort") when three or more distinct directions leave it on the wire
// layer, or on the bus layer. A wire/bus segment that *ends* at the point adds the direction of its
// other end; one that merely *passes through* adds both of its directions once the point is known to
// break lines (an end, a pin, a label or a bus entry there, or -- `aBreakCrossings` -- always, which
// is what makes two crossing wires a 4-way junction). A symbol pin, a bus entry or a sheet at the point
// adds a direction of its own that matches nothing else (`uniqueAngle++`); a label only marks the point
// as breaking. An explicit bus entry there is only allowed when it feeds more than one wire.
//
// This studio's wires are polylines, so every segment is taken as its own two-point `SCH_LINE` (a
// vertex is then the end of two lines -- the same two directions KiCad would see). Bus labels are not
// told apart from wire labels (a label only ever sets "breaks", which `aBreakCrossings` already did).

export type Pt = readonly [number, number];

export interface JunctionSchematic {
  /** Wire and bus polylines. */
  wires: ReadonlyArray<{ bus?: boolean; pts: readonly Pt[] }>;
  /** Symbol pin connection points and power symbols' anchors -- each a "symbol connected here". */
  pinTips: readonly Pt[];
  /** Local/global/hierarchical label anchors. */
  labels: readonly Pt[];
  /** Bus-to-wire entries as their two endpoints. */
  busEntries: ReadonlyArray<{ a: Pt; b: Pt }>;
}

export interface PointInfo {
  hasBusEntry: boolean;
  hasBusEntryToMultipleWires: boolean;
  hasBusAtPoint: boolean;
  isJunction: boolean;
}

const same = (a: Pt, b: Pt) => a[0] === b[0] && a[1] === b[1];

/** `SCH_LINE::GetAngleFrom`: the (rounded, whole-degree) direction towards the other end. */
function angleTo(from: Pt, to: Pt): number {
  return Math.round((Math.atan2(to[1] - from[1], to[0] - from[0]) * 180) / Math.PI);
}

/** `p` strictly between `a` and `b` on the segment. */
function onSegmentInterior(p: Pt, a: Pt, b: Pt): boolean {
  const cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
  if (cross !== 0) return false;
  const dot = (p[0] - a[0]) * (b[0] - a[0]) + (p[1] - a[1]) * (b[1] - a[1]);
  const len2 = (b[0] - a[0]) ** 2 + (b[1] - a[1]) ** 2;
  return dot > 0 && dot < len2;
}

export function analyzePoint(sch: JunctionSchematic, p: Pt): PointInfo {
  const WIRES = 0 as const;
  const BUSES = 1 as const;
  // `aBreakCrossings`: every through-line breaks, so the per-layer `breakLines` flags never gate anything here.
  const exitAngles: [Set<number>, Set<number>] = [new Set(), new Set()];
  const midPoint: [Array<[Pt, Pt]>, Array<[Pt, Pt]>] = [[], []];
  let uniqueAngle = 10000;
  const info: PointInfo = { hasBusEntry: false, hasBusEntryToMultipleWires: false, hasBusAtPoint: false, isJunction: false };

  for (const w of sch.wires) {
    const layer = w.bus ? BUSES : WIRES;
    for (let i = 0; i + 1 < w.pts.length; i++) {
      const a = w.pts[i]!;
      const b = w.pts[i + 1]!;
      if (same(a, b)) continue; // `GetStartPoint() == GetEndPoint()`
      if (same(a, p)) exitAngles[layer].add(angleTo(p, b));
      else if (same(b, p)) exitAngles[layer].add(angleTo(p, a));
      else if (onSegmentInterior(p, a, b)) midPoint[layer].push([a, b]);
      if (layer === BUSES && (same(a, p) || same(b, p) || onSegmentInterior(p, a, b))) info.hasBusAtPoint = true;
    }
  }
  for (const e of sch.busEntries) {
    if (same(e.a, p) || same(e.b, p)) {
      exitAngles[BUSES].add(uniqueAngle++);
      exitAngles[WIRES].add(uniqueAngle++);
      info.hasBusEntry = true;
    }
  }
  for (const tip of sch.pinTips) if (same(tip, p)) exitAngles[WIRES].add(uniqueAngle++);
  // labels: only a "this point breaks lines" mark -- already true above

  for (const layer of [WIRES, BUSES] as const) {
    for (const [a, b] of midPoint[layer]) {
      exitAngles[layer].add(angleTo(p, b));
      exitAngles[layer].add(angleTo(p, a));
    }
  }
  if (info.hasBusEntry) info.hasBusEntryToMultipleWires = exitAngles[WIRES].size > 2 && exitAngles[BUSES].size === 1;
  info.isJunction = exitAngles[WIRES].size >= 3 || exitAngles[BUSES].size >= 3;
  return info;
}

/** `SCH_SCREEN::IsExplicitJunctionAllowed`. */
export function isExplicitJunctionAllowed(sch: JunctionSchematic, p: Pt): boolean {
  const info = analyzePoint(sch, p);
  return info.isJunction && (!info.hasBusEntry || info.hasBusEntryToMultipleWires);
}

/**
 * Where a junction click can snap to (`grid.BestSnapAnchor` for a junction's item grid): every wire vertex, every pin, and
 * every point where two wire segments cross in their interiors -- the one place a junction is needed without a vertex to
 * click on. A point appears once.
 */
export function junctionCandidates(sch: JunctionSchematic): Pt[] {
  const seen = new Set<string>();
  const out: Pt[] = [];
  const add = (p: Pt) => {
    const key = `${p[0]},${p[1]}`;
    if (!seen.has(key)) {
      seen.add(key);
      out.push(p);
    }
  };
  const segs: Array<[Pt, Pt]> = [];
  for (const w of sch.wires) {
    for (let i = 0; i < w.pts.length; i++) add(w.pts[i]!);
    for (let i = 0; i + 1 < w.pts.length; i++) segs.push([w.pts[i]!, w.pts[i + 1]!]);
  }
  for (const t of sch.pinTips) add(t);
  for (let i = 0; i < segs.length; i++) {
    for (let j = i + 1; j < segs.length; j++) {
      const x = crossingPoint(segs[i]![0], segs[i]![1], segs[j]![0], segs[j]![1]);
      if (x) add(x);
    }
  }
  return out;
}

/** Where two segments cross strictly inside both (null for parallel segments, a touch at an end, or a miss), rounded to a whole um. */
function crossingPoint(a: Pt, b: Pt, c: Pt, d: Pt): Pt | null {
  const r = [b[0] - a[0], b[1] - a[1]] as const;
  const s = [d[0] - c[0], d[1] - c[1]] as const;
  const denom = r[0] * s[1] - r[1] * s[0];
  if (denom === 0) return null;
  const t = ((c[0] - a[0]) * s[1] - (c[1] - a[1]) * s[0]) / denom;
  const u = ((c[0] - a[0]) * r[1] - (c[1] - a[1]) * r[0]) / denom;
  if (t <= 0 || t >= 1 || u <= 0 || u >= 1) return null;
  return [Math.round(a[0] + t * r[0]), Math.round(a[1] + t * r[1])];
}

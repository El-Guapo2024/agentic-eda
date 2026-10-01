// Where to draw a junction dot -- sch_painter.cpp draws one wherever
// `SCH_SCREEN::IsJunctionNeeded` says a real electrical connection point
// has 3+ items meeting at it, or an item lands on another wire's interior
// (a T). This app's `Wire.pts` is one polyline per wire (not KiCad's many
// atomic 2-point SCH_LINEs), so the two cases collapse into one geometric
// test here:
//   (a) 3+ wire *endpoints*, same net, exactly coincident -- the original
//       rule `painter.ts`'s own `junctionPoints` already had.
//   (b) a wire's endpoint, a power symbol's anchor, or a label's anchor
//       landing on the *interior* of another wire's segment (a T) --
//       found missing this session: the model side already treats every
//       one of these as a real connection (`eda_kicad::sch_import::
//       reconcile`'s own "T-junctions" step unions *any* seeded point --
//       wire point, pin, label, power symbol -- that lands on a wire's
//       interior; `erc.rs`'s `point_on_segment_interior_any` likewise
//       already keeps such a point from reporting dangling), but nothing
//       drew the dot a user expects at a point the model already wired
//       up -- a correctness gap between "what's connected" and "what's
//       drawn", not just a missing decoration (eeschema draws this dot
//       precisely so a true electrical T reads differently on screen
//       from two wires that merely cross without connecting, which this
//       app drew identically, dot or no dot, before this fix).
// `pointOnSegmentInterior` below is `crates/kicad/src/sch_import.rs`'s
// own function of the same name, ported arithmetic-for-arithmetic
// (axis-aligned segments only -- every wire segment in this app's model
// is strictly horizontal or vertical, matching eeschema's own strong
// orthogonal-wire convention) so a dot is drawn at points the backend's
// own union-find already considers joined.
//
// Deliberately narrower than the backend's full generality, in two ways,
// both judged rare enough in practice not to be worth the extra
// plumbing: a *pin* landing on a wire's interior with no wire endpoint
// there (unusual -- a user normally ends the wire at the pin instead, the
// case (a)/(b)-wire-endpoint path above already covers) is not checked
// here (would need resolved pin positions threaded into what is
// otherwise a cheap, geometry-only module); and one wire's own *interior
// bend* landing on another wire's interior (as opposed to its endpoint)
// is not checked either. A no-connect flag's own anchor is deliberately
// never treated as a junction candidate even though it is still a seeded
// point backend-side -- drawing a connection dot exactly where an X "no
// connection" marker sits would contradict the marker, not corroborate
// it.
export interface JunctionWire {
  net: string;
  pts: readonly (readonly [number, number])[];
}

function pointOnSegmentInterior(p: readonly [number, number], a: readonly [number, number], b: readonly [number, number]): boolean {
  if (a[0] === b[0]) {
    return p[0] === a[0] && p[1] > Math.min(a[1], b[1]) && p[1] < Math.max(a[1], b[1]);
  } else if (a[1] === b[1]) {
    return p[1] === a[1] && p[0] > Math.min(a[0], b[0]) && p[0] < Math.max(a[0], b[0]);
  }
  return false;
}

/** `extraAnchors`: power-symbol and label anchor points (net-agnostic for this test, same as the backend's own T-junction union -- landing on the interior is what *merges* nets, so requiring them to already match would make the rule unable to ever fire). */
export function junctionPoints(wires: readonly JunctionWire[], extraAnchors: readonly (readonly [number, number])[] = []): Array<[number, number]> {
  // (a) 3+ coincident wire endpoints, same net.
  const counts = new Map<string, { at: [number, number]; n: number }>();
  for (const w of wires) {
    if (w.pts.length === 0) continue;
    for (const p of [w.pts[0]!, w.pts[w.pts.length - 1]!]) {
      const key = `${w.net}|${p[0]},${p[1]}`;
      const e = counts.get(key);
      if (e) e.n++;
      else counts.set(key, { at: [p[0], p[1]], n: 1 });
    }
  }
  const dots: Array<[number, number]> = [...counts.values()].filter((e) => e.n >= 3).map((e) => e.at);
  const seen = new Set(dots.map(([x, y]) => `${x},${y}`));
  const addIfNew = (p: readonly [number, number]) => {
    const key = `${p[0]},${p[1]}`;
    if (!seen.has(key)) {
      seen.add(key);
      dots.push([p[0], p[1]]);
    }
  };

  // (b) T-junctions: every wire endpoint plus every extra anchor, tested
  // against every wire's every segment.
  const candidates: Array<[number, number]> = extraAnchors.map((p) => [p[0], p[1]] as [number, number]);
  for (const w of wires) {
    if (w.pts.length > 0) candidates.push(w.pts[0] as [number, number], w.pts[w.pts.length - 1] as [number, number]);
  }
  for (const other of wires) {
    for (let i = 0; i + 1 < other.pts.length; i++) {
      const a = other.pts[i]!;
      const b = other.pts[i + 1]!;
      for (const end of candidates) {
        if (pointOnSegmentInterior(end, a, b)) addIfNew(end);
      }
    }
  }
  return dots;
}

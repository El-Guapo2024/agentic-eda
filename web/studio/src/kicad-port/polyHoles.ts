// A board polygon's holes (UI-LEFT-2 PP10): `PCB_SHAPE::GetPolyShape()` is a `SHAPE_POLY_SET`, so a zone with holes made a polygon keeps them (`CONVERT_TOOL::CreatePolys`:
// `graphic->SetPolyShape( poly )`). The board sends them beside the outline (`Shape.holes`); a command that sends the polygon back (`add_shape`, `replace_shape`) gives
// them bridged into one ring, which the backend takes apart again (crates/ops/src/poly_holes.rs, `unfractureSingle`) -- so a copy, a paste or an edit of the outline
// keeps them.
type P = readonly [number, number];

/**
 * The outline with each hole bridged in: from the outline corner nearest the hole's first corner, round the hole and back the same way (two coincident sides
 * running opposite ways, the pair `unfractureSingle` takes apart). The outline as it is when there are no holes.
 */
export function bridgeHoles(outline: readonly P[], holes: readonly (readonly P[])[] | undefined): [number, number][] {
  const out: [number, number][] = outline.map((p) => [p[0], p[1]]);
  for (const hole of holes ?? []) {
    if (hole.length < 3 || out.length < 3) continue;
    // the nearest pair: an outline corner and a hole corner
    let best = { i: 0, j: 0, d: Infinity };
    out.forEach((o, i) =>
      hole.forEach((h, j) => {
        const d = (o[0] - h[0]) ** 2 + (o[1] - h[1]) ** 2;
        if (d < best.d) best = { i, j, d };
      })
    );
    const loop: [number, number][] = [];
    for (let k = 0; k <= hole.length; k++) {
      const h = hole[(best.j + k) % hole.length]!;
      loop.push([h[0], h[1]]);
    }
    const at = out[best.i]!;
    out.splice(best.i + 1, 0, ...loop, [at[0], at[1]]);
  }
  return out;
}

/** Even-odd: inside the outline and in none of the holes. */
export function insidePolygonWithHoles(x: number, y: number, outline: readonly P[], holes: readonly (readonly P[])[] | undefined): boolean {
  let inside = false;
  for (const r of [outline, ...(holes ?? [])]) {
    for (let i = 0, j = r.length - 1; i < r.length; j = i++) {
      const [xi, yi] = r[i]!;
      const [xj, yj] = r[j]!;
      if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
    }
  }
  return inside;
}

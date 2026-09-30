// The ratsnest: a minimum spanning tree over each net's pad centers,
// shown when there's no routing yet. Ported from the old studio.html's
// `mst()` -- KiCad's real ratsnest is smarter (it prefers unrouted-only
// edges and updates incrementally rather than a full MST each frame),
// but an MST is the same "shortest total airwire" idea and is cheap
// enough to recompute every poll for boards this size.

export function minimumSpanningTree(points: Array<[number, number]>): Array<[[number, number], [number, number]]> {
  const n = points.length;
  const edges: Array<[[number, number], [number, number]]> = [];
  if (n === 0) return edges;
  const inTree = new Array<boolean>(n).fill(false);
  const best = new Array<number>(n).fill(Infinity);
  const from = new Array<number>(n).fill(-1);
  best[0] = 0;
  for (let k = 0; k < n; k++) {
    let u = -1;
    // `best`/`from` are indexed by plain loop counters, always 0..n -- safe
    // to assert non-null rather than thread `| undefined` through the
    // classic MST algorithm's arithmetic.
    for (let i = 0; i < n; i++) if (!inTree[i] && (u < 0 || best[i]! < best[u]!)) u = i;
    if (u < 0) break;
    inTree[u] = true;
    if (from[u]! >= 0) edges.push([points[from[u]!]!, points[u]!]);
    for (let v = 0; v < n; v++) {
      if (inTree[v]) continue;
      const [ux, uy] = points[u]!;
      const [vx, vy] = points[v]!;
      const d = Math.hypot(ux - vx, uy - vy);
      if (d < best[v]!) {
        best[v] = d;
        from[v] = u;
      }
    }
  }
  return edges;
}

/** Every net's pad centers, from placed parts only. */
export function padPointsByNet(parts: Array<{ placed: boolean; pads?: Array<{ net: string | null; x: number; y: number }> }>): Map<string, Array<[number, number]>> {
  const byNet = new Map<string, Array<[number, number]>>();
  for (const p of parts) {
    if (!p.placed) continue;
    for (const pad of p.pads ?? []) {
      if (!pad.net) continue;
      const list = byNet.get(pad.net) ?? [];
      list.push([pad.x, pad.y]);
      byNet.set(pad.net, list);
    }
  }
  return byNet;
}

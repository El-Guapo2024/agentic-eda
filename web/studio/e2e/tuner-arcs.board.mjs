// Builds the scratch board of tuner-arcs.check.js: adds to a board's design.json a differential pair that goes down and back up round four corners, each a pair
// of concentric quarter arcs (P at r 2 mm; N inside at 1 mm, then outside at 3 mm, so both nets are 17 mm + 4 pi mm long), every arc a track of its own
// (`PCB_ARC`, `Track::new_arc`: its 32-chord tessellation and the offset of its middle). The board's history is cleared, so the edit is not an undo step.
//
//   node web/studio/e2e/tuner-arcs.board.mjs <board dir>
import { readFileSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2];
if (!dir) throw new Error("usage: tuner-arcs.board.mjs <board dir>");
const file = join(dir, "design.json");
const d = JSON.parse(readFileSync(file, "utf8"));
const P = "/LED channels (D1–D8)/USB_DP", N = "/LED channels (D1–D8)/USB_DN";
const mm = (v) => Math.round(v * 1e6);
const kiround = (v) => (v < 0 ? Math.ceil(v - 0.5) : Math.floor(v + 0.5));
const pt = (x, y) => ({ x: mm(x), y: mm(y) });

// `eda_model::ir::tessellate_arc( start, mid, end, TRACK_ARC_SEGMENTS )`, line for line.
function tessellate(start, mid, end, n = 32) {
  const [sx, sy, mx, my, ex, ey] = [start.x, start.y, mid.x, mid.y, end.x, end.y];
  const dd = 2 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
  const ux = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / dd;
  const uy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / dd;
  const r = Math.hypot(sx - ux, sy - uy);
  const tau = 2 * Math.PI;
  const norm = (a) => ((a % tau) + tau) % tau;
  const ang = (x, y) => Math.atan2(y - uy, x - ux);
  const [a0, a1, a2] = [ang(sx, sy), ang(mx, my), ang(ex, ey)];
  let sweep = norm(a2 - a0);
  if (norm(a1 - a0) > sweep) sweep -= tau;
  const pts = [];
  for (let i = 0; i <= n; i++) {
    const a = a0 + sweep * (i / n);
    pts.push({ x: kiround(ux + r * Math.cos(a)), y: kiround(uy + r * Math.sin(a)) });
  }
  pts[0] = start;
  pts[n] = end;
  return pts;
}

const tracks = [];
const line = (net, ...xy) => tracks.push({ id: `trk_arcpair_${tracks.length}`, net, layer: "F.Cu", width: mm(0.25), pts: xy.map(([x, y]) => pt(x, y)) });
line(P, [5, 9.5], [8, 9.5]);
line(P, [10, 11.5], [10, 14]);
line(P, [12, 16], [18, 16]);
line(P, [20, 14], [20, 11.5]);
line(P, [22, 9.5], [25, 9.5]);
line(N, [5, 10.5], [8, 10.5]);
line(N, [9, 11.5], [9, 14]);
line(N, [12, 17], [18, 17]);
line(N, [21, 14], [21, 11.5]);
line(N, [22, 10.5], [25, 10.5]);
const s = Math.SQRT1_2;
for (const [net, [cx, cy], r, a, b, [dx, dy]] of [
  [P, [8, 11.5], 2, [8, 9.5], [10, 11.5], [1, -1]],
  [P, [12, 14], 2, [10, 14], [12, 16], [-1, 1]],
  [P, [18, 14], 2, [18, 16], [20, 14], [1, 1]],
  [P, [22, 11.5], 2, [20, 11.5], [22, 9.5], [-1, -1]],
  [N, [8, 11.5], 1, [8, 10.5], [9, 11.5], [1, -1]],
  [N, [12, 14], 3, [9, 14], [12, 17], [-1, 1]],
  [N, [18, 14], 3, [18, 17], [21, 14], [1, 1]],
  [N, [22, 11.5], 1, [21, 11.5], [22, 10.5], [-1, -1]],
]) {
  const start = pt(...a), end = pt(...b), mid = pt(cx + dx * r * s, cy + dy * r * s);
  tracks.push({ id: `trk_arcpair_arc_${tracks.length}`, net, layer: "F.Cu", width: mm(0.25), pts: tessellate(start, mid, end), arc_mid_offset: { x: mid.x - start.x, y: mid.y - start.y } });
}
d.routing = d.routing ?? { tracks: [], vias: [], zones: [] };
d.routing.tracks = [...(d.routing.tracks ?? []), ...tracks];
d.routing.vias = d.routing.vias ?? [];
d.routing.zones = d.routing.zones ?? [];
writeFileSync(file, JSON.stringify(d, null, 2));
rmSync(join(dir, ".history"), { recursive: true, force: true });
console.log(`added ${tracks.length} tracks (8 arcs) to ${file}`);

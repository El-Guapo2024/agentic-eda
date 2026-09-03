// Differential bench: runs elkjs "layered" on the same case corpus that
// crates/layout/examples/dump_cases.rs produced (with our eda-layout
// result baked into each case as `port`), computes the same metrics on
// elkjs's output (`elk`), and writes bench/elk/report.md.
//
// See README.md for the precise metric definitions -- keep this file's
// metric code and crates/layout/examples/dump_cases.rs's metric code in
// sync; they must implement identical semantics.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ELK from "elkjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const CASES_PATH = path.join(__dirname, "cases.json");
const REPORT_PATH = path.join(__dirname, "report.md");

const elk = new ELK();

const SIDE_TO_ELK = { Top: "NORTH", Bottom: "SOUTH", Left: "WEST", Right: "EAST" };

function portPoint(side, offset, width, height) {
  switch (side) {
    case "Top":
      return { x: offset, y: 0 };
    case "Bottom":
      return { x: offset, y: height };
    case "Left":
      return { x: 0, y: offset };
    case "Right":
      return { x: width, y: offset };
    default:
      throw new Error(`unknown side ${side}`);
  }
}

function buildElkGraph(kase) {
  const { nodes, edges, options } = kase;
  const children = nodes.map((n) => ({
    id: String(n.id),
    width: n.width,
    height: n.height,
    layoutOptions: { "elk.portConstraints": "FIXED_POS" },
    ports: n.ports.map((p, i) => {
      const pt = portPoint(p.side, p.offset, n.width, n.height);
      return {
        id: `${n.id}_p${i}`,
        x: pt.x,
        y: pt.y,
        width: 1,
        height: 1,
        layoutOptions: { "elk.port.side": SIDE_TO_ELK[p.side] },
      };
    }),
  }));
  const elkEdges = edges.map((e, i) => ({
    id: `e${i}`,
    sources: [`${e.from.node}_p${e.from.port}`],
    targets: [`${e.to.node}_p${e.to.port}`],
  }));
  return {
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.edgeRouting": "ORTHOGONAL",
      "elk.layered.spacing.nodeNodeBetweenLayers": String(options.layer_spacing),
      "elk.spacing.nodeNode": String(options.node_spacing),
      "elk.spacing.edgeNode": String(options.node_spacing),
      "elk.hierarchyHandling": "INCLUDE_CHILDREN",
    },
    children,
    edges: elkEdges,
  };
}

/// Extracts {positions: {id: {x,y}}, polylines: [[{x,y},...],...]} in the
/// same shape as our Rust LayoutResult, from an elkjs-laid-out graph.
function extractResult(laid, caseEdges) {
  const positions = {};
  for (const child of laid.children) {
    positions[child.id] = { x: Math.round(child.x), y: Math.round(child.y) };
  }
  const polylines = [];
  for (let i = 0; i < caseEdges.length; i++) {
    const edge = laid.edges.find((e) => e.id === `e${i}`);
    const points = [];
    if (edge && edge.sections && edge.sections.length > 0) {
      for (const section of edge.sections) {
        points.push(section.startPoint);
        if (section.bendPoints) points.push(...section.bendPoints);
        points.push(section.endPoint);
      }
    }
    polylines.push(points.map((p) => ({ x: Math.round(p.x), y: Math.round(p.y) })));
  }
  return { positions, polylines };
}

// ---- metrics (must mirror compute_metrics in dump_cases.rs) ----

function segmentsCross(a0, a1, b0, b1) {
  const aHoriz = a0.y === a1.y;
  const bHoriz = b0.y === b1.y;
  if (aHoriz === bHoriz) return false;
  const [h0, h1, v0, v1] = aHoriz ? [a0, a1, b0, b1] : [b0, b1, a0, a1];
  const hx0 = Math.min(h0.x, h1.x);
  const hx1 = Math.max(h0.x, h1.x);
  const hy = h0.y;
  const vy0 = Math.min(v0.y, v1.y);
  const vy1 = Math.max(v0.y, v1.y);
  const vx = v0.x;
  return vx > hx0 && vx < hx1 && hy > vy0 && hy < vy1;
}

function computeMetrics(nodes, positions, polylines) {
  let wireLength = 0;
  let nonOrthogonalSegments = 0;
  let maxBendCount = 0;
  const allSegments = [];
  for (const poly of polylines) {
    if (poly.length >= 2) maxBendCount = Math.max(maxBendCount, poly.length - 2);
    for (let i = 0; i + 1 < poly.length; i++) {
      const a = poly[i];
      const b = poly[i + 1];
      wireLength += Math.abs(a.x - b.x) + Math.abs(a.y - b.y);
      if (a.x !== b.x && a.y !== b.y) nonOrthogonalSegments++;
      allSegments.push([a, b]);
    }
  }
  let crossings = 0;
  for (let i = 0; i < allSegments.length; i++) {
    for (let j = i + 1; j < allSegments.length; j++) {
      if (segmentsCross(allSegments[i][0], allSegments[i][1], allSegments[j][0], allSegments[j][1])) {
        crossings++;
      }
    }
  }
  const boxes = nodes.map((n) => {
    const p = positions[String(n.id)];
    return [p.x, p.x + n.width, p.y, p.y + n.height];
  });
  let nodeOverlaps = 0;
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const [l1, r1, t1, b1] = boxes[i];
      const [l2, r2, t2, b2] = boxes[j];
      if (l1 < r2 && l2 < r1 && t1 < b2 && t2 < b1) nodeOverlaps++;
    }
  }
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const [l, r, t, b] of boxes) {
    minX = Math.min(minX, l);
    maxX = Math.max(maxX, r);
    minY = Math.min(minY, t);
    maxY = Math.max(maxY, b);
  }
  const bboxArea = boxes.length === 0 ? 0 : (maxX - minX) * (maxY - minY);
  return { crossings, wire_length: wireLength, bbox_area: bboxArea, node_overlaps: nodeOverlaps, non_orthogonal_segments: nonOrthogonalSegments, max_bend_count: maxBendCount };
}

function ratio(portVal, elkVal) {
  if (elkVal === 0) return portVal === 0 ? 1 : Infinity;
  return portVal / elkVal;
}

async function main() {
  const doc = JSON.parse(fs.readFileSync(CASES_PATH, "utf8"));
  const rows = [];
  const flagged = [];

  for (const kase of doc.cases) {
    const graph = buildElkGraph(kase);
    const start = process.hrtime.bigint();
    const laid = await elk.layout(graph);
    const elapsedNs = Number(process.hrtime.bigint() - start);
    const { positions, polylines } = extractResult(laid, kase.edges);
    const metrics = computeMetrics(kase.nodes, positions, polylines);
    kase.elk = { positions, polylines, time_ns: elapsedNs, metrics };

    const pm = kase.port.metrics;
    const em = metrics;
    rows.push({ name: kase.name, pm, em, port_time_ns: kase.port.time_ns, elk_time_ns: elapsedNs });

    const crossRatio = ratio(pm.crossings, em.crossings);
    const wireRatio = ratio(pm.wire_length, em.wire_length);
    if (crossRatio > 1.25 || wireRatio > 1.25) {
      flagged.push({ name: kase.name, crossRatio, wireRatio, pm, em });
    }
  }

  fs.writeFileSync(CASES_PATH, JSON.stringify(doc, null, 2));

  const fmtRatio = (r) => (r === Infinity ? "inf" : r.toFixed(2));
  let md = "# ELK vs eda-layout bench report\n\n";
  md += `Generated ${new Date().toISOString()} over ${rows.length} cases. See README.md for metric definitions.\n\n`;
  md += "| case | crossings (port/elk) | wire_length (port/elk) | bbox_area (port/elk) | overlaps (port/elk) | non_ortho (port/elk) | max_bend (port/elk) | time_ns (port/elk) |\n";
  md += "|---|---|---|---|---|---|---|---|\n";
  for (const r of rows) {
    const { pm, em } = r;
    md += `| ${r.name} | ${pm.crossings}/${em.crossings} (${fmtRatio(ratio(pm.crossings, em.crossings))}) | ${pm.wire_length}/${em.wire_length} (${fmtRatio(ratio(pm.wire_length, em.wire_length))}) | ${pm.bbox_area}/${em.bbox_area} (${fmtRatio(ratio(pm.bbox_area, em.bbox_area))}) | ${pm.node_overlaps}/${em.node_overlaps} | ${pm.non_orthogonal_segments}/${em.non_orthogonal_segments} | ${pm.max_bend_count}/${em.max_bend_count} | ${r.port_time_ns}/${r.elk_time_ns} |\n`;
  }

  md += "\n## Summary\n\n";
  if (flagged.length === 0) {
    md += "No cases where the port is worse than elkjs by more than 25% on crossings or wire length.\n";
  } else {
    md += `${flagged.length} case(s) flagged (port worse than elkjs by >25% on crossings or wire length):\n\n`;
    for (const f of flagged) {
      md += `- **${f.name}**: crossings ratio ${fmtRatio(f.crossRatio)} (${f.pm.crossings} vs ${f.em.crossings}), wire_length ratio ${fmtRatio(f.wireRatio)} (${f.pm.wire_length} vs ${f.em.wire_length})\n`;
    }
  }

  fs.writeFileSync(REPORT_PATH, md);
  console.log(md);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});

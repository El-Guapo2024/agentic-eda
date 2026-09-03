//! Dumps a corpus of test graphs (JSON) along with our layout's results and
//! metrics, for differential benchmarking against elkjs (see bench/elk/).
//!
//! Usage: cargo run --example dump_cases -- [output_path]
//! Default output path: bench/elk/cases.json

use eda_layout::graph::{EdgeEndpoint, LayoutGraph, Node, Point, Side};
use eda_layout::{layout, LayoutOptions};
use serde_json::json;
use std::time::Instant;

/// Small deterministic PRNG (same recipe as invariants.rs) so cases are
/// reproducible without pulling in an external crate.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn range(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn random_graph(n: usize, seed: u64) -> LayoutGraph {
    let mut rng = Lcg(seed);
    let mut g = LayoutGraph::new();
    for i in 0..n {
        g.add_node(Node::with_default_ports(i, 2540 + (i as i64 % 4) * 2540, 1270));
    }
    let edge_count = (n as f64 * 1.5) as usize;
    for _ in 0..edge_count {
        let a = rng.range(n);
        let mut b = rng.range(n);
        while b == a {
            b = rng.range(n);
        }
        g.add_edge(EdgeEndpoint { node: a, port: 1 }, EdgeEndpoint { node: b, port: 0 });
    }
    g
}

fn chain(n: usize) -> LayoutGraph {
    let mut g = LayoutGraph::new();
    for i in 0..n {
        g.add_node(Node::with_default_ports(i, 2540, 1270));
    }
    for i in 0..n - 1 {
        g.add_edge(EdgeEndpoint { node: i, port: 1 }, EdgeEndpoint { node: i + 1, port: 0 });
    }
    g
}

fn fanout_star(n_leaves: usize) -> LayoutGraph {
    let mut g = LayoutGraph::new();
    g.add_node(Node::with_default_ports(0, 5080, 2540));
    for i in 1..=n_leaves {
        g.add_node(Node::with_default_ports(i, 2540, 1270));
        g.add_edge(EdgeEndpoint { node: 0, port: 1 }, EdgeEndpoint { node: i, port: 0 });
    }
    g
}

fn dense_bipartite(left_n: usize, right_n: usize) -> LayoutGraph {
    let mut g = LayoutGraph::new();
    for i in 0..left_n {
        g.add_node(Node::with_default_ports(i, 2540, 1270));
    }
    for i in 0..right_n {
        g.add_node(Node::with_default_ports(left_n + i, 2540, 1270));
    }
    for l in 0..left_n {
        for r in 0..right_n {
            g.add_edge(EdgeEndpoint { node: l, port: 1 }, EdgeEndpoint { node: left_n + r, port: 0 });
        }
    }
    g
}

fn with_cycles(n: usize) -> LayoutGraph {
    let mut g = LayoutGraph::new();
    for i in 0..n {
        g.add_node(Node::with_default_ports(i, 2540, 1270));
    }
    for i in 0..n {
        g.add_edge(EdgeEndpoint { node: i, port: 1 }, EdgeEndpoint { node: (i + 1) % n, port: 0 });
    }
    // extra back edges to make cycle-breaking non-trivial
    g.add_edge(EdgeEndpoint { node: n - 1, port: 1 }, EdgeEndpoint { node: 0, port: 0 });
    if n > 3 {
        g.add_edge(EdgeEndpoint { node: n - 2, port: 1 }, EdgeEndpoint { node: 1, port: 0 });
    }
    g
}

/// Two ports on the same side of one node, feeding two different downstream
/// nodes -- exercises within-side port ordering.
fn two_ports_same_side() -> LayoutGraph {
    use eda_layout::graph::Port;
    let mut g = LayoutGraph::new();
    let mut hub = Node::new(0, 5080, 2540);
    hub.ports.push(Port { side: Side::Right, offset: 1270 });
    hub.ports.push(Port { side: Side::Right, offset: 3810 });
    hub.ports.push(Port { side: Side::Left, offset: 1270 });
    g.add_node(hub);
    g.add_node(Node::with_default_ports(1, 2540, 1270));
    g.add_node(Node::with_default_ports(2, 2540, 1270));
    g.add_node(Node::with_default_ports(3, 2540, 1270));
    g.add_edge(EdgeEndpoint { node: 0, port: 0 }, EdgeEndpoint { node: 1, port: 0 });
    g.add_edge(EdgeEndpoint { node: 0, port: 1 }, EdgeEndpoint { node: 2, port: 0 });
    g.add_edge(EdgeEndpoint { node: 3, port: 1 }, EdgeEndpoint { node: 0, port: 2 });
    g
}

fn side_str(s: Side) -> &'static str {
    match s {
        Side::Top => "Top",
        Side::Bottom => "Bottom",
        Side::Left => "Left",
        Side::Right => "Right",
    }
}

fn point_json(p: Point) -> serde_json::Value {
    json!({ "x": p.x, "y": p.y })
}

/// Metrics computed identically here and in bench/elk/run.mjs -- see
/// bench/elk/README.md for the precise definitions.
struct Metrics {
    crossings: u64,
    wire_length: i64,
    bbox_area: i64,
    node_overlaps: u64,
    non_orthogonal_segments: u64,
    max_bend_count: u64,
}

fn segments_cross(a0: Point, a1: Point, b0: Point, b1: Point) -> bool {
    // Only handles axis-aligned segments (orthogonal routing). Returns true
    // for a proper crossing (interiors intersect at a single point), not
    // for touching endpoints or collinear overlap.
    let a_horiz = a0.y == a1.y;
    let b_horiz = b0.y == b1.y;
    if a_horiz == b_horiz {
        return false; // parallel (or both same orientation) -> no proper crossing counted
    }
    let (h0, h1, v0, v1) = if a_horiz { (a0, a1, b0, b1) } else { (b0, b1, a0, a1) };
    let (hx0, hx1) = (h0.x.min(h1.x), h0.x.max(h1.x));
    let hy = h0.y;
    let (vy0, vy1) = (v0.y.min(v1.y), v0.y.max(v1.y));
    let vx = v0.x;
    let touches = vx > hx0 && vx < hx1 && hy > vy0 && hy < vy1;
    touches
}

fn compute_metrics(g: &LayoutGraph, positions: &std::collections::BTreeMap<usize, Point>, polylines: &[Vec<Point>]) -> Metrics {
    let mut wire_length: i64 = 0;
    let mut non_orthogonal_segments: u64 = 0;
    let mut max_bend_count: u64 = 0;
    let mut all_segments: Vec<(Point, Point)> = Vec::new();
    for poly in polylines {
        if poly.len() >= 2 {
            max_bend_count = max_bend_count.max((poly.len() - 2) as u64);
        }
        for w in poly.windows(2) {
            let (a, b) = (w[0], w[1]);
            wire_length += (a.x - b.x).abs() + (a.y - b.y).abs();
            if a.x != b.x && a.y != b.y {
                non_orthogonal_segments += 1;
            }
            all_segments.push((a, b));
        }
    }
    let mut crossings: u64 = 0;
    for i in 0..all_segments.len() {
        for j in (i + 1)..all_segments.len() {
            let (a0, a1) = all_segments[i];
            let (b0, b1) = all_segments[j];
            if segments_cross(a0, a1, b0, b1) {
                crossings += 1;
            }
        }
    }
    let boxes: Vec<(i64, i64, i64, i64)> = g
        .nodes
        .iter()
        .map(|n| {
            let p = positions[&n.id];
            (p.x, p.x + n.width, p.y, p.y + n.height)
        })
        .collect();
    let mut node_overlaps: u64 = 0;
    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            let (l1, r1, t1, b1) = boxes[i];
            let (l2, r2, t2, b2) = boxes[j];
            if l1 < r2 && l2 < r1 && t1 < b2 && t2 < b1 {
                node_overlaps += 1;
            }
        }
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for &(l, r, t, b) in &boxes {
        min_x = min_x.min(l);
        max_x = max_x.max(r);
        min_y = min_y.min(t);
        max_y = max_y.max(b);
    }
    let bbox_area = if boxes.is_empty() { 0 } else { (max_x - min_x) * (max_y - min_y) };
    Metrics { crossings, wire_length, bbox_area, node_overlaps, non_orthogonal_segments, max_bend_count }
}

fn case_json(name: &str, g: &LayoutGraph, opts: &LayoutOptions) -> serde_json::Value {
    let start = Instant::now();
    let result = layout(g, opts);
    let elapsed_ns = start.elapsed().as_nanos() as u64;
    let metrics = compute_metrics(g, &result.positions, &result.edge_polylines);

    let nodes_json: Vec<_> = g
        .nodes
        .iter()
        .map(|n| {
            json!({
                "id": n.id,
                "width": n.width,
                "height": n.height,
                "ports": n.ports.iter().map(|p| json!({"side": side_str(p.side), "offset": p.offset})).collect::<Vec<_>>(),
            })
        })
        .collect();
    let edges_json: Vec<_> = g
        .edges
        .iter()
        .map(|e| {
            json!({
                "from": {"node": e.from.node, "port": e.from.port},
                "to": {"node": e.to.node, "port": e.to.port},
            })
        })
        .collect();
    let positions_json: serde_json::Map<String, serde_json::Value> =
        result.positions.iter().map(|(id, p)| (id.to_string(), point_json(*p))).collect();
    let polylines_json: Vec<_> = result.edge_polylines.iter().map(|poly| poly.iter().map(|p| point_json(*p)).collect::<Vec<_>>()).collect();

    json!({
        "name": name,
        "nodes": nodes_json,
        "edges": edges_json,
        "options": {
            "grid": opts.grid,
            "layer_spacing": opts.layer_spacing,
            "node_spacing": opts.node_spacing,
        },
        "port": {
            "positions": positions_json,
            "polylines": polylines_json,
            "time_ns": elapsed_ns,
            "metrics": {
                "crossings": metrics.crossings,
                "wire_length": metrics.wire_length,
                "bbox_area": metrics.bbox_area,
                "node_overlaps": metrics.node_overlaps,
                "non_orthogonal_segments": metrics.non_orthogonal_segments,
                "max_bend_count": metrics.max_bend_count,
            }
        }
    })
}

fn main() {
    let out_path = std::env::args().nth(1).unwrap_or_else(|| "bench/elk/cases.json".to_string());
    let opts = LayoutOptions::default();
    let mut cases = Vec::new();

    for &n in &[5usize, 10, 20, 30, 50] {
        for seed in 1u64..=5 {
            let g = random_graph(n, seed);
            cases.push(case_json(&format!("random_n{n}_seed{seed}"), &g, &opts));
        }
    }
    cases.push(case_json("chain_20", &chain(20), &opts));
    cases.push(case_json("fanout_star_12", &fanout_star(12), &opts));
    cases.push(case_json("dense_bipartite_6x6", &dense_bipartite(6, 6), &opts));
    cases.push(case_json("with_cycles_10", &with_cycles(10), &opts));
    cases.push(case_json("two_ports_same_side", &two_ports_same_side(), &opts));

    let doc = json!({ "cases": cases });
    if let Some(parent) = std::path::Path::new(&out_path).parent() {
        std::fs::create_dir_all(parent).expect("create output dir");
    }
    std::fs::write(&out_path, serde_json::to_string_pretty(&doc).unwrap()).expect("write cases json");
    eprintln!("wrote {} cases to {out_path}", doc["cases"].as_array().unwrap().len());
}

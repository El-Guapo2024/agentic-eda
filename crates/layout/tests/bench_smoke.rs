//! Smoke test for the bench corpus generator: `dump_cases` must run
//! successfully on the full corpus, and every case it emits must produce a
//! port layout that passes the same core invariants as
//! `tests/invariants.rs` (box non-overlap, orthogonal polylines, grid
//! snapping, no box intersections, stub-tip endpoints).

use eda_layout::graph::{EdgeEndpoint, LayoutGraph, Node, Point};
use eda_layout::{layout, LayoutOptions, LayoutResult};

fn assert_no_overlaps(g: &LayoutGraph, result: &LayoutResult) {
    let boxes: Vec<(Point, i64, i64)> = g
        .nodes
        .iter()
        .map(|n| {
            let p = result.positions[&n.id];
            (p, n.width, n.height)
        })
        .collect();
    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            let (pi, wi, hi) = boxes[i];
            let (pj, wj, hj) = boxes[j];
            let overlap_x = pi.x < pj.x + wj && pj.x < pi.x + wi;
            let overlap_y = pi.y < pj.y + hj && pj.y < pi.y + hi;
            assert!(!(overlap_x && overlap_y), "nodes {} and {} overlap", g.nodes[i].id, g.nodes[j].id);
        }
    }
}

fn assert_orthogonal(result: &LayoutResult) {
    for (ei, poly) in result.edge_polylines.iter().enumerate() {
        for w in poly.windows(2) {
            let (a, b) = (w[0], w[1]);
            assert!(a.x == b.x || a.y == b.y, "edge {ei} has a non-orthogonal segment {a:?} -> {b:?}");
        }
    }
}

fn assert_grid_snapped(g: &LayoutGraph, result: &LayoutResult, grid: i64) {
    for n in &g.nodes {
        let p = result.positions[&n.id];
        assert_eq!(p.x % grid, 0, "node {} x not grid-snapped", n.id);
        assert_eq!(p.y % grid, 0, "node {} y not grid-snapped", n.id);
    }
    for poly in &result.edge_polylines {
        for p in poly {
            assert_eq!(p.x % grid, 0, "polyline point not grid-snapped: {p:?}");
            assert_eq!(p.y % grid, 0, "polyline point not grid-snapped: {p:?}");
        }
    }
}

fn assert_no_box_intersections(g: &LayoutGraph, result: &LayoutResult) {
    let boxes: Vec<(i64, i64, i64, i64)> = g
        .nodes
        .iter()
        .map(|n| {
            let p = result.positions[&n.id];
            (p.x, p.x + n.width, p.y, p.y + n.height)
        })
        .collect();
    for (ei, poly) in result.edge_polylines.iter().enumerate() {
        for seg in poly.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            for &(left, right, top, bottom) in &boxes {
                let hits = if a.y == b.y {
                    let y = a.y;
                    y > top && y < bottom && a.x.max(b.x) > left && a.x.min(b.x) < right
                } else if a.x == b.x {
                    let x = a.x;
                    x > left && x < right && a.y.max(b.y) > top && a.y.min(b.y) < bottom
                } else {
                    a.x.max(b.x) > left && a.x.min(b.x) < right && a.y.max(b.y) > top && a.y.min(b.y) < bottom
                };
                assert!(!hits, "edge {ei} segment {a:?} -> {b:?} cuts through box ({left},{right})x({top},{bottom})");
            }
        }
    }
}

fn assert_stub_tips(g: &LayoutGraph, result: &LayoutResult) {
    for (ei, edge) in g.edges.iter().enumerate() {
        let poly = &result.edge_polylines[ei];
        if poly.is_empty() {
            continue;
        }
        let from_pos = result.positions[&edge.from.node];
        let to_pos = result.positions[&edge.to.node];
        let expect_from = g.nodes[edge.from.node].stub_tip(from_pos, edge.from.port);
        let expect_to = g.nodes[edge.to.node].stub_tip(to_pos, edge.to.port);
        let first = *poly.first().unwrap();
        let last = *poly.last().unwrap();
        let matches_forward = first == expect_from && last == expect_to;
        let matches_backward = first == expect_to && last == expect_from;
        assert!(matches_forward || matches_backward, "edge {ei} endpoints don't match stub tips");
    }
}

fn assert_invariants(g: &LayoutGraph, result: &LayoutResult, grid: i64) {
    assert_no_overlaps(g, result);
    assert_orthogonal(result);
    assert_grid_snapped(g, result, grid);
    assert_no_box_intersections(g, result);
    assert_stub_tips(g, result);
}

// --- corpus generators, mirroring crates/layout/examples/dump_cases.rs ---

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
    g.add_edge(EdgeEndpoint { node: n - 1, port: 1 }, EdgeEndpoint { node: 0, port: 0 });
    if n > 3 {
        g.add_edge(EdgeEndpoint { node: n - 2, port: 1 }, EdgeEndpoint { node: 1, port: 0 });
    }
    g
}

fn two_ports_same_side() -> LayoutGraph {
    use eda_layout::graph::{Port, Side};
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

fn corpus() -> Vec<(String, LayoutGraph)> {
    let mut cases = Vec::new();
    for &n in &[5usize, 10, 20, 30, 50] {
        for seed in 1u64..=5 {
            cases.push((format!("random_n{n}_seed{seed}"), random_graph(n, seed)));
        }
    }
    cases.push(("chain_20".to_string(), chain(20)));
    cases.push(("fanout_star_12".to_string(), fanout_star(12)));
    cases.push(("dense_bipartite_6x6".to_string(), dense_bipartite(6, 6)));
    cases.push(("with_cycles_10".to_string(), with_cycles(10)));
    cases.push(("two_ports_same_side".to_string(), two_ports_same_side()));
    cases
}

#[test]
fn dump_cases_corpus_passes_invariants() {
    let opts = LayoutOptions::default();
    let cases = corpus();
    assert_eq!(cases.len(), 30, "expected 25 random cases + 5 shape cases");
    for (name, g) in &cases {
        let result = layout(g, &opts);
        assert_invariants(g, &result, opts.grid);
        assert_eq!(result.edge_polylines.len(), g.edges.len(), "case {name}: polyline count must match edge count");
    }
}

/// Runs the actual `dump_cases` example binary end-to-end (not just the
/// in-test copy of its generators) and checks it produces a well-formed
/// JSON corpus with a `port` result on every case.
#[test]
fn dump_cases_example_runs_and_emits_json() {
    let out_dir = std::env::temp_dir().join(format!("eda_layout_bench_smoke_{}", std::process::id()));
    std::fs::create_dir_all(&out_dir).unwrap();
    let out_path = out_dir.join("cases.json");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let status = std::process::Command::new(env!("CARGO"))
        .args(["run", "-q", "--example", "dump_cases", "--"])
        .arg(&out_path)
        .current_dir(manifest_dir)
        .status()
        .expect("failed to run dump_cases example");
    assert!(status.success(), "dump_cases example exited with failure");

    let contents = std::fs::read_to_string(&out_path).expect("dump_cases did not write output file");
    let doc: serde_json::Value = serde_json::from_str(&contents).expect("dump_cases output is not valid JSON");
    let cases = doc["cases"].as_array().expect("missing cases array");
    assert_eq!(cases.len(), 30);
    for case in cases {
        assert!(case["name"].is_string());
        assert!(case["port"]["positions"].is_object());
        assert!(case["port"]["polylines"].is_array());
        assert!(case["port"]["metrics"]["crossings"].is_number());
    }

    let _ = std::fs::remove_dir_all(&out_dir);
}

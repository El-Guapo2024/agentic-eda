//! Generic internal graph model for the Sugiyama pipeline.
//! Not tied to ConstraintModel — that wiring is future work.

pub type NodeId = usize;

/// Fixed pin-stub length (um): how far a wire extends beyond the box
/// boundary, on the port's side, before it may bend. Canonical for the
/// whole workspace — engine/render/gates all call through
/// [`Node::stub_tip`] (directly or via `eda_engine::geometry::stub_tip`) so
/// this single value never drifts.
pub const STUB_LEN: i64 = 1_270;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

/// A connection point on a node's boundary, given as an offset (um) from the
/// node's top-left corner, measured along the named side.
#[derive(Debug, Clone, Copy)]
pub struct Port {
    pub side: Side,
    pub offset: i64,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub width: i64,
    pub height: i64,
    pub ports: Vec<Port>,
}

impl Node {
    pub fn new(id: NodeId, width: i64, height: i64) -> Self {
        Self { id, width, height, ports: Vec::new() }
    }

    /// Convenience: a node with a single bottom-center output port and a
    /// single top-center input port (index 0 = top, index 1 = bottom).
    pub fn with_default_ports(id: NodeId, width: i64, height: i64) -> Self {
        Self {
            id,
            width,
            height,
            ports: vec![
                Port { side: Side::Top, offset: width / 2 },
                Port { side: Side::Bottom, offset: width / 2 },
            ],
        }
    }

    /// Absolute location of `ports[port_idx]` given the node's top-left
    /// corner position.
    pub fn port_point(&self, top_left: Point, port_idx: usize) -> Point {
        let p = &self.ports[port_idx];
        match p.side {
            Side::Top => Point { x: top_left.x + p.offset, y: top_left.y },
            Side::Bottom => Point { x: top_left.x + p.offset, y: top_left.y + self.height },
            Side::Left => Point { x: top_left.x, y: top_left.y + p.offset },
            Side::Right => Point { x: top_left.x + self.width, y: top_left.y + p.offset },
        }
    }

    /// Absolute location of the tip of `ports[port_idx]`'s pin stub: the
    /// port point extended [`STUB_LEN`] outward, on the port's side. This is
    /// where a wire must actually terminate (not the box boundary).
    pub fn stub_tip(&self, top_left: Point, port_idx: usize) -> Point {
        let p = self.port_point(top_left, port_idx);
        match self.ports[port_idx].side {
            Side::Top => Point { x: p.x, y: p.y - STUB_LEN },
            Side::Bottom => Point { x: p.x, y: p.y + STUB_LEN },
            Side::Left => Point { x: p.x - STUB_LEN, y: p.y },
            Side::Right => Point { x: p.x + STUB_LEN, y: p.y },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EdgeEndpoint {
    pub node: NodeId,
    pub port: usize,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: EdgeEndpoint,
    pub to: EdgeEndpoint,
}

#[derive(Debug, Clone, Default)]
pub struct LayoutGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl LayoutGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: Node) -> NodeId {
        debug_assert_eq!(node.id, self.nodes.len(), "node ids must be assigned in insertion order 0..n");
        let id = node.id;
        self.nodes.push(node);
        id
    }

    pub fn add_edge(&mut self, from: EdgeEndpoint, to: EdgeEndpoint) {
        self.edges.push(Edge { from, to });
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

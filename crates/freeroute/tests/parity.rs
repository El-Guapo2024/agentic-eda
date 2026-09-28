//! Rooms and doors checked against FreeRouting v1.9 itself.
//!
//! Each file in `tests/parity/` was written by `parity/RoomParity.java`
//! running the real FreeRouting on one of its example boards, or on one of
//! our own in `parity/fixtures/`: the board's shapes in the order its search
//! tree stores them, a start point, and every room and door FreeRouting made
//! filling that net's free space from there.
//! This rebuilds the same tree, runs the same fill, and requires the same
//! rooms in the same order, each with the same doors in the same order.
//!
//! Regenerate the files with `parity/dump.sh`, which needs Java 25 and a
//! FreeRouting checkout. The committed set is a sample; to check more
//! boards, point `FREEROUTE_PARITY_DIR` at a directory of dumps. A dump made
//! with `--steps` also records each completion, and is compared step by
//! step: the first difference then names the completion where the port
//! parts from FreeRouting, and whether growing the room already differs.
//!
//! The dumps also carry every pin's and via's raw pad shapes and the
//! clearance rules, so the board model is checked too: each pad, grown as
//! the port grows it, must come out as FreeRouting's tree shape; likewise
//! each area -- keepout or copper pour, round, polygonal or with holes --
//! split into the same convex pieces line for line, then grown; and the
//! board outline, its edges widened as traces are, and each trace, shape
//! for shape and line for line. Rooms
//! are checked on boards with traces too: a polygon trace shape is stored
//! under its bounding octagon, and tested exactly where FreeRouting tests it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eda_freeroute::board::{clearance_offset, drill_tree_shape, outline_tree_shapes, piece_tree_shapes, trace_tree_shapes, AreaShape, PadShape};
use eda_freeroute::door::{RoomGraph, RoomId, RoomState};
use eda_freeroute::geometry::{
    Circle, IntBox, IntOctagon, IntPoint, Line, PolygonShape, Polyline, PolylineArea, PolylineShape, Simplex, TileShape,
};
use eda_freeroute::room::{complete_shape, IncompleteRoom, TreeObject};
use eda_freeroute::rules::ClearanceMatrix;

#[derive(Debug, Clone)]
struct Item {
    id: u64,
    shape_index: u32,
    layer: i32,
    /// Recorded for the net the dump was made for.
    obstacle: bool,
    /// Where the tree shape is a polygon, not the octagon it is stored under.
    exact: Option<TileShape>,
}

impl TreeObject for Item {
    fn id(&self) -> u64 {
        self.id
    }
    fn shape_index(&self) -> u32 {
        self.shape_index
    }
    fn layer(&self) -> i32 {
        self.layer
    }
    fn is_obstacle_for(&self, _net: i32) -> bool {
        self.obstacle
    }
    fn is_free_space_room(&self) -> bool {
        false
    }
    fn exact_shape(&self) -> Option<&TileShape> {
        self.exact.as_ref()
    }
}

fn octagon(f: &[&str]) -> IntOctagon {
    let v: Vec<i64> = f.iter().map(|s| s.parse().unwrap()).collect();
    IntOctagon::new(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7])
}

fn octagon_text(o: &IntOctagon) -> String {
    format!(
        "{} {} {} {} {} {} {} {}",
        o.left_x, o.bottom_y, o.right_x, o.top_y, o.upper_left_diag_x, o.lower_right_diag_x, o.lower_left_diag_x, o.upper_right_diag_x
    )
}

/// Replay one dump; the first line where the port and FreeRouting differ,
/// or `None` if they agree throughout.
fn replay(dump: &str) -> Option<String> {
    let polygons: HashMap<(u64, u32), TileShape> =
        exact_shapes(dump).into_iter().filter(|(_, s)| matches!(s, TileShape::Simplex(_))).collect();
    let mut board = None;
    let mut net = 0;
    let mut graph: Option<RoomGraph<Item>> = None;
    let mut want = Vec::new();
    for line in dump.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.first().copied().unwrap_or("") {
            "board" => {
                let v: Vec<i64> = f[1..5].iter().map(|s| s.parse().unwrap()).collect();
                board = Some(IntBox::new(v[0], v[1], v[2], v[3]));
            }
            "net" => {
                net = f[1].parse().unwrap();
                graph = Some(RoomGraph::new(board.expect("board before net"), net));
            }
            "item" => {
                let (id, shape_index) = (f[1].parse().unwrap(), f[2].parse().unwrap());
                let exact = polygons.get(&(id, shape_index)).cloned();
                let item = Item { id, shape_index, layer: f[3].parse().unwrap(), obstacle: f[4] == "1", exact };
                graph.as_mut().expect("net before items").insert_item(octagon(&f[5..13]), item);
            }
            "start" => {
                let (layer, x, y): (i32, i64, i64) = (f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap());
                graph.as_mut().unwrap().add_incomplete_room(None, layer, IntBox::new(x, y, x, y).to_octagon());
            }
            "room" | "door" | "step" | "grown" | "cand" | "made" => want.push(line.to_string()),
            _ => {}
        }
    }
    let mut g = graph.expect("no net in dump");
    let steps = want.iter().any(|l| l.starts_with("step "));
    let id_no = |g: &RoomGraph<Item>, r: RoomId| match g.room(r).state {
        RoomState::Complete { id_no, .. } => id_no,
        RoomState::Incomplete { .. } => panic!("incomplete room left after the fill"),
        RoomState::Obstacle { .. } => panic!("no obstacle rooms without routable items"),
    };
    let mut got = Vec::new();
    let mut step = 0;
    while let Some(&r) = g.incomplete_rooms().first() {
        if steps {
            let room = g.room(r).clone();
            let RoomState::Incomplete { contained } = room.state else { unreachable!() };
            // What complete_expansion_room ignores: the completed room behind
            // the first overlap door.
            let ignore = room.doors().iter().find_map(|&d| {
                let door = g.door(d);
                match g.room(door.other(r)?).state {
                    RoomState::Complete { id_no, leaf: Some(leaf), .. } if door.dimension == 2 => Some((id_no, leaf, g.door_shape(d))),
                    _ => None,
                }
            });
            let shape = room.shape.map_or("none".to_string(), |s| octagon_text(&s));
            got.push(format!("step {step} {} {shape} {} {}", room.layer, octagon_text(&contained), ignore.map_or(0, |i| i.0)));
            let incomplete = IncompleteRoom { shape: room.shape, layer: room.layer, contained };
            let grown = complete_shape(g.tree(), g.board(), &incomplete, g.net(), ignore.map(|i| i.1), ignore.map(|i| i.2));
            got.push(format!("grown {}", grown.len()));
            for c in &grown {
                got.push(format!("cand {} {}", octagon_text(&c.shape), octagon_text(&c.contained)));
            }
        }
        let made = g.complete_expansion_room(r);
        if steps {
            let ids: String = made.iter().map(|&m| format!(" {}", id_no(&g, m))).collect();
            got.push(format!("made{ids}"));
        }
        step += 1;
    }
    g.check_invariants().unwrap();

    for &r in g.complete_rooms() {
        let room = g.room(r);
        let RoomState::Complete { id_no, net_dependent, .. } = room.state else { unreachable!() };
        got.push(format!("room {id_no} {} {} {}", net_dependent as u8, room.layer, octagon_text(&room.shape.unwrap())));
    }
    for &r in g.complete_rooms() {
        for &d in g.room(r).doors() {
            let door = g.door(d);
            got.push(format!("door {} {} {}", id_no(&g, r), id_no(&g, door.other(r).unwrap()), door.dimension));
        }
    }
    for (i, (w, h)) in want.iter().zip(&got).enumerate() {
        if w != h {
            return Some(format!("line {i} (net {net}):\n  FreeRouting: {w}\n  port:        {h}"));
        }
    }
    if want.len() != got.len() {
        return Some(format!("FreeRouting made {} room and door lines, the port {}", want.len(), got.len()));
    }
    None
}

/// A pad shape from its dump fields: kind, then numbers.
fn pad_shape(f: &[&str]) -> Option<PadShape> {
    let n: Vec<i64> = f[1..].iter().filter_map(|s| s.parse().ok()).collect();
    let p = |i: usize| IntPoint::new(n[i], n[i + 1]);
    Some(match f[0] {
        "circle" => PadShape::Circle(Circle::new(p(0), n[2])),
        "box" => PadShape::Box(IntBox::new(n[0], n[1], n[2], n[3])),
        "octagon" => PadShape::Octagon(octagon(&f[1..9])),
        "simplex" => PadShape::Polygon(Simplex::new((0..n[0] as usize).map(|i| Line::new(p(1 + 4 * i), p(3 + 4 * i))).collect())),
        _ => return None,
    })
}

/// The clearance rules a dump records, and the trace class its tree was
/// built for. Only nonzero entries are recorded; any class or layer beyond
/// them has none, which is what the matrix answers outside its bounds.
fn clearance_rules(dump: &str) -> (ClearanceMatrix, i32) {
    let (mut entries, mut trace_class) = (Vec::new(), 0);
    for line in dump.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.first().copied().unwrap_or("") {
            "tree_class" => trace_class = f[1].parse().unwrap(),
            "cm" => entries.push([f[1], f[2], f[3], f[4]].map(|s| s.parse::<i64>().unwrap())),
            _ => {}
        }
    }
    let classes = entries.iter().map(|e| e[0].max(e[1]) + 1).max().unwrap_or(0) as usize;
    let layers = entries.iter().map(|e| e[2] + 1).max().unwrap_or(0) as usize;
    let mut rules = ClearanceMatrix::new(classes, layers);
    for e in &entries {
        rules.set(e[0] as usize, e[1] as usize, e[2] as usize, e[3]);
    }
    (rules, trace_class)
}

/// FreeRouting's tree shapes by item: each shape index with its octagon.
fn tree_shapes(dump: &str) -> HashMap<u64, Vec<(u32, String)>> {
    let mut tree: HashMap<u64, Vec<(u32, String)>> = HashMap::new();
    for line in dump.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.first() == Some(&"item") {
            tree.entry(f[1].parse().unwrap()).or_default().push((f[2].parse().unwrap(), f[5..13].join(" ")));
        }
    }
    tree
}

/// Grow every pin's and via's raw shapes as the port does, and compare with
/// FreeRouting's tree shapes: the number of pads checked, and every
/// difference.
fn check_pads(dump: &str) -> (usize, Vec<String>) {
    let tree = tree_shapes(dump);
    let (rules, trace_class) = clearance_rules(dump);
    let pads: Vec<Vec<&str>> = dump.lines().map(|l| l.split_whitespace().collect::<Vec<_>>()).filter(|f| f.first() == Some(&"pad")).collect();
    let mut diffs = Vec::new();
    for f in &pads {
        let (id, index, layer, class): (u64, u32, i32, i32) = (f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap(), f[4].parse().unwrap());
        let Some(shape) = pad_shape(&f[5..]) else {
            diffs.push(format!("pad {id}/{index}: unsupported shape {}", f[5..].join(" ")));
            continue;
        };
        let got = drill_tree_shape(&shape, clearance_offset(&rules, class, trace_class, layer)).map(|o| octagon_text(&o));
        let want = tree.get(&id).and_then(|shapes| shapes.iter().find(|(i, _)| *i == index)).map(|(_, o)| o.clone());
        if got != want {
            diffs.push(format!("pad {id}/{index} {}:\n    FreeRouting: {want:?}\n    port:        {got:?}", f[5..].join(" ")));
        }
    }
    (pads.len(), diffs)
}

/// Dump words read in turn.
struct Words<'a> {
    words: Vec<&'a str>,
    next: usize,
}

impl<'a> Words<'a> {
    fn word(&mut self) -> &'a str {
        self.next += 1;
        self.words[self.next - 1]
    }

    fn nums(&mut self, n: usize) -> Vec<i64> {
        (0..n).map(|_| self.word().parse().unwrap()).collect()
    }

    /// A convex shape of the given kind: "box", "octagon" or "simplex".
    fn tile(&mut self, kind: &str) -> Option<TileShape> {
        Some(match kind {
            "box" => {
                let n = self.nums(4);
                TileShape::Box(IntBox::new(n[0], n[1], n[2], n[3]))
            }
            "octagon" => {
                let n = self.nums(8);
                TileShape::Octagon(IntOctagon::new(n[0], n[1], n[2], n[3], n[4], n[5], n[6], n[7]))
            }
            "simplex" => {
                let k = self.nums(1)[0];
                let mut n = vec![k];
                n.extend(self.nums(4 * k as usize));
                TileShape::Simplex(Simplex::new(lines_from(&n)))
            }
            _ => return None,
        })
    }

    /// A polygon by the corners FreeRouting keeps, taken as they are: its
    /// constructor has already cleaned them, and doing so again need not
    /// leave them unchanged.
    fn polygon(&mut self) -> PolygonShape {
        let k = self.nums(1)[0] as usize;
        let n = self.nums(2 * k);
        PolygonShape { corners: (0..k).map(|i| IntPoint::new(n[2 * i], n[2 * i + 1])).collect() }
    }

    fn part(&mut self) -> Option<PolylineShape> {
        match self.word() {
            "polygon" => Some(PolylineShape::Polygon(self.polygon())),
            kind => self.tile(kind).map(PolylineShape::Tile),
        }
    }

    /// An area record's shape; `None` for a kind the port cannot read.
    fn area(&mut self) -> Option<AreaShape> {
        match self.word() {
            "circle" => {
                let n = self.nums(3);
                Some(AreaShape::Circle(Circle::new(IntPoint::new(n[0], n[1]), n[2])))
            }
            "polygon" => Some(AreaShape::Polygon(self.polygon())),
            "holes" => {
                let k = self.nums(1)[0] as usize;
                let border = self.part()?;
                let holes = (0..k).map(|_| self.part()).collect::<Option<Vec<_>>>()?;
                Some(AreaShape::WithHoles(PolylineArea { border, holes }))
            }
            kind => self.tile(kind).map(AreaShape::Tile),
        }
    }
}

/// A convex shape as the dump writes it.
fn shape_text(shape: &TileShape) -> String {
    match shape {
        TileShape::Box(b) => format!("box {} {} {} {}", b.ll.x, b.ll.y, b.ur.x, b.ur.y),
        TileShape::Octagon(o) => format!("octagon {}", octagon_text(o)),
        TileShape::Simplex(s) => {
            let lines: Vec<String> = s.lines.iter().map(|l| format!("{} {} {} {}", l.a.x, l.a.y, l.b.x, l.b.y)).collect();
            format!("simplex {} {}", s.lines.len(), lines.join(" ")).trim_end().to_string()
        }
    }
}

/// What checking a dump's areas found.
#[derive(Default)]
struct AreaCheck {
    areas: usize,
    pieces: usize,
    /// Polygons built from untidy corners.
    polygons: usize,
    unported: usize,
    diffs: Vec<String>,
}

/// Split every area into convex pieces as the port does and compare them
/// with FreeRouting's, exactly and in order; then grow them into tree
/// shapes and compare those, all in order. Also build each polygon from
/// the untidy corners the dump gives, as FreeRouting did.
fn check_areas(dump: &str) -> AreaCheck {
    let tree = tree_shapes(dump);
    let (rules, trace_class) = clearance_rules(dump);
    let mut want_pieces: HashMap<u64, Vec<String>> = HashMap::new();
    for line in dump.lines() {
        if let Some((id, shape)) = line.strip_prefix("area_piece ").and_then(|r| r.split_once(' ')) {
            want_pieces.entry(id.parse().unwrap()).or_default().push(shape.to_string());
        }
    }
    let mut section = 0.0;
    let mut check = AreaCheck::default();
    for line in dump.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.first().copied().unwrap_or("") {
            "area_section" => section = f[1].parse().unwrap(),
            "polygon_made" => {
                // polygon_made <id> polygon <given> -> polygon <made>
                let mut w = Words { words: f[3..].to_vec(), next: 0 };
                let given = w.polygon().corners;
                w.next += 2;
                let want = w.polygon().corners;
                check.polygons += 1;
                let got = PolygonShape::new(&given).corners;
                if got != want {
                    check.diffs.push(format!("polygon of area {} from {given:?}:\n    FreeRouting: {want:?}\n    port:        {got:?}", f[1]));
                }
            }
            "area" => {
                let (id, layer, class): (u64, i32, i32) = (f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap());
                let Some(area) = (Words { words: f[4..].to_vec(), next: 0 }).area() else {
                    check.unported += 1;
                    continue;
                };
                check.areas += 1;
                let split = area.split_to_convex();
                let pieces: Vec<String> = match &split {
                    Some(pieces) => pieces.iter().map(shape_text).collect(),
                    None => vec!["none".to_string()],
                };
                let want = want_pieces.get(&id).cloned().unwrap_or_default();
                check.pieces += want.len();
                if pieces != want {
                    let k = pieces.iter().zip(&want).position(|(p, w)| p != w).unwrap_or(pieces.len().min(want.len()));
                    check.diffs.push(format!(
                        "area {id} ({}): {} pieces, FreeRouting {}; first difference, piece {k}:\n    FreeRouting: {:?}\n    port:        {:?}",
                        f[4],
                        pieces.len(),
                        want.len(),
                        want.get(k),
                        pieces.get(k)
                    ));
                    continue;
                }
                // FreeRouting stores no tree shape where there is no
                // bounding octagon, and the dump skips those.
                let got: Vec<String> = piece_tree_shapes(&split.unwrap_or_default(), clearance_offset(&rules, class, trace_class, layer), section)
                    .iter()
                    .flatten()
                    .map(octagon_text)
                    .collect();
                let mut want = tree.get(&id).cloned().unwrap_or_default();
                want.sort_by_key(|(i, _)| *i);
                let want: Vec<String> = want.into_iter().map(|(_, o)| o).collect();
                if got != want {
                    check.diffs.push(format!("area {id} {}:\n    FreeRouting: {want:?}\n    port:        {got:?}", f[4..].join(" ")));
                }
            }
            _ => {}
        }
    }
    check
}

/// Widen the board outline's edges as the port does and compare with
/// FreeRouting's tree shapes for it, all in order. `None` if the dump has
/// no outline, or one with a generated keepout, which is not ported yet.
fn check_outline(dump: &str) -> Option<Result<usize, String>> {
    let tree = tree_shapes(dump);
    let (rules, trace_class) = clearance_rules(dump);
    let mut header = None;
    let mut shapes = Vec::new();
    for line in dump.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let n: Vec<i64> = f.iter().skip(1).filter_map(|s| s.parse().ok()).collect();
        match f.first().copied().unwrap_or("") {
            "outline" => header = Some(n),
            "outline_shape" => shapes.push((0..n[0] as usize).map(|i| Line::new(IntPoint::new(n[1 + 4 * i], n[2 + 4 * i]), IntPoint::new(n[3 + 4 * i], n[4 + 4 * i]))).collect::<Vec<_>>()),
            _ => {}
        }
    }
    let h = header?;
    let (id, half_width, class, layers, keepout) = (h[0] as u64, h[1], h[2] as i32, h[3] as usize, h[4]);
    if keepout != 0 {
        return None;
    }
    let got: Vec<String> = outline_tree_shapes(&shapes, half_width, clearance_offset(&rules, class, trace_class, 0), layers)
        .iter()
        .map(|o| o.map_or("none".to_string(), |o| octagon_text(&o)))
        .collect();
    let mut want = tree.get(&id).cloned().unwrap_or_default();
    want.sort_by_key(|(i, _)| *i);
    let want: Vec<String> = want.into_iter().map(|(_, o)| o).collect();
    if got == want {
        return Some(Ok(got.len()));
    }
    let first = got.iter().zip(&want).position(|(g, w)| g != w).unwrap_or(got.len().min(want.len()));
    Some(Err(format!(
        "{} vs {} outline shapes; first difference at {first}:\n    FreeRouting: {:?}\n    port:        {:?}",
        want.len(),
        got.len(),
        want.get(first),
        got.get(first)
    )))
}

/// Lines from dump numbers: a count, then `ax ay bx by` per line.
fn lines_from(n: &[i64]) -> Vec<Line> {
    (0..n[0] as usize).map(|i| Line::new(IntPoint::new(n[1 + 4 * i], n[2 + 4 * i]), IntPoint::new(n[3 + 4 * i], n[4 + 4 * i]))).collect()
}

/// FreeRouting's exact tree shapes: an octagon from its item record, or the
/// box or polygon an exact record gives.
fn exact_shapes(dump: &str) -> HashMap<(u64, u32), TileShape> {
    let mut shapes = HashMap::new();
    for line in dump.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let n: Vec<i64> = f.iter().skip(1).filter_map(|s| s.parse().ok()).collect();
        match f.first().copied().unwrap_or("") {
            "item" => {
                shapes.insert((n[0] as u64, n[1] as u32), TileShape::Octagon(octagon(&f[5..13])));
            }
            "exact" => {
                let shape = match f[3] {
                    "box" => TileShape::Box(IntBox::new(n[2], n[3], n[4], n[5])),
                    "simplex" => TileShape::Simplex(Simplex::new(lines_from(&n[2..]))),
                    kind => panic!("unexpected exact shape {kind}"),
                };
                shapes.insert((n[0] as u64, n[1] as u32), shape);
            }
            _ => {}
        }
    }
    shapes
}

/// Widen every trace as the port does and compare each segment's shape
/// with FreeRouting's exactly: the numbers of traces and segments checked,
/// and every difference.
fn check_traces(dump: &str) -> (usize, usize, Vec<String>) {
    let want = exact_shapes(dump);
    let (rules, trace_class) = clearance_rules(dump);
    let (mut traces, mut segments, mut diffs) = (0, 0, Vec::new());
    for line in dump.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.first() != Some(&"trace") {
            continue;
        }
        let n: Vec<i64> = f[1..].iter().map(|s| s.parse().unwrap()).collect();
        let (id, layer, half_width, class) = (n[0] as u64, n[1] as i32, n[2], n[3] as i32);
        let polyline = Polyline { lines: lines_from(&n[4..]) };
        let got = trace_tree_shapes(&polyline, half_width, clearance_offset(&rules, class, trace_class, layer));
        traces += 1;
        for (i, g) in got.iter().enumerate() {
            segments += 1;
            let w = want.get(&(id, i as u32));
            if g.as_ref() != w {
                diffs.push(format!("trace {id} segment {i}:\n    FreeRouting: {w:?}\n    port:        {g:?}"));
            }
        }
    }
    (traces, segments, diffs)
}

fn dumps() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = match std::env::var_os("FREEROUTE_PARITY_DIR").map(PathBuf::from) {
        // Tests run in the crate's directory; a relative path is taken from
        // the workspace root, where cargo is usually run.
        Some(d) if d.is_relative() => manifest.join("../..").join(d),
        Some(d) => d,
        None => manifest.join("tests/parity"),
    };
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("parity dumps in {}: {e}", dir.display()));
    let mut files: Vec<_> = entries.map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "txt")).collect();
    files.sort();
    assert!(!files.is_empty(), "no parity dumps in {}", dir.display());
    files
}

#[test]
fn rooms_and_doors_match_freerouting() {
    let (mut compared, mut failures) = (0, Vec::new());
    for path in dumps() {
        let dump = std::fs::read_to_string(&path).unwrap();
        compared += 1;
        if let Some(diff) = replay(&dump) {
            failures.push(format!("{}: {diff}", path.file_name().unwrap().to_string_lossy()));
        }
    }
    assert!(compared > 0, "no board without traces to compare rooms on");
    assert!(failures.is_empty(), "{} of {compared} boards differ from FreeRouting:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn pin_and_via_shapes_match_freerouting() {
    let (mut checked, mut failures) = (0, Vec::new());
    for path in dumps() {
        let (pads, diffs) = check_pads(&std::fs::read_to_string(&path).unwrap());
        checked += pads;
        if !diffs.is_empty() {
            let shown: Vec<_> = diffs.iter().take(3).cloned().collect();
            failures.push(format!("{}: {} of {pads} pads differ, e.g.\n  {}", path.file_name().unwrap().to_string_lossy(), diffs.len(), shown.join("\n  ")));
        }
    }
    assert!(checked > 0, "no pads in the dumps");
    assert!(failures.is_empty(), "{} boards differ from FreeRouting:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn area_shapes_match_freerouting() {
    let (mut total, mut failures) = (AreaCheck::default(), Vec::new());
    for path in dumps() {
        let check = check_areas(&std::fs::read_to_string(&path).unwrap());
        total.areas += check.areas;
        total.pieces += check.pieces;
        total.polygons += check.polygons;
        total.unported += check.unported;
        if !check.diffs.is_empty() {
            let shown: Vec<_> = check.diffs.iter().take(3).cloned().collect();
            failures.push(format!(
                "{}: {} of {} areas differ, e.g.\n  {}",
                path.file_name().unwrap().to_string_lossy(),
                check.diffs.len(),
                check.areas,
                shown.join("\n  ")
            ));
        }
    }
    eprintln!(
        "areas checked: {} ({} convex pieces; {} polygons built from untidy corners); of shapes the port cannot read: {}",
        total.areas, total.pieces, total.polygons, total.unported
    );
    assert_eq!(total.unported, 0, "areas of shapes the port cannot read");
    assert!(failures.is_empty(), "{} boards differ from FreeRouting:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn outline_shapes_match_freerouting() {
    let (mut checked, mut edges, mut skipped, mut failures) = (0, 0, 0, Vec::new());
    for path in dumps() {
        match check_outline(&std::fs::read_to_string(&path).unwrap()) {
            None => skipped += 1,
            Some(Ok(n)) => (checked, edges) = (checked + 1, edges + n),
            Some(Err(e)) => failures.push(format!("{}: {e}", path.file_name().unwrap().to_string_lossy())),
        }
    }
    eprintln!("outlines checked: {checked} ({edges} edge shapes); skipped: {skipped}");
    assert!(failures.is_empty(), "{} outlines differ from FreeRouting:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn trace_shapes_match_freerouting() {
    let (mut traces, mut segments, mut failures) = (0, 0, Vec::new());
    for path in dumps() {
        let (t, n, diffs) = check_traces(&std::fs::read_to_string(&path).unwrap());
        (traces, segments) = (traces + t, segments + n);
        if !diffs.is_empty() {
            let shown: Vec<_> = diffs.iter().take(2).cloned().collect();
            failures.push(format!("{}: {} of {n} segments differ, e.g.\n  {}", path.file_name().unwrap().to_string_lossy(), diffs.len(), shown.join("\n  ")));
        }
    }
    eprintln!("traces checked: {traces} ({segments} segments)");
    assert!(failures.is_empty(), "{} boards differ from FreeRouting:\n{}", failures.len(), failures.join("\n"));
}

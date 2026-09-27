//! Rooms and doors checked against FreeRouting v1.9 itself.
//!
//! Each file in `tests/parity/` was written by `parity/RoomParity.java`
//! running the real FreeRouting on one of its example boards: the board's
//! shapes in the order its search tree stores them, a start point, and every
//! room and door FreeRouting made filling that net's free space from there.
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
//! each area -- keepout or copper pour -- of a shape ported so far. Boards
//! with traces take part in that check only: their trace shapes are not
//! octagons until traces are ported, so the room check skips them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eda_freeroute::board::{area_tree_shapes, clearance_offset, drill_tree_shape, AreaShape, PadShape};
use eda_freeroute::door::{RoomGraph, RoomId, RoomState};
use eda_freeroute::geometry::{Circle, IntBox, IntOctagon, IntPoint, Line, Simplex};
use eda_freeroute::room::{complete_shape, IncompleteRoom, TreeObject};
use eda_freeroute::rules::ClearanceMatrix;

#[derive(Debug, Clone)]
struct Item {
    id: u64,
    shape_index: u32,
    layer: i32,
    /// Recorded for the net the dump was made for.
    obstacle: bool,
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
                let item = Item { id: f[1].parse().unwrap(), shape_index: f[2].parse().unwrap(), layer: f[3].parse().unwrap(), obstacle: f[4] == "1" };
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

/// Grow every area of a shape ported so far as the port does, and compare
/// with FreeRouting's tree shapes, all of them in order: the numbers of
/// areas checked and not yet portable, and every difference.
fn check_areas(dump: &str) -> (usize, usize, Vec<String>) {
    let tree = tree_shapes(dump);
    let (rules, trace_class) = clearance_rules(dump);
    let mut section = 0.0;
    let (mut checked, mut unported, mut diffs) = (0, 0, Vec::new());
    for line in dump.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.first().copied().unwrap_or("") {
            "area_section" => section = f[1].parse().unwrap(),
            "area" if f[4] == "circle" => {
                let n: Vec<i64> = f[1..].iter().filter_map(|s| s.parse().ok()).collect();
                let (id, layer, class) = (n[0] as u64, n[1] as i32, n[2] as i32);
                let area = AreaShape::Circle(Circle::new(IntPoint::new(n[3], n[4]), n[5]));
                let got: Vec<String> =
                    area_tree_shapes(&area, clearance_offset(&rules, class, trace_class, layer), section).iter().map(octagon_text).collect();
                let mut want = tree.get(&id).cloned().unwrap_or_default();
                want.sort_by_key(|(i, _)| *i);
                let want: Vec<String> = want.into_iter().map(|(_, o)| o).collect();
                checked += 1;
                if got != want {
                    diffs.push(format!("area {id} {}:\n    FreeRouting: {want:?}\n    port:        {got:?}", f[4..].join(" ")));
                }
            }
            "area" => unported += 1,
            _ => {}
        }
    }
    (checked, unported, diffs)
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
        // Trace shapes stand in as bounding octagons, which FreeRouting does
        // not use for rooms: leave those boards to the pad check.
        if dump.lines().any(|l| l.starts_with("item ") && l.ends_with(" approx")) {
            continue;
        }
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
fn round_area_shapes_match_freerouting() {
    let (mut checked, mut unported, mut failures) = (0, 0, Vec::new());
    for path in dumps() {
        let (n, skipped, diffs) = check_areas(&std::fs::read_to_string(&path).unwrap());
        (checked, unported) = (checked + n, unported + skipped);
        if !diffs.is_empty() {
            let shown: Vec<_> = diffs.iter().take(3).cloned().collect();
            failures.push(format!("{}: {} of {n} areas differ, e.g.\n  {}", path.file_name().unwrap().to_string_lossy(), diffs.len(), shown.join("\n  ")));
        }
    }
    eprintln!("round areas checked: {checked}; areas of shapes not ported yet: {unported}");
    assert!(failures.is_empty(), "{} boards differ from FreeRouting:\n{}", failures.len(), failures.join("\n"));
}

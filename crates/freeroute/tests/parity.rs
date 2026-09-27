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

use std::path::{Path, PathBuf};

use eda_freeroute::door::{RoomGraph, RoomId, RoomState};
use eda_freeroute::geometry::{IntBox, IntOctagon};
use eda_freeroute::room::{complete_shape, IncompleteRoom, TreeObject};

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
        match f[0] {
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

#[test]
fn rooms_and_doors_match_freerouting() {
    let dir = std::env::var_os("FREEROUTE_PARITY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity"));
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "txt")).collect();
    files.sort();
    assert!(!files.is_empty(), "no parity dumps in {}", dir.display());
    let mut failures = Vec::new();
    for path in &files {
        let dump = std::fs::read_to_string(path).unwrap();
        if let Some(diff) = replay(&dump) {
            failures.push(format!("{}: {diff}", path.file_name().unwrap().to_string_lossy()));
        }
    }
    assert!(failures.is_empty(), "{} of {} boards differ from FreeRouting:\n{}", failures.len(), files.len(), failures.join("\n"));
}

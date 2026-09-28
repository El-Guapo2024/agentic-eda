//! The router checked against FreeRouting v1.9's own maze search.
//!
//! Each file in `tests/maze/` was written by `parity/MazeParity.java`
//! running the real FreeRouting on a board: the board as FreeRouting loaded
//! it -- layers, rules, router settings, every item with its raw shapes --
//! the shapes of both its search trees, the connection its batch autorouter
//! routes first, and every step of the maze search for it.
//!
//! The board is read with [`eda_freeroute::dump::read_board`], and every
//! tree shape the port computes from it must match FreeRouting's.
//!
//! Regenerate the files with `parity/maze_dump.sh`; point
//! `FREEROUTE_MAZE_DIR` at a directory of dumps to check more boards.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eda_freeroute::board::{tree_shapes, TreeKind};
use eda_freeroute::dump::read_board;
use eda_freeroute::geometry::{IntOctagon, TileShape};
use eda_freeroute::model::Board;

fn dumps() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = match std::env::var_os("FREEROUTE_MAZE_DIR").map(PathBuf::from) {
        // Tests run in the crate's directory; a relative path is taken from
        // the workspace root, where cargo is usually run.
        Some(d) if d.is_relative() => manifest.join("../..").join(d),
        Some(d) => d,
        None => manifest.join("tests/maze"),
    };
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("maze dumps in {}: {e}", dir.display()));
    let mut files: Vec<_> = entries.map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "txt")).collect();
    files.sort();
    assert!(!files.is_empty(), "no maze dumps in {}", dir.display());
    files
}

fn octagon_text(o: &IntOctagon) -> String {
    format!(
        "{} {} {} {} {} {} {} {}",
        o.left_x, o.bottom_y, o.right_x, o.top_y, o.upper_left_diag_x, o.lower_right_diag_x, o.lower_left_diag_x, o.upper_right_diag_x
    )
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

/// FreeRouting's tree shapes, by item and shape index: the autoroute tree's
/// exact shapes (an octagon, or what an `exact` record gives), and the
/// default tree's.
struct DumpedTrees {
    class: i32,
    autoroute: HashMap<(u32, usize), String>,
    default: HashMap<(u32, usize), String>,
}

fn dumped_trees(dump: &str) -> DumpedTrees {
    let mut t = DumpedTrees { class: 0, autoroute: HashMap::new(), default: HashMap::new() };
    for line in dump.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.first().copied().unwrap_or("") {
            "tree_class" => t.class = f[1].parse().unwrap(),
            "item" => {
                t.autoroute.insert((f[1].parse().unwrap(), f[2].parse().unwrap()), format!("octagon {}", f[5..13].join(" ")));
            }
            "exact" => {
                t.autoroute.insert((f[1].parse().unwrap(), f[2].parse().unwrap()), f[3..].join(" "));
            }
            "ditem" => {
                t.default.insert((f[1].parse().unwrap(), f[2].parse().unwrap()), f[4..].join(" "));
            }
            _ => {}
        }
    }
    t
}

/// Compare every item's shapes in both trees, as the port computes them
/// from the board, with FreeRouting's: the number of shapes compared, and
/// every difference.
fn check_trees(board: &Board, dump: &str) -> (usize, Vec<String>) {
    let want = dumped_trees(dump);
    let mut diffs = Vec::new();
    let mut compared = 0;
    for item in &board.items {
        for (kind, class, dumped) in [(TreeKind::FortyFive, want.class, &want.autoroute), (TreeKind::Plain, 0, &want.default)] {
            let shapes = tree_shapes(board, item, kind, class);
            for (i, shape) in shapes.iter().enumerate() {
                let got = match (kind, shape) {
                    // The autoroute dump skips shapes without a bounding
                    // octagon; the default one writes "none".
                    (TreeKind::FortyFive, None) => continue,
                    (_, None) => "none".to_string(),
                    (_, Some(s)) => shape_text(s),
                };
                compared += 1;
                let w = dumped.get(&(item.id, i));
                if w != Some(&got) {
                    diffs.push(format!("item {} shape {i} in the {kind:?} tree:\n    FreeRouting: {w:?}\n    port:        {got:?}", item.id));
                }
            }
            // A shape FreeRouting has that the port does not.
            let n = shapes.len();
            if let Some(extra) = dumped.keys().filter(|(id, i)| *id == item.id && *i >= n).min() {
                diffs.push(format!("item {} in the {kind:?} tree: FreeRouting has shape {}, the port {n} shapes", item.id, extra.1));
            }
        }
    }
    (compared, diffs)
}

#[test]
fn boards_read_and_tree_shapes_match_freerouting() {
    let (mut boards, mut shapes, mut failures) = (0, 0, Vec::new());
    for path in dumps() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let dump = std::fs::read_to_string(&path).unwrap();
        let board = match read_board(&dump) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        boards += 1;
        if !dump.lines().any(|l| l.starts_with("tree_class ")) {
            // Nothing to route, so FreeRouting built no autoroute tree.
            continue;
        }
        let (n, diffs) = check_trees(&board, &dump);
        shapes += n;
        if !diffs.is_empty() {
            let shown: Vec<_> = diffs.iter().take(3).cloned().collect();
            failures.push(format!("{name}: {} of {n} tree shapes differ, e.g.\n  {}", diffs.len(), shown.join("\n  ")));
        }
    }
    eprintln!("boards read: {boards}; tree shapes compared: {shapes}");
    assert!(failures.is_empty(), "{} boards differ from FreeRouting:\n{}", failures.len(), failures.join("\n"));
}

/// The dump's `ctrl` records, by name.
fn dumped_control(dump: &str) -> HashMap<String, Vec<String>> {
    let mut c: HashMap<String, Vec<String>> = HashMap::new();
    for line in dump.lines() {
        if let Some(rest) = line.strip_prefix("ctrl ") {
            let mut f = rest.split_whitespace();
            let name = f.next().unwrap().to_string();
            let values: Vec<String> = f.map(String::from).collect();
            if name == "via_mask" {
                c.entry(name).or_default().push(values.join(" "));
            } else {
                c.insert(name, values);
            }
        }
    }
    c
}

/// The net and item routed first, from the `route` record.
fn route(dump: &str) -> Option<(i32, u32)> {
    let line = dump.lines().find(|l| l.starts_with("route "))?;
    let f: Vec<&str> = line.split_whitespace().collect();
    if f[1] == "none" {
        return None;
    }
    Some((f[1].parse().unwrap(), f[2].parse().unwrap()))
}

#[test]
fn routing_controls_match_freerouting() {
    use eda_freeroute::autoroute::Control;
    let (mut checked, mut failures) = (0, Vec::new());
    for path in dumps() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let dump = std::fs::read_to_string(&path).unwrap();
        let Some((net, _)) = route(&dump) else { continue };
        let board = read_board(&dump).unwrap();
        let c = Control::for_batch(&board, net, 1);
        let want = dumped_control(&dump);
        let ints = |v: &[i64]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let floats = |v: &[f64]| v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>();
        let bools = |v: &[bool]| v.iter().map(|x| if *x { "1".to_string() } else { "0".to_string() }).collect::<Vec<_>>();
        let got: Vec<(&str, Vec<String>)> = vec![
            ("trace_half_width", ints(&c.trace_half_width)),
            ("compensated_trace_half_width", ints(&c.compensated_trace_half_width)),
            ("via_radius_arr", floats(&c.via_radius)),
            ("layer_active", bools(&c.layer_active)),
            ("trace_clearance_class_no", vec![c.trace_clearance_class.to_string()]),
            ("via_clearance_class", vec![c.via_clearance_class.to_string()]),
            ("min_normal_via_cost", floats(&[c.min_normal_via_cost])),
            ("min_cheap_via_cost", floats(&[c.min_cheap_via_cost])),
            ("max_via_radius", floats(&[c.max_via_radius])),
            ("attach_smd_allowed", bools(&[c.attach_smd_allowed])),
            ("vias_allowed", bools(&[c.vias_allowed])),
            ("with_neckdown", bools(&[c.with_neckdown])),
            ("via_lower_bound", vec![c.via_lower_bound.to_string()]),
            ("via_upper_bound", vec![c.via_upper_bound.to_string()]),
            ("max_shove_trace_recursion_depth", vec![c.max_shove_trace_recursion_depth.to_string()]),
            ("max_shove_via_recursion_depth", vec![c.max_shove_via_recursion_depth.to_string()]),
            ("max_spring_over_recursion_depth", vec![c.max_spring_over_recursion_depth.to_string()]),
            (
                "via_mask",
                c.via_masks.iter().map(|m| format!("{} {} {}", m.from_layer, m.to_layer, u8::from(m.attach_smd_allowed))).collect(),
            ),
        ];
        checked += 1;
        for (field, value) in got {
            // Java prints doubles its own way; compare them as numbers.
            let same = match want.get(field) {
                Some(w) => {
                    w.len() == value.len()
                        && w.iter().zip(&value).all(|(a, b)| a == b || a.parse::<f64>().ok().zip(b.parse::<f64>().ok()).is_some_and(|(x, y)| x == y))
                }
                None => false,
            };
            if !same {
                failures.push(format!("{name}: {field}: FreeRouting {:?}, port {value:?}", want.get(field)));
            }
        }
    }
    eprintln!("controls checked: {checked}");
    assert!(failures.is_empty(), "{} fields differ:\n{}", failures.len(), failures.join("\n"));
}

mod search {
    use super::*;
    use eda_freeroute::autoroute::engine::{Engine, Expandable};
    use eda_freeroute::autoroute::maze::{ListElement, MazeSearch};
    use eda_freeroute::autoroute::Control;
    use eda_freeroute::door::{RoomId, RoomState};

    /// A room as the dump names it.
    fn room(e: &Engine, r: Option<RoomId>) -> String {
        let Some(r) = r else { return "none".to_string() };
        let room = e.graph.room(r);
        match room.state {
            RoomState::Complete { id_no, .. } => format!("r{id_no}"),
            RoomState::Obstacle { item, .. } => {
                let p = e.graph.tree().payload(item);
                match p {
                    eda_freeroute::door::Entry::Item(t) => format!("o{}.{}", t.id, t.shape_index),
                    _ => "o?".to_string(),
                }
            }
            RoomState::Incomplete { .. } => "inc".to_string(),
        }
    }

    /// An expandable object as the dump names it.
    fn door(e: &Engine, d: Option<Expandable>) -> String {
        match d {
            None => "none".to_string(),
            Some(Expandable::Door(d)) => {
                let door = e.graph.door(d);
                format!("door {} {} {}", room(e, Some(door.first)), room(e, Some(door.second)), door.dimension)
            }
            Some(Expandable::Target(t)) => {
                let target = &e.targets[t];
                format!("target {} {} {}", e.board.items[target.item].id, target.tree_entry_no, room(e, Some(target.room)))
            }
            Some(Expandable::Drill(d)) => {
                let drill = &e.drills[d];
                format!("drill {} {} {} {}", drill.location.x, drill.location.y, drill.first_layer, drill.last_layer)
            }
            Some(Expandable::Page(p)) => {
                let b = e.pages.pages[p].shape;
                format!("page {} {} {} {}", b.ll.x, b.ll.y, b.ur.x, b.ur.y)
            }
        }
    }

    /// A step as the dump writes it, but with values in Rust's notation.
    fn step(e: &Engine, n: usize, el: &ListElement) -> String {
        let s = el.shape_entry;
        format!(
            "step {n} {} {} {:?} {:?} {} {} {} {:?} {:?} {:?} {:?} {} {}",
            door(e, Some(el.door)),
            el.section,
            el.sorting_value,
            el.expansion_value,
            door(e, el.backtrack_door),
            el.backtrack_section,
            room(e, el.next_room),
            s.a.x,
            s.a.y,
            s.b.x,
            s.b.y,
            u8::from(el.room_ripped),
            u8::from(el.already_checked)
        )
    }

    /// Whether two step lines agree, numbers compared as numbers.
    fn same(want: &str, got: &str) -> bool {
        let (w, g): (Vec<&str>, Vec<&str>) = (want.split_whitespace().collect(), got.split_whitespace().collect());
        w.len() == g.len()
            && w.iter().zip(&g).all(|(a, b)| a == b || matches!((a.parse::<f64>(), b.parse::<f64>()), (Ok(x), Ok(y)) if x.to_bits() == y.to_bits()))
    }

    /// Run the port's search for the dump's connection; the first line where
    /// it and FreeRouting differ, or the number of steps if they agree.
    pub fn replay(dump: &str) -> Option<Result<usize, String>> {
        let (net, _) = route(dump)?;
        let board = read_board(dump).unwrap();
        let ids = |key: &str| -> Vec<usize> {
            let line = dump.lines().find(|l| l.starts_with(key)).unwrap_or("");
            line.split_whitespace()
                .skip(1)
                .map(|s| {
                    let id: u32 = s.parse().unwrap();
                    board.items.iter().position(|i| i.id == id).unwrap()
                })
                .collect()
        };
        let (start, dest) = (ids("start_item"), ids("dest_item"));
        let ctrl = Control::for_batch(&board, net, 1);
        let mut engine = Engine::new(&board, net, ctrl.trace_clearance_class);
        let want: Vec<&str> = dump.lines().filter(|l| l.starts_with("step ") || l.starts_with("result ") || l.starts_with("path ") || l.starts_with("maze ")).collect();
        let Some(mut maze) = MazeSearch::new(&mut engine, &ctrl, &start, &dest) else {
            return Some(if want.first() == Some(&"maze none") { Ok(0) } else { Err(format!("the port found no start; FreeRouting: {:?}", want.first())) });
        };
        let mut got = Vec::new();
        let mut n = 0;
        loop {
            if let Some(e) = maze.peek() {
                let line = step(maze.engine, n, e);
                let w = want.get(got.len()).copied().unwrap_or("(nothing)");
                if !same(w, &line) {
                    let prev = if got.is_empty() { "(start)".to_string() } else { want[got.len() - 1].to_string() };
                    return Some(Err(format!("step {n} differs, after\n    {prev}\n  FreeRouting: {w}\n  port:        {line}")));
                }
                got.push(line);
                n += 1;
            }
            if !maze.occupy_next_element() {
                break;
            }
        }
        let found = maze.find_connection();
        let result = match found {
            None => "result none".to_string(),
            Some(f) => format!("result {} {}", door(maze.engine, Some(f.door)), f.section),
        };
        let w = want.get(got.len()).copied().unwrap_or("(nothing)");
        if !same(w, &result) {
            return Some(Err(format!("after {n} steps\n  FreeRouting: {w}\n  port:        {result}")));
        }
        got.push(result);
        if let Some(f) = found {
            let (mut curr, mut section) = (Some(f.door), f.section);
            while let Some(d) = curr {
                let line = format!("path {} {section}", door(maze.engine, Some(d)));
                let w = want.get(got.len()).copied().unwrap_or("(nothing)");
                if !same(w, &line) {
                    return Some(Err(format!("path differs\n  FreeRouting: {w}\n  port:        {line}")));
                }
                got.push(line);
                let el = maze.engine.element(d, section);
                (curr, section) = (el.backtrack_door, el.section_no_of_backtrack_door);
            }
        }
        Some(Ok(n))
    }
}

#[test]
fn maze_search_matches_freerouting() {
    let (mut searched, mut steps, mut failures) = (0, 0, Vec::new());
    for path in dumps() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let dump = std::fs::read_to_string(&path).unwrap();
        let outcome = std::panic::catch_unwind(|| search::replay(&dump));
        match outcome {
            Ok(None) => {}
            Ok(Some(Ok(n))) => {
                searched += 1;
                steps += n;
            }
            Ok(Some(Err(e))) => failures.push(format!("{name}: {e}")),
            Err(p) => {
                let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                failures.push(format!("{name}: stopped: {msg}"));
            }
        }
    }
    eprintln!("searches matched: {searched} ({steps} steps)");
    assert!(failures.is_empty(), "{} searches differ from FreeRouting:\n{}", failures.len(), failures.join("\n"));
}

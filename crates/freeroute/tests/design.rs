//! The board built from a design against the one FreeRouting reads from the
//! DSN written for it: `tests/design/<name>/` holds the design, its intent
//! and FreeRouting's dump of that DSN (see `parity/design_dump.sh`).

use std::path::Path;

use eda_freeroute::design::board_from_design;
use eda_freeroute::dump::read_board;
use eda_freeroute::model::Board;

/// The first field the two boards differ in, or `None`.
fn first_difference(port: &Board, java: &Board) -> Option<String> {
    macro_rules! check {
        ($($field:tt)+) => {
            if port.$($field)+ != java.$($field)+ {
                return Some(format!("{}:\n  port {:?}\n  java {:?}", stringify!($($field)+), port.$($field)+, java.$($field)+));
            }
        };
    }
    check!(bounds);
    check!(layers);
    check!(host_cad);
    check!(area_section);
    check!(id_max);
    check!(rules.clearance);
    check!(rules.max_clearance);
    check!(rules.via_infos);
    check!(rules.via_rules);
    check!(rules.net_classes);
    check!(rules.nets);
    check!(rules.min_trace_half_width);
    check!(rules.default_via_diameter);
    check!(rules.pin_edge_to_turn_dist);
    check!(rules.board_max_trace_half_width);
    check!(rules.max_trace_half_width);
    check!(rules.pull_tight_accuracy);
    check!(settings);
    if port.rules.padstacks.len() != java.rules.padstacks.len() {
        return Some(format!("{} padstacks, FreeRouting {}", port.rules.padstacks.len(), java.rules.padstacks.len()));
    }
    for (a, b) in port.rules.padstacks.iter().zip(&java.rules.padstacks) {
        if a != b {
            return Some(format!("padstack {}:\n  port {a:?}\n  java {b:?}", b.no));
        }
    }
    if port.items.len() != java.items.len() {
        return Some(format!("{} items, FreeRouting {}", port.items.len(), java.items.len()));
    }
    for (a, b) in port.items.iter().zip(&java.items) {
        if a != b {
            return Some(format!("item {}:\n  port {a:?}\n  java {b:?}", b.id));
        }
    }
    (port != java).then(|| "the boards differ elsewhere".to_string())
}

#[test]
fn boards_from_designs_match_freerouting() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/design");
    let mut names: Vec<_> = std::fs::read_dir(&dir).expect("tests/design").map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    names.sort();
    assert!(!names.is_empty(), "no boards in {}", dir.display());
    let mut failed = Vec::new();
    for case in &names {
        let name = case.file_name().unwrap().to_string_lossy().to_string();
        let design = serde_json::from_str(&std::fs::read_to_string(case.join("design.json")).unwrap()).unwrap();
        let model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(case.join("intent.yaml")).unwrap()).unwrap();
        let java = read_board(&std::fs::read_to_string(case.join("board.txt")).unwrap()).unwrap();
        let port = board_from_design(&design, &model, &model.board).unwrap();
        match first_difference(&port, &java) {
            None => println!("{name}: {} items match", java.items.len()),
            Some(d) => {
                println!("{name}: {d}");
                failed.push(name);
            }
        }
    }
    assert!(failed.is_empty(), "boards differ: {failed:?}");
}

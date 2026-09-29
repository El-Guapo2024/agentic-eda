//! Route a placed design with the port, and write it back routed:
//!
//!   cargo run --release -p eda-freeroute --example route_design -- design.json intent.yaml [out.json] [max passes]
//!
//! Prints what each pass did and what is left unrouted. With
//! `NO_EXIT_RESTRICTIONS` set, traces may leave pins any way.

use std::time::Instant;

use eda_freeroute::autoroute::batch::autoroute_passes;
use eda_freeroute::design::{board_from_design, routing_section};
use eda_freeroute::routing::RoutingBoard;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: route_design <design.json> <intent.yaml> [out.json] [max passes]");
        std::process::exit(2);
    }
    let mut design: eda_model::ir::Design = serde_json::from_str(&std::fs::read_to_string(&args[1]).expect("read the design")).expect("parse the design");
    let model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&args[2]).expect("read the intent")).expect("parse the intent");
    let max_passes = args.get(4).map_or(20, |a| a.parse().expect("a number of passes"));
    let start = Instant::now();
    let mut board = match board_from_design(&design, &model, &model.board) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if std::env::var_os("NO_EXIT_RESTRICTIONS").is_some() {
        board.rules.pin_edge_to_turn_dist = -1.0;
    }
    let mut rb = RoutingBoard::new(board);
    for p in autoroute_passes(&mut rb, 1, max_passes) {
        println!("pass {}: {} items, {} routed, {} not routed", p.pass_no, p.items, p.routed, p.not_routed);
    }
    let routing = routing_section(&design, &model, &model.board, &rb).expect("write back");
    println!("{} tracks, {} vias in {:.2?}", routing.tracks.len(), routing.vias.len(), start.elapsed());
    if let Some(out) = args.get(3) {
        design.routing = Some(routing);
        std::fs::write(out, serde_json::to_string_pretty(&design).expect("serialise")).expect("write the design");
    }
}

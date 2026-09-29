//! Route a placed design and optimize it, timing each optimizer pass:
//!
//!   cargo run --release -p eda-freeroute --example optimize_design -- design.json intent.yaml [max optimizer passes]

use std::time::Instant;

use eda_freeroute::autoroute::optimize::{cumulative_trace_length, route_attempt, via_count, Optimizer, IMPROVEMENT_THRESHOLD};
use eda_freeroute::design::board_from_design;
use eda_freeroute::routing::RoutingBoard;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let design: eda_model::ir::Design = serde_json::from_str(&std::fs::read_to_string(&args[1]).expect("read the design")).expect("parse the design");
    let model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&args[2]).expect("read the intent")).expect("parse the intent");
    let max_passes = args.get(3).map_or(i32::MAX, |a| a.parse().expect("a number of passes"));
    let mut rb = RoutingBoard::new(board_from_design(&design, &model, &model.board).expect("a board"));
    let t = Instant::now();
    let passes = eda_freeroute::autoroute::batch::autoroute_passes(&mut rb, 1, 20);
    println!("routed in {:.2?} ({} passes): {} vias, {:.1} mm", t.elapsed(), passes.len(), via_count(&rb), cumulative_trace_length(&rb) / 10_000.0);
    let mut opt = Optimizer::default();
    let mut improved_total: f64 = -1.0;
    let mut pass_no = 0;
    while (improved_total >= IMPROVEMENT_THRESHOLD as f64 || improved_total < 0.0) && pass_no < max_passes {
        pass_no += 1;
        let t = Instant::now();
        let mut pass = opt.begin_pass(&rb, pass_no);
        let (mut attempts, mut kept) = (0, 0);
        while let Some(item) = pass.items.next(&rb) {
            let Some(attempt) = opt.begin_item(&mut rb, item, &pass) else { continue };
            route_attempt(&mut rb, &attempt);
            attempts += 1;
            if opt.finish_item(&mut rb, &attempt, &mut pass).improved {
                kept += 1;
            }
        }
        improved_total = opt.end_pass(&pass) as f64;
        println!(
            "optimizer pass {pass_no}: {kept} of {attempts} kept in {:.2?}: {} vias, {:.1} mm (improved {improved_total:.5})",
            t.elapsed(),
            via_count(&rb),
            cumulative_trace_length(&rb) / 10_000.0
        );
    }
}

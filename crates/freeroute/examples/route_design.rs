//! Route a placed design with the port, and write it back routed:
//!
//!   cargo run --release -p eda-freeroute --example route_design -- design.json intent.yaml [out.json] [max passes]
//!
//! Prints what each pass did and the nets left unrouted.

use std::time::Instant;

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
    let routed = match eda_freeroute::design::route_design(&design, &model, &model.board, max_passes) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    for p in &routed.passes {
        println!("pass {}: {} items, {} routed, {} not routed", p.pass_no, p.items, p.routed, p.not_routed);
    }
    println!("{} tracks, {} vias in {:.2?}; unrouted: {:?}", routed.routing.tracks.len(), routed.routing.vias.len(), start.elapsed(), routed.unrouted);
    if let Some(out) = args.get(3) {
        design.routing = Some(routed.routing);
        std::fs::write(out, serde_json::to_string_pretty(&design).expect("serialise")).expect("write the design");
    }
}

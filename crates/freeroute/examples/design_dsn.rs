//! Write a placed design as the Specctra DSN FreeRouting reads, to ask
//! FreeRouting what board it makes of it:
//!
//!   cargo run --release -p eda-freeroute --example design_dsn -- design.json intent.yaml > board.dsn

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, design, intent] = &args[..] else {
        eprintln!("usage: design_dsn <design.json> <intent.yaml>");
        std::process::exit(2);
    };
    let design: eda_model::ir::Design = serde_json::from_str(&std::fs::read_to_string(design).expect("read the design")).expect("parse the design");
    let model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(intent).expect("read the intent")).expect("parse the intent");
    match eda_freeroute::design::to_dsn(&design, &model, &model.board) {
        Ok(dsn) => print!("{dsn}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

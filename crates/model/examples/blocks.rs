//! Show the functional blocks an intent implies.
//!
//! Partitioning a board into blocks -- power, RF, analog, the MCU and
//! its support -- and placing those as units before placing parts inside
//! them is ordinary layout practice. `floorplan::partition` already
//! derives them from the proximity rules and the netlist; this prints
//! what it finds, so a board can be judged on whether its blocks look
//! like the circuit before anything is placed.
//!
//! `cargo run -p eda-model --example blocks -- <intent.yaml>`
fn main() {
    let path = std::env::args().nth(1).expect("usage: blocks <intent.yaml>");
    let model: eda_model::ConstraintModel =
        serde_yaml::from_str(&std::fs::read_to_string(&path).expect("intent is readable")).expect("intent parses");

    let (modules, free) = eda_model::floorplan::partition(&model);
    println!("{}: {} parts -> {} block(s), {} unassigned", path, model.parts.len(), modules.len(), free.len());
    let mut in_blocks = 0;
    for m in &modules {
        in_blocks += m.refs.len();
        println!("\n  {} ({} parts)", m.name, m.refs.len());
        println!("    {}", m.refs.join(" "));
    }
    if !free.is_empty() {
        println!("\n  unassigned ({})", free.len());
        println!("    {}", free.join(" "));
    }
    println!(
        "\n{} of {} parts are in a block ({:.0}%)",
        in_blocks,
        model.parts.len(),
        100.0 * in_blocks as f64 / model.parts.len().max(1) as f64
    );
}

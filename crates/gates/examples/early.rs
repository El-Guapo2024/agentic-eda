//! Replay a finished placement one part at a time and report the step at
//! which the partial gates first see trouble.
//!
//! The point of the partial gates is early feedback: a constructive
//! placer should hear about a bad choice at part 12, not after the whole
//! board is laid out and re-annealed. This measures how early "early"
//! actually is on a real board.
//!
//! `cargo run -p eda-gates --example early -- <intent.yaml> <design.json>`
use eda_model::ir::{Design, FootprintInstance};
use eda_model::{CheckStatus, ConstraintModel};

fn main() {
    let mut a = std::env::args().skip(1);
    let (intent, design) = match (a.next(), a.next()) {
        (Some(i), Some(d)) => (i, d),
        _ => {
            eprintln!("usage: early <intent.yaml> <design.json>");
            std::process::exit(2);
        }
    };
    let model: ConstraintModel =
        serde_yaml::from_str(&std::fs::read_to_string(&intent).expect("intent is readable")).expect("intent parses");
    let full: Design =
        serde_json::from_str(&std::fs::read_to_string(&design).expect("design is readable")).expect("design parses");

    let pl = full.placement.as_ref().expect("design has a placement");
    let all: Vec<FootprintInstance> = pl.footprints.clone();

    // Honest about the order: `design.json` sorts footprints by refdes,
    // so this replays alphabetically, not in the order a placer would
    // choose. The step number therefore says when a gate *could* have
    // fired given what was on the board, not how early a constructive
    // placer would hear about it -- for that the placer has to drive
    // this, in dependency order. What the replay does prove is that a
    // gate fires as soon as its subject exists rather than only at the
    // end.
    println!("replaying {} parts from {design} (alphabetical order, not placement order)", all.len());
    let mut first_fail: Option<usize> = None;
    for n in 1..=all.len() {
        let mut d = full.clone();
        d.placement.as_mut().unwrap().footprints = all[..n].to_vec();
        let checks = eda_gates::check_placement_partial(&d, &model);
        let fails: Vec<_> = checks.iter().filter(|c| matches!(c.status, CheckStatus::Fail)).collect();
        if !fails.is_empty() && first_fail.is_none() {
            first_fail = Some(n);
            println!("\nfirst failure at part {n} of {} ({})", all.len(), all[n - 1].id);
            for f in fails.iter().take(4) {
                println!("  {} @ {}: {}", f.check, f.location.as_deref().unwrap_or("-"), f.hint.as_deref().unwrap_or(""));
            }
        }
    }
    // And the verdict the old way: only at the end.
    let end: Vec<_> = eda_gates::check_placement(&full, &model)
        .into_iter()
        .filter(|c| matches!(c.status, CheckStatus::Fail))
        .collect();
    println!("\nfinished-board gate: {} failure(s)", end.len());
    for f in end.iter().take(4) {
        println!("  {} @ {}: {}", f.check, f.location.as_deref().unwrap_or("-"), f.hint.as_deref().unwrap_or(""));
    }
    match first_fail {
        Some(n) => println!(
            "\nfired once {} of {} parts were present (alphabetical); a constructive placer in dependency order would reach this sooner",
            n,
            all.len()
        ),
        None => println!("\nno partial failure at any step"),
    }
}

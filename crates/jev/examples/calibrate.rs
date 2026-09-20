//! Run the standing calibration quiz against the live service.
//!
//! `AI_GATEWAY_API_KEY=... cargo run -p eda-jev --example calibrate`
//!
//! Exits non-zero if the model's layout judgement has moved, which is
//! the same verdict the gate reaches inside a real run.
fn main() {
    if !eda_jev::available() {
        eprintln!("no AI_GATEWAY_API_KEY set; nothing to calibrate");
        std::process::exit(2);
    }
    let t0 = std::time::Instant::now();
    let checks = eda_jev::check_calibration();
    let dt = t0.elapsed();

    let fails: Vec<_> = checks.iter().filter(|c| matches!(c.status, eda_model::CheckStatus::Fail)).collect();
    for c in &fails {
        println!("FAIL {} @ {}: {}", c.check, c.location.as_deref().unwrap_or("-"), c.hint.as_deref().unwrap_or("-"));
    }
    println!("jev calibration: {} checks, {} fail, {:.0} ms", checks.len(), fails.len(), dt.as_secs_f64() * 1000.0);
    if !fails.is_empty() {
        std::process::exit(1);
    }
}

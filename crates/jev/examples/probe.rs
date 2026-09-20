//! Live check that the crate can still talk to the gateway.
//!
//! The wire format moved under us once already -- `choice` stopped
//! accepting an `options` array and started requiring a `criteria`
//! record -- and the unit tests only prove we *serialise* what we think
//! we serialise. This actually calls the endpoint.
//!
//! `AI_GATEWAY_API_KEY=... cargo run -p eda-jev --example probe`
fn main() {
    let mut qs = std::collections::BTreeMap::new();
    qs.insert(
        "nearest".to_string(),
        eda_jev::Question::Choice {
            instructions: "Which part must sit closest to the buck IC to minimise the high-di/dt loop?".into(),
            criteria: [
                ("input_cap".to_string(), "the input ceramic capacitor".to_string()),
                ("fb_divider".to_string(), "the feedback divider".to_string()),
            ]
            .into_iter()
            .collect(),
        },
    );
    qs.insert(
        "routable".to_string(),
        eda_jev::Question::Score {
            instructions: "How routable does a board with these features look?".into(),
            criteria: vec!["hard".into(), "moderate".into(), "easy".into()],
        },
    );
    let state = serde_json::json!({ "topology": "synchronous buck", "iout_a": 3, "parts": 12, "board_mm2": 900 });
    match eda_jev::ask(&state, &qs) {
        Ok(a) => {
            for (k, v) in &a {
                println!("{k}: choice={:?} score={:?} p={:?}", v.choice, v.score, v.probabilities);
            }
            // The buck answer is not a matter of opinion; if the service
            // stops getting it right the caller should know.
            assert_eq!(a["nearest"].choice.as_deref(), Some("input_cap"), "Jev missed a textbook placement question");
        }
        Err(e) => {
            for c in e {
                println!("FAIL {} @ {}: {}", c.check, c.location.as_deref().unwrap_or("-"), c.hint.as_deref().unwrap_or("-"));
            }
            std::process::exit(1);
        }
    }
}

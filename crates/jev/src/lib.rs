//! Typed decisions from TypeSafe's Jev, via the Vercel AI Gateway.
//!
//! **What this is for, and what it must never become.**
//!
//! Jev answers typed questions about a state and returns calibrated
//! probabilities instead of prose. That makes it useful here for one
//! thing: deciding *what to spend time on*. Routing L4 costs about seven
//! minutes, and roughly half of our placements fail their gates -- so
//! knowing which candidate to route first is worth real wall-clock.
//!
//! It ranks. It does not approve. A placement Jev scores at 0.99 is still
//! routed and still gated, and the gates still decide. The moment a
//! prediction can pass a board, a hard gate has been replaced by a guess,
//! and a fleet of agents optimising against that will find the hole long
//! before a human notices. Calibrated confidence is an argument for
//! trusting the *ordering*, never for skipping the check.
//!
//! **Why not in the inner loop.** End-to-end latency is 70-500 ms. The
//! annealer evaluates millions of moves; its cost function has to be
//! nanoseconds. Jev belongs at the decision points between stages -- which
//! candidate, which strategy rung, which part -- not inside a search.
//!
//! No key means no prediction, and no prediction is not an error: callers
//! fall back to doing the work in their existing order, which is exactly
//! what they did before this crate existed. What is *not* allowed is
//! inventing a score when the service cannot be reached, so a transport
//! failure returns `Err` and never a default.

use eda_model::CheckResult;
use serde::{Deserialize, Serialize};

const GATEWAY_URL: &str = "https://ai-gateway.vercel.sh/v1/systemone";
const MODEL: &str = "typesafe-ai/jev";

/// One typed question about the state.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Boolean: returns the probability the answer is yes.
    Bool { id: String, prompt: String },
    /// Pick one of a fixed set; returns per-option probabilities.
    Choice { id: String, prompt: String, options: Vec<String> },
    /// Rate against ordered descriptive levels.
    Score { id: String, prompt: String, levels: Vec<String> },
}

#[derive(Debug, Clone, Deserialize)]
pub struct Answer {
    pub id: String,
    #[serde(default)]
    pub probability: Option<f64>,
    #[serde(default)]
    pub choice: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// The gateway key, or `None` when unset.
///
/// Read fresh each call rather than cached: a long-lived fleet process
/// should pick up a rotated key without a restart.
pub fn api_key() -> Option<String> {
    for var in ["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_KEY", "JEV_API_KEY"] {
        if let Ok(k) = std::env::var(var) {
            if !k.trim().is_empty() {
                return Some(k);
            }
        }
    }
    None
}

pub fn available() -> bool {
    api_key().is_some()
}

/// Ask Jev a batch of typed questions about one state.
///
/// Every question goes in a single request -- that is the model's whole
/// shape, and asking them one at a time would pay the round trip N times
/// for no benefit.
pub fn ask(state: &serde_json::Value, questions: &[Question]) -> Result<Vec<Answer>, Vec<CheckResult>> {
    let key = api_key().ok_or_else(|| {
        vec![CheckResult::fail(
            "jev_no_key",
            "AI_GATEWAY_API_KEY",
            "no Vercel AI Gateway key is set, so no typed decision can be requested. \
             Export AI_GATEWAY_API_KEY (a vck_... key) or let the caller proceed in its default order.",
        )]
    })?;
    let body = serde_json::json!({
        "model": MODEL,
        "state": state,
        "questions": questions,
        // The state carries a whole board's geometry; it is the user's
        // design and has no business training anybody's model.
        "zero_data_retention": true,
        "no_training": true,
    });
    let resp = ureq::post(GATEWAY_URL)
        .set("Authorization", &format!("Bearer {key}"))
        .set("Content-Type", "application/json")
        .send_json(body)
        .map_err(|e| vec![CheckResult::fail("jev_transport", MODEL, format!("Jev request failed: {e}"))])?;
    let parsed: serde_json::Value = resp
        .into_json()
        .map_err(|e| vec![CheckResult::fail("jev_decode", MODEL, format!("Jev returned unreadable JSON: {e}"))])?;
    let answers = parsed.get("answers").or_else(|| parsed.get("results")).cloned().unwrap_or(parsed);
    serde_json::from_value(answers)
        .map_err(|e| vec![CheckResult::fail("jev_shape", MODEL, format!("Jev answers did not match the expected shape: {e}"))])
}

/// Probability that a placement with these features clears every routing
/// gate, used only to order candidates.
///
/// Returns `Ok(None)` when no key is configured: not having an opinion is
/// a normal state, and the caller routes in its existing order. A
/// *failure to reach* a configured service is an `Err`, because silently
/// scoring 0.5 would be indistinguishable from a real answer.
pub fn rank_placement(features: &serde_json::Value) -> Result<Option<f64>, Vec<CheckResult>> {
    if !available() {
        return Ok(None);
    }
    let qs = vec![Question::Bool {
        id: "routes_clean".into(),
        prompt: "This is a PCB placement, described by its geometry. Will an autorouter connect every \
                 net on it without clearance violations or unroutable nets? Denser boards, tighter \
                 courtyard gaps and nets stretched far past their packed bound make this less likely."
            .into(),
    }];
    let answers = ask(features, &qs)?;
    Ok(answers.iter().find(|a| a.id == "routes_clean").and_then(|a| a.probability))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_key_means_no_opinion_not_an_error() {
        // A missing key is an ordinary state: the caller keeps its own
        // ordering. Only a *configured* service failing is an error.
        if api_key().is_some() {
            return; // a real key is set in this environment; nothing to assert
        }
        assert!(matches!(rank_placement(&serde_json::json!({})), Ok(None)));
    }

    #[test]
    fn questions_serialise_in_the_shape_the_api_expects() {
        let q = Question::Bool { id: "x".into(), prompt: "y".into() };
        let v = serde_json::to_value(&q).unwrap();
        assert_eq!(v["type"], "bool");
        assert_eq!(v["id"], "x");
    }

    #[test]
    fn a_choice_question_carries_its_options() {
        let q = Question::Choice { id: "rung".into(), prompt: "which".into(), options: vec!["a".into(), "b".into()] };
        let v = serde_json::to_value(&q).unwrap();
        assert_eq!(v["type"], "choice");
        assert_eq!(v["options"][1], "b");
    }
}

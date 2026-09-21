//! A chooser that asks a fast general model which part goes next.
//!
//! Same question as [`crate::ai`], different service. Jev is a typed
//! evaluation model reached through its own endpoint, and in this loop
//! it was a poor trade: every attempt but one died on a 429, the one
//! that finished scored *worse* than sorting (1 gate failure on L1 where
//! `Greedy` scored 0), and a calibration gate meant a gateway hiccup
//! stopped board work entirely.
//!
//! A flash-class chat model is the cheaper shape of the same idea. It is
//! asked once, in bulk, for how placement-critical each part is, and the
//! scores are reused for the whole build -- so a hundred-part board pays
//! one round trip, not one per part, and the build stays reproducible.
//! A remote service consulted afresh at every step would make every
//! measurement taken on this placer noise.
//!
//! What it is asked follows what these models are good at. It sees what
//! the circuit *is* -- that U5 is a buck converter, that this net is the
//! RF supply. It is never shown a coordinate and never asked to compare
//! two distances: on geometry Jev scored 12 of 23 with confidence that
//! went *up* when it was wrong, and there is no reason to expect a chat
//! model to do better at a job our gates already do exactly.
//!
//! Every answer is still checked. The model proposes an order; the gates
//! dispose of the result. A choice that will not apply is discarded and
//! the next one tried, so a wrong answer costs a step, never a board.

use crate::build::Chooser;
use crate::Board;
use eda_model::{CheckResult, ConstraintModel};
use std::collections::BTreeMap;

/// Where the gateway speaks OpenAI-compatible chat.
const ENDPOINT: &str = "https://ai-gateway.vercel.sh/v1/chat/completions";

/// The model to ask. Overridable so a different flash-class model can be
/// tried without a rebuild; the default is the cheap fast one.
const DEFAULT_MODEL: &str = "google/gemini-2.5-flash";

/// Asks a fast chat model how placement-critical each part is, once.
pub struct Flash {
    /// refdes -> criticality, higher first.
    priority: BTreeMap<String, f64>,
    /// Requests actually sent.
    pub requests: usize,
    /// Parts scored.
    pub scored: usize,
    model: String,
}

impl Default for Flash {
    fn default() -> Self {
        Self::new()
    }
}

impl Flash {
    pub fn new() -> Self {
        Flash {
            priority: BTreeMap::new(),
            requests: 0,
            scored: 0,
            model: std::env::var("EDA_FLASH_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string()),
        }
    }

    /// The key, or a failure. No key is a failure here rather than a
    /// silent fall back to refdes order: a run asked for this chooser,
    /// and quietly giving it a different one would make the comparison
    /// this chooser exists to support meaningless.
    fn key() -> Result<String, Vec<CheckResult>> {
        for var in ["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_KEY", "JEV_API_KEY"] {
            if let Ok(k) = std::env::var(var) {
                if !k.trim().is_empty() {
                    return Ok(k);
                }
            }
        }
        Err(vec![CheckResult::fail(
            "flash_no_key",
            "AI_GATEWAY_API_KEY",
            "no AI gateway key is set, so the flash chooser cannot order parts. \
             Export AI_GATEWAY_API_KEY (a vck_... key), or use --placer build.",
        )])
    }

    /// Score every part in the model, in one request.
    ///
    /// A failure here fails the run. Continuing unscored would silently
    /// fall back to whatever order the refdes happen to sort in, which
    /// is the behaviour this chooser exists to replace.
    fn score_all(&mut self, model: &ConstraintModel) -> Result<(), Vec<CheckResult>> {
        let key = Self::key()?;
        let mut lines = Vec::new();
        for p in &model.parts {
            let nets: Vec<&str> = model
                .nets
                .iter()
                .filter(|n| n.pins.iter().any(|pin| pin.split('.').next() == Some(p.reference.as_str())))
                .map(|n| n.name.as_str())
                .collect();
            lines.push(format!("{} | nets: {}", describe(p), if nets.is_empty() { "none".into() } else { nets.join(", ") }));
        }
        let prompt = format!(
            "You are laying out a printed circuit board with {} components.\n\n\
             For each component below, rate from 0 to 10 how much it matters exactly where it is \
             placed. 10 means the circuit misbehaves if it is placed badly; 0 means it can go \
             almost anywhere. Decoupling and input capacitors, crystals, voltage references, \
             current-sense parts and high-current paths are placement-critical. Pull-ups, \
             indicator resistors and test points are not.\n\n\
             Components:\n{}\n\n\
             Reply with ONLY a JSON object mapping each reference designator to its number, \
             like {{\"C1\": 9.5, \"R4\": 1}}. Every component must appear. No other text.",
            model.parts.len(),
            lines.join("\n")
        );

        let body = serde_json::json!({
            "model": self.model,
            // Deterministic ordering matters more here than variety: a
            // placer that answers differently each run cannot be measured.
            "temperature": 0,
            "messages": [{ "role": "user", "content": prompt }],
        });

        let resp = ureq::post(ENDPOINT)
            .set("Authorization", &format!("Bearer {key}"))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(describe_failure)?;
        self.requests += 1;

        let parsed: serde_json::Value = resp
            .into_json()
            .map_err(|e| vec![CheckResult::fail("flash_decode", &self.model, format!("unreadable JSON from the gateway: {e}"))])?;
        let text = parsed["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| {
                vec![CheckResult::fail("flash_shape", &self.model, format!("no message content in the reply: {parsed}"))]
            })?
            .to_string();

        let scores = parse_scores(&text).ok_or_else(|| {
            vec![CheckResult::fail(
                "flash_shape",
                &self.model,
                format!("the reply was not a JSON object of scores: {}", text.chars().take(300).collect::<String>()),
            )]
        })?;

        for p in &model.parts {
            let s = scores.get(&p.reference).copied().ok_or_else(|| {
                vec![CheckResult::fail(
                    "flash_no_score",
                    &p.reference,
                    "the model returned no criticality for this part, so its placement order is unknown",
                )]
            })?;
            self.priority.insert(p.reference.clone(), s);
            self.scored += 1;
        }
        Ok(())
    }
}

/// Pull the JSON object out of a reply.
///
/// Chat models wrap JSON in prose or a fenced block often enough that
/// refusing those outright would fail runs over formatting rather than
/// judgement. The span between the first `{` and last `}` is parsed;
/// anything that is not then a flat object of numbers is still a
/// failure, not a guess.
fn parse_scores(text: &str) -> Option<BTreeMap<String, f64>> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let v: serde_json::Value = serde_json::from_str(text.get(start..=end)?).ok()?;
    let obj = v.as_object()?;
    let mut out = BTreeMap::new();
    for (k, val) in obj {
        out.insert(k.clone(), val.as_f64()?);
    }
    Some(out)
}

/// Say why the gateway refused, not just that it did.
///
/// The same distinction the jev crate needed: 429 is a rate limit and
/// waiting fixes it, 402 is an exhausted balance and waiting does not.
fn describe_failure(e: ureq::Error) -> Vec<CheckResult> {
    let resp = match e {
        ureq::Error::Status(_, resp) => resp,
        ureq::Error::Transport(t) => {
            return vec![CheckResult::fail("flash_transport", "gateway", format!("could not reach the gateway: {t}"))]
        }
    };
    let status = resp.status();
    let hints: Vec<String> = ["retry-after", "x-ratelimit-remaining", "x-ratelimit-reset"]
        .iter()
        .filter_map(|h| resp.header(h).map(|v| format!("{h}: {v}")))
        .collect();
    let body = resp.into_string().unwrap_or_default();
    let mut text = format!("the gateway refused with HTTP {status}: {}", body.chars().take(400).collect::<String>());
    if !hints.is_empty() {
        text.push_str(&format!(" [{}]", hints.join(", ")));
    }
    let check = match status {
        429 => "flash_rate_limit",
        401 | 403 => "flash_auth",
        402 => "flash_credit",
        _ => "flash_transport",
    };
    vec![CheckResult::fail(check, "gateway", text)]
}

/// What the part is, in words, for the model to reason about.
fn describe(p: &eda_model::Part) -> String {
    let mut bits: Vec<String> = Vec::new();
    if let Some(v) = &p.value {
        bits.push(v.clone());
    }
    if let Some(m) = &p.mpn {
        bits.push(m.clone());
    }
    if let Some(k) = &p.package {
        bits.push(k.clone());
    }
    if bits.is_empty() {
        p.reference.clone()
    } else {
        format!("{} ({})", p.reference, bits.join(", "))
    }
}

impl Chooser for Flash {
    fn name(&self) -> &'static str {
        "flash"
    }

    /// Take the most placement-critical part that can actually go down.
    fn choose_part(&mut self, board: &Board, frontier: &[String]) -> Result<Option<String>, Vec<CheckResult>> {
        if self.priority.is_empty() {
            self.score_all(board.model())?;
        }
        let mut viable: Vec<&String> =
            frontier.iter().filter(|p| crate::build::best_pose(board, p).is_some()).collect();
        if viable.is_empty() {
            return Ok(None);
        }
        // Most critical first; refdes breaks ties so a build is the same
        // every run.
        viable.sort_by(|a, b| {
            let pa = self.priority.get(*a).copied().unwrap_or(0.0);
            let pb = self.priority.get(*b).copied().unwrap_or(0.0);
            pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.cmp(b))
        });
        Ok(Some(viable[0].clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_scores;

    #[test]
    fn a_bare_object_parses() {
        let m = parse_scores(r#"{"C1": 9.5, "R4": 1}"#).expect("parses");
        assert_eq!(m["C1"], 9.5);
        assert_eq!(m["R4"], 1.0);
    }

    /// Chat models fence their JSON often enough that refusing would
    /// fail runs over formatting rather than judgement.
    #[test]
    fn a_fenced_object_parses() {
        let m = parse_scores("here you go:\n```json\n{\"C1\": 3}\n```\n").expect("parses");
        assert_eq!(m["C1"], 3.0);
    }

    /// Anything that is not a flat object of numbers is a failure, never
    /// a guess -- a silently dropped part would reorder the board.
    #[test]
    fn a_non_numeric_value_fails_rather_than_being_skipped() {
        assert!(parse_scores(r#"{"C1": "high"}"#).is_none());
        assert!(parse_scores("no json here").is_none());
    }
}

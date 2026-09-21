//! A chooser that asks the evaluation model which part goes next.
//!
//! One question, and only one: of the parts that could go down now,
//! which one should. Where it then lands is the placer's business.
//!
//! # Why the questions are batched
//!
//! Asked once per step, this made one network round trip for every part
//! on the board -- eighty-five on L4 -- and the gateway answered 429
//! before even a seventeen-part board finished. Sequential round trips
//! also cannot be parallelised inside a single board, so the placer ran
//! at the speed of the network rather than the speed of the machine.
//!
//! What is actually being asked, though, barely depends on the board.
//! "How much does it matter where this part goes" is a fact about the
//! part and the circuit: a buck converter's input capacitor is
//! placement-critical at step three and still placement-critical at step
//! sixty. So every part is scored **once**, in bulk, and the scores are
//! reused for the whole build.
//!
//! The placement itself is unchanged: still one part per step, still the
//! gates after every step. Only the asking is batched, never the
//! placing.
//!
//! What it is asked, and what it is never asked, follows what it was
//! measured to be good at. It sees what the circuit *is* -- that U5 is a
//! buck converter, that this net is the RF supply. It is never shown a
//! coordinate and never asked to compare two distances, because on that
//! it scored 12 of 23 with confidence that went *up* when it was
//! wrong.
//!
//! Given circuit context instead, on the same six candidate parts with
//! only the description of the IC changed, it picked the input capacitor
//! for a buck, the reference capacitor for a precision ADC, the RF decap
//! for a transceiver and the crystal for an oscillator: 4 of 4 at 0.96.
//! That is judgement our gates cannot express, and it is the whole
//! reason to pay for a network round trip in a placement loop.
//!
//! Every answer is still checked. The model proposes; the gates dispose.
//! A choice that will not apply is discarded and the next one tried, so
//! a wrong answer costs a step, never a broken board.

use crate::build::Chooser;
use crate::Board;
use eda_model::{CheckResult, ConstraintModel};
use std::collections::BTreeMap;

/// How urgently a part's position matters, worst-to-best as the model
/// sees them. The answer comes back as an expected index over these, so
/// a higher score means more critical.
const LEVELS: [&str; 4] = [
    "its exact position hardly matters; it can go almost anywhere",
    "its position matters a little, for tidiness or short traces",
    "its position matters to how well the circuit performs",
    "its position is critical; the circuit misbehaves if it is placed badly",
];

/// How many parts go in one request.
///
/// The gateway took eighteen questions in a single call during
/// calibration without complaint. Twenty keeps a hundred-part board to
/// five requests instead of eighty-five.
const BATCH: usize = 20;

/// Asks Jev how placement-critical each part is, once, in bulk.
pub struct Ai {
    /// refdes -> criticality, higher first. Populated on the first
    /// request and reused for the whole build, which is also what makes
    /// a build reproducible: a remote service consulted afresh at every
    /// step would make every measurement taken on this placer noise.
    priority: BTreeMap<String, f64>,
    /// Requests actually sent.
    pub requests: usize,
    /// Parts scored.
    pub scored: usize,
}

impl Default for Ai {
    fn default() -> Self {
        Self::new()
    }
}

impl Ai {
    pub fn new() -> Self {
        Ai { priority: BTreeMap::new(), requests: 0, scored: 0 }
    }

    /// Score every part in the model, in batches.
    ///
    /// Done once. A failure here is a failure of the run: continuing
    /// with an unscored board would silently fall back to whatever order
    /// the refdes happen to sort in, which is the behaviour this whole
    /// change exists to remove.
    fn score_all(&mut self, model: &ConstraintModel) -> Result<(), Vec<CheckResult>> {
        let refs: Vec<&str> = model.parts.iter().map(|p| p.reference.as_str()).collect();
        for chunk in refs.chunks(BATCH) {
            let mut qs = BTreeMap::new();
            for r in chunk {
                let nets: Vec<&str> = model
                    .nets
                    .iter()
                    .filter(|n| n.pins.iter().any(|pin| pin.split('.').next() == Some(*r)))
                    .map(|n| n.name.as_str())
                    .collect();
                qs.insert(
                    (*r).to_string(),
                    eda_jev::Question::Score {
                        instructions: format!(
                            "On a printed circuit board, how much does it matter exactly where {} goes? \
                             It connects to: {}. Consider what the part is for -- decoupling and input \
                             capacitors, crystals, voltage references, current-sense and high-current \
                             paths are placement-critical; pull-ups, indicator resistors and test points \
                             are not.",
                            describe(model, r),
                            if nets.is_empty() { "nothing".to_string() } else { nets.join(", ") }
                        ),
                        criteria: LEVELS.iter().map(|s| (*s).to_string()).collect(),
                    },
                );
            }
            let answers = eda_jev::ask(
                &serde_json::json!({
                    "task": "judging how placement-critical each component on a board is",
                    "board_parts": model.parts.len(),
                }),
                &qs,
            )?;
            self.requests += 1;
            for r in chunk {
                let score = answers.get(*r).and_then(|a| a.score).ok_or_else(|| {
                    vec![CheckResult::fail(
                        "ai_no_score",
                        *r,
                        "the model returned no criticality for this part, so its placement order is unknown",
                    )]
                })?;
                self.priority.insert((*r).to_string(), score);
                self.scored += 1;
            }
        }
        Ok(())
    }
}

/// What the part is, in words, for the model to reason about.
///
/// Value and package are what the intent actually carries; the MPN is
/// included because it often says more than either ("TPS62840" tells a
/// layout engineer more than "U5, 0603" ever will).
fn describe(model: &ConstraintModel, r: &str) -> String {
    let Some(p) = model.part(r) else { return r.to_string() };
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
        r.to_string()
    } else {
        format!("{r} ({})", bits.join(", "))
    }
}

/// The nets shared by two parts, named. This is the circuit fact that
/// makes one neighbour the right one: `VIN` says something a distance
/// never will.
fn shared_nets(model: &ConstraintModel, a: &str, b: &str) -> Vec<String> {
    model
        .nets
        .iter()
        .filter(|n| {
            let parts: Vec<&str> = n.pins.iter().map(|p| p.split('.').next().unwrap_or(p)).collect();
            parts.contains(&a) && parts.contains(&b)
        })
        .map(|n| n.name.clone())
        .collect()
}

impl Chooser for Ai {
    fn name(&self) -> &'static str {
        "ai"
    }

    /// Take the most placement-critical part that can actually go down.
    ///
    /// The judgement is the model's; the ordering it implies is applied
    /// one part at a time with the gates checked after each, exactly as
    /// for any other chooser.
    fn choose_part(&mut self, board: &Board, frontier: &[String]) -> Result<Option<String>, Vec<CheckResult>> {
        if self.priority.is_empty() {
            self.score_all(board.model())?;
        }
        // Only parts that have somewhere legal to go.
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

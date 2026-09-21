//! A chooser that asks the evaluation model which part goes next.
//!
//! One question, and only one: of the parts that could go down now,
//! which one should. Where it then lands is the placer's business.
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

/// Asks Jev which part to place next, with the circuit described.
pub struct Ai {
    /// Cache keyed by the question, so a rerun of the same board does
    /// not pay the network again -- and, more importantly, so a build is
    /// reproducible within a session. A remote service in the inner loop
    /// otherwise makes every measurement taken on this placer noise.
    cache: BTreeMap<String, String>,
    /// Steps where the service was asked, for reporting.
    pub asked: usize,
    /// Steps served from the cache.
    pub cached: usize,
}

impl Default for Ai {
    fn default() -> Self {
        Self::new()
    }
}

impl Ai {
    pub fn new() -> Self {
        Ai { cache: BTreeMap::new(), asked: 0, cached: 0 }
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

    /// Ask which part should go down next.
    ///
    /// Only that. Where it then lands is settled by the geometry, so the
    /// model is never shown a coordinate and never asked to compare two
    /// distances -- the one thing it was measured to be unreliable at,
    /// and unreliable without knowing it.
    fn choose_part(&mut self, board: &Board, frontier: &[String]) -> Result<Option<String>, Vec<CheckResult>> {
        // Only offer parts that can actually go down somewhere. Asking
        // about one with no legal pose wastes the question and invites
        // an answer we would have to discard.
        let viable: Vec<&String> = frontier.iter().filter(|p| crate::build::best_pose(board, p).is_some()).collect();
        match viable.len() {
            0 => return Ok(None),
            // No judgement to exercise; do not pay for a round trip.
            1 => return Ok(Some(viable[0].clone())),
            _ => {}
        }

        let model = board.model();
        let placed = board.placed();
        let criteria: BTreeMap<String, String> = viable
            .iter()
            .map(|p| {
                // What it is, and what placed part it would join. Those
                // are the circuit facts the decision turns on.
                let anchors: Vec<String> = board
                    .neighbours_of(p)
                    .into_iter()
                    .filter(|n| placed.contains(n))
                    .map(|n| {
                        let nets = shared_nets(model, p, &n);
                        if nets.is_empty() { n.clone() } else { format!("{n} via {}", nets.join("/")) }
                    })
                    .collect();
                (
                    (*p).clone(),
                    format!("{} -- would sit next to {}", describe(model, p), anchors.join("; ")),
                )
            })
            .collect();

        let state = serde_json::json!({
            "task": "choosing the order to place components on a printed circuit board",
            "principle": "a part whose position is critical to how the circuit works should be placed \
                          while there is still room to put it in the right spot; parts whose position \
                          barely matters can wait",
            "already_placed": placed.len(),
            "still_to_place": model.parts.len() - placed.len(),
        });

        let key = serde_json::to_string(&(&state, &criteria)).unwrap_or_default();
        let pick = if let Some(hit) = self.cache.get(&key) {
            self.cached += 1;
            hit.clone()
        } else {
            let mut qs = BTreeMap::new();
            qs.insert(
                "next".to_string(),
                eda_jev::Question::Choice {
                    instructions: "Which of these parts should be placed next? Choose the one whose \
                                   placement matters most to the circuit working -- decoupling and input \
                                   capacitors, crystals, sensitive references and high-current paths \
                                   before parts whose exact position is unimportant."
                        .to_string(),
                    criteria: criteria.clone(),
                },
            );
            let answers = eda_jev::ask(&state, &qs)?;
            self.asked += 1;
            let c = answers
                .get("next")
                .and_then(|a| a.choice.clone())
                .ok_or_else(|| vec![CheckResult::fail("ai_no_choice", "frontier", "the model returned no choice for this step")])?;
            self.cache.insert(key, c.clone());
            c
        };

        // An answer we did not offer is a hard failure. Quietly falling
        // back to the first option would make a bad model
        // indistinguishable from a good one.
        if let Some(p) = viable.iter().find(|p| ***p == pick) {
            return Ok(Some((*p).clone()));
        }
        Err(vec![CheckResult::fail(
            "ai_bad_choice",
            "frontier",
            format!("the model answered {pick:?}, which was not one of the {} parts offered", viable.len()),
        )])
    }
}

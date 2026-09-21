//! A chooser that asks the evaluation model where a part belongs.
//!
//! What it is asked, and what it is never asked, follows what it was
//! measured to be good at. It sees what the circuit *is* -- that U5 is a
//! buck converter, that this net is the RF supply -- and it picks a
//! neighbour and a side. It is never shown a coordinate and never asked
//! to compare two distances, because on that it scored 12 of 23 with
//! confidence that went *up* when it was wrong.
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

use crate::build::{Chooser, Option_};
use crate::{Board, Cmd};
use eda_model::{CheckResult, ConstraintModel};
use std::collections::BTreeMap;

/// Asks Jev which neighbour and side, with the circuit described.
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

    fn choose(&mut self, board: &Board, options: &[Option_]) -> Result<Option<usize>, Vec<CheckResult>> {
        // Only offer what actually applies. Asking the model to choose
        // among poses that collide wastes the question and invites a
        // wrong answer we would then have to discard.
        let viable: Vec<usize> = options
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                let mut t = board.fork();
                t.apply(&Cmd::Place { part: o.part.clone(), anchor: o.anchor.clone(), side: o.side }).is_ok()
            })
            .map(|(i, _)| i)
            .collect();
        match viable.len() {
            0 => return Ok(None),
            // No judgement to exercise; do not pay for a round trip.
            1 => return Ok(Some(viable[0])),
            _ => {}
        }

        let model = board.model();
        let part = &options[viable[0]].part;
        let criteria: BTreeMap<String, String> = viable
            .iter()
            .map(|&i| {
                let o = &options[i];
                let nets = shared_nets(model, &o.part, &o.anchor);
                let via = if nets.is_empty() {
                    "a placement rule".to_string()
                } else {
                    format!("net{} {}", if nets.len() > 1 { "s" } else { "" }, nets.join(", "))
                };
                (
                    format!("{}_{}", o.anchor, o.side.as_str()),
                    format!("on the {} side of {}, connected by {via}", o.side.as_str(), describe(model, &o.anchor)),
                )
            })
            .collect();

        let state = serde_json::json!({
            "task": "component placement on a printed circuit board",
            "part_to_place": describe(model, part),
            "its_nets": model.nets.iter()
                .filter(|n| n.pins.iter().any(|p| p.split('.').next() == Some(part.as_str())))
                .map(|n| n.name.clone()).collect::<Vec<_>>(),
            "already_placed": board.placed().len(),
        });

        // The cache key is the question, not the board: the same part
        // with the same candidates deserves the same answer.
        let key = serde_json::to_string(&(&state, &criteria)).unwrap_or_default();
        let pick = if let Some(hit) = self.cache.get(&key) {
            self.cached += 1;
            hit.clone()
        } else {
            let mut qs = BTreeMap::new();
            qs.insert(
                "where".to_string(),
                eda_jev::Question::Choice {
                    instructions: format!(
                        "Place {} next. Which neighbour should it sit beside, and on which side, for the best layout? \
                         Think about what the part is for: bypass and input capacitors belong hard against the pin they \
                         serve, sensitive nodes belong away from switching ones.",
                        describe(model, part)
                    ),
                    criteria: criteria.clone(),
                },
            );
            let answers = eda_jev::ask(&state, &qs)?;
            self.asked += 1;
            let c = answers
                .get("where")
                .and_then(|a| a.choice.clone())
                .ok_or_else(|| vec![CheckResult::fail("ai_no_choice", part, "the model returned no choice for this step")])?;
            self.cache.insert(key, c.clone());
            c
        };

        // Map the answer back. An id we did not offer is a hard failure:
        // silently falling back to the first option would make a bad
        // model indistinguishable from a good one.
        for &i in &viable {
            let o = &options[i];
            if format!("{}_{}", o.anchor, o.side.as_str()) == pick {
                return Ok(Some(i));
            }
        }
        Err(vec![CheckResult::fail(
            "ai_bad_choice",
            part,
            format!("the model answered {pick:?}, which was not one of the {} options offered", viable.len()),
        )])
    }
}

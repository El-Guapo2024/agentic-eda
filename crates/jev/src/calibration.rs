//! A standing competence check on the evaluation model.
//!
//! We are about to let Jev decide what a board looks like, on the
//! strength of a measurement taken once. Two things can quietly undo
//! that: the wire format can move (it already did once -- `choice`
//! stopped taking an `options` array mid-project and every choice
//! question started failing), and the service behind the model id can
//! change without the id changing.
//!
//! So the competence is re-measured on every run that uses Jev, against
//! questions whose answers are not matters of opinion. A wrong answer
//! here is a hard failure: a model that no longer knows an input
//! capacitor goes nearest the switching node is not one whose placement
//! ordering we should be following.
//!
//! The set deliberately excludes questions Jev found genuinely arguable
//! when this was calibrated -- I2C pull-up placement (0.73 confidence)
//! and crystal load-cap ground return (0.42). A gate that fails on a
//! defensible disagreement is a gate people learn to ignore.

use crate::{ask, Question};
use eda_model::CheckResult;
use std::collections::BTreeMap;

/// One question with the answer a competent layout engineer would give.
struct Item {
    id: &'static str,
    context: &'static str,
    ask: &'static str,
    /// option id -> what it means. The first is not special; the
    /// expected answer is named separately.
    criteria: &'static [(&'static str, &'static str)],
    expect: &'static str,
}

/// Fourteen questions, each answered at >= 0.86 confidence when this was
/// calibrated against the live service. Several are counterintuitive on
/// purpose: a model that has merely memorised layout folklore gets
/// `plane_split`, `emi_driver`, `return_path` and `antenna_keepout`
/// wrong, and `antenna_keepout` directly contradicts the textbook answer
/// for an antenna *feed line*, which is the trap.
const QUIZ: &[Item] = &[
    Item {
        id: "buck_loop",
        context: "a synchronous buck converter, 12V to 3.3V at 3A, 500kHz",
        ask: "Which component must sit physically closest to the converter IC to minimise the high-di/dt switching loop area?",
        criteria: &[
            ("input_ceramic_cap", "the input ceramic capacitor"),
            ("output_bulk_cap", "the output bulk electrolytic"),
            ("feedback_divider", "the feedback divider resistors"),
            ("enable_pullup", "the enable pull-up resistor"),
        ],
        expect: "input_ceramic_cap",
    },
    Item {
        id: "critical_loop",
        context: "a synchronous buck converter with high-side and low-side FETs",
        ask: "Which current loop has the highest di/dt and so matters most for layout?",
        criteria: &[
            ("vin_cap_both_fets", "input capacitor through both FETs and back"),
            ("inductor_to_outcap", "inductor through the output capacitor"),
            ("fb_divider_loop", "feedback divider back to the IC"),
            ("bootstrap_loop", "bootstrap capacitor to the high-side driver"),
        ],
        expect: "vin_cap_both_fets",
    },
    Item {
        id: "decap_pin",
        context: "an MCU with a 100nF decoupling capacitor on each VDD pin",
        ask: "Where should each decoupling capacitor be placed?",
        criteria: &[
            ("at_its_vdd_pin", "immediately at the VDD pin it serves"),
            ("board_edge", "together at the board edge"),
            ("near_regulator", "grouped near the voltage regulator"),
            ("under_mcu", "anywhere under the MCU body"),
        ],
        expect: "at_its_vdd_pin",
    },
    Item {
        id: "decap_metric",
        context: "judging whether a decoupling capacitor is placed well",
        ask: "What physically determines its effectiveness at high frequency?",
        criteria: &[
            ("loop_inductance", "the total loop inductance including its via path"),
            ("trace_length_only", "the straight-line distance to the IC body"),
            ("capacitance_value", "the capacitance value alone"),
            ("pad_size", "the size of the capacitor's own pads"),
        ],
        expect: "loop_inductance",
    },
    Item {
        id: "return_path",
        context: "a microstrip carrying a 100MHz clock over a solid ground plane",
        ask: "Where does the bulk of the return current actually flow?",
        criteria: &[
            ("beneath_trace", "directly beneath the trace, mirroring it"),
            ("shortest_ohmic", "along the shortest ohmic path to the source ground pin"),
            ("spread_evenly", "spread evenly across the whole plane"),
            ("nearest_via", "concentrated through the nearest stitching via"),
        ],
        expect: "beneath_trace",
    },
    Item {
        id: "plane_split",
        context: "a 4-layer board mixing a 12-bit ADC and digital logic",
        ask: "What is current best practice for the ground plane?",
        criteria: &[
            ("one_solid_plane", "one solid unbroken plane, separated by component placement"),
            ("split_two_planes", "split into analog and digital planes joined at one point"),
            ("star_ground", "a star ground radiating from the supply"),
            ("moat_and_bridge", "a moat around the analog area bridged by a ferrite"),
        ],
        expect: "one_solid_plane",
    },
    Item {
        id: "emi_driver",
        context: "radiated emission from a digital signal trace",
        ask: "Which property of the signal dominates the emission?",
        criteria: &[
            ("edge_rate", "the rise and fall time of the edges"),
            ("clock_freq", "the fundamental clock frequency"),
            ("voltage_swing", "the supply voltage swing"),
            ("duty_cycle", "the duty cycle of the waveform"),
        ],
        expect: "edge_rate",
    },
    Item {
        id: "antenna_keepout",
        context: "a 2.4GHz chip antenna's radiating element, not its feed line",
        ask: "What should be placed in the copper layers directly under the radiating element?",
        criteria: &[
            ("nothing_keepout", "nothing at all, a full keepout on every layer"),
            ("solid_ground", "a solid ground plane for a stable reference"),
            ("hatched_ground", "a hatched ground fill as a compromise"),
            ("power_plane", "the supply plane, kept quiet by filtering"),
        ],
        expect: "nothing_keepout",
    },
    Item {
        id: "esl_pkg",
        context: "choosing a 100nF ceramic decoupling capacitor",
        ask: "Which package has the lowest equivalent series inductance?",
        criteria: &[("c0402", "0402"), ("c0603", "0603"), ("c0805", "0805"), ("c1206", "1206")],
        expect: "c0402",
    },
    Item {
        id: "esd",
        context: "a USB connector with ESD protection diodes",
        ask: "Where do the ESD protection diodes belong?",
        criteria: &[
            ("at_the_connector", "immediately at the connector, before anything else"),
            ("at_the_mcu", "right at the MCU pins"),
            ("mid_trace", "halfway along the trace"),
            ("other_side", "on the opposite side of the board"),
        ],
        expect: "at_the_connector",
    },
    Item {
        id: "sense",
        context: "a 2 milliohm current sense resistor in a high-current path",
        ask: "How should the sense amplifier connect to the resistor?",
        criteria: &[
            ("kelvin", "a Kelvin four-wire connection to the resistor's sense pads"),
            ("tap_anywhere", "tapped anywhere convenient on the power trace"),
            ("via_ground_plane", "through the ground plane"),
            ("single_wire", "a single wire to one end"),
        ],
        expect: "kelvin",
    },
    Item {
        id: "thermal_via",
        context: "thermal vias in the exposed pad of a QFN",
        ask: "How should those vias be finished?",
        criteria: &[
            ("filled_or_tented", "plugged or tented so solder cannot wick away"),
            ("left_open", "left fully open to maximise airflow"),
            ("oversized_drill", "drilled oversize to move more heat"),
            ("one_big_via", "replaced with a single large via"),
        ],
        expect: "filled_or_tented",
    },
    Item {
        id: "orphan_pour",
        context: "an unconnected island of copper pour left on a signal layer",
        ask: "What is the correct treatment?",
        criteria: &[
            ("stitch_or_remove", "stitch it to ground or delete it"),
            ("leave_floating", "leave it floating, it is harmless"),
            ("tie_to_power", "connect it to the nearest supply net"),
            ("tie_to_signal", "tie it to the nearest signal net"),
        ],
        expect: "stitch_or_remove",
    },
    Item {
        id: "connector_edge",
        context: "board-to-board and cable connectors on a controller PCB",
        ask: "Where do connectors normally belong?",
        criteria: &[
            ("board_edge", "at the board edge with their openings facing outward"),
            ("board_centre", "in the middle of the board"),
            ("under_ics", "tucked under the tallest ICs"),
            ("anywhere", "anywhere that shortens traces"),
        ],
        expect: "board_edge",
    },
];

/// How many of the quiz may be wrong before the run fails.
///
/// Zero. Every question here was answered correctly at >= 0.86
/// confidence when calibrated, and none of them is arguable. A single
/// miss means the thing on the other end is not what we measured.
const ALLOWED_MISSES: usize = 0;

/// Ask the quiz and turn the result into checks.
///
/// Returns an empty vector when no key is configured -- a run that isn't
/// using Jev has nothing to calibrate. A configured service that cannot
/// be reached, or that answers wrongly, is a hard failure.
pub fn check_calibration() -> Vec<CheckResult> {
    if !crate::available() {
        return Vec::new();
    }
    let questions: BTreeMap<String, Question> = QUIZ
        .iter()
        .map(|i| {
            (
                i.id.to_string(),
                Question::Choice {
                    instructions: format!("Context: {}. {}", i.context, i.ask),
                    criteria: i.criteria.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                },
            )
        })
        .collect();

    let state = serde_json::json!({
        "domain": "printed circuit board layout and EMC",
        "role": "senior PCB layout engineer",
        "note": "answer each question independently",
    });

    let answers = match ask(&state, &questions) {
        Ok(a) => a,
        // Reaching a *configured* service is not optional: without the
        // calibration we do not know what we are taking advice from.
        Err(mut e) => {
            e.push(CheckResult::fail(
                "jev_calibration",
                "typesafe-ai/jev",
                "the calibration quiz could not be asked, so the model's competence is unknown for this run",
            ));
            return e;
        }
    };

    score(&answers)
}

/// Turn a set of answers into checks.
///
/// Split out from the network call on purpose: a gate nobody can make
/// fail on demand is indistinguishable from one that never fails, and
/// this one exists precisely to fail.
fn score(answers: &BTreeMap<String, crate::Answer>) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let mut misses = 0;
    for item in QUIZ {
        let got = answers.get(item.id).and_then(|a| a.choice.as_deref());
        match got {
            Some(c) if c == item.expect => {}
            Some(c) => {
                misses += 1;
                out.push(CheckResult::fail(
                    "jev_calibration",
                    item.id,
                    format!(
                        "answered {c:?} where the settled answer is {:?} ({}); the model's layout judgement has moved and its placement ordering cannot be trusted",
                        item.expect, item.ask
                    ),
                ));
            }
            None => {
                misses += 1;
                out.push(CheckResult::fail(
                    "jev_calibration",
                    item.id,
                    "no answer came back for this question",
                ));
            }
        }
    }
    if misses <= ALLOWED_MISSES {
        out.push(CheckResult::pass("jev_calibration"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_expected_answer_is_one_of_its_own_options() {
        // A typo in `expect` would make the gate fail every run for a
        // reason that has nothing to do with the model.
        for item in QUIZ {
            assert!(
                item.criteria.iter().any(|(k, _)| *k == item.expect),
                "{}: expected answer {:?} is not among its criteria",
                item.id,
                item.expect
            );
        }
    }

    #[test]
    fn question_ids_are_unique() {
        // They are map keys on the wire; a duplicate would silently
        // drop a question and shrink the quiz.
        let mut seen = std::collections::BTreeSet::new();
        for item in QUIZ {
            assert!(seen.insert(item.id), "duplicate question id {}", item.id);
        }
    }

    #[test]
    fn every_question_offers_a_real_choice() {
        for item in QUIZ {
            assert!(item.criteria.len() >= 3, "{}: too few options to be a test", item.id);
        }
    }

    /// Build an answer map, `overrides` replacing the correct choice.
    fn answers_with(overrides: &[(&str, &str)]) -> BTreeMap<String, crate::Answer> {
        QUIZ.iter()
            .map(|i| {
                let pick = overrides.iter().find(|(id, _)| *id == i.id).map(|(_, c)| *c).unwrap_or(i.expect);
                (
                    i.id.to_string(),
                    crate::Answer {
                        r#type: Some("choice".into()),
                        probability: None,
                        choice: Some(pick.to_string()),
                        score: None,
                        probabilities: None,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn a_fully_correct_run_passes() {
        let checks = score(&answers_with(&[]));
        assert!(checks.iter().all(|c| !matches!(c.status, eda_model::CheckStatus::Fail)), "{checks:?}");
        assert!(checks.iter().any(|c| c.check == "jev_calibration"));
    }

    #[test]
    fn one_wrong_answer_fails_the_run() {
        // The whole point of this gate. A model that puts the output
        // bulk capacitor nearest the buck IC has lost the plot, and the
        // run must stop rather than take placement advice from it.
        let checks = score(&answers_with(&[("buck_loop", "output_bulk_cap")]));
        let fails: Vec<_> = checks.iter().filter(|c| matches!(c.status, eda_model::CheckStatus::Fail)).collect();
        assert_eq!(fails.len(), 1, "expected exactly one failure, got {checks:?}");
        assert_eq!(fails[0].check, "jev_calibration");
        assert_eq!(fails[0].location.as_deref(), Some("buck_loop"));
        assert!(!checks.iter().any(|c| matches!(c.status, eda_model::CheckStatus::Pass)),
                "a run with a miss must not also report a pass");
    }

    #[test]
    fn a_missing_answer_fails_rather_than_being_skipped() {
        // Silence is not agreement: an unanswered question means we did
        // not measure the thing we claim to have measured.
        let mut a = answers_with(&[]);
        a.remove("return_path");
        let fails: Vec<_> = score(&a).into_iter().filter(|c| matches!(c.status, eda_model::CheckStatus::Fail)).collect();
        assert_eq!(fails.len(), 1);
        assert_eq!(fails[0].location.as_deref(), Some("return_path"));
    }

    #[test]
    fn no_key_means_nothing_to_calibrate() {
        if crate::available() {
            return; // a real key is set here; this assertion does not apply
        }
        assert!(check_calibration().is_empty());
    }
}

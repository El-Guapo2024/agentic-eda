//! `PCB_TUNING_PATTERN` (`pcbnew/generators/pcb_tuning_pattern.{h,cpp}`): a length-tuning pattern kept as an item of the board.
//!
//! A pattern is a `PCB_GENERATOR` (a group that remembers how its members were made): the tracks of the meander are its members, and what it
//! stores -- the tuning mode, where it starts and ends, the amplitude and spacing it was built with, the target, the line it replaced -- is enough
//! to build them again. Select it and its properties re-tune it (the tracks are put back to the baseline and the router's tuner runs over it
//! again), delete it and the baseline comes back. The `.kicad_pcb` keeps it as a `(generated (type tuning_pattern) ..)` block.
//!
//! Micrometres, like the rest of the IR, except where a field says nanometres. An additive overlay of `DrawingsSection`: a design without
//! patterns reads and writes as before.

use crate::ir::{Point, Um};
use serde::{Deserialize, Serialize};

/// `LENGTH_TUNING_MODE`: what the pattern tunes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TuningMode {
    /// "Single track" (`7`).
    #[default]
    Single,
    /// "Differential pair" (`8`): the length of a pair.
    DiffPair,
    /// "Diff pair skew" (`9`).
    DiffPairSkew,
}

impl TuningMode {
    /// The token the file keeps (`tuningToString`).
    pub fn token(self) -> &'static str {
        match self {
            TuningMode::Single => "single",
            TuningMode::DiffPair => "diff_pair",
            TuningMode::DiffPairSkew => "diff_pair_skew",
        }
    }

    pub fn from_token(s: &str) -> Option<TuningMode> {
        match s {
            "single" => Some(TuningMode::Single),
            "diff_pair" => Some(TuningMode::DiffPair),
            "diff_pair_skew" => Some(TuningMode::DiffPairSkew),
            _ => None,
        }
    }
}

/// `PNS::MEANDER_SIDE`: the side of its baseline the first meander is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternSide {
    #[default]
    Left,
    /// The side of the cursor.
    Default,
    Right,
}

impl PatternSide {
    pub fn token(self) -> &'static str {
        match self {
            PatternSide::Left => "left",
            PatternSide::Default => "default",
            PatternSide::Right => "right",
        }
    }

    pub fn from_token(s: &str) -> Option<PatternSide> {
        match s {
            "left" => Some(PatternSide::Left),
            "default" => Some(PatternSide::Default),
            "right" => Some(PatternSide::Right),
            _ => None,
        }
    }
}

/// `MINOPTMAX`: the target a pattern tunes to and the range it counts as tuned within (micrometres).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetRange {
    pub min: Um,
    pub opt: Um,
    pub max: Um,
}

/// `MEANDER_SETTINGS::LENGTH_UNCONSTRAINED`, micrometres: a million millimetres, what a pattern with no length target holds.
pub const LENGTH_UNCONSTRAINED_UM: Um = 1_000_000_000;

impl TargetRange {
    /// No target: any length is tuned.
    pub fn unconstrained() -> TargetRange {
        TargetRange { min: 0, opt: LENGTH_UNCONSTRAINED_UM, max: LENGTH_UNCONSTRAINED_UM }
    }

    pub fn is_unconstrained(&self) -> bool {
        self.opt == LENGTH_UNCONSTRAINED_UM
    }

    /// `SetTargetLength( aOpt )`: the target with the default tolerance (`DEFAULT_LENGTH_TOLERANCE`, 0.1 mm) either side of it.
    pub fn around(opt: Um) -> TargetRange {
        TargetRange { min: opt - 100, opt, max: opt + 100 }
    }
}

impl Default for TargetRange {
    fn default() -> Self {
        TargetRange::unconstrained()
    }
}

fn d_radius() -> u32 {
    80
}

fn d_true() -> bool {
    true
}

fn d_skew() -> TargetRange {
    TargetRange { min: -100, opt: 0, max: 100 }
}

/// A length-tuning pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuningPattern {
    /// Stable id (`tun_xxxxxxxxxxxx`), derived from where the pattern starts and the layer it is on; empty on a pattern a command is about to add.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub mode: TuningMode,
    /// The copper layer of the tuned track.
    pub layer: String,
    /// `m_origin`: where the pattern starts (on the track).
    pub origin: Point,
    /// `m_end`: where it ends.
    pub end: Point,
    #[serde(default)]
    pub initial_side: PatternSide,
    pub min_amplitude: Um,
    pub max_amplitude: Um,
    /// "Min spacing": the distance between two meanders.
    pub spacing: Um,
    #[serde(default = "d_radius")]
    pub corner_radius_percent: u32,
    /// Rounded corners (an arc) or chamfered ones (a 45-degree segment).
    #[serde(default = "d_true")]
    pub rounded: bool,
    #[serde(default)]
    pub single_sided: bool,
    /// "Override custom rules": the pattern's own target stands where a design rule has a length (or skew) constraint for the net.
    #[serde(default)]
    pub override_custom_rules: bool,
    #[serde(default)]
    pub target_length: TargetRange,
    #[serde(default = "d_skew")]
    pub target_skew: TargetRange,
    /// `m_baseLine`: the line the pattern was made on, straight, from `origin` to `end` (the clicked net's).
    #[serde(default)]
    pub base_line: Vec<Point>,
    /// `m_baseLineCoupled`: the partner net's, for a pair.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_line_coupled: Vec<Point>,
    /// The width of the tracks and the gap of the pair the tuner last used.
    #[serde(default)]
    pub last_track_width: Um,
    #[serde(default)]
    pub last_diff_pair_gap: Um,
    /// `m_tuningLength`, nanometres: the length (or skew) the tuner reported last.
    #[serde(default)]
    pub last_tuning_length: i64,
    /// `m_lastNetName`: the net of the tuned track.
    #[serde(default)]
    pub last_netname: String,
    /// `m_tuningStatus`: `tuned`, `too_short` or `too_long`.
    #[serde(default)]
    pub last_status: String,
    /// The tracks of the meander (`PCB_GROUP::GetItems`).
    #[serde(default)]
    pub member_ids: Vec<String>,
}

impl TuningPattern {
    /// What the id is derived from.
    pub fn id_seed(&self) -> String {
        format!("{}|{}|{},{}", self.mode.token(), self.layer, self.origin.x, self.origin.y)
    }

    /// `PCB_TUNING_PATTERN::GetRowData`'s "Tuning": the last length with its status, as the Generators Manager lists it ("12.3 mm (tuned)").
    pub fn tuning_info(&self) -> String {
        let status = match self.last_status.as_str() {
            "too_long" => "too long",
            "too_short" => "too short",
            "tuned" => "tuned",
            _ => "unknown",
        };
        format!("{} ({status})", fmt_mm(self.last_tuning_length as f64 / 1_000_000.0))
    }
}

/// A length in millimetres the way the unit formatter writes it: up to four decimals, no trailing zeros, with the unit.
fn fmt_mm(mm: f64) -> String {
    let s = format!("{mm:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} mm", if s.is_empty() || s == "-" { "0" } else { s })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern() -> TuningPattern {
        TuningPattern {
            id: "tun_1".into(),
            mode: TuningMode::DiffPairSkew,
            layer: "F.Cu".into(),
            origin: Point { x: 1_000, y: 2_000 },
            end: Point { x: 9_000, y: 2_000 },
            initial_side: PatternSide::Right,
            min_amplitude: 200,
            max_amplitude: 1_000,
            spacing: 600,
            corner_radius_percent: 100,
            rounded: false,
            single_sided: true,
            override_custom_rules: true,
            target_length: TargetRange::around(50_000),
            target_skew: TargetRange { min: -50, opt: 0, max: 50 },
            base_line: vec![Point { x: 1_000, y: 2_000 }, Point { x: 9_000, y: 2_000 }],
            base_line_coupled: vec![Point { x: 1_000, y: 2_400 }, Point { x: 9_000, y: 2_400 }],
            last_track_width: 200,
            last_diff_pair_gap: 200,
            last_tuning_length: 49_999_999,
            last_netname: "D+".into(),
            last_status: "tuned".into(),
            member_ids: vec!["trk_a".into(), "trk_b".into()],
        }
    }

    #[test]
    fn a_pattern_survives_json_as_a_value_and_as_text() {
        let p = pattern();
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(serde_json::from_value::<TuningPattern>(v).unwrap(), p);
        let text = serde_json::to_string(&p).unwrap();
        assert!(text.contains("\"mode\":\"diff_pair_skew\"") && text.contains("\"initial_side\":\"right\""), "{text}");
        assert_eq!(serde_json::from_str::<TuningPattern>(&text).unwrap(), p);
    }

    #[test]
    fn a_pattern_with_only_the_required_fields_gets_kicads_defaults() {
        let p: TuningPattern = serde_json::from_str(r#"{"mode":"single","layer":"F.Cu","origin":{"x":0,"y":0},"end":{"x":1000,"y":0},"min_amplitude":200,"max_amplitude":1000,"spacing":600}"#).unwrap();
        assert_eq!((p.corner_radius_percent, p.rounded, p.single_sided, p.initial_side), (80, true, false, PatternSide::Left));
        assert!(p.target_length.is_unconstrained());
        assert_eq!(p.target_skew, TargetRange { min: -100, opt: 0, max: 100 });
        assert!(p.member_ids.is_empty() && p.id.is_empty());
        assert_eq!(p.tuning_info(), "0 mm (unknown)");
    }

    #[test]
    fn the_target_helpers_follow_set_target_length() {
        assert_eq!(TargetRange::around(100_000), TargetRange { min: 99_900, opt: 100_000, max: 100_100 });
        assert!(TargetRange::unconstrained().is_unconstrained());
        let mut p = pattern();
        p.last_tuning_length = 75_980_000;
        p.last_status = "too_short".into();
        assert_eq!(p.tuning_info(), "75.98 mm (too short)");
    }
}

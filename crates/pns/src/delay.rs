//! The time domain of the length tuner: `TUNING_PROFILE_PARAMETERS_USER_DEFINED` (`pcbnew/length_delay_calculation/`) and the delay half of
//! `LENGTH_DELAY_CALCULATION::CalculateLengthDetails`.
//!
//! A net class that names a tuning profile gives the nets of the class a propagation delay: per copper layer, attoseconds per millimetre of
//! track (`DELAY_PROFILE_TRACK_PROPAGATION_ENTRY::GetDelay`, zero unless the profile has "Enable time-domain tuning"); for a via, the profile's
//! own delay per millimetre of the stackup the via crosses between the layers of the lines on either side of it, or the override for that
//! layer pair. The delay of a tuning path is the delay of its lines and vias (the pad-to-die delay of the pads it ends on is not in the model).
//! A time domain tuner is given a delay and turns it into the length it would have (`GetTrackLengthForPropagationDelay`: the delay over the
//! track delay of the layer it tunes on), then tunes to that length.
//!
//! Time is in attoseconds, KiCad's internal time unit (`pcbIUScale.IU_PER_PS` is 1e6); lengths in nanometres.

use crate::item::Item;
use crate::node::Node;
use crate::tuning_path::{stackup_distance_nm, LengthEnv, PathItem, TuningPath};
use eda_model::setup_pages::ViaOverride;
use std::collections::BTreeMap;

/// Attoseconds in a picosecond (`IU_PER_PS`).
pub const ATTOSECONDS_PER_PS: i64 = 1_000_000;
/// `MEANDER_SETTINGS::DELAY_UNCONSTRAINED`: `1000000 * IU_PER_PS`.
pub const DELAY_UNCONSTRAINED: i64 = 1_000_000 * ATTOSECONDS_PER_PS;
/// `MEANDER_SETTINGS::DEFAULT_DELAY_TOLERANCE`: `0.1 * IU_PER_PS`.
pub const DEFAULT_DELAY_TOLERANCE: i64 = ATTOSECONDS_PER_PS / 10;

/// The delays of one tuning profile, as a net's tuner reads them.
#[derive(Debug, Clone, PartialEq)]
pub struct DelayParams {
    /// `TUNING_PROFILE::m_EnableTimeDomainTuning`: with it off every delay is zero.
    pub enabled: bool,
    /// Attoseconds per millimetre of track, by copper layer name (`m_TrackPropagationEntriesMap`: one entry a layer; of several rows for a layer the last stands).
    pub track_as_per_mm: BTreeMap<String, i64>,
    /// `m_ViaPropagationDelay`: attoseconds per millimetre of the stackup a via crosses.
    pub via_as_per_mm: i64,
    pub via_overrides: Vec<ViaOverride>,
}

impl LengthEnv {
    /// The delay parameters of the tuning profile `net`'s effective net class names, if it names one the project has.
    pub fn delay_params(&self, net: &str) -> Option<DelayParams> {
        let p = self.rules.tuning_profile_of(net)?;
        let mut track_as_per_mm = BTreeMap::new();
        for e in &p.layer_entries {
            track_as_per_mm.insert(e.signal_layer.clone(), e.delay_as_per_mm);
        }
        Some(DelayParams { enabled: p.enable_time_domain_tuning, track_as_per_mm, via_as_per_mm: p.via_propagation_delay_as, via_overrides: p.via_overrides.clone() })
    }
}

impl DelayParams {
    /// `DELAY_PROFILE_TRACK_PROPAGATION_ENTRY::GetDelay` of the entry of `layer`: attoseconds per millimetre, zero without an entry or without the time domain.
    pub fn track_unit(&self, layer: &str) -> i64 {
        if !self.enabled {
            return 0;
        }
        self.track_as_per_mm.get(layer).copied().unwrap_or(0)
    }

    /// The delay of `length_nm` of track on `layer`: `int64_t( delayUnit * ( length / PCB_IU_PER_MM ) )`.
    pub fn track_delay(&self, layer: &str, length_nm: i64) -> i64 {
        (self.track_unit(layer) as f64 * (length_nm as f64 / 1_000_000.0)) as i64
    }

    /// `GetTrackLengthForPropagationDelay`: the length of track on `layer` that takes `delay` attoseconds, nanometres. A layer without a delay has no length for it
    /// (KiCad divides by zero there); the answer is nothing.
    pub fn length_for_delay(&self, layer: &str, delay: i64) -> i64 {
        let unit = self.track_unit(layer) as f64;
        if unit == 0.0 {
            return 0;
        }
        ((delay as f64 / unit) * 1_000_000.0) as i64
    }

    /// `getPropagationDelay` of a via item: zero without the time domain; the override for the signal layers (`from`, `to`, in the order of the path) and the layers
    /// the via spans (`via_from`, `via_to`) when the profile has one, else the profile's via delay over the height of the stackup between the signal layers.
    pub fn via_delay(&self, signal_from: &str, signal_to: &str, via_from: &str, via_to: &str, stackup_height_nm: i64) -> i64 {
        if !self.enabled {
            return 0;
        }
        // "this assumes that the layers are already in CuStack() order": the signal layers of the item are ordered (`SetLayers`), the rows of the grid are
        // kept that way; here a row matches the layers either way round
        let same = |a: &str, b: &str, c: &str, d: &str| (a == signal_from && b == signal_to || a == signal_to && b == signal_from) && (c == via_from && d == via_to || c == via_to && d == via_from);
        if let Some(o) = self.via_overrides.iter().rev().find(|o| same(&o.signal_layer_from, &o.signal_layer_to, &o.via_layer_from, &o.via_layer_to)) {
            return o.delay_as;
        }
        (self.via_as_per_mm as f64 * (stackup_height_nm as f64 / 1_000_000.0)) as i64
    }
}

impl TuningPath {
    /// `lineDelay( path, startPad, endPad )` (`CalculateRoutedPathDelay`: `CalculateDelay`): the delay of the lines of the path (pad-clipped, as their
    /// length is) and of the vias between lines on different layers, with the height of the stackup they cross counted when the board uses height in
    /// its lengths. Attoseconds.
    pub fn delay(&self, world: &Node, env: &LengthEnv, params: &DelayParams) -> i64 {
        let name = |i: i32| env.rules.layers.get(i as usize).cloned().unwrap_or_default();
        let mut total: i64 = 0;
        for (idx, it) in self.items.iter().enumerate() {
            match it {
                PathItem::Line { line, clipped } => total += params.track_delay(&name(line.layer), clipped.length()),
                PathItem::Via(vid) if env.rules.use_height_for_length_calcs && idx > 0 && idx + 1 < self.items.len() => {
                    let layer_of = |i: &PathItem| if let PathItem::Line { line, .. } = i { Some(line.layer) } else { None };
                    if let (Some(a), Some(b)) = (layer_of(&self.items[idx - 1]), layer_of(&self.items[idx + 1])) {
                        if a != b {
                            let height = stackup_distance_nm(&env.stackup, &env.rules.layers, &name(a), &name(b));
                            // the layers the via spans (`Padstack().StartLayer()`, `EndLayer()`)
                            let (via_from, via_to) = match world.get(*vid) {
                                Some(Item::Via(v)) => (name(v.layers.start()), name(v.layers.end())),
                                _ => (name(0), name(env.rules.layers.len() as i32 - 1)),
                            };
                            total += params.via_delay(&name(a), &name(b), &via_from, &via_to, height);
                        }
                    }
                }
                _ => {}
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::setup_pages::{ProfileLayerEntry, ProfileType, TuningProfile, TuningProfiles};

    fn profile(enabled: bool) -> TuningProfile {
        TuningProfile {
            name: "USB".into(),
            profile_type: ProfileType::Single,
            target_impedance: 50.0,
            enable_time_domain_tuning: enabled,
            layer_entries: vec![
                ProfileLayerEntry { signal_layer: "F.Cu".into(), top_reference_layer: None, bottom_reference_layer: None, width_nm: 200, diff_pair_gap_nm: 0, delay_as_per_mm: 6_100_000 },
                ProfileLayerEntry { signal_layer: "B.Cu".into(), top_reference_layer: None, bottom_reference_layer: None, width_nm: 200, diff_pair_gap_nm: 0, delay_as_per_mm: 6_500_000 },
            ],
            via_propagation_delay_as: 5_000_000,
            via_overrides: vec![ViaOverride { signal_layer_from: "F.Cu".into(), signal_layer_to: "B.Cu".into(), via_layer_from: "F.Cu".into(), via_layer_to: "B.Cu".into(), delay_as: 12_000_000 }],
        }
    }

    fn env(enabled: bool) -> LengthEnv {
        let mut rules = eda_model::BoardRules::default();
        rules.layers = vec!["F.Cu".into(), "B.Cu".into()];
        rules.tuning_profiles = TuningProfiles { profiles: vec![profile(enabled)], class_profiles: [("Default".to_string(), "USB".to_string())].into() };
        LengthEnv { stackup: eda_model::rules::default_stackup(2, 1_600), rules }
    }

    #[test]
    fn a_net_in_a_class_that_names_a_profile_has_its_delays_and_a_net_without_one_has_none() {
        let e = env(true);
        let p = e.delay_params("SIG").expect("the Default class names USB");
        assert_eq!(p.track_unit("F.Cu"), 6_100_000);
        assert_eq!(p.track_unit("In1.Cu"), 0, "a layer with no row has no delay");
        let mut none = env(true);
        none.rules.tuning_profiles.class_profiles.clear();
        assert!(none.delay_params("SIG").is_none());
        // a class that names a profile the project does not have
        none.rules.tuning_profiles.class_profiles.insert("Default".into(), "ghost".into());
        assert!(none.delay_params("SIG").is_none());
    }

    #[test]
    fn a_profile_without_the_time_domain_has_no_delays_at_all() {
        let p = env(false).delay_params("SIG").unwrap();
        assert_eq!((p.track_unit("F.Cu"), p.track_delay("F.Cu", 10_000_000), p.length_for_delay("F.Cu", 61_000_000), p.via_delay("F.Cu", "B.Cu", "F.Cu", "B.Cu", 1_600_000)), (0, 0, 0, 0));
    }

    #[test]
    fn the_delay_of_a_track_and_the_length_of_a_delay_are_inverses_at_the_layers_unit() {
        let p = env(true).delay_params("SIG").unwrap();
        // 10 mm at 6.1 ps/mm is 61 ps
        assert_eq!(p.track_delay("F.Cu", 10_000_000), 61_000_000);
        assert_eq!(p.length_for_delay("F.Cu", 61_000_000), 10_000_000);
        assert_eq!(p.track_delay("B.Cu", 10_000_000), 65_000_000, "another layer, another unit");
        assert_eq!(p.length_for_delay("In1.Cu", 61_000_000), 0);
    }

    #[test]
    fn a_via_has_the_override_for_its_layers_or_the_profiles_delay_over_the_stackup_it_crosses() {
        let p = env(true).delay_params("SIG").unwrap();
        assert_eq!(p.via_delay("F.Cu", "B.Cu", "F.Cu", "B.Cu", 1_600_000), 12_000_000, "the override");
        assert_eq!(p.via_delay("B.Cu", "F.Cu", "B.Cu", "F.Cu", 1_600_000), 12_000_000, "either way round");
        assert_eq!(p.via_delay("F.Cu", "In1.Cu", "F.Cu", "B.Cu", 200_000), 1_000_000, "5 ps/mm over 0.2 mm");
    }
}

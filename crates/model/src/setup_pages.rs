//! The Board Setup pages that hold a handful of settings each: Text & Graphics > Formatting, Board Stackup > Zone Hatch Offsets, Design Rules > Length-tuning Patterns and
//! Design Rules > Tuning Profiles.
//!
//! Each is a whole page stored in the rules overlay (`crate::rules::RulesOverlay`) and laid over the board's rules when the board is loaded, like every other page; the derived
//! KiCad files carry them where KiCad keeps them (the `.kicad_pro`'s `defaults.apply_defaults_to_fp_*`, `board.design_settings.tuning_pattern_settings` and `tuning_profiles`,
//! the board file's `(pcbplotparams (dashed_line_*_ratio ..))` and `(setup (zone_defaults ..))`), and kicad-cli reads them from there.
//!
//! Ported from:
//!   pcbnew/dialogs/panel_setup_formatting.cpp            the dash and gap ratios (`PCB_PLOT_PARAMS`, 12 and 3 from ISO 128-2) and the five "apply board defaults to footprint .." boxes
//!   pcbnew/footprint.cpp                                 `ApplyDefaultSettings`: what those boxes do to a footprint that is put on the board
//!   pcbnew/dialogs/panel_setup_zone_hatch_offsets.cpp    one offset per copper layer (`ZONE_LAYER_PROPERTIES::hatching_offset`), `ZONE_FILLER::addHatchFillTypeOnZone` moves the pattern by it
//!   pcbnew/dialogs/panel_setup_tuning_patterns.cpp       three groups of meander defaults (`PNS::MEANDER_SETTINGS`), `BOARD_DESIGN_SETTINGS`'s `tuning_pattern_settings`
//!   pcbnew/dialogs/panel_setup_tuning_profiles.cpp,
//!   panel_setup_tuning_profile_info.cpp                  a profile's name, type, target impedance, per-layer delays and via delays (`ValidateProfile`)
//!   common/project/tuning_profiles.cpp                   the `.kicad_pro` form (`tuning_profiles_impedance_geometric`)
//!   pcbnew/drc/drc_test_provider_misc.cpp                `testMissingTuningProfiles`: a net class that names a profile the project does not have

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::ir::Nm;
use crate::CheckResult;

// ------------------------------------------------------------------------------------------------------------------------------------ Formatting

/// `PCB_PLOT_PARAMS`: the dash of a dashed line is this many line widths long (ISO 128-2).
pub const DEFAULT_DASH_RATIO: f64 = 12.0;
/// `PCB_PLOT_PARAMS`: the gap of a dashed line is this many line widths long (ISO 128-2).
pub const DEFAULT_GAP_RATIO: f64 = 3.0;

/// The most a dash or gap ratio may be. KiCad's panel takes any number; a line a thousand times longer than it is wide is no dashed line any plot can draw.
pub const MAX_DASH_RATIO: f64 = 1_000.0;

fn d_dash() -> f64 {
    DEFAULT_DASH_RATIO
}
fn d_gap() -> f64 {
    DEFAULT_GAP_RATIO
}

/// Text & Graphics > Formatting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormattingSettings {
    /// "Dash length": the ratio of a dash to the line width (`dashed_line_dash_ratio` of the board's plot parameters).
    #[serde(default = "d_dash")]
    pub dash_length_ratio: f64,
    /// "Gap length": the ratio of the gap between dashes to the line width (`dashed_line_gap_ratio`).
    #[serde(default = "d_gap")]
    pub gap_length_ratio: f64,
    /// "Apply board defaults to footprint fields" (`defaults.apply_defaults_to_fp_fields`): a footprint put on the board takes the Text & Graphics defaults for its fields.
    #[serde(default)]
    pub apply_to_fp_fields: bool,
    /// "Apply board defaults to footprint text" (`apply_defaults_to_fp_text`).
    #[serde(default)]
    pub apply_to_fp_text: bool,
    /// "Apply board defaults to non-copper footprint shapes" (`apply_defaults_to_fp_shapes`).
    #[serde(default)]
    pub apply_to_fp_shapes: bool,
    /// "Apply board defaults to footprint dimensions" (`apply_defaults_to_fp_dimensions`).
    #[serde(default)]
    pub apply_to_fp_dimensions: bool,
    /// "Apply board defaults to footprint barcodes" (`apply_defaults_to_fp_barcodes`).
    #[serde(default)]
    pub apply_to_fp_barcodes: bool,
}

impl Default for FormattingSettings {
    fn default() -> Self {
        FormattingSettings { dash_length_ratio: DEFAULT_DASH_RATIO, gap_length_ratio: DEFAULT_GAP_RATIO, apply_to_fp_fields: false, apply_to_fp_text: false, apply_to_fp_shapes: false, apply_to_fp_dimensions: false, apply_to_fp_barcodes: false }
    }
}

impl FormattingSettings {
    pub fn is_default(&self) -> bool {
        *self == FormattingSettings::default()
    }

    /// Whether any "apply board defaults" box is on.
    pub fn applies_any(&self) -> bool {
        self.apply_to_fp_fields || self.apply_to_fp_text || self.apply_to_fp_shapes || self.apply_to_fp_dimensions || self.apply_to_fp_barcodes
    }

    /// The panel takes whatever number is typed; the page refuses one a dashed line cannot be drawn with. The refusal's `location` is the setting.
    pub fn validate(&self) -> Result<(), CheckResult> {
        for (name, v) in [("dash_length_ratio", self.dash_length_ratio), ("gap_length_ratio", self.gap_length_ratio)] {
            if !v.is_finite() || v <= 0.0 || v > MAX_DASH_RATIO {
                return Err(CheckResult::fail("ops_bad_formatting", name, format!("{} must be a number above 0 and at most {MAX_DASH_RATIO} (a ratio of the line width)", if name == "dash_length_ratio" { "Dash length" } else { "Gap length" })));
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------ Zone Hatch Offsets

/// The most a hatch offset may be: a metre. The filler reduces it modulo the pattern's pitch, so more would mean nothing.
pub const MAX_HATCH_OFFSET_NM: Nm = 1_000_000_000;

/// `ZONE_LAYER_PROPERTIES::hatching_offset`: how far the hatch pattern of the zones on a copper layer is moved from where the board's origin puts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HatchOffset {
    #[serde(default)]
    #[serde(alias = "x_um", deserialize_with = "crate::units::de")]
    pub x_nm: Nm,
    #[serde(default)]
    #[serde(alias = "y_um", deserialize_with = "crate::units::de")]
    pub y_nm: Nm,
}

impl HatchOffset {
    pub fn is_zero(&self) -> bool {
        self.x_nm == 0 && self.y_nm == 0
    }
}

/// Copper layer (the board file's name, `F.Cu`) -> its offset. A layer that is not here has none.
pub type HatchOffsets = BTreeMap<String, HatchOffset>;

/// The page without the layers that have no offset (KiCad writes a layer's `(property ..)` only when it has a value; a layer set to 0, 0 is the same as one never set).
pub fn canonical_hatch_offsets(offsets: &HatchOffsets) -> HatchOffsets {
    offsets.iter().filter(|(_, o)| !o.is_zero()).map(|(l, o)| (l.clone(), *o)).collect()
}

/// A page's check: every entry is for a copper layer the board has, and its offset is within [`MAX_HATCH_OFFSET_NM`]. The refusal's `location` is the layer.
pub fn validate_hatch_offsets(offsets: &HatchOffsets, copper: &[String]) -> Result<(), CheckResult> {
    for (layer, o) in offsets {
        if !copper.iter().any(|c| c == layer) {
            return Err(CheckResult::fail("ops_bad_hatch_offsets", layer, format!("{layer} is not a copper layer of this board")));
        }
        if o.x_nm.abs() > MAX_HATCH_OFFSET_NM || o.y_nm.abs() > MAX_HATCH_OFFSET_NM {
            return Err(CheckResult::fail("ops_bad_hatch_offsets", layer, format!("The hatch offset of {layer} is at most {} mm either way", MAX_HATCH_OFFSET_NM / crate::units::NM_PER_MM)));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------------------------------------------ Length-tuning Patterns

/// `PNS::MEANDER_STYLE`: how the corners of a meander are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerStyle {
    Chamfer,
    Round,
}

impl CornerStyle {
    /// The number the `.kicad_pro` keeps (`corner_style`: 0 chamfer, 1 round).
    pub fn code(self) -> u8 {
        match self {
            CornerStyle::Chamfer => 0,
            CornerStyle::Round => 1,
        }
    }

    pub fn from_code(code: i64) -> CornerStyle {
        if code == 0 {
            CornerStyle::Chamfer
        } else {
            CornerStyle::Round
        }
    }
}

/// `PNS::MEANDER_SETTINGS`, the part of it the page edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeanderSettings {
    /// "Minimum amplitude" (`m_minAmplitude`): the shortest a meander's swing may be. Stepping the amplitude never goes below it.
    #[serde(alias = "min_amplitude_um", deserialize_with = "crate::units::de")]
    pub min_amplitude_nm: Nm,
    /// "Maximum amplitude" (`m_maxAmplitude`): the longest it may be, and what a new pattern starts at; the amplitude hot keys step it.
    #[serde(alias = "max_amplitude_um", deserialize_with = "crate::units::de")]
    pub max_amplitude_nm: Nm,
    /// "Spacing" (`m_spacing`): the distance between two swings.
    #[serde(alias = "spacing_um", deserialize_with = "crate::units::de")]
    pub spacing_nm: Nm,
    /// "Corner style".
    pub corner_style: CornerStyle,
    /// "Corner radius" as a percentage (`m_cornerRadiusPercentage`), 0 to 100.
    pub corner_radius_percentage: u32,
    /// "Single-sided" (`m_singleSided`): the meanders are all on one side of the track.
    pub single_sided: bool,
}

impl MeanderSettings {
    /// `MEANDER_SETTINGS()` with the spacing `BOARD_DESIGN_SETTINGS` gives a single track and a skew pattern (`DEFAULT_MEANDER_SPACING`, 0.6 mm).
    pub fn single_track() -> Self {
        MeanderSettings { min_amplitude_nm: 200_000, max_amplitude_nm: 1_000_000, spacing_nm: 600_000, corner_style: CornerStyle::Round, corner_radius_percentage: 80, single_sided: false }
    }

    /// The same with a differential pair's spacing (`DEFAULT_DP_MEANDER_SPACING`, 1 mm).
    pub fn diff_pair() -> Self {
        MeanderSettings { spacing_nm: 1_000_000, ..MeanderSettings::single_track() }
    }
}

/// The three groups of the Length-tuning Patterns page: what a tuning pattern of each kind starts from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuningPatterns {
    /// `single_track_defaults`: "Tune length of a single track".
    #[serde(default = "MeanderSettings::single_track")]
    pub single_track: MeanderSettings,
    /// `diff_pair_defaults`: "Tune length of a differential pair".
    #[serde(default = "MeanderSettings::diff_pair")]
    pub diff_pair: MeanderSettings,
    /// `diff_pair_skew_defaults`: "Tune skew of a differential pair".
    #[serde(default = "MeanderSettings::single_track")]
    pub diff_pair_skew: MeanderSettings,
}

impl Default for TuningPatterns {
    fn default() -> Self {
        TuningPatterns { single_track: MeanderSettings::single_track(), diff_pair: MeanderSettings::diff_pair(), diff_pair_skew: MeanderSettings::single_track() }
    }
}

/// The most an amplitude or a spacing may be: 100 mm (KiCad's own limit for a text, a line and most lengths a user types).
pub const MAX_MEANDER_NM: Nm = 100_000_000;

impl TuningPatterns {
    pub fn is_default(&self) -> bool {
        *self == TuningPatterns::default()
    }

    /// The panel's own limits and the meander's: an amplitude and a spacing above 0, the minimum amplitude not above the maximum, a corner radius of 0 to 100 per cent.
    /// The refusal's `location` is the setting (`single_track.max_amplitude`).
    pub fn validate(&self) -> Result<(), CheckResult> {
        for (group, s) in [("single_track", &self.single_track), ("diff_pair", &self.diff_pair), ("diff_pair_skew", &self.diff_pair_skew)] {
            let bad = |field: &str, why: &str| Err(CheckResult::fail("ops_bad_tuning_patterns", format!("{group}.{field}"), why.to_string()));
            for (field, v) in [("min_amplitude", s.min_amplitude_nm), ("max_amplitude", s.max_amplitude_nm), ("spacing", s.spacing_nm)] {
                if v <= 0 {
                    return bad(field, &format!("The {} must be above 0.", field.replace('_', " ")));
                }
                if v > MAX_MEANDER_NM {
                    return bad(field, &format!("The {} is at most {} mm.", field.replace('_', " "), MAX_MEANDER_NM / crate::units::NM_PER_MM));
                }
            }
            if s.min_amplitude_nm > s.max_amplitude_nm {
                return bad("min_amplitude", "The minimum amplitude cannot be above the maximum amplitude.");
            }
            if s.corner_radius_percentage > 100 {
                return bad("corner_radius_percentage", "The corner radius is a percentage from 0 to 100.");
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------ Tuning Profiles

/// `TUNING_PROFILE::PROFILE_TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileType {
    Single,
    Differential,
}

impl ProfileType {
    /// The number the `.kicad_pro` keeps (`type`: 0 single, 1 differential).
    pub fn code(self) -> u8 {
        match self {
            ProfileType::Single => 0,
            ProfileType::Differential => 1,
        }
    }

    pub fn from_code(code: i64) -> ProfileType {
        if code == 0 {
            ProfileType::Single
        } else {
            ProfileType::Differential
        }
    }
}

/// One row of a profile's track propagation grid (`DELAY_PROFILE_TRACK_PROPAGATION_ENTRY`): how long a signal takes along a track of this width on this layer.
/// Delays are in KiCad's own unit, attoseconds per millimetre (a picosecond per millimetre is 1,000,000; a typical FR-4 outer layer is 5.6 ps/mm).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileLayerEntry {
    /// The copper layer, by the name the board file writes (`F.Cu`).
    pub signal_layer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_reference_layer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom_reference_layer: Option<String>,
    #[serde(default)]
    #[serde(alias = "width_um", deserialize_with = "crate::units::de")]
    pub width_nm: Nm,
    /// The gap of a differential pair on this layer; 0 in a single-ended profile.
    #[serde(default)]
    #[serde(alias = "diff_pair_gap_um", deserialize_with = "crate::units::de")]
    pub diff_pair_gap_nm: Nm,
    #[serde(default)]
    pub delay_as_per_mm: i64,
}

/// One row of the via overrides grid (`DELAY_PROFILE_VIA_OVERRIDE_ENTRY`): the delay a via takes between two signal layers when it spans these via layers.
/// The delay is a time in attoseconds (a picosecond is 1,000,000).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViaOverride {
    pub signal_layer_from: String,
    pub signal_layer_to: String,
    pub via_layer_from: String,
    pub via_layer_to: String,
    #[serde(default)]
    pub delay_as: i64,
}

/// `TUNING_PROFILE`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuningProfile {
    pub name: String,
    pub profile_type: ProfileType,
    /// "Target impedance" in ohms (0 when none).
    #[serde(default)]
    pub target_impedance: f64,
    /// "Enable time-domain tuning": the delays are used when tuning (a profile without it only names its geometry).
    #[serde(default)]
    pub enable_time_domain_tuning: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layer_entries: Vec<ProfileLayerEntry>,
    /// "Via propagation delay" in attoseconds, for a via no override names.
    #[serde(default)]
    pub via_propagation_delay_as: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via_overrides: Vec<ViaOverride>,
}

/// Design Rules > Tuning Profiles: the profiles, and the profile each net class names (`NETCLASS::SetTuningProfile`; a class not here names none). A class may name a profile the
/// project does not have -- the panel keeps the name of a deleted profile -- and that is what `missing_tuning_profile` reports.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuningProfiles {
    #[serde(default)]
    pub profiles: Vec<TuningProfile>,
    /// Net class name (`Default` included) -> profile name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub class_profiles: BTreeMap<String, String>,
}

/// The longest a profile's name may be, and the most profiles, rows and classes a page may hold (a page is stored in `design.json` whole).
pub const MAX_PROFILE_NAME: usize = 256;
pub const MAX_PROFILES: usize = 200;
pub const MAX_PROFILE_ROWS: usize = 200;
/// The most a delay may be, in attoseconds per millimetre or attoseconds: KiCad keeps them in an `int`.
pub const MAX_DELAY_AS: i64 = i32::MAX as i64;

impl TuningProfiles {
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty() && self.class_profiles.is_empty()
    }

    /// The names of the profiles the project has (an empty name is no name: `testMissingTuningProfiles` skips it).
    pub fn names(&self) -> BTreeSet<&str> {
        self.profiles.iter().map(|p| p.name.as_str()).filter(|n| !n.is_empty()).collect()
    }

    /// The net classes that name a profile the page does not define, with the profile they name: what kicad-cli's DRC reports as `missing_tuning_profile`.
    pub fn missing(&self) -> Vec<(&str, &str)> {
        let have = self.names();
        self.class_profiles.iter().filter(|(_, p)| !p.is_empty() && !have.contains(p.as_str())).map(|(c, p)| (c.as_str(), p.as_str())).collect()
    }

    /// `PANEL_SETUP_TUNING_PROFILES::Validate` / `ValidateProfile` and the limits a stored page needs. `copper` is the board's copper layers. The refusal's `location` is the
    /// profile's name (`profile 2` when it has none) and the problem is in `hint`.
    pub fn validate(&self, copper: &[String]) -> Result<(), CheckResult> {
        if self.profiles.len() > MAX_PROFILES {
            return Err(CheckResult::fail("ops_bad_tuning_profiles", "profiles", format!("A board has at most {MAX_PROFILES} tuning profiles")));
        }
        let have = |l: &str| copper.iter().any(|c| c == l);
        for (i, p) in self.profiles.iter().enumerate() {
            let at = if p.name.is_empty() { format!("profile {}", i + 1) } else { p.name.clone() };
            let bad = |why: String| Err(CheckResult::fail("ops_bad_tuning_profiles", &at, why));
            if p.name.is_empty() {
                return bad("Tuning profile must have a name".into());
            }
            if p.name.len() > MAX_PROFILE_NAME || p.name.chars().any(|c| c.is_control()) {
                return bad(format!("A profile's name is at most {MAX_PROFILE_NAME} characters, with no control characters"));
            }
            if !p.target_impedance.is_finite() || p.target_impedance < 0.0 || p.target_impedance > 1.0e6 {
                return bad("The target impedance is a number of ohms from 0 up".into());
            }
            if p.layer_entries.len() > MAX_PROFILE_ROWS || p.via_overrides.len() > MAX_PROFILE_ROWS {
                return bad(format!("A profile has at most {MAX_PROFILE_ROWS} rows in each grid"));
            }
            if p.via_propagation_delay_as < 0 || p.via_propagation_delay_as > MAX_DELAY_AS {
                return bad("The via propagation delay is from 0 to 2147 ps".into());
            }
            let mut layers = BTreeSet::new();
            for (row, e) in p.layer_entries.iter().enumerate() {
                if !have(&e.signal_layer) {
                    return bad(format!("Row {}: {} is not a copper layer of this board", row + 1, e.signal_layer));
                }
                for r in [&e.top_reference_layer, &e.bottom_reference_layer].into_iter().flatten() {
                    if !have(r) {
                        return bad(format!("Row {}: the reference layer {r} is not a copper layer of this board", row + 1));
                    }
                }
                if !layers.insert(e.signal_layer.as_str()) {
                    return bad("Duplicated signal layer configuration in tuning profile".into());
                }
                if e.width_nm < 0 || e.diff_pair_gap_nm < 0 || e.width_nm > 100_000_000 || e.diff_pair_gap_nm > 100_000_000 {
                    return bad(format!("Row {}: the width and the gap are from 0 to 100 mm", row + 1));
                }
                if e.delay_as_per_mm < 0 || e.delay_as_per_mm > MAX_DELAY_AS {
                    return bad(format!("Row {}: the delay is from 0 to 2147 ps/mm", row + 1));
                }
            }
            for (row, v) in p.via_overrides.iter().enumerate() {
                for l in [&v.signal_layer_from, &v.signal_layer_to, &v.via_layer_from, &v.via_layer_to] {
                    if !have(l) {
                        return bad(format!("Via override {}: {l} is not a copper layer of this board", row + 1));
                    }
                }
                if v.delay_as < 0 || v.delay_as > MAX_DELAY_AS {
                    return bad(format!("Via override {}: the delay is from 0 to 2147 ps", row + 1));
                }
            }
        }
        if self.class_profiles.len() > MAX_PROFILES {
            return Err(CheckResult::fail("ops_bad_tuning_profiles", "class_profiles", format!("At most {MAX_PROFILES} net classes name a profile")));
        }
        for (class, profile) in &self.class_profiles {
            if class.is_empty() || profile.len() > MAX_PROFILE_NAME || profile.chars().any(|c| c.is_control()) {
                return Err(CheckResult::fail("ops_bad_tuning_profiles", if class.is_empty() { "(unnamed class)" } else { class }, "A net class and the profile it names are named with no control characters, at most 256 characters"));
            }
        }
        Ok(())
    }

    /// The page as it would be stored: a class that names no profile is no entry.
    pub fn canonical(&self) -> TuningProfiles {
        TuningProfiles { profiles: self.profiles.clone(), class_profiles: self.class_profiles.iter().filter(|(_, p)| !p.is_empty()).map(|(c, p)| (c.clone(), p.clone())).collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copper() -> Vec<String> {
        crate::rules::copper_layer_names(4)
    }

    fn profile(name: &str) -> TuningProfile {
        TuningProfile { name: name.into(), profile_type: ProfileType::Single, target_impedance: 50.0, enable_time_domain_tuning: true, layer_entries: vec![ProfileLayerEntry { signal_layer: "F.Cu".into(), top_reference_layer: None, bottom_reference_layer: Some("In1.Cu".into()), width_nm: 150, diff_pair_gap_nm: 0, delay_as_per_mm: 5_600_000 }], via_propagation_delay_as: 20_000_000, via_overrides: vec![] }
    }

    #[test]
    fn formatting_starts_at_iso_128_and_refuses_a_ratio_a_line_cannot_have() {
        let f = FormattingSettings::default();
        assert!(f.is_default() && f.validate().is_ok() && !f.applies_any());
        assert_eq!((f.dash_length_ratio, f.gap_length_ratio), (12.0, 3.0));
        assert_eq!(serde_json::from_str::<FormattingSettings>("{}").unwrap(), f, "a design from before the page loads as the default");
        for (bad, loc) in [(FormattingSettings { dash_length_ratio: 0.0, ..f.clone() }, "dash_length_ratio"), (FormattingSettings { gap_length_ratio: -1.0, ..f.clone() }, "gap_length_ratio"), (FormattingSettings { dash_length_ratio: f64::NAN, ..f.clone() }, "dash_length_ratio"), (FormattingSettings { gap_length_ratio: 1_000.5, ..f.clone() }, "gap_length_ratio")] {
            let e = bad.validate().unwrap_err();
            assert_eq!((e.check.as_str(), e.location.as_deref()), ("ops_bad_formatting", Some(loc)));
        }
        assert!(FormattingSettings { apply_to_fp_text: true, ..f }.applies_any());
    }

    #[test]
    fn hatch_offsets_are_for_copper_layers_and_a_zero_offset_is_none() {
        let page: HatchOffsets = [("F.Cu".to_string(), HatchOffset { x_nm: 1_500_000, y_nm: -2_000_000 }), ("In1.Cu".to_string(), HatchOffset::default())].into();
        assert!(validate_hatch_offsets(&page, &copper()).is_ok());
        assert_eq!(canonical_hatch_offsets(&page).keys().collect::<Vec<_>>(), ["F.Cu"]);
        assert_eq!(validate_hatch_offsets(&page, &crate::rules::copper_layer_names(2)).unwrap_err().location.as_deref(), Some("In1.Cu"), "a layer the board does not have");
        let far: HatchOffsets = [("F.Cu".to_string(), HatchOffset { x_nm: 2_000_000_000, y_nm: 0 })].into();
        assert_eq!(validate_hatch_offsets(&far, &copper()).unwrap_err().location.as_deref(), Some("F.Cu"));
        let silk: HatchOffsets = [("F.SilkS".to_string(), HatchOffset { x_nm: 1, y_nm: 0 })].into();
        assert!(validate_hatch_offsets(&silk, &copper()).is_err(), "hatch offsets are for copper");
    }

    #[test]
    fn the_tuning_patterns_start_as_pns_meander_settings_and_are_checked_like_a_meander() {
        let p = TuningPatterns::default();
        assert!(p.is_default() && p.validate().is_ok());
        assert_eq!((p.single_track.spacing_nm, p.diff_pair.spacing_nm, p.diff_pair_skew.spacing_nm), (600_000, 1_000_000, 600_000), "a differential pair's spacing is 1 mm, the others 0.6 mm");
        assert_eq!((p.single_track.min_amplitude_nm, p.single_track.max_amplitude_nm, p.single_track.corner_style, p.single_track.corner_radius_percentage, p.single_track.single_sided), (200_000, 1_000_000, CornerStyle::Round, 80, false));
        assert_eq!(serde_json::from_str::<TuningPatterns>("{}").unwrap(), p);
        let with = |f: &dyn Fn(&mut TuningPatterns)| {
            let mut q = TuningPatterns::default();
            f(&mut q);
            q.validate().unwrap_err()
        };
        assert_eq!(with(&|q| q.diff_pair.max_amplitude_nm = 0).location.as_deref(), Some("diff_pair.max_amplitude"));
        assert_eq!(with(&|q| q.single_track.min_amplitude_nm = 2_000_000).location.as_deref(), Some("single_track.min_amplitude"), "the minimum is above the maximum");
        assert_eq!(with(&|q| q.diff_pair_skew.corner_radius_percentage = 101).location.as_deref(), Some("diff_pair_skew.corner_radius_percentage"));
        assert_eq!(with(&|q| q.single_track.spacing_nm = 200_000_000).location.as_deref(), Some("single_track.spacing"));
        assert_eq!((CornerStyle::from_code(0), CornerStyle::from_code(1), CornerStyle::Chamfer.code(), CornerStyle::Round.code()), (CornerStyle::Chamfer, CornerStyle::Round, 0, 1));
    }

    #[test]
    fn a_profile_must_be_named_and_name_each_layer_once() {
        let ok = TuningProfiles { profiles: vec![profile("50 ohm")], class_profiles: [("Default".to_string(), "50 ohm".to_string())].into() };
        assert!(ok.validate(&copper()).is_ok() && ok.missing().is_empty());
        let with = |f: &dyn Fn(&mut TuningProfile)| {
            let mut p = profile("50 ohm");
            f(&mut p);
            TuningProfiles { profiles: vec![p], class_profiles: BTreeMap::new() }.validate(&copper()).unwrap_err()
        };
        let e = with(&|p| p.name.clear());
        assert_eq!((e.location.as_deref(), e.hint.as_deref()), (Some("profile 1"), Some("Tuning profile must have a name")));
        let e = with(&|p| p.layer_entries.push(p.layer_entries[0].clone()));
        assert_eq!((e.location.as_deref(), e.hint.as_deref()), (Some("50 ohm"), Some("Duplicated signal layer configuration in tuning profile")));
        assert!(with(&|p| p.layer_entries[0].signal_layer = "B.SilkS".into()).hint.unwrap().contains("not a copper layer"));
        assert!(with(&|p| p.layer_entries[0].bottom_reference_layer = Some("In9.Cu".into())).hint.unwrap().contains("reference layer"));
        assert!(with(&|p| p.target_impedance = -1.0).hint.unwrap().contains("ohms"));
        assert!(with(&|p| p.layer_entries[0].delay_as_per_mm = i64::MAX).hint.unwrap().contains("delay"));
        assert!(with(&|p| p.via_overrides.push(ViaOverride { signal_layer_from: "F.Cu".into(), signal_layer_to: "In7.Cu".into(), via_layer_from: "F.Cu".into(), via_layer_to: "B.Cu".into(), delay_as: 0 })).hint.unwrap().contains("Via override 1"));
    }

    #[test]
    fn a_class_that_names_a_profile_the_page_does_not_have_is_what_kicad_reports_as_missing() {
        let page = TuningProfiles { profiles: vec![profile("50 ohm"), profile("")], class_profiles: [("Default".to_string(), "50 ohm".to_string()), ("USB".to_string(), "90 ohm diff".to_string()), ("Power".to_string(), String::new()), ("Blank".to_string(), "".to_string())].into() };
        assert_eq!(page.missing(), vec![("USB", "90 ohm diff")], "a class that names nothing names no missing profile, and an unnamed profile is no profile");
        assert_eq!(page.canonical().class_profiles.len(), 2, "a class that names nothing is not stored");
        assert_eq!(page.names().into_iter().collect::<Vec<_>>(), ["50 ohm"]);
        let e = TuningProfiles { profiles: vec![], class_profiles: [(String::new(), "x".to_string())].into() }.validate(&copper()).unwrap_err();
        assert_eq!(e.location.as_deref(), Some("(unnamed class)"));
    }
}

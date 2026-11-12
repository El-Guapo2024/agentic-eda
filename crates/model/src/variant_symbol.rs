//! A design variant's own symbol (KiCad 25ec2335 Set Variant Symbol..., `SCH_SYMBOL_VARIANT::m_SymbolOverride`; UI-LEFT-2 SS16): which library symbol may
//! stand in for a placed symbol's own in a variant, ported from `eeschema/variant_symbol_utils.cpp` (`ValidateVariantSymbolCompatibility`,
//! `validateCompatibility`, `VariantSymbolPinsMatch`).
//!
//! Every graphical pin of the base symbol must have a pin of the candidate with the same number, unit, body style, position and electrical type; the
//! candidate must have at least as many units, and alternate body styles when the base has them. Pin names, the body graphics and the fields are not
//! checked. A pin's body style here is the body it is drawn in: 1 for [`LibSymbol::pins`], 2 for the alternate body's (`LIB_SYMBOL::GetGraphicalPins`
//! lists every body's pins with its `GetBodyStyle()`); a pin both bodies share is listed in each, in base and candidate alike.

use crate::symbol::{LibPin, LibSymbol};

/// `VARIANT_COMPAT_ERROR`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatError {
    /// A base pin number is absent from the candidate for a given unit / body style.
    MissingPinNumber,
    /// The candidate has a pin occurrence that is absent from the base.
    ExtraPinNumber,
    /// A matching pin number has a different position in library coordinates.
    PinPositionMismatch,
    /// A matching pin number has a different electrical type.
    PinTypeMismatch,
    /// The candidate has fewer units than the base.
    InsufficientUnits,
    /// The base uses alternate body styles but the candidate does not.
    MissingBodyStyle,
}

/// `VARIANT_COMPAT_RESULT`: one problem, with KiCad's human-readable `detail`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatIssue {
    pub error: CompatError,
    pub detail: String,
}

/// `PIN_COMPARISON_DATA` of one graphical pin: number, unit, body style, position (in schematic IU, 100 nm, as KiCad compares and prints them) and type.
#[derive(Debug, Clone, PartialEq)]
struct PinSlot {
    number: String,
    unit: u32,
    body_style: u32,
    position: (i64, i64),
    pin_type: String,
}

/// Library millimetres in schematic IU (`schIUScale`: 10000 per mm), rounded as KiCad's parser rounds a coordinate.
fn iu(mm: f64) -> i64 {
    (mm * 10_000.0).round() as i64
}

/// `GetGraphicalPins()`' comparison data: the normal body's pins as body style 1, the alternate body's as 2. `inherit` reads as `unspecified`
/// ("Library pins without a concrete type resolve to unspecified, as with SCH_PIN::GetType()").
fn slots(sym: &LibSymbol) -> Vec<PinSlot> {
    let slot = |p: &LibPin, style: u32| PinSlot {
        number: p.number.clone(),
        unit: p.unit,
        body_style: style,
        position: (iu(p.at.x), iu(p.at.y)),
        pin_type: if p.electrical_type == "inherit" { "unspecified".to_string() } else { p.electrical_type.clone() },
    };
    let own = sym.drawn_style();
    let mut out: Vec<PinSlot> = sym.pins.iter().map(|p| slot(p, own)).collect();
    if let Some(alt) = &sym.alternate {
        out.extend(alt.pins.iter().map(|p| slot(p, alt.style)));
    }
    out
}

/// `ValidateVariantSymbolCompatibility( aBase, aCandidate )`: every problem found, in KiCad's order (units, body styles, then each base pin, then the
/// candidate's extra pins); empty when `candidate` may be the variant symbol of a symbol drawn from `base`.
pub fn validate_compatibility(base: &LibSymbol, candidate: &LibSymbol) -> Vec<CompatIssue> {
    let mut out = Vec::new();
    let base_units = base.unit_count.max(1);
    let cand_units = candidate.unit_count.max(1);
    if cand_units < base_units {
        out.push(CompatIssue { error: CompatError::InsufficientUnits, detail: format!("Candidate has {cand_units} unit(s), base requires {base_units}") });
    }
    let base_styles = base.body_style_count();
    let cand_styles = candidate.body_style_count();
    if base_styles > 1 && cand_styles < base_styles {
        out.push(CompatIssue { error: CompatError::MissingBodyStyle, detail: format!("Candidate has {cand_styles} body style(s), base requires {base_styles}") });
    }
    let base_pins = slots(base);
    let cand_pins = slots(candidate);
    let mut matched = vec![false; cand_pins.len()];
    for b in &base_pins {
        let same_slot = |c: &PinSlot| c.number == b.number && c.unit == b.unit && c.body_style == b.body_style;
        let find = |matched: &[bool], position: bool, kind: bool| {
            (0..cand_pins.len()).find(|&i| !matched[i] && same_slot(&cand_pins[i]) && (!position || cand_pins[i].position == b.position) && (!kind || cand_pins[i].pin_type == b.pin_type))
        };
        let Some(i) = find(&matched, true, true).or_else(|| find(&matched, true, false)).or_else(|| find(&matched, false, false)) else {
            out.push(CompatIssue { error: CompatError::MissingPinNumber, detail: format!("Unit {}, body style {}: pin '{}' not found in candidate", b.unit, b.body_style, b.number) });
            continue;
        };
        matched[i] = true;
        let c = &cand_pins[i];
        if c.position != b.position {
            out.push(CompatIssue {
                error: CompatError::PinPositionMismatch,
                detail: format!("Unit {}, body style {}: pin '{}' position mismatch ({},{}) vs ({},{})", b.unit, b.body_style, b.number, b.position.0, b.position.1, c.position.0, c.position.1),
            });
        }
        if c.pin_type != b.pin_type {
            out.push(CompatIssue { error: CompatError::PinTypeMismatch, detail: format!("Unit {}, body style {}: pin '{}' type mismatch ('{}' vs '{}')", b.unit, b.body_style, b.number, b.pin_type, c.pin_type) });
        }
    }
    for (i, c) in cand_pins.iter().enumerate() {
        if !matched[i] {
            out.push(CompatIssue { error: CompatError::ExtraPinNumber, detail: format!("Unit {}, body style {}: candidate has extra pin '{}'", c.unit, c.body_style, c.number) });
        }
    }
    out
}

/// `SCH_EDIT_TOOL::SetVariantSymbol`'s message for an incompatible pick (its `wxMessageBox` titled "Variant Symbol Compatibility"): the two library ids, a
/// bullet line per issue, and the advice.
pub fn incompatible_message(picked: &str, base: &str, issues: &[CompatIssue]) -> String {
    let mut msg = format!("The selected symbol '{picked}' is not pin-compatible with the base symbol '{base}'.\n\n");
    for issue in issues {
        msg.push_str("\u{2022} ");
        msg.push_str(&issue.detail);
        msg.push('\n');
    }
    msg.push('\n');
    msg.push_str("Choose a symbol with equivalent pins and pin positions.");
    msg
}

/// `SCH_SYMBOL::GetVariantLibSymbol( aVariantName, aPath )`: the library symbol a placed symbol drawn from `own_lib_id` (whose library symbol is `base`)
/// is drawn as in a variant whose record names `override_id`: none when the record names the symbol's own library id, when `find` has no symbol of that
/// id ("Failed resolutions are cached as nullptr"), or when it is not pin-compatible with `base` (`!ValidateVariantSymbolCompatibility( *m_part, *cached
/// ).empty()`); a symbol with no library symbol of its own takes any.
pub fn resolve_override(base: Option<&LibSymbol>, own_lib_id: &str, override_id: &str, find: impl FnOnce(&str) -> Option<LibSymbol>) -> Option<LibSymbol> {
    if override_id == own_lib_id {
        return None;
    }
    let alt = find(override_id)?;
    match base {
        Some(b) if !validate_compatibility(b, &alt).is_empty() => None,
        _ => Some(alt),
    }
}

/// The text the variant symbol `alt` gives a field it has a non-empty text for (`SCH_SYMBOL::GetFieldText`: `altSymbol->GetField( aFieldName )` "&&
/// !altField->GetText().IsEmpty()"): its Value, Footprint, Datasheet, Description or a user field. `Reference` is never a variant's.
pub fn field_of<'a>(alt: &'a LibSymbol, name: &str) -> Option<&'a str> {
    let text = match name {
        "Reference" => return None,
        "Value" => alt.connectivity.value.as_str(),
        "Footprint" => alt.connectivity.footprint.as_str(),
        "Datasheet" => alt.datasheet.as_str(),
        "Description" => alt.description.as_str(),
        other => alt.connectivity.user_fields.iter().find(|(n, _)| n == other).map_or("", |(_, t)| t.as_str()),
    };
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SPoint;

    fn pin(number: &str, x: f64, y: f64, kind: &str) -> LibPin {
        LibPin { number: number.into(), name: String::new(), electrical_type: kind.into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: 0.0, length_mm: 2.54, unit: 1, hidden: false }
    }

    fn sym(pins: Vec<LibPin>, units: u32) -> LibSymbol {
        let mut s = crate::symbol::builtin("Device:R").expect("the built-in resistor");
        s.pins = pins;
        s.unit_count = units;
        s.alternate = None;
        s
    }

    #[test]
    fn a_symbol_with_the_same_pins_is_compatible_and_each_difference_is_reported_as_kicad_words_it() {
        let base = sym(vec![pin("1", 0.0, 3.81, "passive"), pin("2", 0.0, -3.81, "passive")], 1);
        assert!(validate_compatibility(&base, &base.clone()).is_empty());
        let moved = sym(vec![pin("1", 0.0, 5.08, "passive"), pin("2", 0.0, -3.81, "input"), pin("3", 1.0, 0.0, "passive")], 1);
        let issues: Vec<String> = validate_compatibility(&base, &moved).into_iter().map(|i| i.detail).collect();
        assert_eq!(
            issues,
            vec![
                "Unit 1, body style 1: pin '1' position mismatch (0,38100) vs (0,50800)".to_string(),
                "Unit 1, body style 1: pin '2' type mismatch ('passive' vs 'input')".to_string(),
                "Unit 1, body style 1: candidate has extra pin '3'".to_string(),
            ]
        );
        let fewer = sym(vec![pin("1", 0.0, 3.81, "passive")], 1);
        let issues = validate_compatibility(&base, &fewer);
        assert_eq!(issues[0].detail, "Unit 1, body style 1: pin '2' not found in candidate");
        let mut two = base.clone();
        two.unit_count = 2;
        assert_eq!(validate_compatibility(&two, &base)[0].detail, "Candidate has 1 unit(s), base requires 2");
        let msg = incompatible_message("Device:C", "Device:R", &issues);
        assert!(msg.starts_with("The selected symbol 'Device:C' is not pin-compatible with the base symbol 'Device:R'.\n\n\u{2022} Unit 1"));
        assert!(msg.ends_with("\n\nChoose a symbol with equivalent pins and pin positions."));
    }
}

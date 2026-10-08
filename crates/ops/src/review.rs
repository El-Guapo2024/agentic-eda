//! The DRC and ERC review workflow's verbs: waiving a DRC violation (`pcbnew/dialogs/dialog_drc.cpp`) and
//! choosing how severely each ERC check is reported (`common/dialogs/panel_setup_severities.cpp` on the
//! schematic side, `eeschema/dialogs/dialog_schematic_setup.cpp`).
//!
//! Both are design data, so both are undoable verbs and both reach kicad-cli through the derived project
//! (`eda_kicad::export_kicad_pro_for`): the exclusions as `board.design_settings.drc_exclusions`, the ERC
//! severities as `erc.rule_severities`. The board's own check severities are Board Setup's
//! (`board_setup::set_severities`).

use std::collections::BTreeMap;

use eda_model::ir::{Design, DrawingsSection, DrcExclusion, DrcExclusionKey};
use eda_model::{CheckResult, erc_checks, rules};

type Done = Result<(), Vec<CheckResult>>;

fn drawings(design: &mut Design) -> &mut DrawingsSection {
    design.drawings.get_or_insert_with(Default::default)
}

/// `Cmd::AddDrcExclusions`: `DIALOG_DRC::OnDRCItemRClick`'s "Exclude this violation", "Exclude with comment...", "Edit exclusion
/// comment..." and "Exclude all violations of ..." (and `ExcludeMarker`), as one step. An exclusion whose key `(check, items)` is
/// already listed is replaced -- that is how a comment is edited, and adding a waived violation again is no error.
pub(crate) fn add_drc_exclusions(design: &mut Design, exclusions: &[DrcExclusion]) -> Done {
    if exclusions.is_empty() {
        return Err(vec![CheckResult::fail("ops_bad_drc_exclusion", "drc_exclusions", "no violation to exclude")]);
    }
    for e in exclusions {
        if e.check.trim().is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_drc_exclusion", "check", "an exclusion names the check (`clearance`, ...) that found the violation")]);
        }
        if e.items.is_empty() || e.items.iter().any(|i| i.trim().is_empty()) {
            return Err(vec![CheckResult::fail("ops_bad_drc_exclusion", e.check.as_str(), "an exclusion names the items of the violation (their uuids in the derived board): this one has none")]);
        }
        if !e.ids.is_empty() && e.ids.len() != e.items.len() {
            return Err(vec![CheckResult::fail("ops_bad_drc_exclusion", e.check.as_str(), "`ids` is the items' own ids, one for each of `items`")]);
        }
    }
    let list = &mut drawings(design).drc_exclusions;
    for e in exclusions {
        let mut e = e.clone();
        e.positions_nm.sort_unstable();
        e.positions_nm.dedup();
        match list.iter_mut().find(|x| x.matches(&e.check, &e.items)) {
            Some(slot) => *slot = e,
            None => list.push(e),
        }
    }
    list.sort();
    Ok(())
}

/// `Cmd::DeleteDrcExclusions`: "Remove exclusion for this violation" (and "Remove all exclusions ..."). Refused when none of the keys is
/// listed -- the violation was never waived (or another editor just put it back).
pub(crate) fn delete_drc_exclusions(design: &mut Design, keys: &[DrcExclusionKey]) -> Done {
    let list = &mut drawings(design).drc_exclusions;
    let before = list.len();
    list.retain(|e| !keys.iter().any(|k| e.matches(&k.check, &k.items)));
    if list.len() == before {
        let what = keys.first().map_or("drc_exclusions", |k| k.check.as_str());
        return Err(vec![CheckResult::fail("ops_unknown_drc_exclusion", what, "no exclusion with this check and these items exists")]);
    }
    Ok(())
}

/// `Cmd::SetErcSeverities`: Schematic Setup > Violation Severity (`panel_setup_severities.cpp`), the whole table. `severities` names the
/// checks whose severity differs from KiCad's default for them; a check at its default is dropped so the table only holds choices, and an
/// empty one is "KiCad's defaults".
pub(crate) fn set_erc_severities(design: &mut Design, severities: &BTreeMap<String, String>) -> Done {
    for (key, severity) in severities {
        if !erc_checks::is_erc_check(key) {
            return Err(vec![CheckResult::fail("ops_bad_severity", key.as_str(), "not an ERC check (see Schematic Setup > Violation Severity for the list)")]);
        }
        if !rules::is_severity(severity) {
            return Err(vec![CheckResult::fail("ops_bad_severity", key.as_str(), format!("{severity:?} is not error, warning or ignore"))]);
        }
    }
    let sch = design.schematic.get_or_insert_with(crate::empty_schematic_section);
    sch.extras.erc_severities = severities.iter().filter(|(k, v)| erc_checks::default_severity(k) != Some(v.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
    Ok(())
}

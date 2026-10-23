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
use eda_model::sch_extras::{DEFAULT_CONNECTION_GRID_MILS, MAX_CONNECTION_GRID_MILS, MIN_CONNECTION_GRID_MILS};
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

/// `Cmd::ClearDrcExclusions`: Delete All Markers with the exclusions (`DIALOG_DRC::OnDeleteAllClick`). Refused when no violation is waived.
pub(crate) fn clear_drc_exclusions(design: &mut Design) -> Done {
    let list = &mut drawings(design).drc_exclusions;
    if list.is_empty() {
        return Err(vec![CheckResult::fail("ops_unknown_drc_exclusion", "drc_exclusions", "no violation is excluded")]);
    }
    list.clear();
    Ok(())
}

/// `Cmd::SetErcSeverities`: Schematic Setup > Violation Severity (`panel_setup_severities.cpp`), the whole table, stored as sent (the page
/// names the checks whose severity differs from KiCad's default, and the ones the table already named: the derived project ignores the two
/// library-link checks unless the table says otherwise, so going back to their default has to be said, as for the board's table). An empty
/// table is "KiCad's defaults".
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
    sch.extras.erc_severities = severities.clone();
    Ok(())
}

/// `Cmd::SetIntersheetRefs`: Schematic Setup > Formatting > Inter-sheet References (`PANEL_SETUP_FORMATTING::TransferDataFromWindow` takes the controls as they are:
/// "Show inter-sheet references", "Show own page reference", "Standard (1,2,3)" or "Abbreviated (1..3)", the prefix and the suffix). Kept on the root sheet; KiCad's
/// defaults are the absence of a setting. Refused when nothing changes.
pub(crate) fn set_intersheet_refs(design: &mut Design, settings: &eda_model::sch_extras::IntersheetRefs) -> Done {
    let sch = design.schematic.get_or_insert_with(crate::empty_schematic_section);
    let now = (*settings != eda_model::sch_extras::IntersheetRefs::default()).then(|| settings.clone());
    if sch.extras.intersheet_refs == now {
        return Err(vec![CheckResult::fail("ops_no_change", "intersheet_refs", "the inter-sheet references are already set so")]);
    }
    sch.extras.intersheet_refs = now;
    Ok(())
}

/// `Cmd::SetConnectionGrid`: Schematic Setup > Formatting > Connection grid spacing (`PANEL_SETUP_FORMATTING::TransferDataFromWindow` validates the value
/// between `MIN_CONNECTION_GRID_MILS` and 10000 mils). KiCad's default, 50 mils, is the absence of a setting.
pub(crate) fn set_connection_grid(design: &mut Design, mils: u32) -> Done {
    if !(MIN_CONNECTION_GRID_MILS..=MAX_CONNECTION_GRID_MILS).contains(&mils) {
        return Err(vec![CheckResult::fail("ops_bad_connection_grid", format!("{mils}"), format!("the connection grid is from {MIN_CONNECTION_GRID_MILS} to {MAX_CONNECTION_GRID_MILS} mils"))]);
    }
    let sch = design.schematic.get_or_insert_with(crate::empty_schematic_section);
    sch.extras.connection_grid_mils = (mils != DEFAULT_CONNECTION_GRID_MILS).then_some(mils);
    Ok(())
}

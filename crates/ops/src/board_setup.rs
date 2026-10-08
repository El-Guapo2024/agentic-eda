//! Board Setup's verbs (`pcbnew/dialogs/dialog_board_setup.cpp` and its pages): each one stores a page of
//! the dialog in `design.drawings.rules` ([`eda_model::rules::RulesOverlay`]), and the loader lays the
//! overlay over the intent's rules (`crates/cli/src/board.rs::load`). Same shape as
//! `Cmd::SetTeardropSettings`/`SetDimensionSettings`: whole page in, whole page replaced, one undo step
//! (the design snapshot the CLI keeps takes the overlay back with everything else).
//!
//! Every verb validates like the page it comes from -- `BOARD_DESIGN_SETTINGS::ValidateDesignRules` and
//! the panels' own checks -- and refuses with the setting's name in `location`, so the dialog can put the
//! message next to the field.

use std::collections::BTreeMap;

use eda_model::ir::Design;
use eda_model::rules::{self, Constraints, MaskPaste, NetClassSettings, RulesOverlay, StackupSettings, TextGraphicsDefaults};
use eda_model::{CheckResult, ConstraintModel};

type Done = Result<(), Vec<CheckResult>>;

fn overlay(design: &mut Design) -> &mut RulesOverlay {
    design.drawings.get_or_insert_with(Default::default).rules.get_or_insert_with(Default::default)
}

/// The first `(setting, message)` a page's range check found, as a refusal.
fn first(check: &str, problems: Vec<(&'static str, String)>) -> Done {
    match problems.into_iter().next() {
        Some((setting, message)) => Err(vec![CheckResult::fail(check, setting, message)]),
        None => Ok(()),
    }
}

/// `Cmd::SetNetClasses`.
pub(crate) fn set_net_classes(design: &mut Design, settings: &NetClassSettings) -> Done {
    rules::validate_net_classes(settings).map_err(|c| vec![c])?;
    overlay(design).net_classes = Some(settings.clone());
    Ok(())
}

/// `Cmd::SetConstraints`.
pub(crate) fn set_constraints(design: &mut Design, constraints: &Constraints) -> Done {
    first("ops_bad_constraints", constraints.validate())?;
    overlay(design).constraints = Some(constraints.clone());
    Ok(())
}

/// `Cmd::SetMaskPaste`.
pub(crate) fn set_mask_paste(design: &mut Design, settings: &MaskPaste) -> Done {
    first("ops_bad_mask_paste", rules::validate_mask_paste(settings))?;
    overlay(design).mask_paste = Some(settings.clone());
    Ok(())
}

/// `Cmd::SetTextGraphicsDefaults`.
pub(crate) fn set_text_graphics(design: &mut Design, settings: &TextGraphicsDefaults) -> Done {
    first("ops_bad_text_defaults", settings.validate())?;
    overlay(design).text_graphics = Some(*settings);
    Ok(())
}

/// `Cmd::SetStackup`: the copper layer count and the layers' thickness.
///
/// Fewer copper layers than the board has is refused while anything is still on a layer that would go
/// (KiCad asks, then deletes those items; an edit that deletes copper is not one Undo can be trusted to
/// bring back cleanly from a settings dialog, so it is the user's decision to move them first).
pub(crate) fn set_stackup(design: &mut Design, model: &ConstraintModel, settings: &StackupSettings) -> Done {
    settings.validate().map_err(|why| vec![CheckResult::fail("ops_bad_stackup", "stackup", why)])?;
    let keep = rules::copper_layer_names(settings.copper_layers as usize);
    let gone: Vec<&String> = model.board.layers.iter().filter(|l| !keep.contains(l)).collect();
    if !gone.is_empty() {
        let mut used: BTreeMap<&str, usize> = BTreeMap::new();
        if let Some(rt) = &design.routing {
            for t in &rt.tracks {
                *used.entry(t.layer.as_str()).or_default() += 1;
            }
            for z in &rt.zones {
                *used.entry(z.layer.as_str()).or_default() += 1;
            }
            for v in &rt.vias {
                for l in [&v.from_layer, &v.to_layer] {
                    *used.entry(l.as_str()).or_default() += 1;
                }
            }
        }
        for l in &gone {
            if let Some(n) = used.get(l.as_str()) {
                return Err(vec![CheckResult::fail("ops_bad_stackup", l.as_str(), format!("{n} routed item(s) are on {l}, which a {}-layer board does not have; move or delete them first", settings.copper_layers))]);
            }
        }
    }
    overlay(design).stackup = Some(settings.clone());
    Ok(())
}

/// `Cmd::SetRuleSeverities`: DRC settings key -> `error` | `warning` | `ignore`, for any of the checks Violation
/// Severity lists.
pub(crate) fn set_severities(design: &mut Design, severities: &BTreeMap<String, String>) -> Done {
    for (key, severity) in severities {
        if !rules::DRC_CHECKS.iter().any(|(k, _)| k == key) {
            return Err(vec![CheckResult::fail("ops_bad_severity", key.as_str(), "not a DRC check (see Violation Severity for the list)")]);
        }
        if !rules::is_severity(severity) {
            return Err(vec![CheckResult::fail("ops_bad_severity", key.as_str(), format!("{severity:?} is not error, warning or ignore"))]);
        }
    }
    overlay(design).severities = Some(severities.clone());
    Ok(())
}

/// The longest `.kicad_dru` text the dialog stores (a real rules file is a few kilobytes).
pub const MAX_CUSTOM_RULES_BYTES: usize = 256 * 1024;

/// `Cmd::SetCustomRules`: the text of the board's `.kicad_dru`, stored as written. Whether kicad-cli accepts it is
/// asked separately (`POST /api/check_rules`, which runs kicad-cli on it); like KiCad's own panel, saving rules
/// that do not compile is allowed -- DRC then runs without them.
pub(crate) fn set_custom_rules(design: &mut Design, text: &str) -> Done {
    if text.len() > MAX_CUSTOM_RULES_BYTES {
        return Err(vec![CheckResult::fail("ops_bad_custom_rules", "custom_rules", format!("the rules text is {} bytes; the most stored is {MAX_CUSTOM_RULES_BYTES}", text.len()))]);
    }
    if text.contains('\0') {
        return Err(vec![CheckResult::fail("ops_bad_custom_rules", "custom_rules", "the rules text has a NUL byte in it")]);
    }
    overlay(design).custom_rules = Some(text.to_string());
    Ok(())
}

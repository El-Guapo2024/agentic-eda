//! The DRC Rule Editor's verb (`pcbnew.DRETool.drcRuleEditor`, `pcbnew/drc/rule_editor/dialog_drc_rule_editor.cpp`): `Cmd::EditDrcRule`. Each of the
//! dialog's edits that writes the rules file -- Save on a rule (`saveRule`), Delete (`RemoveRule`), Move Up and Move Down -- reads the board's
//! `.kicad_dru` text as the editor reads it (`eda_drc::rule_editor::load`), changes one rule and stores the text the saver writes
//! (`DRC_RULE_SAVER::GenerateRulesText`) as Board Setup > Custom Rules stores it (`set_custom_rules`). One undo step each. New and duplicated rules live in
//! the dialog until they are saved, as KiCad's do (`IsNew`); kicad-cli is what judges the board by the rules.
//!
//! An edit names the rule it changes by its place in the editor's list (`entry`) and by the name the dialog read (`name`): when the file changed since
//! (an agent's edit, Board Setup's text), the verb refuses rather than change another rule -- KiCad's tool rebuilds the dialog when the file changes on disk
//! (`DRC_RULE_EDITOR_TOOL::ShowDRCRuleEditorDialog`).

use eda_drc::rule_editor::{self, EditorBoard, Entry, RuleEdit};
use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel};
use serde::{Deserialize, Serialize};

type Done = Result<(), Vec<CheckResult>>;

/// What `Cmd::EditDrcRule` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrcRuleAction {
    /// Save on a rule panel: `rule` replaces the rule at `entry`, or is added after the others when `entry` is none (a new or duplicated rule).
    Save,
    Delete,
    MoveUp,
    MoveDown,
}

/// The editor's view of the board (`EditorBoard`): its copper stack, its enabled layers in `UIOrder`, and the names the user gave them.
pub fn editor_board(model: &ConstraintModel) -> EditorBoard {
    let setup = &model.board.layer_setup;
    let mut enabled = model.board.layers.clone();
    enabled.extend(setup.canonical().enabled.into_iter().filter(|l| eda_model::layers::is_optional_layer(l)));
    EditorBoard { copper: model.board.layers.clone(), enabled, names: setup.names.clone() }
}

/// The text of the board's rules file: what Board Setup stored, else what the board came with.
pub fn rules_text(model: &ConstraintModel) -> String {
    model.board.custom_rules_text.clone().unwrap_or_default()
}

/// The editor's list of the board's rules, or why the file does not open.
pub fn entries(model: &ConstraintModel, board: &EditorBoard) -> Result<Vec<Entry>, String> {
    rule_editor::load(&rules_text(model), &board.env()).map_err(|e| format!("The rules file does not load, so the editor cannot show it: {e}. Edit it as text in Board Setup > Custom Rules."))
}

fn refuse(message: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail("ops_bad_drc_rule", "custom_rules", message)]
}

/// `Cmd::EditDrcRule`.
pub(crate) fn edit_drc_rule(design: &mut Design, model: &ConstraintModel, action: DrcRuleAction, entry: Option<usize>, name: &str, rule: Option<&RuleEdit>) -> Done {
    let board = editor_board(model);
    let mut list = entries(model, &board).map_err(refuse)?;
    if let Some(i) = entry {
        match list.get(i) {
            Some(e) if e.rule_name == name => {}
            _ => return Err(refuse(format!("The rules changed since the editor read them (no rule {name:?} at {i}); reopen the DRC Rule Editor."))),
        }
    }
    match action {
        DrcRuleAction::Save => {
            let rule = rule.ok_or_else(|| refuse("Save needs the rule the panel holds"))?;
            rule_editor::apply_edit(&mut list, entry, rule, &board).map_err(refuse)?;
        }
        DrcRuleAction::Delete => {
            let i = entry.ok_or_else(|| refuse("Delete needs the rule"))?;
            rule_editor::delete_entry(&mut list, i).map_err(refuse)?;
        }
        DrcRuleAction::MoveUp | DrcRuleAction::MoveDown => {
            let i = entry.ok_or_else(|| refuse("Move needs the rule"))?;
            if rule_editor::move_entry(&mut list, i, action == DrcRuleAction::MoveUp).is_none() {
                return Err(vec![CheckResult::fail("ops_nothing_to_change", "custom_rules", "the rule is already the first (last) of its kind")]);
            }
        }
    }
    crate::board_setup::set_custom_rules(design, &rule_editor::save(&list, &board))
}

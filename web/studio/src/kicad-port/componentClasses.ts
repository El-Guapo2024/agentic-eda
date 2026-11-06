// The logic behind Board Setup > Design Rules > Component Classes (pcbnew/dialogs/panel_assign_component_classes.cpp, the panel; its base layout
// panel_assign_component_classes_base.cpp; common/project/component_class_settings.cpp, what the page stores), as pure functions: the condition rows the
// "+" menu adds and what each starts with, what a row's fields are sent as (`CONDITION_DATA::GetPrimaryField` / `GetSecondaryField`), the assignment a panel
// stands for (`PANEL_COMPONENT_CLASS_ASSIGNMENT::GenerateAssignmentData`), the page's Validate message and "Import references from selection".
//
// The page itself (components/boardSetup/ComponentClassesPage.tsx) holds the draft and renders; its OK is one undoable command (`set_component_classes`, crates/ops/src/
// board_setup.rs), which compiles every assignment as the board does (`COMPONENT_CLASS_MANAGER::CompileAssignmentRule`) and refuses with the same message.
// Whether an assignment compiles and which footprints it matches is the rule engine's to say (`POST /api/component_classes`, eda_drc::rule_board), never
// worked out here.

import type { ClassAssignment, ClassCondition, ClassConditionKind, ComponentClassSettings } from "../api/types";

/** `PANEL_COMPONENT_CLASS_ASSIGNMENT::OnAddConditionClick`'s menu, in its order: what each entry adds. */
export const CONDITION_MENU: ReadonlyArray<{ kind: ClassConditionKind; label: string }> = [
  { kind: "reference", label: "Add Reference Condition" },
  { kind: "footprint", label: "Add Footprint Condition" },
  { kind: "side", label: "Add Side Condition" },
  { kind: "rotation", label: "Add Rotation Condition" },
  { kind: "footprint_field", label: "Add Footprint Field Value Condition" },
  { kind: "sheet_name", label: "Add Sheet Condition" },
  { kind: "custom", label: "Add Custom Expression Condition" },
];

/** Each condition panel's `m_title` (`PANEL_COMPONENT_CLASS_CONDITION_*_BASE`). */
export const CONDITION_TITLE: Readonly<Record<ClassConditionKind, string>> = {
  reference: "Reference:",
  footprint: "Footprint:",
  side: "Side:",
  rotation: "Rotation:",
  footprint_field: "Footprint field:",
  custom: "Custom:",
  sheet_name: "Sheet name:",
};

/** `PANEL_COMPONENT_CLASS_CONDITION_SIDE`'s read-only choice, "Any" first and selected. */
export const SIDE_CHOICES: readonly string[] = ["Any", "Front", "Back"];

/** `PANEL_COMPONENT_CLASS_CONDITION_ROTATION`'s list (the box also takes a typed angle), "Any" first and selected; the unit label is `°`. */
export const ROTATION_CHOICES: readonly string[] = ["Any", "0", "90", "180", "-90"];

/** One condition row as the page edits it: the kind and the text of its one or two fields as typed. */
export interface ConditionRow {
  kind: ClassConditionKind;
  primary: string;
  secondary: string;
}

/** One assignment panel as the page edits it (`PANEL_COMPONENT_CLASS_ASSIGNMENT`). */
export interface AssignmentDraft {
  componentClass: string;
  /** "Match all" (true, the panel's default) or "Match any". */
  all: boolean;
  conditions: ConditionRow[];
}

/** The page (`PANEL_ASSIGN_COMPONENT_CLASSES`): "Assign component class per sheet" and the custom assignments. */
export interface ComponentClassesDraft {
  sheetClasses: boolean;
  assignments: AssignmentDraft[];
}

/**
 * A row `PANEL_COMPONENT_CLASS_ASSIGNMENT::AddCondition` adds: empty fields, but the side and rotation boxes, which their constructors set to "Any"
 * (`m_side->SetValue( wxT( "Any" ) )`, `m_rotation->SetValue( wxT( "Any" ) )`).
 */
export function newCondition(kind: ClassConditionKind): ConditionRow {
  return { kind, primary: kind === "side" || kind === "rotation" ? "Any" : "", secondary: "" };
}

/** `OnAddAssignmentClick` -> `addAssignment`: an empty panel, "Match all" (the radio's `SetValue( true )` in the base). */
export function newAssignment(): AssignmentDraft {
  return { componentClass: "", all: true, conditions: [] };
}

/** `TransferDataToWindow`: the page as the project has it -- each assignment's class and operator, its conditions in order (`SetPrimaryField` / `SetSecondaryField`). */
export function draftOf(settings: ComponentClassSettings | undefined): ComponentClassesDraft {
  return {
    sheetClasses: settings?.sheet_classes ?? false,
    assignments: (settings?.assignments ?? []).map((a) => ({
      componentClass: a.class,
      all: a.all ?? true,
      conditions: (a.conditions ?? []).map((c) => ({ kind: c.kind, primary: c.primary ?? "", secondary: c.secondary ?? "" })),
    })),
  };
}

/**
 * `CONDITION_DATA::GetPrimaryField` / `GetSecondaryField`: the control's text trimmed at both ends and, for every kind but a custom expression, with every `"` and
 * `'` taken out (the text goes between quotes in the rule KiCad compiles). A row without a second control (all but the footprint field) has no second field.
 */
export function conditionField(kind: ClassConditionKind, text: string, secondary = false): string {
  if (secondary && kind !== "footprint_field") return "";
  let data = text.trim();
  if (kind !== "custom") data = data.replace(/["']/g, "");
  return data;
}

/** `PANEL_COMPONENT_CLASS_ASSIGNMENT::GenerateAssignmentData`: the class as typed (`m_componentClass->GetValue()`, not trimmed), the operator and the conditions' fields. */
export function assignmentData(a: AssignmentDraft): ClassAssignment {
  return {
    class: a.componentClass,
    all: a.all,
    conditions: a.conditions.map((c): ClassCondition => ({ kind: c.kind, primary: conditionField(c.kind, c.primary), secondary: conditionField(c.kind, c.secondary, true) })),
  };
}

/** `TransferDataFromWindow`: the page as the project stores it (`SetEnableSheetComponentClasses`, then each panel's `GenerateAssignmentData` in order). */
export function settingsOf(draft: ComponentClassesDraft): ComponentClassSettings {
  return { sheet_classes: draft.sheetClasses, assignments: draft.assignments.map(assignmentData) };
}

/** `PANEL_ASSIGN_COMPONENT_CLASSES::Validate`'s message for an assignment that gives no rule. */
export function assignmentErrorMessage(componentClass: string): string {
  return `Error with conditions for component class assignment ${componentClass}`;
}

/**
 * `PANEL_COMPONENT_CLASS_CONDITION_REFERENCE::onMenu` ("Import references from selection"): the references of the footprints that were selected when the dialog
 * opened, each once, sorted (`wxString::Cmp`), joined by a comma. `null` when there are none (the menu entry is disabled: `menu.Enable( ID_IMPORT_REFS,
 * !m_selectionRefs.empty() )`).
 */
export function selectionRefsText(refs: Iterable<string>): string | null {
  const sorted = [...new Set(refs)].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  return sorted.length === 0 ? null : sorted.join(",");
}

/** Whether two pages store the same thing (the Apply that would change nothing). */
export function sameSettings(a: ComponentClassSettings, b: ComponentClassSettings | undefined): boolean {
  const norm = (s: ComponentClassSettings | undefined) =>
    JSON.stringify({
      sheet: s?.sheet_classes ?? false,
      assignments: (s?.assignments ?? []).map((x) => ({ c: x.class, all: x.all ?? true, conds: (x.conditions ?? []).map((c) => [c.kind, c.primary ?? "", c.secondary ?? ""]) })),
    });
  return norm(a) === norm(b);
}

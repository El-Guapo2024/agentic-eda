// Board Setup > Component Classes' logic against KiCad's panel (pcbnew/dialogs/panel_assign_component_classes.cpp): the "+" menu, what a new row starts
// with, what a row's fields are sent as (`CONDITION_DATA::GetPrimaryField`), the assignment a panel stands for, the Validate message and the import of the
// selection's references.
import { test } from "node:test";
import assert from "node:assert/strict";
import { CONDITION_MENU, CONDITION_TITLE, assignmentData, assignmentErrorMessage, conditionField, draftOf, newAssignment, newCondition, sameSettings, selectionRefsText, settingsOf } from "./componentClasses";

test("the + menu lists KiCad's seven conditions in its order, with its words", () => {
  assert.deepEqual(
    CONDITION_MENU.map((m) => m.label),
    ["Add Reference Condition", "Add Footprint Condition", "Add Side Condition", "Add Rotation Condition", "Add Footprint Field Value Condition", "Add Sheet Condition", "Add Custom Expression Condition"]
  );
  assert.deepEqual(CONDITION_MENU.map((m) => m.kind), ["reference", "footprint", "side", "rotation", "footprint_field", "sheet_name", "custom"]);
  assert.equal(CONDITION_TITLE.footprint_field, "Footprint field:");
  assert.equal(CONDITION_TITLE.sheet_name, "Sheet name:");
});

test("a new side or rotation row starts at Any, the others empty; a new assignment matches all", () => {
  assert.deepEqual(newCondition("side"), { kind: "side", primary: "Any", secondary: "" });
  assert.deepEqual(newCondition("rotation"), { kind: "rotation", primary: "Any", secondary: "" });
  assert.deepEqual(newCondition("reference"), { kind: "reference", primary: "", secondary: "" });
  assert.deepEqual(newAssignment(), { componentClass: "", all: true, conditions: [] });
});

test("GetPrimaryField trims and takes the quotes out, but not of a custom expression", () => {
  assert.equal(conditionField("reference", "  R1,'R2'  "), "R1,R2");
  assert.equal(conditionField("footprint", ' "Lib:FP" '), "Lib:FP");
  assert.equal(conditionField("custom", "  A.intersectsArea('RULE_1') "), "A.intersectsArea('RULE_1')");
  assert.equal(conditionField("footprint_field", " 'MPN' ", true), "MPN", "the field condition's value is its second field");
  assert.equal(conditionField("reference", "x", true), "", "a row with one control has no second field");
});

test("GenerateAssignmentData keeps the class as typed and each condition as GetPrimaryField gives it", () => {
  const data = assignmentData({ componentClass: "CAP ", all: false, conditions: [{ kind: "reference", primary: " C1,C2 ", secondary: "junk" }, { kind: "footprint_field", primary: "MY_FIELD", secondary: " 'MY_VALUE' " }] });
  assert.deepEqual(data, { class: "CAP ", all: false, conditions: [{ kind: "reference", primary: "C1,C2", secondary: "" }, { kind: "footprint_field", primary: "MY_FIELD", secondary: "MY_VALUE" }] });
  assert.equal(assignmentErrorMessage("CAP_1"), "Error with conditions for component class assignment CAP_1");
});

test("the page is read and sent back as the project has it", () => {
  const settings = { sheet_classes: true, assignments: [{ class: "CAP_1", conditions: [{ kind: "side" as const, primary: "Front" }] }, { class: "ANY", all: false, conditions: [] }] };
  const draft = draftOf(settings);
  assert.equal(draft.sheetClasses, true);
  assert.deepEqual(draft.assignments[0], { componentClass: "CAP_1", all: true, conditions: [{ kind: "side", primary: "Front", secondary: "" }] });
  assert.ok(sameSettings(settingsOf(draft), settings), "nothing to apply when nothing changed");
  assert.ok(!sameSettings(settingsOf({ ...draft, sheetClasses: false }), settings));
  assert.deepEqual(draftOf(undefined), { sheetClasses: false, assignments: [] });
});

test("Import references from selection: each reference once, sorted, comma separated; none disables the entry", () => {
  assert.equal(selectionRefsText(["U2", "R10", "R2", "U2"]), "R10,R2,U2", "wxString::Cmp order");
  assert.equal(selectionRefsText([]), null);
});

import { test } from "node:test";
import assert from "node:assert/strict";
import type { SchField } from "../api/types";
import { autoplaceKeptCmd, canMoveDown, canMoveUp, columnMenu, DEFAULT_SHOWN_COLUMNS, FIELD_COLUMNS, fieldOrderCmd, MANDATORY_ROWS, moveRow, orientationChoice, orientationCmd, shownColumns, shownColumnsText, toggleColumn } from "./symbolFieldsGrid";
import { fieldLookCmds, lookOf } from "./symbolPropsDialog";

test("the grid has FIELDS_GRID_TABLE's schematic columns, the first eight shown at open, and the label menu toggles each", () => {
  // `GetColLabelValue` up to `FDC_SCH_EDIT_COUNT`: no Private (the Symbol Editor's only, `getColumnCount`)
  assert.deepEqual(
    FIELD_COLUMNS.map((c) => c.label),
    ["Name", "Value", "Show", "Show Name", "H Align", "V Align", "Italic", "Bold", "Text Size", "Orientation", "X Position", "Y Position", "Font", "Color", "Allow Autoplacement"]
  );
  // `ShowHideColumns( "0 1 2 3 4 5 6 7" )`
  const shown = shownColumns(DEFAULT_SHOWN_COLUMNS);
  assert.deepEqual([...shown], ["name", "value", "shown", "show_name", "h_align", "v_align", "italic", "bold"]);
  assert.equal(shownColumnsText(shown), DEFAULT_SHOWN_COLUMNS);
  // a number that is no column, and extra white space, are passed over
  assert.equal(shownColumnsText(shownColumns(" 1\t14  99 -1 x")), "1 14");
  // `onGridLabelRightClick`: a check item per column; `doPopupSelection` shows a hidden one, hides a shown one
  const menu = columnMenu(shown);
  assert.equal(menu.length, 15);
  assert.deepEqual(menu.filter((m) => m.checked).length, 8);
  const more = toggleColumn(shown, "pos_x");
  assert.ok(more.has("pos_x") && !shown.has("pos_x"), "the set given is not changed");
  assert.ok(!toggleColumn(more, "name").has("name"));
});

test("Move Up and Move Down keep the mandatory rows first, and OK's order is sent only when it is not the server's", () => {
  // `OnMoveUp`: row > mandatory; `OnMoveDown`: row >= mandatory and not the last
  assert.equal(MANDATORY_ROWS, 4);
  assert.deepEqual([3, 4, 5].map((r) => canMoveUp(r)), [false, false, true]);
  assert.deepEqual([3, 4, 5, 6].map((r) => canMoveDown(r, 7)), [false, true, true, false]);
  assert.deepEqual(moveRow(["a", "b", "c"], 1, true), ["b", "a", "c"]);
  assert.deepEqual(moveRow(["a", "b", "c"], 2, false), ["a", "b", "c"], "the last row goes no further down");
  // the order the server gives after the dialog's other edits: the fields the symbol had in their order, then the added ones by name
  assert.equal(fieldOrderCmd("R1", ["MPN", "Note"], ["MPN", "Note", "Zed", "Aaa"]) && true, true, "added fields not in name order: sent");
  assert.equal(fieldOrderCmd("R1", ["MPN", "Note"], ["MPN", "Note", "Aaa", "Zed"]), null);
  assert.equal(fieldOrderCmd("R1", ["MPN", "Note", "Tol"], ["MPN", "Tol"]), null, "a field removed changes nothing of the order");
  assert.deepEqual(fieldOrderCmd("R1#2", ["MPN", "Note"], ["Note", "MPN"]), { op: "sch_edit", verb: "reorder_fields", key: "R1#2", names: ["Note", "MPN"] });
});

test("Angle and Mirror read GetOrientation's parts and set them absolutely", () => {
  assert.deepEqual(orientationChoice(270, "x"), { angle: 270, mirror: "x" });
  assert.deepEqual(orientationChoice(-90, null), { angle: 270, mirror: null });
  assert.deepEqual(orientationChoice(45, undefined), { angle: 0, mirror: null }, "a turn that is no quarter shows 0");
  assert.equal(orientationCmd("U1", 2, { angle: 90, mirror: null }, { angle: 90, mirror: null }), null);
  assert.deepEqual(orientationCmd("U1", 2, { angle: 90, mirror: null }, { angle: 180, mirror: "y" }), { op: "sch_edit", verb: "set_symbol_orientation", id: "U1", unit: 2, angle: 180, mirror: "y" });
  assert.deepEqual(orientationCmd("#PWR01", undefined, { angle: 0, mirror: null }, { angle: 0, mirror: "x" }), { op: "sch_edit", verb: "set_symbol_orientation", id: "#PWR01", angle: 0, mirror: "x" });
  assert.deepEqual(autoplaceKeptCmd("U1#2"), { op: "sch_edit", verb: "autoplace_fields_kept", keys: ["U1#2"] });
});

test("the Orientation, X / Y Position and Allow Autoplacement cells read the field's own frame and send Symbol Properties' field edit", () => {
  const f: SchField = { name: "Value", text: "10k", at: [1_000_000, 2_000_000], vertical: false, h: "left", v: "center", visible: true, id: "fld:R1:Value", local: [2_540_000, -1_270_000], local_vertical: true, allow_autoplace: true };
  const was = lookOf(f);
  assert.deepEqual([was.xMm, was.yMm, was.vertical, was.allowAutoplace], [2.54, -1.27, true, true]);
  assert.deepEqual(fieldLookCmds("fld:R1:Value", was, was, true), [], "nothing changed: nothing sent");
  const cmds = fieldLookCmds("fld:R1:Value", was, { ...was, xMm: 5.08, vertical: false, allowAutoplace: false }, true);
  assert.deepEqual(cmds, [{ op: "sch_edit", verb: "edit_field", id: "fld:R1:Value", local_vertical: false, local_at: { x: 5_080_000, y: -1_270_000 }, allow_autoplace: false, keep_autoplaced: true }]);
  // a field the server sent no own-frame place for has no place cells to change, and Field Properties' path keeps its old form
  const bare = lookOf({ ...f, local: undefined, local_vertical: undefined });
  assert.equal(bare.xMm, undefined);
  assert.deepEqual(fieldLookCmds("fld:R1:Value", bare, { ...bare, bold: true }), [{ op: "sch_edit", verb: "edit_field", id: "fld:R1:Value", bold: true }]);
});

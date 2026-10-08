import { test } from "node:test";
import assert from "node:assert/strict";
import { deleteCmds, type SchIds } from "./schDelete";

const sch: SchIds = {
  symbols: [{ id: "U1" }],
  wires: [{ id: "wire_a" }],
  labels: [{ id: "lbl_a" }],
  texts: [{ id: "txt_a" }],
  power_symbols: [{ id: "#PWR01" }],
  no_connects: [{ id: "nc_a" }],
  bus_entries: [{ id: "bent_a" }],
  junctions: [{ id: "jct_a" }],
  lines: [{ id: "sln_a" }],
  graphics: [{ id: "shp_a" }, { id: "tbox_a" }],
  sheets: [{ id: "sheet_a" }],
};

test("every item kind has its own delete verb", () => {
  const cmds = deleteCmds(sch, ["U1", "wire_a", "lbl_a", "txt_a", "#PWR01", "nc_a", "bent_a", "jct_a", "sln_a", "shp_a", "sheet_a"], new Set());
  assert.deepEqual(
    cmds.map((c) => (c.op === "sch_edit" ? `sch_edit:${c.verb}` : c.op)),
    ["delete_symbol", "delete_wire", "delete_label", "delete_sch_text", "delete_power_symbol", "delete_no_connect", "delete_bus_entry", "delete_junction", "delete_sch_line", "sch_edit:delete_graphic", "sch_edit:delete_sheet"]
  );
});

test("locked items and unknown ids are skipped", () => {
  const cmds = deleteCmds(sch, ["U1", "txt_a", "nope"], new Set(["txt_a"]));
  assert.deepEqual(cmds, [{ op: "delete_symbol", id: "U1" }]);
});

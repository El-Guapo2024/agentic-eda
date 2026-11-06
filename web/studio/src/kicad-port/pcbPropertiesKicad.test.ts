// The board editor's Properties panel against KiCad's own registrations: for every item class the panel lists, the rows the studio's registry gives
// (`registeredRows`: every property that is not hidden from the panel, in its group and display order) are the rows KiCad's give.
//
// KiCad's side is the table of kicad-port/pcbKicadRegistrations.ts, transcribed from the `PROPERTY_MANAGER` calls of KiCad 8303b2ad -- `BOARD_ITEM_DESC` (pcbnew/board_item.cpp),
// `BOARD_CONNECTED_ITEM_DESC` (board_connected_item.cpp), `FOOTPRINT_DESC` (footprint.cpp), `PAD_DESC` (pad.cpp), `TRACK_VIA_DESC` (pcb_track.cpp),
// `EDA_TEXT_DESC` (common/eda_text.cpp), `PCB_TEXT_DESC` (pcb_text.cpp), `PCB_FIELD_DESC` (pcb_field.cpp), `EDA_SHAPE_DESC` (common/eda_shape.cpp, with
// KiCad 25ec2335's line endings after "Fill Color"), `PCB_SHAPE_DESC` (pcb_shape.cpp), `DIMENSION_DESC` and its five subclasses (pcb_dimension.cpp) and
// `PCB_GROUP_DESC` (pcb_group.cpp) -- call for call, in the order each file makes them, and replayed through the same port of the manager
// (`propertyManager.ts`, whose walk has tests of its own). A hidden property (`SetIsHiddenFromPropertiesManager`) is in the table so the walk sees what
// KiCad's sees, and is not a row. Zones, text boxes, tables, barcodes, points and the KiCad 10 items are not in this table.
import { test } from "node:test";
import assert from "node:assert/strict";
import { registeredRows } from "./propertyGrid";
import { PCB_PROPERTIES } from "./pcbProperties";

import { kicadPcbRegistry as kicadRegistry } from "./pcbKicadRegistrations";

const CLASSES = ["FOOTPRINT", "PAD", "PCB_TRACK", "PCB_ARC", "PCB_VIA", "PCB_TEXT", "PCB_FIELD", "PCB_SHAPE", "PCB_DIM_ALIGNED", "PCB_DIM_ORTHOGONAL", "PCB_DIM_RADIAL", "PCB_DIM_LEADER", "PCB_DIM_CENTER", "PCB_GROUP"];

for (const type of CLASSES) {
  test(`${type}: the studio's rows are KiCad's, in KiCad's groups and order`, () => {
    assert.deepEqual(registeredRows(PCB_PROPERTIES, type), registeredRows(kicadRegistry(), type));
  });
}

test("the table itself: a pad's rows as KiCad 8303b2ad lists them (the walk puts BOARD_ITEM's and BOARD_CONNECTED_ITEM's first)", () => {
  const rows = registeredRows(kicadRegistry(), "PAD");
  assert.deepEqual(rows.slice(0, 4), ["/Position X", "/Position Y", "/Net", "/Orientation"]);
  assert.ok(rows.includes("Teardrops/Enable Teardrops") && rows.includes("Overrides/Thermal Relief Gap"));
  assert.equal(rows.filter((r) => r.startsWith("Pad Properties/")).length, 16);
});

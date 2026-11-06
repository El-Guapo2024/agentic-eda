// Bus notation in net class patterns against KiCad's NET_SETTINGS (common/project/net_settings.cpp: ParseBusVector, ParseBusGroup, ForEachBusMember):
// the cases both ports are held to (src/kicad/bus_patterns.json, written by eda_model::bus_pattern's test), and what the Net Classes page does with them.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { busMembers, expandPatterns, parseBusGroup, parseBusVector } from "./busPattern";
import { applyAssignments } from "./boardSetupRules";

const fixture = JSON.parse(readFileSync("src/kicad/bus_patterns.json", "utf8")) as { pattern: string; members: string[] }[];

test("each pattern stands for the members the backend expands it to (src/kicad/bus_patterns.json)", () => {
  assert.ok(fixture.length >= 20);
  for (const c of fixture) assert.deepEqual(busMembers(c.pattern), c.members, c.pattern);
});

test("a vector and a group, and what is neither", () => {
  assert.deepEqual(parseBusVector("D[0..7]")?.length, 8);
  assert.deepEqual(parseBusVector("USB_D[PM]"), null, "a regular expression's class is no range");
  assert.deepEqual(parseBusGroup("{SDA SCL}"), ["SDA", "SCL"]);
  assert.deepEqual(parseBusGroup("V*"), null);
  assert.deepEqual(expandPatterns(["D[0..1]", "D1", "GND"]), ["D0", "D1", "GND"], "an exact repeat once (addSinglePatternAssignment)");
});

test("the page's rows are stored as their members", () => {
  const classes = applyAssignments([{ name: "bus", nets: [], priority: 0 }], [{ pattern: "D[0..2]", netclass: "bus" }, { pattern: " D1 ", netclass: "bus" }, { pattern: "{SDA SCL}", netclass: "bus" }]);
  assert.deepEqual(classes[0]!.nets, ["D0", "D1", "D2", "SDA", "SCL"]);
});

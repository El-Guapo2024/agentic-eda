import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import type { Constraints, MaskPaste, NetClass, Stackup, StackupSettings, TextGraphicsDefaults } from "../api/types";
import {
  CONSTRAINT_RANGES,
  MAXIMUM_CLEARANCE_UM,
  TEXT_GRAPHICS_ROWS,
  addAssignment,
  applyAssignments,
  assignmentRows,
  boardNetNames,
  classOfNet,
  copperLayerNames,
  copperLayersLost,
  defaultStackup,
  expandPattern,
  globMatch,
  isDielectric,
  isMaterialEditable,
  isThicknessEditable,
  mmText,
  netsMatching,
  newStackupSettings,
  proposePattern,
  severitiesToSend,
  severityOf,
  stackupThicknessUm,
  textRowOf,
  validateConstraints,
  validateMaskPaste,
  validateNetClasses,
  validateStackup,
  validateTextGraphics,
  withCopperLayers,
  withKnownKinds,
  withLineWidth,
  withTextFields,
} from "./boardSetupRules";

const cls = (name: string, nets: string[] = [], extra: Partial<NetClass> = {}): NetClass => ({ name, nets, priority: 0, ...extra });

// ---------------------------------------------------------------- patterns

test("a pattern is anchored: * is any run, ? any one character, everything else is itself", () => {
  assert.ok(globMatch("SPI_*", "SPI_MOSI"));
  assert.ok(globMatch("VBUS", "VBUS"));
  assert.ok(!globMatch("SPI_*", "I2C_SCL"));
  assert.ok(!globMatch("VBUS", "VBUS2"), "the whole name has to match");
  assert.ok(!globMatch("BUS", "VBUS"));
  assert.ok(globMatch("*", "anything"));
  assert.ok(globMatch("*", ""));
  assert.ok(globMatch("A*B*C", "AxxBxxC"));
  assert.ok(globMatch("A**C", "AC"));
  assert.ok(!globMatch("A*B*C", "AxxCxxB"));
  assert.ok(globMatch("D?", "D7") && !globMatch("D?", "D") && !globMatch("D?", "D77"));
  assert.ok(globMatch("+3.3V", "+3.3V") && !globMatch("+3.3V", "+3x3V"), "a dot is a dot, not any character");
  assert.ok(globMatch("Net-(R1-Pad1)", "Net-(R1-Pad1)"), "parentheses are the net's own characters");
  assert.ok(globMatch("A*", "A*B"), "a star in the name is matched by the pattern's star like any character");
  assert.ok(globMatch("*Pad1)", "Net-(R1-Pad1)"));
});

test("an alternation stands for one pattern per alternative", () => {
  assert.deepEqual(expandPattern("GND"), ["GND"]);
  assert.deepEqual(expandPattern("A|B|C"), ["A", "B", "C"]);
  assert.deepEqual(expandPattern("SPI_M(ISO|OSI)"), ["SPI_MISO", "SPI_MOSI"]);
  assert.deepEqual(expandPattern("D(1|2)_P"), ["D1_P", "D2_P"]);
  assert.deepEqual(expandPattern("A(|1)"), ["A", "A1"], "an empty alternative is the prefix itself");
  assert.deepEqual(expandPattern("A|"), ["A"], "nothing is not a pattern");
  assert.deepEqual(expandPattern("Net-(R1-Pad1)"), ["Net-(R1-Pad1)"], "parentheses without an alternation are part of the name");
  assert.deepEqual(expandPattern("(a|b)(c|d)"), ["(a|b)(c|d)"], "more than one group is left as written");
  assert.deepEqual(expandPattern(""), []);
});

test("the pattern proposed for the nets picked is KiCad's: the name, or the common prefix with the tails", () => {
  assert.equal(proposePattern([]), "");
  assert.equal(proposePattern(["VBUS"]), "VBUS");
  assert.equal(proposePattern(["SPI_MOSI", "SPI_MISO"]), "SPI_M(ISO|OSI)");
  assert.equal(proposePattern(["D10", "D2", "D1"]), "D(1|2|10)", "the tails are in natural order");
  assert.equal(proposePattern(["GND", "VCC"]), "GND|VCC", "no common prefix: the names joined");
  assert.equal(proposePattern(["/A", "/B"]), "/A|/B", "a lone slash is not a prefix worth factoring");
  assert.equal(proposePattern(["a2", "A1"]), "A1|a2", "case is ignored when ordering, kept when writing");
  assert.equal(proposePattern(["VBUS", "VBUS"]), "VBUS", "the same net twice is one net");
});

test("what is proposed matches exactly the nets it was proposed for", () => {
  const sets = [["SPI_MOSI", "SPI_MISO", "SPI_SCK"], ["GND", "VCC"], ["D1", "D2", "D10"], ["A", "A1"], ["/rail/3V3", "/rail/5V"]];
  const universe = ["SPI_MOSI", "SPI_MISO", "SPI_SCK", "SPI_CS", "GND", "VCC", "D1", "D2", "D10", "D3", "A", "A1", "A2", "/rail/3V3", "/rail/5V", "/rail/12V", "SPI_M"];
  for (const set of sets) {
    const got = netsMatching(proposePattern(set), universe);
    assert.deepEqual([...got].sort(), [...set].sort(), proposePattern(set));
  }
});

test("assigning a pattern adds one row per plain pattern, skips a copy, and moves it from another class", () => {
  let rows = addAssignment([], "SPI_M(ISO|OSI)", "bus");
  assert.deepEqual(rows, [{ pattern: "SPI_MISO", netclass: "bus" }, { pattern: "SPI_MOSI", netclass: "bus" }]);
  rows = addAssignment(rows, "SPI_MISO", "bus");
  assert.equal(rows.length, 2, "an exact copy is not added");
  rows = addAssignment(rows, " SPI_MISO ", "fast");
  assert.deepEqual(rows, [{ pattern: "SPI_MOSI", netclass: "bus" }, { pattern: "SPI_MISO", netclass: "fast" }], "it moved: the first class that matches would have hidden it");
  assert.deepEqual(addAssignment(rows, "", "bus"), rows, "an empty pattern adds nothing");
});

test("the assignment table and the classes' pattern lists are two views of the same thing", () => {
  const classes = [cls("power", ["VBUS", "3V3"]), cls("signal", ["SPI_*"])];
  const rows = assignmentRows(classes);
  assert.deepEqual(rows, [{ pattern: "VBUS", netclass: "power" }, { pattern: "3V3", netclass: "power" }, { pattern: "SPI_*", netclass: "signal" }]);
  assert.deepEqual(applyAssignments(classes, rows), classes);
  const edited = applyAssignments(classes, [...rows.slice(0, 2), { pattern: "I2C_S(CL|DA)", netclass: "signal" }, { pattern: "x", netclass: "gone" }]);
  assert.deepEqual(edited.map((c) => c.nets), [["VBUS", "3V3"], ["I2C_SCL", "I2C_SDA"]], "a row for a class that is not there is dropped, an alternation is expanded");
  assert.equal(edited[1]!.priority, 0, "the class keeps everything but its patterns");
});

test("the first class with a matching pattern owns the net", () => {
  const classes = [cls("usb", ["USB_*"]), cls("fast", ["USB_D?", "SPI_*"])];
  assert.equal(classOfNet(classes, "USB_DP"), "usb");
  assert.equal(classOfNet(classes, "SPI_CLK"), "fast");
  assert.equal(classOfNet(classes, "GND"), "Default");
  assert.equal(classOfNet([], "GND"), "Default");
});

test("the nets of a board are those its pads, tracks, vias and zones name", () => {
  const nets = boardNetNames({
    parts: [{ pads: [{ net: "GND" }, { net: null }, { net: "VCC" }] }, { pads: undefined }, { pads: [{ net: "GND" }] }],
    routing: { tracks: [{ net: "SIG" }, { net: "" }], vias: [{ net: "GND" }], zones: [{ net: "AGND" }] },
  });
  assert.deepEqual(nets, ["AGND", "GND", "SIG", "VCC"]);
  assert.deepEqual(boardNetNames({ parts: [], routing: null }), []);
});

// ---------------------------------------------------------------- net class checks

test("the Net Classes page is refused for what KiCad's panel refuses", () => {
  const def = cls("Default", [], { clearance: 200, track_width: 200, via_diameter: 600, via_drill: 300 });
  assert.deepEqual(validateNetClasses(def, [cls("power", ["VBUS"], { clearance: 400 })]), []);

  const names = validateNetClasses(def, [cls("A"), cls(" "), cls("a"), cls("default")]);
  assert.deepEqual(names.map((i) => [i.row, i.field, i.message]), [[2, "name", "Netclass must have a name."], [3, "name", "Netclass name already in use."], [4, "name", "Netclass name already in use."]]);

  const sizes = validateNetClasses(def, [cls("x", [], { track_width: 0, clearance: 0, via_diameter: -1, diff_pair_gap: 0, via_drill: Number.NaN, microvia_diameter: 0 })]);
  assert.deepEqual(sizes.map((i) => i.field), ["track_width", "via_diameter", "via_drill", "microvia_diameter"], "a zero clearance and a zero gap are fine; a number the field could not read is not");

  assert.equal(validateNetClasses(def, [cls("big", [], { clearance: MAXIMUM_CLEARANCE_UM + 1 })])[0]!.field, "clearance");
  assert.deepEqual(validateNetClasses(def, [cls("big", [], { clearance: MAXIMUM_CLEARANCE_UM })]), []);

  const drill = validateNetClasses(def, [cls("v", [], { via_diameter: 400, via_drill: 400 })]);
  assert.deepEqual(drill.map((i) => [i.row, i.field]), [[1, "via_drill"]]);
  assert.equal(validateNetClasses(cls("Default", [], { via_diameter: 400, via_drill: 400 }), [])[0]!.row, 0, "the Default class is checked too");

  assert.deepEqual(validateNetClasses(def, [cls("p", [], { priority: 1.5 })]).map((i) => [i.row, i.field]), [[1, "priority"]]);
  assert.deepEqual(validateNetClasses(def, [cls("p", [], { priority: -2 })]), [], "a negative priority is a priority");

  assert.equal(validateNetClasses(cls("Base"), [])[0]!.message, "The default net class is required.");
  assert.deepEqual(validateNetClasses(def, [], [{ pattern: "  ", netclass: "power" }, { pattern: "", netclass: "" }]).map((i) => [i.row, i.field]), [[0, "pattern"]], "a row with no class is not a mistake: it is dropped");
});

// ---------------------------------------------------------------- constraints, mask and paste

const constraints = (over: Partial<Constraints> = {}): Constraints => ({
  min_clearance_um: 200,
  min_connection_um: 0,
  min_track_width_um: 200,
  min_annular_width_um: 50,
  min_via_diameter_um: 500,
  min_through_hole_um: 300,
  min_microvia_diameter_um: 200,
  min_microvia_drill_um: 100,
  min_hole_to_hole_um: 250,
  min_hole_clearance_um: 250,
  min_copper_edge_clearance_um: 500,
  min_silk_clearance_um: 0,
  min_groove_width_um: 0,
  min_silk_text_height_um: 800,
  min_silk_text_thickness_um: 80,
  max_error_um: 5,
  min_resolved_spokes: 2,
  use_height_for_length_calcs: true,
  zones_allow_external_fillets: false,
  ...over,
});

test("the Constraints page is range-checked like ValidateDesignRules", () => {
  assert.deepEqual(validateConstraints(constraints()), {});
  const bad = validateConstraints(constraints({ min_clearance_um: 30_000, max_error_um: 0, min_resolved_spokes: 100 }));
  assert.deepEqual(Object.keys(bad).sort(), ["max_error_um", "min_clearance_um", "min_resolved_spokes"]);
  assert.equal(bad.min_clearance_um, "Value must be between 0 and 25 mm.");
  assert.equal(bad.max_error_um, "Value must be between 0.001 and 1 mm.");
  assert.deepEqual(validateConstraints(constraints({ min_silk_clearance_um: -5_000 })), {}, "the silk clearance may be negative, down to -10 mm");
  assert.deepEqual(Object.keys(validateConstraints(constraints({ min_silk_clearance_um: -10_001 }))), ["min_silk_clearance_um"]);
  assert.deepEqual(validateConstraints(constraints({ min_copper_edge_clearance_um: -10 })), {}, "-0.01 mm is the legacy flag value");
  assert.equal(validateConstraints(constraints({ min_track_width_um: Number.NaN })).min_track_width_um, "Value is not a number.");
  assert.ok(validateConstraints(constraints({ min_resolved_spokes: 1.5 })).min_resolved_spokes);
  assert.equal(new Set(CONSTRAINT_RANGES.map((r) => r.key)).size, CONSTRAINT_RANGES.length);
  assert.equal(mmText(25_000), "25");
  assert.equal(mmText(-10), "-0.01");
  assert.equal(mmText(0), "0");
});

test("the Solder Mask/Paste page is range-checked, the paste ratio as a fraction", () => {
  const ok: MaskPaste = { expansion_um: 50, min_width_um: 100, to_copper_clearance_um: 0, allow_bridges_in_footprints: false, tent_vias_front: true, tent_vias_back: true, paste_margin_um: -50, paste_margin_ratio: -0.05 };
  assert.deepEqual(validateMaskPaste(ok), {});
  assert.deepEqual(Object.keys(validateMaskPaste({ ...ok, expansion_um: 25_001, min_width_um: -1, paste_margin_ratio: 1.5 })).sort(), ["expansion_um", "min_width_um", "paste_margin_ratio"]);
  assert.equal(validateMaskPaste({ ...ok, paste_margin_um: Number.NaN }).paste_margin_um, "Value is not a number.");
  assert.deepEqual(validateMaskPaste({ ...ok, paste_margin_ratio: 1 }), {}, "100 % is the largest");
});

// ---------------------------------------------------------------- text and graphics

const factory = (): TextGraphicsDefaults => {
  const row = (line: number, size: number, thickness: number) => ({ line_width_um: line, text_width_um: size, text_height_um: size, text_thickness_um: thickness, italic: false, upright: true });
  return { silk: row(100, 1000, 100), copper: row(200, 1500, 300), edge_cuts_line_width_um: 50, courtyard_line_width_um: 50, fab: row(100, 1000, 150), others: row(100, 1000, 150) };
};

test("the text defaults follow the grid's own limits", () => {
  assert.deepEqual(validateTextGraphics(factory()), {});
  let t = withTextFields(factory(), "silk", { text_thickness_um: 400 }); // a quarter of the 1 mm text is 0.25
  t = withLineWidth(t, "copper", 2);
  t = withTextFields(t, "fab", { text_width_um: 300_000 });
  t = withLineWidth(t, "courtyard", 200_000);
  const bad = validateTextGraphics(t);
  assert.deepEqual(Object.keys(bad).sort(), ["copper", "courtyard", "fab", "silk"]);
  assert.match(bad.silk!, /too large.*0\.25 mm/);
  assert.match(bad.copper!, /line width/);
  assert.match(bad.fab!, /Text size/);
  assert.match(validateTextGraphics(withTextFields(factory(), "others", { text_thickness_um: 3 })).others!, /too small/);
  assert.deepEqual(Object.keys(validateTextGraphics(withTextFields(factory(), "silk", { text_thickness_um: Number.NaN }))), ["silk"]);
});

test("a row of the grid reads and writes its own fields", () => {
  const t = factory();
  assert.deepEqual(TEXT_GRAPHICS_ROWS.map((r) => r.id), ["silk", "copper", "edge_cuts", "courtyard", "fab", "others"]);
  assert.deepEqual(TEXT_GRAPHICS_ROWS.filter((r) => !r.hasText).map((r) => r.id), ["edge_cuts", "courtyard"]);
  assert.equal(textRowOf(t, "edge_cuts").line_width_um, 50);
  assert.equal(textRowOf(t, "copper").line_width_um, 200);
  assert.equal(withLineWidth(t, "edge_cuts", 80).edge_cuts_line_width_um, 80);
  assert.equal(withLineWidth(t, "fab", 120).fab.line_width_um, 120);
  assert.equal(withLineWidth(t, "fab", 120).silk.line_width_um, 100, "the other rows are left alone");
  assert.equal(withTextFields(t, "others", { italic: true }).others.italic, true);
  assert.equal(t.others.italic, false, "nothing is changed in place");
});

// ---------------------------------------------------------------- stackup

const fixture = JSON.parse(readFileSync("src/kicad/default_stackups.json", "utf8")) as {
  defaults: Array<{ copper_layers: number; board_thickness_um: number; stackup: StackupSettings["stackup"] }>;
  resizes: Array<{ from: StackupSettings; to_copper_layers: number; expected: StackupSettings }>;
};

test("the default stackups are the ones the backend computes (src/kicad/default_stackups.json)", () => {
  assert.ok(fixture.defaults.length >= 4);
  for (const d of fixture.defaults) {
    assert.deepEqual(defaultStackup(d.copper_layers, d.board_thickness_um), d.stackup, `${d.copper_layers} layers, ${d.board_thickness_um} um`);
    assert.deepEqual(newStackupSettings(d.copper_layers, d.board_thickness_um), { copper_layers: d.copper_layers, stackup: d.stackup });
    assert.ok(Math.abs(stackupThicknessUm(d.stackup) - d.board_thickness_um) <= 2, "the layers add up to the board thickness");
  }
});

test("a new copper layer count keeps what the layers that remain say (the backend's with_copper_layers)", () => {
  assert.ok(fixture.resizes.length >= 3);
  for (const r of fixture.resizes) assert.deepEqual(withCopperLayers(r.from, r.to_copper_layers), r.expected, `to ${r.to_copper_layers} layers`);
});

test("the copper layers are named front, inner and back; the stackup is checked like the backend does", () => {
  assert.deepEqual(copperLayerNames(2), ["F.Cu", "B.Cu"]);
  assert.deepEqual(copperLayerNames(6), ["F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu"]);
  const four = newStackupSettings(4, 1600);
  assert.equal(validateStackup(four), null);
  assert.match(validateStackup({ ...four, copper_layers: 3 })!, /2, 4, 6/);
  assert.match(validateStackup({ ...four, copper_layers: 34 })!, /2, 4, 6/);
  assert.match(validateStackup({ ...four, copper_layers: 6 })!, /copper layers/, "the table has to hold the layers the count names");
  const negative = { ...four, stackup: { layers: four.stackup.layers.map((l) => (l.name === "In1.Cu" ? { ...l, thickness_mm: -0.1 } : l)) } };
  assert.match(validateStackup(negative)!, /In1\.Cu/);
  assert.match(validateStackup({ ...four, stackup: { layers: four.stackup.layers.map((l) => (l.name === "In1.Cu" ? { ...l, thickness_mm: Number.NaN } : l)) } })!, /In1\.Cu/);
});

test("a stackup from before the table was editable gets its copper layers typed; a smaller board names what it would lose", () => {
  const legacy: Stackup = { layers: [{ name: "F.Cu", material: null, thickness_mm: 0.035 }, { name: "dielectric 1", material: "FR4", thickness_mm: 1.5 }, { name: "B.Cu", material: null, thickness_mm: 0.035 }] };
  const typed = withKnownKinds(legacy);
  assert.deepEqual(typed.layers.map((l) => l.kind), ["copper", undefined, "copper"]);
  assert.equal(legacy.layers[0]!.kind, undefined, "nothing is changed in place");
  assert.deepEqual(withKnownKinds(newStackupSettings(4, 1600).stackup), newStackupSettings(4, 1600).stackup, "a typed stackup is left as it is");

  const routing = {
    tracks: [{ layer: "F.Cu" }, { layer: "In1.Cu" }, { layer: "In3.Cu" }],
    vias: [{ from: "F.Cu", to: "In3.Cu" }, { from: "F.Cu", to: "B.Cu" }],
    zones: [{ layer: "In3.Cu" }, { layer: "F.SilkS" }],
  };
  assert.deepEqual(copperLayersLost(routing, 2), [{ layer: "In1.Cu", items: 1 }, { layer: "In3.Cu", items: 3 }]);
  assert.deepEqual(copperLayersLost(routing, 4), [{ layer: "In3.Cu", items: 3 }]);
  assert.deepEqual(copperLayersLost(routing, 6), []);
  assert.deepEqual(copperLayersLost(null, 2), []);
});

test("copper, dielectrics and masks have a thickness; only dielectrics a dielectric constant", () => {
  const kinds = ["Top Silk Screen", "Top Solder Paste", "Top Solder Mask", "copper", "core", "prepreg", "Bottom Solder Mask", "Bottom Solder Paste", "Bottom Silk Screen"];
  assert.deepEqual(kinds.filter(isThicknessEditable), ["Top Solder Mask", "copper", "core", "prepreg", "Bottom Solder Mask"]);
  assert.deepEqual(kinds.filter(isMaterialEditable), ["Top Silk Screen", "Top Solder Mask", "core", "prepreg", "Bottom Solder Mask", "Bottom Silk Screen"]);
  assert.deepEqual(kinds.filter(isDielectric), ["core", "prepreg"]);
  assert.equal(isThicknessEditable(undefined), false);
});

// ---------------------------------------------------------------- violation severity

test("a check has the table's severity, else KiCad's default", () => {
  const items = [{ key: "clearance", defaultSeverity: "error" as const }, { key: "silk_overlap", defaultSeverity: "warning" as const }, { key: "lib_footprint_issues", defaultSeverity: "warning" as const }];
  assert.equal(severityOf(items[0]!, {}), "error");
  assert.equal(severityOf(items[0]!, { clearance: "ignore" }), "ignore");

  // Nothing touched: only what the table already named is sent back (the app's own fixed ignores).
  assert.deepEqual(severitiesToSend(items, {}, { lib_footprint_issues: "ignore" }), { lib_footprint_issues: "ignore" });
  // A change away from the default is sent; the rest is not.
  assert.deepEqual(severitiesToSend(items, { clearance: "warning" }, { lib_footprint_issues: "ignore" }), { clearance: "warning", lib_footprint_issues: "ignore" });
  // A change back to the default is not dropped when the table named the check (or the app's fixed ignore would come back).
  assert.deepEqual(severitiesToSend(items, { lib_footprint_issues: "warning" }, { lib_footprint_issues: "ignore" }), { lib_footprint_issues: "warning" });
  // A check the table named, set back to its default by the user, stays named: it says "this is what I want".
  assert.deepEqual(severitiesToSend(items, { silk_overlap: "warning" }, { silk_overlap: "ignore" }), { silk_overlap: "warning" });
  // A check nobody touched and the table does not name is left out.
  assert.deepEqual(severitiesToSend(items, { clearance: "error" }, {}), {});
});

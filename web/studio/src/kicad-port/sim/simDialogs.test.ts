// Round 6 (r6-editors-exports S6-1..S6-6): the analysis dialog, Plot Setup, User-defined Signals and the signal filter against
// eeschema/dialogs/dialog_sim_command.cpp, dialog_user_defined_signals.cpp, common/widgets/wx_grid.cpp and common/eda_pattern_match.cpp.
import { test } from "node:test";
import assert from "node:assert/strict";
import { OPTION_ADJUST_INCLUDE_PATHS, OPTION_DEFAULT_FLAGS, buildCommand, commandDialogOptions, optionsOf, parseCommand, selectDcSources } from "./simCommand";
import { simValueNormalize, simValueToDouble } from "./spiceValue";
import { applyPlotSetup, plotSetupOf, newPlotTab } from "./simPlotTab";
import { deleteGridRows } from "./simFrame";
import { signalMatches } from "./simSignals";

test("analysis dialog's include-path box is the project's setting (TransferDataToWindow after SetSimOptions)", () => {
  // a new tab: SetSimOptions( OPTION_DEFAULT_FLAGS ), then GetFixIncludePaths() = false wins
  const off = commandDialogOptions(OPTION_DEFAULT_FLAGS, { fix_include_paths: false });
  assert.equal(off.fixIncludePaths, false);
  assert.equal(optionsOf(off) & OPTION_ADJUST_INCLUDE_PATHS, 0);
  assert.equal(off.saveAllVoltages, true);
  // an edited tab without the bit, the project's setting true: checked
  const on = commandDialogOptions(OPTION_DEFAULT_FLAGS & ~OPTION_ADJUST_INCLUDE_PATHS, { fix_include_paths: true });
  assert.equal(on.fixIncludePaths, true);
  assert.notEqual(optionsOf(on) & OPTION_ADJUST_INCLUDE_PATHS, 0);
});

test("SIM_VALUE::ToDouble reads SI notation and SIM_VALUE::Normalize writes it", () => {
  assert.equal(simValueToDouble("2M"), 2e6);
  assert.equal(simValueToDouble("1.5m"), 1.5e-3);
  assert.equal(simValueToDouble("10k"), 1e4);
  assert.equal(simValueToDouble("1e3"), 1000);
  assert.equal(simValueToDouble("-2.5u"), -2.5 * 10 ** -6);
  assert.equal(simValueToDouble(".5"), 0.5);
  // unreadable or empty is the default, NaN
  assert.ok(Number.isNaN(simValueToDouble("")));
  assert.ok(Number.isNaN(simValueToDouble("abc")));
  assert.ok(Number.isNaN(simValueToDouble("1Meg")));
  assert.ok(Number.isNaN(simValueToDouble(" 1")));
  assert.equal(simValueNormalize(0.0015), "1.5m");
  assert.equal(simValueNormalize(12), "12");
  assert.equal(simValueNormalize(0), "0");
  assert.equal(simValueNormalize(2e6), "2M");
  assert.equal(simValueNormalize(-3300), "-3.3k");
});

test("plot setup prefills the tick range and reads SI values (SetPlotSettings, ApplySettings)", () => {
  const tab = newPlotTab(".tran 1u 1m");
  const y1 = tab.y1!;
  y1.locked = false;
  y1.tickValues = [0, 2, 4, 6, 8, 10, 12];
  let set = plotSetupOf(tab);
  assert.equal(set.lock1, false);
  assert.equal(set.min1, "0");
  assert.equal(set.max1, "12");
  y1.locked = true;
  y1.axisMin = 0.0015;
  y1.axisMax = 2;
  set = plotSetupOf(tab);
  assert.equal(set.min1, "1.5m");
  assert.equal(set.max1, "2");
  applyPlotSetup(tab, { ...set, max1: "2M" });
  assert.equal(y1.axisMax, 2e6);
  assert.equal(y1.axisMin, 0.0015);
  // text SIM_VALUE cannot read is NaN, as KiCad's
  applyPlotSetup(tab, { ...set, min1: "abc" });
  assert.ok(Number.isNaN(y1.axisMin));
});

test("user signals Delete removes the cursor row (WX_GRID::OnDeleteRows)", () => {
  const rows = [
    { id: 0, s: "V(a)*2" },
    { id: 1, s: "V(b)" },
    { id: 2, s: "V(c)" },
  ];
  const r = deleteGridRows(rows, [], 1);
  assert.deepEqual(r.rows.map((x) => x.id), [0, 2]);
  assert.equal(r.cursor, 0);
  // selected rows win over the cursor
  assert.deepEqual(deleteGridRows(rows, [0, 2], 1).rows.map((x) => x.id), [1]);
  // no cursor, nothing selected: nothing happens
  assert.deepEqual(deleteGridRows(rows, [], -1).rows.map((x) => x.id), [0, 1, 2]);
  // the first row deleted: the cursor stays on row 0
  assert.equal(deleteGridRows(rows, [], 0).cursor, 0);
});

test("a DC source is selected only where updateDCSources lists it (parseCommand's SetStringSelection)", () => {
  const items = [
    { ref: "BAT1", device: "V" },
    { ref: "V1", device: "V" },
    { ref: "R1", device: "R" },
  ];
  const names = (ref: string) => ({ BAT1: "VBAT1", V1: "V1", R1: "R1" })[ref] ?? "";
  // TEMP in capitals: the list is "TEMP"
  let f = selectDcSources(parseCommand(".dc TEMP -40 85 5"), items);
  assert.deepEqual(buildCommand(f, names), { command: ".dc TEMP -40 85 5" });
  // `updateDCSources( 't' )` is case-sensitive: lower-case temp lists nothing, so KiCad refuses it rather than writing an empty name
  f = selectDcSources(parseCommand(".dc temp -40 85 5"), items);
  assert.equal(f.dcType1, "TEMP");
  assert.deepEqual(buildCommand(f, names), { error: "A DC source must be specified." });
  // the exporter's name is not a reference of the list
  f = selectDcSources(parseCommand(".dc VBAT1 0 5 0.1"), items);
  assert.deepEqual(buildCommand(f, names), { error: "A DC source must be specified." });
  // a listed reference, matched case-insensitively (FindString)
  f = selectDcSources(parseCommand(".dc v1 0 5 0.1"), items);
  assert.equal(f.dcSource1, "");
  f = selectDcSources(parseCommand(".dc V1 0 5 0.1 R1 1k 2k 1k"), items);
  assert.equal(f.dcSource1, "V1");
  assert.equal(f.dcSource2, "R1");
});

test("the signal filter takes a regular expression only as ^..$ or /../ (EDA_COMBINED_MATCHER CTX_SIGNAL)", () => {
  assert.equal(signalMatches("V(OUT)", "V(OUT)"), true);
  assert.equal(signalMatches("V(OUT)", "V(VOUT)"), false);
  assert.equal(signalMatches("V(OUT)", "I(VOUT)"), false);
  assert.equal(signalMatches("/^I/", "I(R1)"), true);
  assert.equal(signalMatches("/^I/", "V(I1)"), false);
  assert.equal(signalMatches("R1.", "I(R10)"), false);
  assert.equal(signalMatches("^V\\(A\\)$", "V(A)"), true);
  assert.equal(signalMatches("v(*", "V(x)"), true);
  assert.equal(signalMatches("", "V(x)"), true);
});

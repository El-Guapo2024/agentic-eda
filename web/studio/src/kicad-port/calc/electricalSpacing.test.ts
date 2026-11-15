// The IPC 2221 page, Board Classes and Galvanic Corrosion against values worked out by hand from KiCad 25ec2335's tables (panel_electrical_spacing_ipc2221.cpp
// `clist`, panel_board_class.cpp `clist`, panel_galvanic_corrosion.cpp `m_entries` and `fillTable`), and the printf conversions they print with.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ipc2221Table } from "./electricalSpacing";
import { boardClassCells } from "./boardClass";
import { CORROSION_ENTRIES, corrosionCell, corrosionLabel, corrosionTable } from "./galvanicCorrosion";
import { formatFixed0, printfG as formatG, kiRound } from "./cformat";
import { UNIT_MIL, UNIT_MM, UNIT_MICRON } from "./unitSelectors";

test("%g, %.0f and KiROUND as glibc / KiCad print and round", () => {
  assert.equal(formatG(0.5), "0.5");
  assert.equal(formatG(1.968503937007874), "1.9685");
  assert.equal(formatG(123456789), "1.23457e+08");
  assert.equal(formatG(0.0000123), "1.23e-05");
  assert.equal(formatG(100000), "100000");
  assert.equal(formatG(1e6), "1e+06");
  assert.equal(formatG(999999.5), "1e+06");
  assert.equal(formatG(0.0001), "0.0001");
  assert.equal(formatG(Number.NaN), "nan");
  assert.equal(formatG(0), "0");
  assert.equal(formatFixed0(-1980.0000000000002), "-1980");
  assert.equal(formatFixed0(2.5), "2");
  assert.equal(formatFixed0(3.5), "4");
  assert.equal(formatFixed0(-0.2), "-0");
  assert.equal(kiRound(-2.5), -3);
  assert.equal(kiRound(2.5), 3);
});

test("IPC 2221: below 500 V the voltage is 500 V; 600 V adds 100 V of the per-volt row; units scale the cells", () => {
  const t = ipc2221Table("500", UNIT_MM);
  assert.equal(t.voltageText, "500");
  assert.deepEqual(t.cells[0], ["0.05", "0.1", "0.1", "0.075", "0.075", "0.13", "0.13", "0.13"]);
  assert.deepEqual(t.cells[8], ["0.25", "2.5", "12.5", "1.6", "0.8", "0.8", "1.5", "3"]);
  assert.deepEqual(t.cells[9], t.cells[8]);
  assert.equal(ipc2221Table("120", UNIT_MM).voltageText, "500");
  assert.equal(ipc2221Table("", UNIT_MM).voltageText, "500");
  const t600 = ipc2221Table("600", UNIT_MM);
  // 0.25 + 0.0025 * 100 = 0.5; 2.5 + 0.5 = 3; 12.5 + 2.5 = 15; 1.6 + 0.305; 3 + 0.61
  assert.deepEqual(t600.cells[9], ["0.5", "3", "15", "1.905", "1.105", "1.105", "1.805", "3.61"]);
  assert.equal(ipc2221Table("1000 V", UNIT_MM).cells[9]![0], "1.5");
  assert.equal(ipc2221Table("500", UNIT_MIL).cells[0]![0], "1.9685");
  assert.equal(ipc2221Table("500", UNIT_MICRON).cells[0]![0], "50");
  const bad = ipc2221Table("abc", UNIT_MM);
  assert.equal(bad.voltageText, "nan");
  assert.equal(bad.cells[9]![0], "nan");
  assert.equal(bad.cells[0]![0], "0.05");
});

test("Board Classes: the six classes in mm and in mil, -- where a class has no value", () => {
  const mm = boardClassCells(UNIT_MM);
  assert.deepEqual(mm[0], ["0.8", "0.5", "0.31", "0.21", "0.15", "0.12"]);
  assert.deepEqual(mm[2], ["--", "--", "0.45", "0.34", "0.24", "0.2"]);
  assert.deepEqual(mm[4], ["1.57", "1.13", "0.9", "--", "--", "--"]);
  assert.equal(boardClassCells(UNIT_MIL)[0]![0], "31.4961");
});

test("Galvanic Corrosion: differences in mV, colours by sign and threshold, labels", () => {
  assert.equal(CORROSION_ENTRIES.length, 39);
  const rh = CORROSION_ENTRIES[0]!;
  const mg = CORROSION_ENTRIES[38]!;
  const cu = CORROSION_ENTRIES.find((x) => x.symbol === "Cu")!;
  const au = CORROSION_ENTRIES.find((x) => x.symbol === "Au")!;
  // Rh - Mg = -1.98 V: warm hue, KiROUND( 1.98 * 99 ) = 196 -> (59, 26, 3), white text
  assert.deepEqual(corrosionCell(rh, mg, 0), { text: "-1980", bg: [59, 26, 3], fg: [255, 255, 255] });
  // Mg - Rh = +1.98 V: cold hue (30, 30, 50)
  assert.deepEqual(corrosionCell(mg, rh, 0).bg, [30, 30, 50]);
  assert.deepEqual(corrosionCell(cu, cu, 0), { text: "0", bg: [193, 231, 255], fg: [0, 0, 0] });
  // Cu - Au = 0.44 V = 440 mV: within a 500 mV threshold -> color_ok, black text (luminance 0.2126*122 + 0.7152*166 + 0.0722*194 = 158.7)
  assert.deepEqual(corrosionCell(cu, au, 500), { text: "440", bg: [122, 166, 194], fg: [0, 0, 0] });
  // above 300 mV: cold, KiROUND( 0.44 * 99 ) = 44 -> (182, 182, 202)
  assert.deepEqual(corrosionCell(cu, au, 300).bg, [182, 182, 202]);
  // a threshold that is not a number: every difference counts as within it
  assert.deepEqual(corrosionCell(cu, au, Number.NaN).bg, [122, 166, 194]);
  assert.equal(corrosionTable(0).length, 39);
  assert.equal(corrosionLabel(cu, true), "Cu");
  assert.equal(corrosionLabel(cu, false), "Copper");
});

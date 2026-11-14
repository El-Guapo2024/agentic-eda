import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibPin, Schematic } from "../api/types";
import { autostartEvent, danglingState, labelIsBus } from "./schAutostart";
import { allPinItems } from "./schPins";

const pin = (number: string, y: number, angle: number, extra: Partial<LibPin> = {}): LibPin => ({ number, name: `P${number}`, electrical_type: "passive", shape: "line", at: [0, y], angle_deg: angle, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, ...extra });

// R1 at (50, 50) mm: pin 1 above, pin 2 below, pin 3 hidden at the middle; a wire from pin 2's end down, a label at the wire's far end, one free label, a bus with a
// bus entry, a sheet with a pin.
function sheet(extra: Partial<Schematic> = {}): Schematic {
  const base = {
    lib_symbols: { "Dev:R": { graphics: [], pins: [pin("1", 3.81, 270), pin("2", -3.81, 90), pin("3", 0, 0, { hidden: true })], pin_name_offset: 0 } },
    symbols: [{ id: "R1", lib_id: "Dev:R", at: [50_000_000, 50_000_000], rot: 0, mirror: null, unit: 1, body_style: 1, value: null, pins: [] }],
    power_symbols: [],
    wires: [],
    no_connects: [],
    labels: [],
    texts: [],
    title_block: null,
    bus_entries: [],
    sheets: [],
    sheet_path: [],
  } as unknown as Schematic;
  return { ...base, ...extra } as Schematic;
}

const tip = (sch: Schematic, n: string) => allPinItems(sch).find((p) => p.number === n)!.tip;

test("a pin with nothing at its end is dangling and a click on it starts a wire; one a wire ends on does not", () => {
  const bare = sheet();
  const t1 = tip(bare, "1");
  const t2 = tip(bare, "2");
  const sch = sheet({ wires: [{ id: "w1", net: "N", pins: [], bus: false, pts: [t2, [t2[0], t2[1] + 5_080_000]] }] } as Partial<Schematic>);
  const d = danglingState(sch);
  assert.ok(d.pins.has("pin:R1:1") && !d.pins.has("pin:R1:2"));
  assert.deepEqual(autostartEvent(sch, "pin:R1:1", t1, { showHiddenPins: false }), { tool: "wire", at: [t1[0], t1[1]] });
  assert.equal(autostartEvent(sch, "pin:R1:2", t2, { showHiddenPins: false }), null, "connected");
  // a click on the symbol at a pin's end is the pin's (`case SCH_SYMBOL_T`: `GetPin( pos )`), and away from it nothing
  assert.deepEqual(autostartEvent(sch, "R1", t1, { showHiddenPins: false })?.tool, "wire");
  assert.equal(autostartEvent(sch, "R1", [t1[0] + 1_270_000, t1[1]], { showHiddenPins: false }), null);
  // a hidden pin only with hidden pins shown
  const t3 = tip(sch, "3");
  assert.equal(autostartEvent(sch, "R1", t3, { showHiddenPins: false }), null);
  assert.equal(autostartEvent(sch, "R1", t3, { showHiddenPins: true })?.tool, "wire");
});

test("a wire's free end starts a wire, a bus's a bus; the joined ends and a graphic line do not", () => {
  const bare = sheet();
  const t2 = tip(bare, "2");
  const far: [number, number] = [t2[0], t2[1] + 5_080_000];
  const sch = sheet({ wires: [{ id: "w1", net: "N", pins: [], bus: false, pts: [t2, far] }, { id: "b1", net: "D[0..3]", pins: [], bus: true, pts: [[0, 0], [20_000_000, 0]] }] } as Partial<Schematic>);
  assert.deepEqual(autostartEvent(sch, "w1", far, { showHiddenPins: false }), { tool: "wire", at: far });
  assert.equal(autostartEvent(sch, "w1", t2, { showHiddenPins: false }), null, "on the pin");
  assert.equal(autostartEvent(sch, "b1", [20_000_000, 0], { showHiddenPins: false })?.tool, "bus");
  // the middle of a wire is no end
  assert.equal(autostartEvent(sch, "w1", [t2[0], t2[1] + 2_540_000], { showHiddenPins: false }), null);
  // two segments of one polyline join at their shared point
  const bend = sheet({ wires: [{ id: "w2", net: "N", pins: [], bus: false, pts: [[0, 0], [10_000_000, 0], [10_000_000, 10_000_000]] }] } as Partial<Schematic>);
  assert.equal(autostartEvent(bend, "w2", [10_000_000, 0], { showHiddenPins: false }), null);
  assert.equal(autostartEvent(bend, "w2", [0, 0], { showHiddenPins: false })?.tool, "wire");
});

test("a dangling label or sheet pin starts a wire, a bus label a bus; one on a wire does not", () => {
  const sch = sheet({
    wires: [{ id: "w1", net: "N", pins: [], bus: false, pts: [[0, 0], [10_000_000, 0]] }],
    labels: [
      { id: "l1", net: "N", at: [5_000_000, 0], scope: "local", shape: null },
      { id: "l2", net: "FREE", at: [0, 20_000_000], scope: "global", shape: "input" },
      { id: "l3", net: "D[0..7]", at: [0, 30_000_000], scope: "local", shape: null },
    ],
    sheets: [{ id: "s1", name: "S", file: "s.kicad_sch", at: [60_000_000, 0], size: [20_000_000, 10_000_000], pins: [{ id: "sp1", name: "IN", shape: "input", at: [60_000_000, 5_000_000] }] }],
  } as unknown as Partial<Schematic>);
  assert.equal(autostartEvent(sch, "l1", [5_000_000, 0], { showHiddenPins: false }), null, "a wire passes through it");
  assert.deepEqual(autostartEvent(sch, "l2", [0, 20_000_000], { showHiddenPins: false })?.tool, "wire");
  assert.equal(autostartEvent(sch, "l2", [0, 21_270_000], { showHiddenPins: false }), null, "only at its anchor");
  assert.equal(autostartEvent(sch, "l3", [0, 30_000_000], { showHiddenPins: false })?.tool, "bus");
  assert.equal(autostartEvent(sch, "sp1", [60_000_000, 5_000_000], { showHiddenPins: false })?.tool, "wire");
  assert.ok(labelIsBus("{A B}") && labelIsBus("D[0..7]") && !labelIsBus("D0"));
});

test("a bus entry's free end: a wire from the end off the bus, a bus from an entry on no bus", () => {
  const entry = { id: "e1", at: [10_000_000, 0] as [number, number], size: [2_540_000, 2_540_000] as [number, number] };
  const onBus = sheet({ wires: [{ id: "b1", net: "D[0..3]", pins: [], bus: true, pts: [[0, 0], [20_000_000, 0]] }], bus_entries: [entry] } as Partial<Schematic>);
  const end: [number, number] = [12_540_000, 2_540_000];
  assert.equal(autostartEvent(onBus, "e1", end, { showHiddenPins: false })?.tool, "wire");
  assert.equal(autostartEvent(onBus, "e1", [10_000_000, 0], { showHiddenPins: false }), null, "the end on the bus");
  const loose = sheet({ bus_entries: [entry] } as Partial<Schematic>);
  assert.equal(autostartEvent(loose, "e1", [10_000_000, 0], { showHiddenPins: false })?.tool, "bus");
});

test("a power symbol's pin, hidden as KiCad's power libraries have it, only with hidden pins shown; a junction or a no-connect never", () => {
  const sch = sheet({ power_symbols: [{ id: "#PWR01", lib_id: "power:GND", at: [0, 0], rot: 0, net: "GND" }], junctions: [{ id: "j1", at: [5_000_000, 5_000_000] }], no_connects: [{ id: "n1", at: [7_000_000, 7_000_000] }] } as unknown as Partial<Schematic>);
  assert.equal(autostartEvent(sch, "#PWR01", [0, 0], { showHiddenPins: false }), null);
  assert.equal(autostartEvent(sch, "#PWR01", [0, 0], { showHiddenPins: true })?.tool, "wire");
  assert.equal(autostartEvent(sch, "j1", [5_000_000, 5_000_000], { showHiddenPins: false }), null);
  assert.equal(autostartEvent(sch, "n1", [7_000_000, 7_000_000], { showHiddenPins: false }), null);
  // a pin with a no-connect flag on it is not dangling
  const t1 = tip(sheet(), "1");
  const flagged = sheet({ no_connects: [{ id: "n2", at: t1 }] } as unknown as Partial<Schematic>);
  assert.equal(autostartEvent(flagged, "pin:R1:1", t1, { showHiddenPins: false }), null);
});

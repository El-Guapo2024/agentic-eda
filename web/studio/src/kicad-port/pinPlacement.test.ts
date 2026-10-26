import { test } from "node:test";
import assert from "node:assert/strict";
import { pinOccupiedMessage, pinPlacementOccupant } from "./pinPlacement";
import type { LibrarySymbolPin } from "../api/types";

const pin = (over: Partial<LibrarySymbolPin>): LibrarySymbolPin => ({
  number: "1",
  name: "",
  electrical_type: "input",
  shape: "line",
  at: { x: 0, y: 0 },
  angle_deg: 0,
  length_mm: 2.54,
  unit: 1,
  body_style: 1,
  hidden: false,
  name_size_mm: 1.27,
  number_size_mm: 1.27,
  ...over,
});

test("pinPlacementOccupant: with Synchronized Pins Mode off nothing is asked", () => {
  const second = pin({ id: "b", number: "2", unit: 2 });
  assert.equal(pinPlacementOccupant(pin({ number: "1" }), [second], false), null);
});

test("pinPlacementOccupant: a pin already at the position on a matching body style is the occupant, and the message names its unit", () => {
  const first = pin({ id: "a", number: "1", unit: 1 });
  const second = pin({ id: "b", number: "2", unit: 2 });
  const occupant = pinPlacementOccupant(pin({ number: "3", unit: 3 }), [first, second], true);
  assert.equal(occupant, first, "PlacePin tests the symbol's pins in order; the first one it meets wins");
  assert.equal(
    pinOccupiedMessage(occupant!.unit),
    "This position is already occupied by another pin, in unit 1.\n\nDisable the 'Synchronized Pins Mode' option to avoid this message."
  );
});

test("pinPlacementOccupant: a body style of 0 counts for every style; two different non-zero styles are not stacked", () => {
  const common = pin({ id: "a", body_style: 0 });
  assert.equal(pinPlacementOccupant(pin({ number: "2", body_style: 2 }), [common], true), common);
  const alt = pin({ id: "b", body_style: 2 });
  assert.equal(pinPlacementOccupant(pin({ number: "2", body_style: 1 }), [alt], true), null);
});

test("pinPlacementOccupant: a pin at another position, or the same pin, is no occupant", () => {
  const here = pin({ id: "a", number: "1" });
  assert.equal(pinPlacementOccupant(pin({ number: "2", at: { x: 2.54, y: 0 } }), [here], true), null);
  assert.equal(pinPlacementOccupant({ ...here, number: "1" }, [here], true), null);
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { FAB_PROPERTY_CHOICES, padFabForm, padFabPatch } from "./padFab";

test("Pad Properties' fabrication property and pad-to-die length and delay: the dialog's choices, and the edit keys that change", () => {
  assert.equal(FAB_PROPERTY_CHOICES.length, 9);
  assert.deepEqual(FAB_PROPERTY_CHOICES[2], ["pad_prop_fiducial_loc", "Fiducial, local to footprint"], "getSelectedProperty's order: local before global");
  const before = padFabForm({ property: "pad_prop_heatsink", die_length: 250_000 });
  assert.deepEqual(before, { property: "pad_prop_heatsink", dieLength: 250_000, dieDelay: null });
  assert.deepEqual(padFabPatch(before, before, true), {});
  assert.deepEqual(padFabPatch(before, { property: "pad_prop_none", dieLength: null, dieDelay: 12.5e6 }, true), { property: "pad_prop_none", die_length: 0, die_delay: 12_500_000 });
  // a pad without a connection takes neither (`m_padToDieOpt->Enable( hasConnection )`)
  assert.deepEqual(padFabPatch(before, before, false), { die_length: 0 });
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { scaledModelsWarning } from "./stepScaleWarning";

const model = (name: string, scale: [number, number, number]) => ({ name, scale });

test("scaledModelsWarning: one model scaled (2,1,1) gives KiCad's warning with its file name", () => {
  const w = scaledModelsWarning([
    { placed: true, models: [model("R.step", [1, 1, 1])] },
    { placed: true, models: [model("${KICAD10_3DMODEL_DIR}/C.step", [2, 1, 1])] },
  ]);
  assert.ok(w);
  assert.equal(w.title, "Model Scale Warning");
  assert.equal(w.message, "Scaled models detected.  Model scaling is not reliable for mechanical export.");
  assert.equal(w.extended, "Non-unity scaled models:\n\n${KICAD10_3DMODEL_DIR}/C.step");
});

test("scaledModelsWarning: none when every model is unity, or the scaled one is not on the board", () => {
  assert.equal(scaledModelsWarning([{ placed: true, models: [model("R.step", [1, 1, 1])] }]), null);
  assert.equal(scaledModelsWarning([{ placed: false, models: [model("R.step", [2, 2, 2])] }]), null);
});

test("scaledModelsWarning: the scan stops after the footprint that brings the count to 5", () => {
  const many = Array.from({ length: 7 }, (_, i) => ({ placed: true, models: [model(`m${i}.step`, [1, 1, 2])] }));
  assert.equal(scaledModelsWarning(many)!.extended.split("\n").filter((l) => l.endsWith(".step")).length, 5);
  const twoEach = Array.from({ length: 4 }, (_, i) => ({ placed: true, models: [model(`a${i}.step`, [2, 2, 2]), model(`b${i}.step`, [2, 2, 2])] }));
  assert.equal(scaledModelsWarning(twoEach)!.extended.split("\n").filter((l) => l.endsWith(".step")).length, 6, "the footprint that passes 5 is listed whole");
});

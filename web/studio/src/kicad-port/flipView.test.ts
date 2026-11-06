import { test } from "node:test";
import assert from "node:assert/strict";
import { isSideSpecificLayer, keptReadableInFlippedView } from "./flipView";

test("LSET::SideSpecificMask: front and back technical layers and copper are side specific, the user and drawing layers are not", () => {
  for (const l of ["F.Cu", "In1.Cu", "B.Cu", "F.SilkS", "B.Silkscreen", "F.Mask", "B.Paste", "F.Adhes", "B.CrtYd", "F.Fab"]) assert.equal(isSideSpecificLayer(l), true, l);
  for (const l of ["Dwgs.User", "Cmts.User", "Edge.Cuts", "Margin", "User.1", "Eco1.User"]) assert.equal(isSideSpecificLayer(l), false, l);
});

test("the flipped view keeps a user layer's text readable and leaves the silkscreen mirrored", () => {
  assert.equal(keptReadableInFlippedView(true, "Dwgs.User"), true);
  assert.equal(keptReadableInFlippedView(true, "F.SilkS"), false);
  assert.equal(keptReadableInFlippedView(false, "Dwgs.User"), false, "an unflipped view draws every text as it is");
});

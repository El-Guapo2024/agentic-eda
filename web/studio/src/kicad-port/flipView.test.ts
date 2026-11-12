import { test } from "node:test";
import assert from "node:assert/strict";
import { isSideSpecific, isSideSpecificLayer, keptReadableInFlippedView } from "./flipView";

test("LSET::SideSpecificMask: front and back technical layers and copper are side specific, the user and drawing layers are not", () => {
  for (const l of ["F.Cu", "In1.Cu", "B.Cu", "F.SilkS", "B.Silkscreen", "F.Mask", "B.Paste", "F.Adhes", "B.CrtYd", "F.Fab"]) assert.equal(isSideSpecificLayer(l), true, l);
  for (const l of ["Dwgs.User", "Cmts.User", "Edge.Cuts", "Margin", "User.1", "Eco1.User"]) assert.equal(isSideSpecificLayer(l), false, l);
});

test("the flipped view keeps a user layer's text readable and leaves the silkscreen mirrored", () => {
  assert.equal(keptReadableInFlippedView(true, "Dwgs.User"), true);
  assert.equal(keptReadableInFlippedView(true, "F.SilkS"), false);
  assert.equal(keptReadableInFlippedView(false, "Dwgs.User"), false, "an unflipped view draws every text as it is");
});

test("BOARD_ITEM::IsSideSpecific (25ec2335): a user layer typed front or back is side specific, so its text is not kept readable", () => {
  const types = { "User.1": "front", "User.2": "back", "User.3": "auxiliary" };
  assert.equal(isSideSpecific("User.1", types), true);
  assert.equal(isSideSpecific("User.2", types), true);
  assert.equal(isSideSpecific("User.3", types), false);
  assert.equal(keptReadableInFlippedView(true, "User.1", types), false);
  assert.equal(keptReadableInFlippedView(true, "User.3", types), true);
  assert.equal(keptReadableInFlippedView(true, "User.1"), true, "no types: the layer's default, auxiliary");
});

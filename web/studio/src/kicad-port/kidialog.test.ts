import { test } from "node:test";
import assert from "node:assert/strict";
import { clearDoNotShowAgain, noteAnswer, rememberedAnswer } from "./kidialog";

test("KIDIALOG: an answer given with Do not show again ticked is remembered for that question; a Cancel and an unticked answer are not", () => {
  clearDoNotShowAgain();
  assert.equal(rememberedAnswer("router_tool.cpp:performDragging"), undefined);
  noteAnswer("router_tool.cpp:performDragging", false, true);
  assert.equal(rememberedAnswer("router_tool.cpp:performDragging"), undefined, "the box was not ticked");
  noteAnswer("router_tool.cpp:performDragging", true, false);
  assert.equal(rememberedAnswer("router_tool.cpp:performDragging"), undefined, "Cancel means cancel");
  noteAnswer("router_tool.cpp:performDragging", true, true);
  assert.equal(rememberedAnswer("router_tool.cpp:performDragging"), true);
  assert.equal(rememberedAnswer("router_tool.cpp:InlineBreakTrack"), undefined, "another question is asked");
  assert.equal(rememberedAnswer(undefined), undefined, "a question with no box is always asked");
  // a question whose Cancel is a choice of its own remembers it
  noteAnswer("q", true, false, false);
  assert.equal(rememberedAnswer("q"), false);
  clearDoNotShowAgain();
  assert.equal(rememberedAnswer("router_tool.cpp:performDragging"), undefined);
});

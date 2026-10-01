import { test } from "node:test";
import assert from "node:assert/strict";
import { computeClickModifiers, computeDragModifiers, hasModifier, isCrossingSelection, guessSelectionCandidates, applySingleClickModifier, applyBoxSelectionModifiers, SIZE_RATIO, type GuessCandidate } from "./selection";

test("computeClickModifiers: plain click has no modifier", () => {
  const m = computeClickModifiers(false, false, false);
  assert.equal(hasModifier(m), false);
});

test("computeClickModifiers: Shift alone is additive (selection_tool.cpp m_additive = !ctrl && shift)", () => {
  const m = computeClickModifiers(true, false, false);
  assert.deepEqual(m, { additive: true, subtractive: false, exclusiveOr: false, skipHeuristics: false });
});

test("computeClickModifiers: Ctrl/Cmd+Shift is subtractive", () => {
  const m = computeClickModifiers(true, true, false);
  assert.deepEqual(m, { additive: false, subtractive: true, exclusiveOr: false, skipHeuristics: false });
});

test("computeClickModifiers: Ctrl/Cmd alone is exclusive-or (toggle) -- this app never models m_CtrlClickHighlight, which defaults off anyway", () => {
  const m = computeClickModifiers(false, true, false);
  assert.deepEqual(m, { additive: false, subtractive: false, exclusiveOr: true, skipHeuristics: false });
});

test("computeClickModifiers: Alt sets skipHeuristics independently of Shift/Ctrl", () => {
  assert.equal(computeClickModifiers(false, false, true).skipHeuristics, true);
  assert.equal(computeClickModifiers(true, true, true).skipHeuristics, true);
});

test("computeDragModifiers: Shift-drag or Ctrl/Cmd-drag alone adds; Alt suppresses both (selection_tool.cpp m_drag_additive/_subtractive)", () => {
  assert.deepEqual(computeDragModifiers(true, false, false), { additive: true, subtractive: false });
  assert.deepEqual(computeDragModifiers(false, true, false), { additive: true, subtractive: false });
  assert.deepEqual(computeDragModifiers(true, true, false), { additive: true, subtractive: true }, "both held: additive AND subtractive both true, same as source -- the apply step treats subtractive as taking priority (see SelectMultiple's aSubtractive-first check)");
  assert.deepEqual(computeDragModifiers(true, false, true), { additive: false, subtractive: false }, "Alt suppresses drag modifiers entirely");
});

test("isCrossingSelection: left-to-right is a window (fully enclosed) select, right-to-left is crossing", () => {
  assert.equal(isCrossingSelection(0, 100), false, "dragged left->right: window");
  assert.equal(isCrossingSelection(100, 0), true, "dragged right->left: crossing");
  assert.equal(isCrossingSelection(50, 50), false, "no horizontal movement: window (not < start)");
});

function candidate(slopUm: number, areaUm2: number, layer: string | null = null): GuessCandidate {
  return { slopUm, areaUm2, layer };
}

test("guessSelectionCandidates: 0 or 1 candidates pass through untouched", () => {
  assert.deepEqual(guessSelectionCandidates([], 100, null), []);
  const one = [candidate(0, 1000)];
  assert.deepEqual(guessSelectionCandidates(one, 100, null), one);
});

test("guessSelectionCandidates: prefers the exact/near hit over a sloppier one beyond one pixel", () => {
  const exact = candidate(0, 5000);
  const sloppy = candidate(50, 5000); // 50um sloppiness, onePixelUm = 10 -> beyond threshold
  const result = guessSelectionCandidates([exact, sloppy], 10, null);
  assert.deepEqual(result, [exact]);
});

test("guessSelectionCandidates: keeps near-tied hits within one pixel of each other", () => {
  const a = candidate(0, 5000);
  const b = candidate(5, 5000); // within onePixelUm=10 of the best (0)
  const result = guessSelectionCandidates([a, b], 10, null);
  assert.equal(result.length, 2);
});

test("guessSelectionCandidates: a small item inside a much bigger one wins (SIZE_RATIO)", () => {
  const via = candidate(0, 1000); // tiny via footprint
  const footprint = candidate(0, 1000 * SIZE_RATIO * 10); // much bigger courtyard, same exact hit
  const result = guessSelectionCandidates([via, footprint], 10, null);
  assert.deepEqual(result, [via]);
});

test("guessSelectionCandidates: never rejects every candidate even if areas are all tied past the ratio chain", () => {
  // Three items each just over the ratio vs. the previous -- all of them
  // end up in `rejected` relative to the smallest in a naive pass, but
  // source's own guard ("do not remove everything") only fires relative
  // to the FULL pool size: here one survivor (the smallest) remains, so
  // the guard doesn't need to intervene and the result is just that one.
  const a = candidate(0, 100);
  const b = candidate(0, 100 * SIZE_RATIO + 1);
  const c = candidate(0, 100 * SIZE_RATIO * SIZE_RATIO + 10);
  const result = guessSelectionCandidates([a, b, c], 10, null);
  assert.deepEqual(result, [a]);
});

test("guessSelectionCandidates: equal-area candidates (e.g. two same-size overlapping footprints) are left for the active-layer pass / disambiguation menu", () => {
  const a = candidate(0, 1000, "F.Cu");
  const b = candidate(0, 1000, "B.Cu");
  const result = guessSelectionCandidates([a, b], 10, null);
  assert.equal(result.length, 2);
});

test("guessSelectionCandidates: active layer preference rejects off-layer items once something matches", () => {
  const front = candidate(0, 1000, "F.Cu");
  const back = candidate(0, 1000, "B.Cu");
  const result = guessSelectionCandidates([front, back], 10, "F.Cu");
  assert.deepEqual(result, [front]);
});

test("guessSelectionCandidates: layer-agnostic candidates (layer: null, e.g. a footprint) always survive the active-layer pass, alongside a genuine active-layer match", () => {
  const part = candidate(0, 1000, null);
  const front = candidate(0, 1000, "F.Cu");
  const back = candidate(0, 1000, "B.Cu");
  const result = guessSelectionCandidates([part, front, back], 10, "F.Cu");
  assert.deepEqual(result, [part, front], "the off-layer track is rejected; the layer-agnostic footprint and the on-layer track both survive");
});

test("applySingleClickModifier: no modifier replaces the whole selection with just the hit", () => {
  const current = new Set(["R1", "R2"]);
  assert.deepEqual(applySingleClickModifier(current, "C1", { additive: false, subtractive: false, exclusiveOr: false }), ["C1"]);
});

test("applySingleClickModifier: additive always adds, even if already present (a no-op re-add, never a remove)", () => {
  assert.deepEqual(applySingleClickModifier(new Set(["R1"]), "C1", { additive: true, subtractive: false, exclusiveOr: false }), ["R1", "C1"]);
  assert.deepEqual(applySingleClickModifier(new Set(["R1"]), "R1", { additive: true, subtractive: false, exclusiveOr: false }), ["R1"]);
});

test("applySingleClickModifier: subtractive always removes, whether or not it was present", () => {
  assert.deepEqual(applySingleClickModifier(new Set(["R1", "C1"]), "C1", { additive: false, subtractive: true, exclusiveOr: false }), ["R1"]);
  assert.deepEqual(applySingleClickModifier(new Set(["R1"]), "C1", { additive: false, subtractive: true, exclusiveOr: false }), ["R1"]);
});

test("applySingleClickModifier: exclusiveOr toggles", () => {
  assert.deepEqual(applySingleClickModifier(new Set(["R1"]), "C1", { additive: false, subtractive: false, exclusiveOr: true }), ["R1", "C1"]);
  assert.deepEqual(applySingleClickModifier(new Set(["R1", "C1"]), "C1", { additive: false, subtractive: false, exclusiveOr: true }), ["R1"]);
});

test("applyBoxSelectionModifiers: no modifier (already-cleared selection) just adds everything found", () => {
  assert.deepEqual(applyBoxSelectionModifiers(new Set(), ["R1", "R2"], { subtractive: false, exclusiveOr: false }).sort(), ["R1", "R2"]);
});

test("applyBoxSelectionModifiers: subtractive removes every hit from the current selection, additive (not checked here) never applies per-item", () => {
  const result = applyBoxSelectionModifiers(new Set(["R1", "R2", "R3"]), ["R1", "R2"], { subtractive: true, exclusiveOr: false });
  assert.deepEqual(result, ["R3"]);
});

test("applyBoxSelectionModifiers: exclusiveOr (plain Ctrl-drag) toggles each hit independently", () => {
  const result = applyBoxSelectionModifiers(new Set(["R1"]), ["R1", "R2"], { subtractive: false, exclusiveOr: true });
  assert.deepEqual(result.sort(), ["R2"], "R1 was selected and gets toggled off, R2 was not and gets toggled on");
});

test("guessSelectionCandidates: active-layer pass is a no-op when nothing matches the active layer (never reject everything)", () => {
  const a = candidate(0, 1000, "B.Cu");
  const b = candidate(0, 1000, "In1.Cu");
  const result = guessSelectionCandidates([a, b], 10, "F.Cu");
  assert.equal(result.length, 2, "neither is on F.Cu, so the pass leaves both for disambiguation");
});

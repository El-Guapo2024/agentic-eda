import { test } from "node:test";
import assert from "node:assert/strict";
import { braceLight, braceMatch, changedRange, convertSmartQuotesAndDashes, tabIndent, toggleComment } from "./scintillaTricks";

test("BraceMatch: nested pairs, both ways, a parenthesis in a string counts (no lexer), none for an unmatched one", () => {
  const t = "(rule x (constraint clearance (min 1mm)))";
  assert.equal(braceMatch(t, 0), t.length - 1);
  assert.equal(braceMatch(t, t.length - 1), 0);
  assert.equal(braceMatch(t, 8), t.length - 2);
  assert.equal(braceMatch('(a ")" b)', 0), 4);
  assert.equal(braceMatch("((a)", 0), -1);
  assert.equal(braceMatch("abc", 1), -1, "not a brace");
});

test("onScintillaUpdateUI: the brace before the caret first, else the one after it; a brace with no match is bad", () => {
  const t = "(a (b))";
  assert.deepEqual(braceLight(t, 7), { kind: "match", at: [6, 0] });
  assert.deepEqual(braceLight(t, 3), { kind: "match", at: [3, 5] }, "after the space: the ( at the caret");
  assert.deepEqual(braceLight(t, 4), { kind: "match", at: [3, 5] }, "after a (: that one, not the b");
  assert.equal(braceLight(t, 2), null);
  assert.deepEqual(braceLight("(a", 1), { kind: "bad", at: 0 });
  assert.deepEqual(braceLight("x)", 2), { kind: "bad", at: 1 });
  assert.equal(braceLight("[a]", 1, "()"), null, "only the editor's braces");
});

test("Ctrl+/: # at the start of each selected line, or the first # of each taken off; the lines selected whole", () => {
  const t = "(rule a\n  (constraint clearance)\n)\n";
  assert.deepEqual(toggleComment(t, 2, 12), { text: "#(rule a\n#  (constraint clearance)\n)\n", anchor: 0, caret: 35 });
  const c = "#(rule a\n  # (constraint clearance)\nx\n";
  assert.deepEqual(toggleComment(c, 0, 12), { text: "(rule a\n   (constraint clearance)\nx\n", anchor: 0, caret: 34 });
  // the first selected line decides: commented, so the second (not commented) stays
  assert.equal(toggleComment("#a\nb", 0, 4).text, "a\nb");
  // a last line with no line end
  assert.deepEqual(toggleComment("abc", 1, 1), { text: "#abc", anchor: 0, caret: 4 });
});

test("Tab: a tab typed, or the leading blanks indented to the next 4 columns; several lines indented by 4", () => {
  assert.deepEqual(tabIndent("(rule a)", 5, 5), { text: "(rule\t a)", anchor: 6, caret: 6 });
  assert.deepEqual(tabIndent("ab", 0, 2), { text: "\t", anchor: 1, caret: 1 }, "the selection goes first, then the caret is in the (empty) indentation");
  assert.deepEqual(tabIndent("  (x)", 1, 1), { text: "\t(x)", anchor: 1, caret: 1 }, "in the blanks: indent to column 4");
  assert.deepEqual(tabIndent("\t(x)", 1, 1), { text: "\t\t(x)", anchor: 2, caret: 2 });
  assert.deepEqual(tabIndent("a\nb\n\nc", 0, 6), { text: "\ta\n\tb\n\n\tc", anchor: 0, caret: 9 }, "an empty line keeps no indentation");
  assert.deepEqual(tabIndent("a\nb\nc", 0, 4), { text: "\ta\n\tb\nc", anchor: 0, caret: 6 }, "a selection that ends at a line's start leaves that line");
});

test("ConvertSmartQuotesAndDashes, and the one range an edit changes", () => {
  assert.equal(convertSmartQuotesAndDashes("“A.Type == ‘Via’” – x — y"), "\"A.Type == 'Via'\" - x - y");
  assert.deepEqual(changedRange("(rule a)", "(rule\t a)"), { from: 5, to: 5, insert: "\t" });
  assert.deepEqual(changedRange("abc", "abc"), { from: 3, to: 3, insert: "" });
  assert.deepEqual(changedRange("#a\n#b", "a\nb"), { from: 0, to: 4, insert: "a\n" });
});

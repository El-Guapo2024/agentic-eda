import { test } from "node:test";
import assert from "node:assert/strict";
import { backspaceUnindent, braceLight, braceMatch, changedRange, columnAt, convertSmartQuotesAndDashes, indentGuides, tabIndent, toggleComment } from "./scintillaTricks";

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

test("Backspace in a line's leading blanks un-indents to the step below (SetBackSpaceUnIndents)", () => {
  // a caret at the end of 8 columns of blanks: back to 4, made of a tab (`useTabs`)
  assert.deepEqual(backspaceUnindent("(rule a\n        (b))", 16, 16), { text: "(rule a\n\t(b))", anchor: 9, caret: 9 });
  // 6 columns: back to the step at 4 (`indentation % IndentSize()`)
  assert.deepEqual(backspaceUnindent("x\n      y", 8, 8), { text: "x\n\ty", anchor: 3, caret: 3 });
  // a caret within the blanks counts too; the caret goes to the end of the new blanks
  assert.deepEqual(backspaceUnindent("\t\ty", 1, 1), { text: "\ty", anchor: 1, caret: 1 });
  // after text, at the line's start, or with a selection: a plain Backspace
  assert.equal(backspaceUnindent("  ab", 3, 3), null);
  assert.equal(backspaceUnindent("a\n  b", 2, 2), null);
  assert.equal(backspaceUnindent("    b", 2, 4), null);
});

test("the indentation guides are in the leading blanks at each indent step, the matched brace's column lit", () => {
  const text = "(rule a\n\t(constraint\n\t\t(min 1)))\n        x";
  const g = indentGuides(text, 4);
  // line 2: one tab, no guide (column 0); line 3: the second tab starts at column 4; line 4: spaces at columns 4 (lit)
  const second = text.indexOf("\t\t") + 1;
  assert.equal(g.get(second), true);
  assert.equal(g.has(text.indexOf("\t")), false);
  const spaces = text.indexOf("        ");
  assert.equal(g.get(spaces + 4), true);
  assert.equal(g.size, 2);
  assert.equal(indentGuides(text, 0).get(second), false, "no highlight");
  assert.equal(columnAt(text, text.indexOf("(min")), 8);
});

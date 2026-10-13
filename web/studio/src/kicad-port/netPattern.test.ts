import { test } from "node:test";
import assert from "node:assert/strict";
import { NetRegex, hasRegexSyntax, isRegex, patternMatches } from "./netPattern";

// The same cases as crates/model/src/net_pattern.rs: the page's list of the nets a pattern matches is the backend's.
const yes = (pattern: string, net: string) => assert.ok(patternMatches(pattern, net), `${JSON.stringify(pattern)} should match ${JSON.stringify(net)}`);
const no = (pattern: string, net: string) => assert.ok(!patternMatches(pattern, net), `${JSON.stringify(pattern)} should not match ${JSON.stringify(net)}`);

test("a wildcard pattern is anchored and a name is a pattern", () => {
  yes("GND", "GND");
  no("GND", "GND2");
  no("GND", "AGND");
  yes("USB_*", "USB_DP");
  yes("USB_D?", "USB_DP");
  yes("USB_D?", "USB_D"); // as a regular expression the `?` makes the D optional: either reading will do
  no("USB_D?", "USB_DPM");
  yes("*", "anything");
  yes("/Sheet1/NET", "/Sheet1/NET");
  yes("A*B*C", "AxxBxxC");
  no("A*B*C", "AxxBxx");
});

test("a regular expression is anchored too and either reading will do", () => {
  yes("USB_D[PM]", "USB_DP");
  yes("USB_D[PM]", "USB_DM");
  no("USB_D[PM]", "USB_DX");
  no("USB_D[PM]", "USB_DPM");
  yes("USB_D.*", "USB_DP2");
  yes("^VBUS$", "VBUS");
  no("^VBUS$", "VBUS2");
  yes("(DP|DM)", "DP");
  no("(DP|DM)", "DPM");
  yes("D[0-9]+", "D15");
  no("D[0-9]+", "D");
  yes("D\\d{1,2}", "D7");
  yes("D\\d{1,2}", "D15");
  no("D\\d{1,2}", "D150");
  yes("A(?:B|C)D?", "AC");
  yes("A(?:B|C)D?", "ABD");
  yes("[[:alpha:]]+_[[:digit:]]", "MOSI_1");
  yes("[^A-Z]+", "abc_1");
  no("[^A-Z]+", "abC");
  yes("USB_*", "USB");
  yes("USB_*", "USB___");
  yes("USB_*", "USB_DP");
  yes("V3.3", "V3.3");
  yes("V3.3", "V3x3");
});

test("a pattern that is no regular expression is still a wildcard pattern", () => {
  for (const bad of ["USB_(", "USB_D[", "a**", "a\\1", "(?=a)", "a{3,1}", "x\\bX"]) assert.ok(!isRegex(bad), bad);
  yes("USB_(", "USB_(");
  yes("USB_[*", "USB_[DP");
  no("USB_(", "USB_");
  for (const good of ["USB_D[PM]", "(a|b)+c?", "", "a{2}", "a{2,}", "a+?"]) assert.ok(isRegex(good), good);
});

test("matching does not blow up on patterns made to make a backtracking matcher blow up", () => {
  const net = `${"a".repeat(200)}!`;
  assert.ok(!patternMatches("(a+)+$", net));
  assert.ok(!patternMatches("(a|aa)+b", net));
  assert.ok(!patternMatches("(a*)*b", net));
  assert.ok(patternMatches("(a*)*", "a".repeat(200)));
  assert.ok(!isRegex("((((a{200}){200}){200}){200})"), "a program that would have a hundred thousand instructions is no regular expression");
  assert.ok(!isRegex("(".repeat(100)));
  assert.ok(!isRegex(`${"(".repeat(100)}a${")".repeat(100)}`), "nested deeper than a pattern may be");
  assert.ok(NetRegex.compile("a{255}") !== null && NetRegex.compile("a{256}") === null);
});

test("the empty net and the empty pattern", () => {
  assert.ok(patternMatches("", ""));
  assert.ok(!patternMatches("", "A"));
  assert.ok(patternMatches("*", ""));
  assert.ok(patternMatches("A*", "A"));
  assert.ok(hasRegexSyntax("a.b") && hasRegexSyntax("[ab]") && !hasRegexSyntax("USB_*") && !hasRegexSyntax("D?"));
});

test("counted repeats and alternation in the middle", () => {
  yes("(CLK|DAT)[0-3]", "CLK2");
  yes("(CLK|DAT)[0-3]", "DAT0");
  no("(CLK|DAT)[0-3]", "CLK4");
  yes("A{3}", "AAA");
  no("A{3}", "AA");
  yes("A{2,3}B", "AAAB");
  no("A{2,3}B", "AAAAB");
  yes("(AB){2}", "ABAB");
  yes("x|y|z", "y");
  no("x|y|z", "xy");
  yes("\\.", ".");
  no("\\.", "x");
  yes("[a\\]b]+", "a]b");
  yes("[-a]+", "a-");
  yes("[a-]+", "-a");
});

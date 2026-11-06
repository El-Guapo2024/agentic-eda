import { test } from "node:test";
import assert from "node:assert/strict";
import { EdaCombinedMatcher, permissiveMatches } from "./edaPatternMatch";

test("EDA_COMBINED_MATCHER( CTX_NET ): a substring, case-sensitive", () => {
  const m = new EdaCombinedMatcher("CLK", "net");
  assert.ok(m.find("/sys/CLK_IN"));
  assert.ok(!m.find("/sys/clk_in"), "wxString::Find is case-sensitive");
});

test("a wildcard pattern is found anywhere: `*` any run, `?` one character, the rest literal", () => {
  const m = new EdaCombinedMatcher("D?_*", "net");
  assert.ok(m.find("/USB/D+_P"));
  assert.ok(m.find("D1_x"));
  assert.ok(!m.find("D_x"));
  assert.ok(new EdaCombinedMatcher("a.b", "net").find("a.b"), "a dot is literal in a wildcard pattern");
  assert.ok(!new EdaCombinedMatcher("a.b", "net").find("axb"));
});

test("a regular expression must be explicit: /.../ (the closing / optional) or ^...$", () => {
  assert.ok(new EdaCombinedMatcher("/^D[0-9]+$/", "net").find("D12"));
  assert.ok(!new EdaCombinedMatcher("/^D[0-9]+$/", "net").find("D12x"));
  assert.ok(new EdaCombinedMatcher("/USB_D[PN]", "net").find("/x/USB_DP"), "no trailing slash: still a regex");
  assert.ok(new EdaCombinedMatcher("^GND$", "net").find("GND"));
  assert.ok(!new EdaCombinedMatcher("^GND$", "net").find("AGND"));
  // a pattern that does not compile is no regex matcher: the wildcard and substring readings still count
  assert.ok(new EdaCombinedMatcher("/[", "net").find("x/[y"));
});

test("EDA_ITEM::Matches in PERMISSIVE mode: both upper-cased first", () => {
  assert.ok(permissiveMatches("R12", "r1"));
  assert.ok(permissiveMatches("Resistor_SMD:R_0603", "*0603"));
  assert.ok(!permissiveMatches("C1", "R"));
});

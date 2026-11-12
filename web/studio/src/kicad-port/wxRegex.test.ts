// The choosers' regular expressions (kicad-port/wxRegex.ts), checked on the cases crates/model/src/wx_regex.rs checks its `Flavor::Advanced` and `Flavor::Find` on.
import { test } from "node:test";
import assert from "node:assert/strict";
import { advancedMetasyntax, WxRegex } from "./wxRegex";

const re = (p: string) => {
  const r = WxRegex.compile(p);
  assert.ok(r, `${p} compiles`);
  return r!;
};

test("PCRE2's syntax: ASCII classes, the POSIX classes, \\b, \\h, \\xhh, \\A \\z \\Z, leftmost-first", () => {
  assert.ok(re("\\bres").isMatch("a res") && !re("\\bres").isMatch("ares"));
  assert.ok(!re("\\w").isMatch("é"), "no PCRE2_UCP: é is no \\w");
  assert.ok(re("[[:word:]]+").isMatch("x_1") && re("[[:alpha:]]").isMatch("q") && !re("[[:digit:]]").isMatch("q"));
  assert.ok(re("a\\hb").isMatch("a\tb"));
  assert.ok(re("\\x41").isMatch("A") && re("\\xZ").isMatch("xZ"), "\\x without two hex digits is the letter x");
  assert.ok(re("\\Aab\\z").isMatch("ab") && !re("\\Aab\\z").isMatch("ab\n") && re("ab$").isMatch("ab\n"));
  const m = re("a|ab").findAt([..."ab"].map((c) => c.codePointAt(0)!));
  assert.deepEqual(m?.slice(0, 2), [0, 1], "a|ab on ab is a: PCRE's leftmost-first");
  assert.equal(re("(a+)+$").isMatch("a".repeat(40) + "b"), false, "no backtracking blow-up");
  assert.equal(re("r_0[68]0[35]").findStart("x r_0603"), 2);
});

test("what does not compile: back references, look-arounds, a letter PCRE2 does not know escaped, a capital class name, a { that is no count", () => {
  for (const bad of ["(a)\\1", "(?=a)", "(?<n>a)", "\\q", "[[:ALPHA:]]", "a{x}", "a**", "(", "[a", "[z-a]"]) assert.equal(WxRegex.compile(bad), null, bad);
});

test("wxRE_ADVANCED: Tcl's word boundaries, the directors and the leading options (ConvertMetasyntax, ConvertWordBoundaries)", () => {
  assert.ok(re("\\yres\\y").isMatch("a res b") && !re("\\yres\\y").isMatch("ares"));
  assert.ok(re("\\Yes").isMatch("res") && !re("\\Yes").isMatch("es"));
  assert.ok(re("\\mre").isMatch("x re") && !re("\\mre").isMatch("are"));
  assert.ok(re("or\\M").isMatch("resistor") && !re("or\\M").isMatch("orb"));
  assert.ok(re("[[:<:]]a").isMatch("b a") && !re("[[:<:]]a").isMatch("ba"));
  assert.ok(re("\\\\y").isMatch("a\\y"), "an escaped backslash is a pair");
  assert.ok(re("***:abc").isMatch("xabcx"));
  assert.ok(re("***=a.c").isMatch("a.c") && !re("***=a.c").isMatch("abc"));
  assert.ok(re("(?i)ABC").isMatch("xabc"));
  assert.ok(!WxRegex.compile("(?c)abc", true)!.isMatch("ABC"));
  assert.ok(re("(?q)a+").isMatch("a+") && !re("(?q)a+").isMatch("aa"));
  assert.equal(WxRegex.compile("(?e)\\y"), null, "extended syntax: no conversion, and \\y does not compile");
  assert.equal(WxRegex.compile("(?m)a"), null, "an option this port does not read");
  assert.deepEqual(advancedMetasyntax("\\m\\M\\y\\Y", false), { pattern: "[[:<:]][[:>:]]\\b\\B", icase: false });
});

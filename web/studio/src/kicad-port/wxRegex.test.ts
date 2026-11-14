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

test("what does not compile: a back reference to no group, a look-behind of no fixed length, \\X, a letter PCRE2 does not know escaped, a capital class name, a { that is no count", () => {
  for (const bad of ["a\\1", "(?<=a+)b", "\\k<n>", "\\X", "\\q", "[[:ALPHA:]]", "a{x}", "a**", "(", "[a", "[z-a]"]) assert.equal(WxRegex.compile(bad), null, bad);
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
  assert.equal(WxRegex.compile("(?m)a"), null, "a leading option wxWidgets' ConvertMetasyntax does not take");
  assert.deepEqual(advancedMetasyntax("\\m\\M\\y\\Y", false), { pattern: "[[:<:]][[:>:]]\\b\\B", icase: false });
});

test("PCRE2's other forms match as the box's libpcre2-32 does (UI-LEFT-3 C3-11): back references, look-arounds, named groups, options, \\Q..\\E, \\p{..}", () => {
  // the cases of crates/model/src/wx_regex.rs `pcre2s_other_forms_match_as_libpcre2_does` (a leading `(?opts)` is wxWidgets' ConvertMetasyntax's, left out):
  // undefined -- does not compile; null -- no match; else the match's span in characters
  const cases: [string, string, [number, number] | null | undefined][] = [
    ["(a)\\1", "xaay", [1, 3]],
    ["(a)\\1", "xaby", null],
    ["(\\w+) \\1", "say hello hello", [4, 15]],
    ["(a)|b\\1", "b", null],
    ["(?<n>ab)\\k<n>", "abab", [0, 4]],
    ["(?'n'ab)\\k'n'", "abab", [0, 4]],
    ["(?P<n>ab)(?P=n)", "abab", [0, 4]],
    ["(?<n>a)\\k{n}", "aa", [0, 2]],
    ["(?<n>a)\\g{n}", "aa", [0, 2]],
    ["(a)(b)\\g{-1}", "abb", [0, 3]],
    ["(a)\\g1", "aa", [0, 2]],
    ["(a)\\g{1}", "aa", [0, 2]],
    ["a(?=b)", "acab", [2, 3]],
    ["a(?!b)", "abac", [2, 3]],
    ["(?<=a)b", "cbab", [3, 4]],
    ["(?<!a)b", "abcb", [3, 4]],
    ["(?<=ab|c)d", "xcd", [2, 3]],
    ["(?<=a(b))c", "abc", [2, 3]],
    ["\\Qa.b*\\E+", "a.b**", [0, 5]],
    ["[\\Q]\\E]", "]", [0, 1]],
    ["\\Qa.b", "xa.b", [1, 4]],
    ["a(?i)bc", "aBC", [0, 3]],
    ["a(?i)bc", "ABC", null],
    ["(?i:a)b", "Ab", [0, 2]],
    ["(?i:a)b", "AB", null],
    ["(a(?i)b|c)", "C", [0, 1]],
    ["^b$", "a\nb\nc", null],
    ["(?s-s)a.b", "a\nb", null],
    ["(?U)a+", "aaa", [0, 1]],
    ["(?U)a+?", "aaa", [0, 3]],
    ["(?^i)A", "a", [0, 1]],
    ["\\p{Lu}+", "abCDe", [2, 4]],
    ["\\P{L}", "ab1", [2, 3]],
    ["\\pL+", "12ab", [2, 4]],
    ["\\p{Greek}+", "aαβγb", [1, 4]],
    ["\\p{L&}", "1A", [1, 2]],
    ["\\p{Xan}+", "--a1--", [2, 4]],
    ["\\p{Nd}", "x٣", [1, 2]],
    ["[\\p{Lu}\\d]+", "xA1B2y", [1, 5]],
    ["[^\\p{L}]", "ab-", [2, 3]],
    ["\\p{^Lu}", "AbC", [1, 2]],
    ["\\p{Han}", "a中", [1, 2]],
    ["\\p{sc:Greek}", "ͅα", [1, 2]],
    ["\\p{scx:Greek}", "ͅα", [0, 1]],
    ["\\p{Alphabetic}+", "12ab", [2, 4]],
    ["\\p{White_Space}", "a b", [1, 2]],
    ["(?>a+)b", "aaab", [0, 4]],
    ["(?>a+)a", "aaa", null],
    ["a++b", "aab", [0, 3]],
    ["a*+a", "aaa", null],
    ["a?+", "a", [0, 1]],
    ["\\o{101}", "A", [0, 1]],
    ["\\101", "A", [0, 1]],
    ["\\cA", "\u0001", [0, 1]],
    ["\\R", "a\r\nb", [1, 3]],
    ["(a*)*b", "b", [0, 1]],
    ["(a|b)*?c", "abc", [0, 3]],
    ["(?<=\\d{3})x", "123x", [3, 4]],
    ["x(?=(\\d))\\1", "x1", [0, 2]],
    ["(?<=a+)b", "ab", undefined],
    ["\\k<nope>", "a", undefined],
    ["(a)\\2", "aa", undefined],
    ["(?<n>a)(?<n>b)", "ab", undefined],
    ["(?J)(?<n>a)|(?<n>b)\\k<n>", "bb", [0, 2]],
  ];
  for (const [p, t, want] of cases) {
    const r = WxRegex.compile(p);
    if (want === undefined) {
      assert.equal(r, null, p);
      continue;
    }
    assert.ok(r, `${p} compiles`);
    const m = r!.findAt([...t].map((c) => c.codePointAt(0)!), 0);
    assert.deepEqual(m ? [m[0], m[1]] : null, want, `${p} on ${JSON.stringify(t)}`);
  }
  // the match limit: a pattern that backtracks without end gives up
  assert.equal(WxRegex.compile("(?:(a+)+)\\1$")!.isMatch(`${"a".repeat(40)}!`), false);
});

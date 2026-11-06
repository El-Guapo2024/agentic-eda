// KiCad's permissive pattern matcher (common/eda_pattern_match.cpp at 8303b2ad): `EDA_COMBINED_MATCHER` in the contexts the Net Inspector's custom groups
// (`CTX_NET`) and the Search panel (`CTX_SEARCH`, through `EDA_ITEM::Matches` in `EDA_SEARCH_MATCH_MODE::PERMISSIVE`) build it in. Both contexts add the same
// three matchers, in this order, each only when its `SetPattern` accepts the pattern:
//   * `EDA_PATTERN_MATCH_REGEX`: only an explicit regular expression -- one written `^...$`, or `/.../` ("Requiring a '/' on the end means they get no feedback
//     while they type": a leading `/` is enough, a trailing one is dropped). It is found anywhere in the text.
//   * `EDA_PATTERN_MATCH_WILDCARD`: the pattern with `?` as any one character and `*` as any run, every other regex character escaped, compiled as a REGEX
//     (`/.../`), so found anywhere in the text, not anchored.
//   * `EDA_PATTERN_MATCH_SUBSTR`: `wxString::Find`, case-sensitive.
// `Find( term )` is true when any of them finds the pattern in the term.
//
// Different from KiCad: wxRegEx compiles with `wxRE_ADVANCED` (Tcl's ARE); this compiles with JavaScript's RegExp (the `u` flag), which reads the usual classes,
// quantifiers, groups and anchors the same way. A pattern JavaScript refuses is a matcher not added, as wxRegEx's refusal is.
//
// Pure: no store, no DOM. Unit-tested in edaPatternMatch.test.ts.

/** `COMBINED_MATCHER_CONTEXT`: the two contexts this port has; they build the same matchers. */
export type MatcherContext = "net" | "search";

interface PatternMatch {
  /** `FIND_RESULT`: where the pattern starts in the candidate, or -1. */
  find(candidate: string): number;
}

/** `EDA_PATTERN_MATCH_SUBSTR::Find`. */
function substrMatcher(pattern: string): PatternMatch {
  return { find: (c) => c.indexOf(pattern) };
}

/** `EDA_PATTERN_MATCH_REGEX::SetPattern`: null when the pattern is not an explicit regular expression or does not compile. */
function regexMatcher(aPattern: string): PatternMatch | null {
  let pattern: string;
  if (aPattern.startsWith("^") && aPattern.endsWith("$")) {
    pattern = aPattern;
  } else if (aPattern.startsWith("/")) {
    pattern = aPattern.slice(1);
    if (pattern.endsWith("/")) pattern = pattern.slice(0, -1);
  } else {
    return null; // "For now regular expressions must be explicit"
  }
  let re: RegExp;
  try {
    re = new RegExp(pattern, "u");
  } catch {
    return null;
  }
  return {
    find: (c) => {
      const m = re.exec(c);
      return m ? m.index : -1;
    },
  };
}

/** `EDA_PATTERN_MATCH_WILDCARD::SetPattern`: `?` and `*` become `.` and `.*`, the characters of `.*+?^${}()|[]/\` are escaped, and the result is a REGEX `/.../`. */
function wildcardMatcher(aPattern: string): PatternMatch | null {
  const toReplace = ".*+?^${}()|[]/\\";
  let regex = "";
  for (const c of aPattern) {
    if (c === "?") regex += ".";
    else if (c === "*") regex += ".*";
    else if (toReplace.includes(c)) regex += "\\" + c;
    else regex += c;
  }
  return regexMatcher("/" + regex + "/");
}

/** `EDA_COMBINED_MATCHER( aPattern, CTX_NET | CTX_SEARCH )`. */
export class EdaCombinedMatcher {
  readonly pattern: string;
  private readonly matchers: PatternMatch[] = [];

  constructor(pattern: string, _context: MatcherContext = "search") {
    this.pattern = pattern;
    // `AddMatcher`: REGEX, then WILDCARD, then SUBSTR, each kept when `SetPattern` takes the pattern
    for (const m of [regexMatcher(pattern), wildcardMatcher(pattern), substrMatcher(pattern)]) if (m) this.matchers.push(m);
  }

  /** `EDA_COMBINED_MATCHER::Find( aTerm )`: any matcher finds the pattern in `term`. */
  find(term: string): boolean {
    return this.matchers.some((m) => m.find(term) >= 0);
  }
}

/** `EDA_ITEM::Matches( aText, aSearchData )` in `EDA_SEARCH_MATCH_MODE::PERMISSIVE` with `matchCase` off (the Search panel's): both are upper-cased, then matched. */
export function permissiveMatches(text: string, findString: string): boolean {
  return new EdaCombinedMatcher(findString.toUpperCase(), "search").find(text.toUpperCase());
}

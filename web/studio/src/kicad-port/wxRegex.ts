// The library choosers' regular expressions as KiCad compiles them (UI-LEFT-2 LL4): `EDA_PATTERN_MATCH_REGEX` and `EDA_PATTERN_MATCH_RELATIONAL`
// (common/eda_pattern_match.cpp) hand a pattern to `wxRegEx::Compile( pattern, wxRE_ADVANCED )`, and wxWidgets 3.2 compiles every pattern with PCRE2
// (src/common/regex.cpp of 3.2.4: `wx_regcomp` with `PCRE2_UTF | PCRE2_ALT_BSUX | PCRE2_DOTALL`, after `ConvertMetasyntax` and, for `wxRE_ADVANCED`,
// `ConvertWordBoundaries`). This is a port of the server's engine, crates/model/src/wx_regex.rs (`Flavor::Advanced`), so the items the browser scores
// (recently used, already placed, the project's) match as the installed libraries the server searches do -- not JavaScript's RegExp, whose syntax differs
// (Unicode classes under `u`, look-behinds, no `[[:alpha:]]`, no `\A \z \Z`, no `\x` without two digits).
//
// The syntax: literals, `.`, classes with ranges, `^` negation, the POSIX classes (`[:alpha:]` ... `[:word:]` `[:ascii:]`), `\d \w \s \h \v` and their capitals,
// `\N`, groups `( .. )` and `(?: .. )`, `|`, `^ $ \A \z \Z`, `\b \B`, `[[:<:]] [[:>:]]`, `* + ? {m} {m,} {m,n}` (lazy with a `?`), the one-character escapes
// `\a \e \f \n \r \t \xhh \uhhhh \U`, and an escaped punctuation character. The classes are PCRE2's ASCII ones (no `PCRE2_UCP`): `é` is no `\w`. Matching is
// leftmost-first (PCRE's), by simulation of the compiled machine (no backtracking blow-up).
//
// PCRE2's other forms, as the server's (UI-LEFT-3 C3-11): back references (`\1`, `\g{n}`, `\g{-n}`, `\k<name>`, `\k'name'`, `\k{name}`, `(?P=name)`),
// look-aheads and fixed-length look-behinds, named groups (`(?<n>..)`, `(?'n'..)`, `(?P<n>..)`, `(?J)`), inline options (`(?imnsxJU-..)`, `(?^)`, `(?i:..)`),
// comments, `\Q..\E`, `\p{..}` / `\P{..}` (the general categories, `L&`, the scripts and script extensions, the binary properties, `Any`, `Xan`, `Xps`,
// `Xsp`, `Xwd`, `Xuc`; names matched loosely, the property itself the browser's RegExp's), atomic groups and possessive quantifiers, `\o{..}`, `\cX`, `\R`,
// `\N{U+hh}`; a pattern with a back reference, a look-around or an atomic group matched by backtracking within PCRE2's match limit. A pattern with
// anything else (`\X`, `\K`, `\G`, conditions, recursion, branch resets, `\p{bc:..}`, verbs) does not compile (`null`), as the server's does not; a binary
// property the browser does not know (`Prepended_Concatenation_Mark`, `Grapheme_Link`) does not compile here only.

import { BINARY_NAMES, SCRIPT_NAMES } from "./wxRegexNames";

const MAX_PROGRAM = 4000;
const MAX_DEPTH = 64;

type ClassItem =
  | { k: "range"; lo: number; hi: number }
  | { k: "digit" | "word" | "space" | "hspace" | "vspace"; yes: boolean }
  | { k: "posix"; name: PosixName }
  | { k: "prop"; test: (c: number) => boolean; yes: boolean };

type PosixName = "alpha" | "digit" | "alnum" | "upper" | "lower" | "space" | "punct" | "xdigit" | "blank" | "cntrl" | "print" | "graph" | "word" | "ascii";
const POSIX: readonly PosixName[] = ["alpha", "digit", "alnum", "upper", "lower", "space", "punct", "xdigit", "blank", "cntrl", "print", "graph", "word", "ascii"];

type Node =
  | { t: "char"; c: number; icase: boolean }
  | { t: "any"; dotall: boolean }
  | { t: "class"; items: ClassItem[]; negate: boolean; icase: boolean }
  | { t: "bol"; multi: boolean }
  | { t: "eol"; multi: boolean }
  | { t: "endonly" }
  | { t: "wordb"; yes: boolean }
  | { t: "wordedge"; start: boolean }
  | { t: "group"; inner: Node; g: number }
  | { t: "seq"; items: Node[] }
  | { t: "alt"; branches: Node[] }
  | { t: "repeat"; node: Node; min: number; max: number | null; greedy: boolean }
  | { t: "backref"; groups: number[]; icase: boolean }
  | { t: "namedref"; name: string; icase: boolean }
  | { t: "lookahead"; negate: boolean; inner: Node }
  | { t: "lookbehind"; negate: boolean; branches: { len: number; node: Node }[] }
  | { t: "atomic"; inner: Node };

type Inst =
  | { op: "char"; c: number; icase: boolean }
  | { op: "any"; dotall: boolean }
  | { op: "class"; k: number }
  | { op: "split"; a: number; b: number }
  | { op: "jmp"; to: number }
  | { op: "bol"; multi: boolean }
  | { op: "eol"; multi: boolean }
  | { op: "endonly" }
  | { op: "wordb"; yes: boolean }
  | { op: "wordedge"; start: boolean }
  | { op: "save"; n: number }
  | { op: "match" }
  | { op: "backref"; groups: number[]; icase: boolean }
  | { op: "lookahead"; negate: boolean; end: number }
  | { op: "lookbehind"; negate: boolean; branches: { len: number; start: number }[]; end: number }
  | { op: "atomic"; end: number }
  | { op: "submatch" }
  | { op: "loopmark"; k: number }
  | { op: "loopcheck"; k: number; exit: number };

/** PCRE2's `match_limit`: a backtracking match that takes more steps fails (`PCRE2_ERROR_MATCHLIMIT`, no match for `wxRegEx::Matches`). */
const MATCH_LIMIT = 10_000_000;

const isAsciiDigit = (c: number) => c >= 48 && c <= 57;
const isAsciiUpper = (c: number) => c >= 65 && c <= 90;
const isAsciiLower = (c: number) => c >= 97 && c <= 122;
const isAsciiAlpha = (c: number) => isAsciiUpper(c) || isAsciiLower(c);
const isAsciiAlnum = (c: number) => isAsciiAlpha(c) || isAsciiDigit(c);
const isAsciiPunct = (c: number) => (c >= 33 && c <= 47) || (c >= 58 && c <= 64) || (c >= 91 && c <= 96) || (c >= 123 && c <= 126);
const isAsciiHex = (c: number) => isAsciiDigit(c) || (c >= 65 && c <= 70) || (c >= 97 && c <= 102);
const wordChar = (c: number) => isAsciiAlnum(c) || c === 95;
const spaceChar = (c: number) => c === 32 || (c >= 9 && c <= 13);
const HSPACE = new Set([9, 32, 0xa0, 0x1680, 0x180e, 0x202f, 0x205f, 0x3000]);
const isHSpace = (c: number) => HSPACE.has(c) || (c >= 0x2000 && c <= 0x200a);
const isVSpace = (c: number) => (c >= 10 && c <= 13) || c === 0x85 || c === 0x2028 || c === 0x2029;

function posixMatches(name: PosixName, c: number): boolean {
  switch (name) {
    case "alpha":
      return isAsciiAlpha(c);
    case "digit":
      return isAsciiDigit(c);
    case "alnum":
      return isAsciiAlnum(c);
    case "upper":
      return isAsciiUpper(c);
    case "lower":
      return isAsciiLower(c);
    case "space":
      return spaceChar(c);
    case "punct":
      return isAsciiPunct(c);
    case "xdigit":
      return isAsciiHex(c);
    case "blank":
      return c === 32 || c === 9;
    case "cntrl":
      return c < 0x20 || c === 0x7f;
    case "print":
      return c >= 32 && c <= 126;
    case "graph":
      return c >= 33 && c <= 126;
    case "word":
      return wordChar(c);
    case "ascii":
      return c < 128;
  }
}

function itemMatches(i: ClassItem, c: number): boolean {
  switch (i.k) {
    case "range":
      return i.lo <= c && c <= i.hi;
    case "digit":
      return isAsciiDigit(c) === i.yes;
    case "word":
      return wordChar(c) === i.yes;
    case "space":
      return spaceChar(c) === i.yes;
    case "hspace":
      return isHSpace(c) === i.yes;
    case "vspace":
      return isVSpace(c) === i.yes;
    case "posix":
      return posixMatches(i.name, c);
    case "prop":
      return i.test(c) === i.yes;
  }
}

/** PCRE2's loose name matching: letters without case, spaces, hyphens and underscores ignored. */
const loose = (name: string) => name.replace(/[ \-_]/g, "").toLowerCase();

const GENERAL_CATEGORIES = ["Lu", "Ll", "Lt", "Lm", "Lo", "Mn", "Mc", "Me", "Nd", "Nl", "No", "Pc", "Pd", "Ps", "Pe", "Pi", "Pf", "Po", "Sm", "Sc", "Sk", "So", "Zs", "Zl", "Zp", "Cc", "Cf", "Cs", "Co", "Cn"];

/** The test of a `\\p{..}` name (crates/model/src/wx_regex.rs `prop_named`), the property the browser's RegExp's; null for a name PCRE2 does not know or the
 *  browser cannot test. */
function propTest(name: string): ((c: number) => boolean) | null {
  const re = (source: string) => {
    try {
      const r = new RegExp(`^${source}$`, "u");
      return (c: number) => r.test(String.fromCodePoint(c));
    } catch {
      return null;
    }
  };
  const colon = name.indexOf(":");
  const kind = colon < 0 ? null : loose(name.slice(0, colon));
  const value = loose(colon < 0 ? name : name.slice(colon + 1));
  const script = (v: string, ext: boolean) => {
    const s = SCRIPT_NAMES.find(([long, short]) => loose(long) === v || loose(short) === v);
    return s ? re(`\\p{${ext ? "Script_Extensions" : "Script"}=${s[0]}}`) : null;
  };
  if (kind === "sc" || kind === "script") return script(value, false);
  if (kind === "scx" || kind === "scriptextensions") return script(value, true);
  if (kind !== null) return null;
  switch (value) {
    case "any":
      return () => true;
    case "l&":
    case "lc":
      return re("[\\p{Lu}\\p{Ll}\\p{Lt}]");
    case "xan":
      return re("[\\p{L}\\p{N}]");
    case "xps":
    case "xsp":
      return re("[\\p{Z}\\t\\n\\v\\f\\r]");
    case "xwd":
      return re("[\\p{L}\\p{N}_]");
    case "xuc":
      return (c: number) => c === 0x24 || c === 0x40 || c === 0x60 || (c >= 0xa0 && !(c >= 0xd800 && c <= 0xdfff));
  }
  const g = GENERAL_CATEGORIES.find((x) => x.toLowerCase() === value || (value.length === 1 && x.toLowerCase().startsWith(value)));
  if (g) return re(`\\p{${value.length === 1 ? g[0] : g}}`);
  const b = BINARY_NAMES.find(([long, short]) => loose(long) === value || loose(short) === value);
  if (b) return re(`\\p{${b[0]}}`);
  return script(value, true);
}

const lowerOf = (c: number) => String.fromCodePoint(c).toLowerCase();
const upperOf = (c: number) => String.fromCodePoint(c).toUpperCase();

function classMatches(items: readonly ClassItem[], negate: boolean, c: number, icase: boolean): boolean {
  const test = (x: number) => items.some((i) => itemMatches(i, x));
  let hit = test(c);
  if (!hit && icase) for (const s of [lowerOf(c), upperOf(c)]) for (const ch of s) if (test(ch.codePointAt(0)!)) hit = true;
  return hit !== negate;
}

function sameChar(a: number, b: number, icase: boolean): boolean {
  return a === b || (icase && lowerOf(a) === lowerOf(b));
}

/** PCRE2's one-character escapes (`\a \e \f \n \r \t`) and an escaped character that is no ASCII letter or digit; null for any other letter or digit. */
function simpleEscape(c: number): number | null {
  switch (c) {
    case 97:
      return 7; // a
    case 101:
      return 0x1b; // e
    case 102:
      return 12; // f
    case 110:
      return 10; // n
    case 114:
      return 13; // r
    case 116:
      return 9; // t
  }
  return isAsciiAlnum(c) ? null : c;
}

function shorthand(c: string): ClassItem {
  const yes = c === c.toLowerCase();
  const k = c.toLowerCase() === "d" ? "digit" : c.toLowerCase() === "w" ? "word" : "space";
  return { k, yes };
}

/** PCRE2's options a pattern can change inside itself (`(?imnsxJU)`, `(?^)`). */
interface Opts {
  icase: boolean;
  multiline: boolean;
  dotall: boolean;
  extended: boolean;
  noAutoCapture: boolean;
  dupNames: boolean;
  ungreedy: boolean;
}

const isNameChar = (c: number) => isAsciiAlnum(c) || c === 95;

/** The number of characters `n` always matches (a look-behind's branch must have one), null when it varies. */
function fixedLength(n: Node): number | null {
  switch (n.t) {
    case "char":
    case "any":
    case "class":
      return 1;
    case "bol":
    case "eol":
    case "endonly":
    case "wordb":
    case "wordedge":
    case "lookahead":
    case "lookbehind":
      return 0;
    case "group":
    case "atomic":
      return fixedLength(n.inner);
    case "seq": {
      let sum = 0;
      for (const i of n.items) {
        const l = fixedLength(i);
        if (l === null) return null;
        sum += l;
      }
      return sum;
    }
    case "alt": {
      const first = n.branches.length > 0 ? fixedLength(n.branches[0]!) : null;
      return first !== null && n.branches.every((b) => fixedLength(b) === first) ? first : null;
    }
    case "repeat": {
      const l = fixedLength(n.node);
      return n.max === n.min && l !== null ? l * n.min : null;
    }
    default:
      return null;
  }
}

function needsBacktracking(n: Node): boolean {
  switch (n.t) {
    case "backref":
    case "namedref":
    case "lookahead":
    case "lookbehind":
    case "atomic":
      return true;
    case "group":
      return needsBacktracking(n.inner);
    case "seq":
      return n.items.some(needsBacktracking);
    case "alt":
      return n.branches.some(needsBacktracking);
    case "repeat":
      return needsBacktracking(n.node);
    default:
      return false;
  }
}

/** The named back references numbered (several groups of one name under `(?J)`: the first set); false for a name no group has, or a group the pattern lacks. */
function resolveRefs(n: Node, names: readonly [string, number][], groups: number): Node | null {
  const r = (x: Node) => resolveRefs(x, names, groups);
  switch (n.t) {
    case "namedref": {
      const gs = names.filter(([k]) => k === n.name).map(([, g]) => g);
      return gs.length === 0 ? null : { t: "backref", groups: gs, icase: n.icase };
    }
    case "backref":
      return n.groups.every((g) => g >= 1 && g <= groups) ? n : null;
    case "group": {
      const inner = r(n.inner);
      return inner && { ...n, inner };
    }
    case "atomic": {
      const inner = r(n.inner);
      return inner && { ...n, inner };
    }
    case "lookahead": {
      const inner = r(n.inner);
      return inner && { ...n, inner };
    }
    case "repeat": {
      const node = r(n.node);
      return node && { ...n, node };
    }
    case "seq": {
      const items = n.items.map(r);
      return items.every((i) => i !== null) ? { t: "seq", items: items as Node[] } : null;
    }
    case "alt": {
      const branches = n.branches.map(r);
      return branches.every((i) => i !== null) ? { t: "alt", branches: branches as Node[] } : null;
    }
    case "lookbehind": {
      const branches = n.branches.map((b) => ({ len: b.len, node: r(b.node) }));
      return branches.every((b) => b.node !== null) ? { ...n, branches: branches as { len: number; node: Node }[] } : null;
    }
    default:
      return n;
  }
}

class Parser {
  pos = 0;
  depth = 0;
  groups = 0;
  names: [string, number][] = [];
  /** What the last atom was: `\Q..\E`'s characters (a quantifier takes the last), or an item no quantifier may follow. */
  last: "plain" | "quoted" | "unrepeatable" = "plain";
  opts: Opts;
  constructor(
    readonly chars: number[],
    readonly startOpts: Opts
  ) {
    this.opts = { ...startOpts };
  }

  peek(): number | undefined {
    return this.chars[this.pos];
  }
  eat(c: string): boolean {
    if (this.peek() === c.codePointAt(0)) {
      this.pos++;
      return true;
    }
    return false;
  }
  startsWith(s: string): boolean {
    const want = [...s].map((x) => x.codePointAt(0)!);
    return want.every((w, k) => this.chars[this.pos + k] === w);
  }

  /** `(?x)`: white space and `#` comments are no part of the pattern. */
  skipExtended(): void {
    if (!this.opts.extended) return;
    for (let c = this.peek(); c !== undefined; c = this.peek()) {
      if (/\s/u.test(String.fromCodePoint(c))) this.pos++;
      else if (c === 35) {
        while (this.peek() !== undefined && this.chars[this.pos++] !== 10);
      } else if (this.startsWith("(?#")) {
        while (this.peek() !== undefined && this.chars[this.pos++] !== 41);
      } else break;
    }
  }

  alt(): Node | null {
    const first = this.seq();
    if (!first) return null;
    const branches = [first];
    while (this.eat("|")) {
      const b = this.seq();
      if (!b) return null;
      branches.push(b);
    }
    return branches.length === 1 ? branches[0]! : { t: "alt", branches };
  }

  seq(): Node | null {
    const items: Node[] = [];
    for (;;) {
      this.skipExtended();
      const c = this.peek();
      if (c === undefined || c === 124 || c === 41) break; // | )
      const r = this.repeat();
      if (!r) return null;
      items.push(r);
    }
    return { t: "seq", items };
  }

  repeat(): Node | null {
    this.last = "plain";
    const atom = this.atom();
    if (!atom) return null;
    const kind = this.last as Parser["last"];
    this.skipExtended();
    let min: number;
    let max: number | null;
    const c = this.peek();
    if (c === 42) [min, max] = [0, null];
    else if (c === 43) [min, max] = [1, null];
    else if (c === 63) [min, max] = [0, 1];
    else if (c === 123) {
      // `{m}`, `{m,}`, `{m,n}`; a `{` that is no count does not compile
      let close = -1;
      for (let i = this.pos; i < this.chars.length; i++)
        if (this.chars[i] === 125) {
          close = i;
          break;
        }
      if (close < 0) return null;
      const body = String.fromCodePoint(...this.chars.slice(this.pos + 1, close));
      const num = (s: string) => (/^\d+$/.test(s) ? Number(s) : null);
      const comma = body.indexOf(",");
      let lo: number | null;
      let hi: number | null;
      if (comma < 0) lo = hi = num(body);
      else {
        lo = num(body.slice(0, comma));
        hi = body.slice(comma + 1) === "" ? null : num(body.slice(comma + 1));
        if (body.slice(comma + 1) !== "" && hi === null) return null;
      }
      if (lo === null || (comma < 0 && hi === null)) return null;
      if ((hi !== null && hi < lo) || lo > 255 || (hi !== null && hi > 255)) return null;
      this.pos = close;
      [min, max] = [lo, hi];
    } else return atom;
    this.pos++;
    // lazy with a `?` (greedy under `(?U)`), possessive with a `+`
    const possessive = this.eat("+");
    const greedy = !possessive && this.eat("?") ? this.opts.ungreedy : !this.opts.ungreedy || possessive;
    const n = this.peek();
    if (n === 42 || n === 43 || n === 123) return null; // a second quantifier on the same atom
    const wrap = (node: Node): Node => {
      const r: Node = { t: "repeat", node, min, max, greedy };
      return possessive ? { t: "atomic", inner: r } : r;
    };
    if (kind === "unrepeatable") return null;
    if (kind === "quoted" && atom.t === "seq") {
      const last = atom.items.pop();
      if (!last) return null;
      return { t: "seq", items: [...atom.items, wrap(last)] };
    }
    return wrap(atom);
  }

  groupName(close: number): string | null {
    let name = "";
    for (let c = this.peek(); c !== undefined; c = this.peek()) {
      this.pos++;
      if (c === close) return name !== "" && !isAsciiDigit(name.codePointAt(0)!) && name.length <= 32 ? name : null;
      if (!isNameChar(c)) return null;
      name += String.fromCodePoint(c);
    }
    return null;
  }

  /** `(?imnsxJU-..)` / `(?^..)` up to `)` or `:` (left to read). */
  options(): Opts | null {
    let o = { ...this.opts };
    let on = true;
    if (this.eat("^")) o = { icase: this.startOpts.icase, multiline: false, dotall: this.startOpts.dotall, extended: false, noAutoCapture: false, dupNames: false, ungreedy: false };
    for (;;) {
      const c = this.peek();
      if (c === undefined) return null;
      const ch = String.fromCodePoint(c);
      if (ch === ")" || ch === ":") return o;
      if (ch === "-" && on) on = false;
      else if (ch === "i") o.icase = on;
      else if (ch === "m") o.multiline = on;
      else if (ch === "s") o.dotall = on;
      else if (ch === "x") o.extended = on;
      else if (ch === "n") o.noAutoCapture = on;
      else if (ch === "J") o.dupNames = on;
      else if (ch === "U") o.ungreedy = on;
      else return null;
      this.pos++;
    }
  }

  groupBody(): Node | null {
    const saved = { ...this.opts };
    const inner = this.alt();
    this.opts = saved;
    if (!inner || !this.eat(")")) return null;
    this.depth--;
    return inner;
  }

  capture(name: string | null): Node | null {
    const g = ++this.groups;
    if (name !== null) {
      if (this.names.some(([k]) => k === name) && !this.opts.dupNames) return null;
      this.names.push([name, g]);
    }
    const inner = this.groupBody();
    return inner && { t: "group", inner, g };
  }

  atom(): Node | null {
    const c = this.peek();
    if (c === undefined) return null;
    if (c === 91 && (this.startsWith("[[:<:]]") || this.startsWith("[[:>:]]"))) {
      const start = this.chars[this.pos + 3] === 60;
      this.pos += 7;
      return { t: "wordedge", start };
    }
    this.pos++;
    const icase = this.opts.icase;
    switch (c) {
      case 40: {
        // (
        if (++this.depth > MAX_DEPTH) return null;
        if (!this.eat("?")) return this.opts.noAutoCapture ? this.groupBody() : this.capture(null);
        const k = this.peek();
        if (k === undefined) return null;
        const next = this.chars[this.pos + 1];
        const kc = String.fromCodePoint(k);
        if (kc === ":") {
          this.pos++;
          return this.groupBody();
        }
        if (kc === "#") {
          while (this.peek() !== undefined && this.peek() !== 41) this.pos++;
          if (!this.eat(")")) return null;
          this.depth--;
          this.last = "unrepeatable";
          return { t: "seq", items: [] };
        }
        if (kc === "=" || kc === "!") {
          this.pos++;
          const inner = this.groupBody();
          return inner && { t: "lookahead", negate: kc === "!", inner };
        }
        if (kc === "<" && (next === 61 || next === 33)) {
          this.pos += 2;
          const inner = this.groupBody();
          if (!inner) return null;
          const alternatives = inner.t === "alt" ? inner.branches : [inner];
          const branches: { len: number; node: Node }[] = [];
          for (const b of alternatives) {
            const len = fixedLength(b);
            if (len === null) return null;
            branches.push({ len, node: b });
          }
          return { t: "lookbehind", negate: next === 33, branches };
        }
        if (kc === ">") {
          this.pos++;
          const inner = this.groupBody();
          return inner && { t: "atomic", inner };
        }
        if (kc === "<" || kc === "'") {
          this.pos++;
          const name = this.groupName(kc === "<" ? 62 : 39);
          return name === null ? null : this.capture(name);
        }
        if (kc === "P" && next === 60) {
          this.pos += 2;
          const name = this.groupName(62);
          return name === null ? null : this.capture(name);
        }
        if (kc === "P" && next === 61) {
          this.pos += 2;
          const name = this.groupName(41);
          if (name === null) return null;
          this.depth--;
          return { t: "namedref", name, icase };
        }
        const o = this.options();
        if (!o) return null;
        if (this.eat(")")) {
          this.opts = o;
          this.depth--;
          this.last = "unrepeatable";
          return { t: "seq", items: [] };
        }
        this.pos++; // ':'
        const saved = this.opts;
        this.opts = o;
        const inner = this.groupBody();
        this.opts = saved;
        return inner;
      }
      case 41:
      case 42:
      case 43:
      case 63:
      case 123:
        return null;
      case 46:
        return { t: "any", dotall: this.opts.dotall };
      case 94:
        return { t: "bol", multi: this.opts.multiline };
      case 36:
        return { t: "eol", multi: this.opts.multiline };
      case 91:
        return this.klass();
      case 92: {
        const e = this.peek();
        if (e === undefined) return null;
        this.pos++;
        return this.escape(e);
      }
    }
    return { t: "char", c, icase };
  }

  /** `n` hex digits at the parse position as one character (`PCRE2_ALT_BSUX`'s `\xhh`, `\uhhhh`); null, the position kept, when they are not there. */
  hexChar(n: number): number | null {
    const digits = this.chars.slice(this.pos, this.pos + n);
    if (digits.length !== n || !digits.every(isAsciiHex)) return null;
    this.pos += n;
    return parseInt(String.fromCodePoint(...digits), 16);
  }

  braced(): string | null {
    if (!this.eat("{")) return null;
    const close = this.chars.indexOf(125, this.pos);
    if (close < 0) return null;
    const text = String.fromCodePoint(...this.chars.slice(this.pos, close));
    this.pos = close + 1;
    return text;
  }

  property(yes: boolean): ClassItem | null {
    let name: string | null;
    if (this.peek() === 123) name = this.braced();
    else {
      const c = this.peek();
      if (c === undefined) return null;
      this.pos++;
      name = String.fromCodePoint(c);
    }
    if (name === null) return null;
    if (name.startsWith("^")) {
      name = name.slice(1);
      yes = !yes;
    }
    const test = propTest(name.trim());
    return test ? { k: "prop", test, yes } : null;
  }

  quoted(): number[] {
    const out: number[] = [];
    for (let c = this.peek(); c !== undefined; c = this.peek()) {
      if (c === 92 && this.chars[this.pos + 1] === 69) {
        this.pos += 2;
        break;
      }
      out.push(c);
      this.pos++;
    }
    return out;
  }

  octal(digits: string): number | null {
    if (!/^[0-7]+$/.test(digits)) return null;
    const n = parseInt(digits, 8);
    return n <= 0x10ffff ? n : null;
  }

  escape(e: number): Node | null {
    const ch = String.fromCodePoint(e);
    const icase = this.opts.icase;
    const klass = (item: ClassItem): Node => ({ t: "class", items: [item], negate: false, icase });
    const char = (c: number): Node => ({ t: "char", c, icase });
    switch (ch) {
      case "d":
      case "D":
      case "w":
      case "W":
      case "s":
      case "S":
        return klass(shorthand(ch));
      case "h":
      case "H":
        return klass({ k: "hspace", yes: ch === "h" });
      case "v":
      case "V":
        return klass({ k: "vspace", yes: ch === "v" });
      case "p":
      case "P": {
        const item = this.property(ch === "p");
        return item && klass(item);
      }
      case "N": {
        if (this.peek() === 123) {
          const t = this.braced();
          if (!t || !/^U\+[0-9A-Fa-f]+$/.test(t)) return null;
          return char(parseInt(t.slice(2), 16));
        }
        return { t: "class", items: [{ k: "range", lo: 10, hi: 10 }], negate: true, icase: false };
      }
      case "b":
        return { t: "wordb", yes: true };
      case "B":
        return { t: "wordb", yes: false };
      case "A":
        return { t: "bol", multi: false };
      case "Z":
        return { t: "eol", multi: false };
      case "z":
        return { t: "endonly" };
      case "x":
        return char(this.hexChar(2) ?? 120);
      case "u":
        return char(this.hexChar(4) ?? 117);
      case "U":
        return char(85);
      case "Q":
        this.last = "quoted";
        return { t: "seq", items: this.quoted().map((c) => char(c)) };
      case "E":
        this.last = "unrepeatable";
        return { t: "seq", items: [] };
      case "o": {
        const d = this.braced();
        const c = d === null ? null : this.octal(d);
        return c === null ? null : char(c);
      }
      case "c": {
        const x = this.peek();
        if (x === undefined || x > 126 || x < 32) return null;
        this.pos++;
        return char(String.fromCodePoint(x).toUpperCase().codePointAt(0)! ^ 0x40);
      }
      case "R":
        // `(?>\r\n|\n|\x0b|\f|\r|\x85|\x{2028}|\x{2029})`
        return {
          t: "atomic",
          inner: {
            t: "alt",
            branches: [
              { t: "seq", items: [{ t: "char", c: 13, icase: false }, { t: "char", c: 10, icase: false }] },
              { t: "class", items: [{ k: "vspace", yes: true }], negate: false, icase: false },
            ],
          },
        };
      case "g": {
        let text: string | null;
        if (this.peek() === 123) text = this.braced();
        else {
          const start = this.pos;
          if (this.peek() === 45 || this.peek() === 43) this.pos++;
          while (this.peek() !== undefined && isAsciiDigit(this.peek()!)) this.pos++;
          text = String.fromCodePoint(...this.chars.slice(start, this.pos));
        }
        if (!text) return null;
        if (/^[-+]?\d+$/.test(text)) {
          if (text.startsWith("+")) return null;
          const n = Number(text);
          const g = text.startsWith("-") ? this.groups + 1 + n : n;
          return g <= 0 ? null : { t: "backref", groups: [g], icase };
        }
        return { t: "namedref", name: text, icase };
      }
      case "k": {
        const open = this.peek();
        const close = open === 60 ? 62 : open === 39 ? 39 : open === 123 ? 125 : null;
        if (close === null) return null;
        this.pos++;
        const name = this.groupName(close);
        return name === null ? null : { t: "namedref", name, icase };
      }
      case "0": {
        const start = this.pos;
        while (this.pos < start + 2 && this.peek() !== undefined && this.peek()! >= 48 && this.peek()! <= 55) this.pos++;
        const c = this.octal("0" + String.fromCodePoint(...this.chars.slice(start, this.pos)));
        return c === null ? null : char(c);
      }
    }
    if (e >= 49 && e <= 57) {
      // a back reference when below 10, starting with 8 or 9, or with that many groups before it; else up to three octal digits
      const start = this.pos - 1;
      while (this.peek() !== undefined && isAsciiDigit(this.peek()!)) this.pos++;
      const n = Number(String.fromCodePoint(...this.chars.slice(start, this.pos)));
      if (n < 10 || e === 56 || e === 57 || n <= this.groups) return { t: "backref", groups: [n], icase };
      let end = start;
      while (end < start + 3 && this.chars[end] !== undefined && this.chars[end]! >= 48 && this.chars[end]! <= 55) end++;
      this.pos = end;
      const c = this.octal(String.fromCodePoint(...this.chars.slice(start, end)));
      return c === null ? null : char(c);
    }
    const s = simpleEscape(e);
    return s === null ? null : char(s);
  }

  klass(): Node | null {
    const negate = this.eat("^");
    const items: ClassItem[] = [];
    let first = true;
    const pending: number[] = [];
    for (;;) {
      if (pending.length > 0) {
        const q = pending.shift()!;
        items.push({ k: "range", lo: q, hi: q });
        first = false;
        continue;
      }
      const c = this.peek();
      if (c === undefined) return null;
      this.pos++;
      if (c === 93 && !first) break; // ]
      first = false;
      let lo: number;
      if (c === 91 && this.peek() === 58) {
        // [:name:]
        let close = -1;
        for (let i = this.pos + 1; i + 1 < this.chars.length; i++)
          if (this.chars[i] === 58 && this.chars[i + 1] === 93) {
            close = i;
            break;
          }
        if (close < 0) return null;
        const name = String.fromCodePoint(...this.chars.slice(this.pos + 1, close));
        this.pos = close + 2;
        if (!(POSIX as readonly string[]).includes(name)) return null;
        items.push({ k: "posix", name: name as PosixName });
        continue;
      } else if (c === 92) {
        const e = this.peek();
        if (e === undefined) return null;
        this.pos++;
        const ch = String.fromCodePoint(e);
        if ("dDwWsS".includes(ch)) {
          items.push(shorthand(ch));
          continue;
        }
        if ("hHvV".includes(ch)) {
          items.push(ch.toLowerCase() === "h" ? { k: "hspace", yes: ch === "h" } : { k: "vspace", yes: ch === "v" });
          continue;
        }
        if (ch === "p" || ch === "P") {
          const item = this.property(ch === "p");
          if (!item) return null;
          items.push(item);
          continue;
        }
        if (ch === "Q") {
          pending.push(...this.quoted());
          continue;
        }
        if (ch === "E") continue;
        if (ch === "b") lo = 8; // in a class PCRE2's `\b` is a backspace
        else if (ch === "x") lo = this.hexChar(2) ?? 120;
        else if (ch === "u") lo = this.hexChar(4) ?? 117;
        else if (ch === "U") lo = 85;
        else if (ch === "o") {
          const d = this.braced();
          const o = d === null ? null : this.octal(d);
          if (o === null) return null;
          lo = o;
        } else {
          const s = simpleEscape(e);
          if (s === null) return null;
          lo = s;
        }
      } else lo = c;
      // a range `a-z` (a `-` that ends the class is a literal)
      if (this.peek() === 45 && this.chars[this.pos + 1] !== undefined && this.chars[this.pos + 1] !== 93) {
        this.pos++;
        let hi = this.peek();
        if (hi === undefined) return null;
        this.pos++;
        if (hi === 92) {
          hi = this.peek();
          if (hi === undefined) return null;
          this.pos++;
        }
        if (hi < lo) return null;
        items.push({ k: "range", lo, hi });
      } else items.push({ k: "range", lo, hi: lo });
    }
    return { t: "class", items, negate, icase: this.opts.icase };
  }
}

/**
 * `ConvertMetasyntax` and `ConvertWordBoundaries` for `wxRE_ADVANCED` (crates/model/src/wx_regex.rs `advanced_metasyntax`): a leading `***:` dropped, `***=` the
 * rest a literal, a leading `(?opts)` with `i` `c` `s` `e` `q` read (any other letter: does not compile here), and `\m \M \y \Y` as `[[:<:]] [[:>:]] \b \B`.
 */
export function advancedMetasyntax(pattern: string, icase: boolean): { pattern: string; icase: boolean } | null {
  const literal = (rest: string) => [...rest].map((c) => (/^[\p{L}\p{N}]$/u.test(c) ? c : `\\${c}`)).join("");
  let expr = pattern;
  let advanced = true;
  if ([...expr].length > 3 && expr.startsWith("***")) {
    if (expr[3] === ":") expr = expr.slice(4);
    else if (expr[3] === "=") return { pattern: literal(expr.slice(4)), icase };
  }
  if (expr.startsWith("(?")) {
    const m = /^\(\?([a-z]*)\)/.exec(expr);
    if (m) {
      let quoted = false;
      for (const c of m[1]!) {
        if (c === "i") icase = true;
        else if (c === "c") icase = false;
        else if (c === "s") continue;
        else if (c === "e") advanced = false;
        else if (c === "q") quoted = true;
        else return null;
      }
      const rest = expr.slice(m[0].length);
      if (quoted) return { pattern: literal(rest), icase };
      expr = rest;
    }
  }
  if (!advanced) return { pattern: expr, icase };
  let out = "";
  const cs = [...expr];
  for (let i = 0; i < cs.length; i++) {
    if (cs[i] !== "\\") {
      out += cs[i];
      continue;
    }
    const e = cs[++i];
    if (e === undefined) {
      out += "\\";
      break;
    }
    out += e === "m" ? "[[:<:]]" : e === "M" ? "[[:>:]]" : e === "y" ? "\\b" : e === "Y" ? "\\B" : `\\${e}`;
  }
  return { pattern: out, icase };
}

interface Thread {
  pc: number;
  slots: (number | null)[];
}

class LimitReached {}

/** A compiled pattern: `WxRegex.compile( pattern )` is `wxRegEx::Compile( pattern, wxRE_ADVANCED )`, null when it does not compile. */
export class WxRegex {
  private program: Inst[] = [];
  private classes: { items: ClassItem[]; negate: boolean; icase: boolean }[] = [];
  private counted = 0;
  private loops = 0;
  private constructor(
    readonly groups: number,
    /** A back reference, a look-around or an atomic group: matched by backtracking, as PCRE2 matches, within its match limit. */
    private readonly backtrack: boolean
  ) {}

  static compile(pattern: string, icase = false): WxRegex | null {
    const conv = advancedMetasyntax(pattern, icase);
    if (!conv) return null;
    const opts: Opts = { icase: conv.icase, multiline: false, dotall: true, extended: false, noAutoCapture: false, dupNames: false, ungreedy: false };
    const p = new Parser(
      [...conv.pattern].map((c) => c.codePointAt(0)!),
      opts
    );
    const parsed = p.alt();
    if (!parsed || p.pos !== p.chars.length) return null;
    const ast = resolveRefs(parsed, p.names, p.groups);
    if (!ast) return null;
    const r = new WxRegex(p.groups, needsBacktracking(ast));
    if (r.push({ op: "save", n: 0 }) === null || !r.emit(ast) || r.push({ op: "save", n: 1 }) === null || r.push({ op: "match" }) === null) return null;
    return r;
  }

  private push(i: Inst): number | null {
    if (i.op !== "save") {
      if (this.counted >= MAX_PROGRAM) return null;
      this.counted++;
    }
    this.program.push(i);
    return this.program.length - 1;
  }

  private emitSub(n: Node): boolean {
    return this.emit(n) && this.push({ op: "submatch" }) !== null;
  }

  private emit(n: Node): boolean {
    switch (n.t) {
      case "char":
        return this.push({ op: "char", c: n.c, icase: n.icase }) !== null;
      case "any":
        return this.push({ op: "any", dotall: n.dotall }) !== null;
      case "class":
        this.classes.push({ items: n.items, negate: n.negate, icase: n.icase });
        return this.push({ op: "class", k: this.classes.length - 1 }) !== null;
      case "bol":
      case "eol":
        return this.push({ op: n.t, multi: n.multi }) !== null;
      case "endonly":
        return this.push({ op: "endonly" }) !== null;
      case "wordb":
        return this.push({ op: "wordb", yes: n.yes }) !== null;
      case "wordedge":
        return this.push({ op: "wordedge", start: n.start }) !== null;
      case "group":
        return this.push({ op: "save", n: 2 * n.g }) !== null && this.emit(n.inner) && this.push({ op: "save", n: 2 * n.g + 1 }) !== null;
      case "seq":
        return n.items.every((i) => this.emit(i));
      case "alt": {
        const jumps: number[] = [];
        for (let k = 0; k < n.branches.length; k++) {
          if (k + 1 < n.branches.length) {
            const split = this.push({ op: "split", a: 0, b: 0 });
            if (split === null || !this.emit(n.branches[k]!)) return false;
            const j = this.push({ op: "jmp", to: 0 });
            if (j === null) return false;
            jumps.push(j);
            this.program[split] = { op: "split", a: split + 1, b: this.program.length };
          } else if (!this.emit(n.branches[k]!)) return false;
        }
        for (const j of jumps) this.program[j] = { op: "jmp", to: this.program.length };
        return true;
      }
      case "repeat": {
        const split = (body: number, out: number): Inst => (n.greedy ? { op: "split", a: body, b: out } : { op: "split", a: out, b: body });
        for (let k = 0; k < n.min; k++) if (!this.emit(n.node)) return false;
        if (n.max === null) {
          const at = this.push({ op: "split", a: 0, b: 0 });
          if (at === null) return false;
          // the backtracking matcher stops repeating an iteration that read nothing
          const guard = this.backtrack ? this.loops++ : null;
          if (guard !== null && this.push({ op: "loopmark", k: guard }) === null) return false;
          if (!this.emit(n.node)) return false;
          const check = guard !== null ? this.push({ op: "loopcheck", k: guard, exit: 0 }) : null;
          if (this.push({ op: "jmp", to: at }) === null) return false;
          this.program[at] = split(at + 1, this.program.length);
          if (check !== null && guard !== null) this.program[check] = { op: "loopcheck", k: guard, exit: this.program.length };
        } else {
          const splits: number[] = [];
          for (let k = n.min; k < n.max; k++) {
            const s = this.push({ op: "split", a: 0, b: 0 });
            if (s === null || !this.emit(n.node)) return false;
            splits.push(s);
          }
          for (const s of splits) this.program[s] = split(s + 1, this.program.length);
        }
        return true;
      }
      case "backref":
        return this.push({ op: "backref", groups: n.groups, icase: n.icase }) !== null;
      case "namedref":
        return false;
      case "lookahead": {
        const at = this.push({ op: "lookahead", negate: n.negate, end: 0 });
        if (at === null || !this.emitSub(n.inner)) return false;
        this.program[at] = { op: "lookahead", negate: n.negate, end: this.program.length };
        return true;
      }
      case "lookbehind": {
        const at = this.push({ op: "lookbehind", negate: n.negate, branches: [], end: 0 });
        if (at === null) return false;
        const branches: { len: number; start: number }[] = [];
        for (const b of n.branches) {
          branches.push({ len: b.len, start: this.program.length });
          if (!this.emitSub(b.node)) return false;
        }
        this.program[at] = { op: "lookbehind", negate: n.negate, branches, end: this.program.length };
        return true;
      }
      case "atomic": {
        const at = this.push({ op: "atomic", end: 0 });
        if (at === null || !this.emitSub(n.inner)) return false;
        this.program[at] = { op: "atomic", end: this.program.length };
        return true;
      }
    }
  }

  /** Whether an anchor or a boundary holds at `pos`. */
  private holds(ins: Inst, pos: number, chars: readonly number[]): boolean {
    const before = pos > 0 && wordChar(chars[pos - 1]!);
    const after = pos < chars.length && wordChar(chars[pos]!);
    switch (ins.op) {
      case "bol":
        return pos === 0 || (ins.multi && chars[pos - 1] === 10);
      case "eol":
        // PCRE2's `$`: the end, or before a line feed that ends the text; under `(?m)` before any line feed
        return pos === chars.length || (pos + 1 === chars.length && chars[pos] === 10) || (ins.multi && chars[pos] === 10);
      case "endonly":
        return pos === chars.length;
      case "wordb":
        return (before !== after) === ins.yes;
      case "wordedge":
        return (after && !before && ins.start) || (before && !after && !ins.start);
      default:
        return false;
    }
  }

  /** Whether the instruction reads the character `c`. */
  private reads(ins: Inst, c: number): boolean {
    switch (ins.op) {
      case "char":
        return sameChar(ins.c, c, ins.icase);
      case "any":
        return ins.dotall || c !== 10;
      case "class": {
        const k = this.classes[ins.k]!;
        return classMatches(k.items, k.negate, c, k.icase);
      }
      default:
        return false;
    }
  }

  /** The leftmost-first match at or after character `from`: the start and end of each group (group 0 the whole match), in characters; null for none. */
  findAt(chars: readonly number[], from = 0): (number | null)[] | null {
    const width = 2 * (this.groups + 1);
    if (this.backtrack) {
      const budget = { left: MATCH_LIMIT };
      try {
        for (let start = from; start <= chars.length; start++) {
          const slots: (number | null)[] = new Array(width).fill(null);
          if (this.backtrackFrom(0, start, slots, new Array(this.loops).fill(-1), chars, budget) !== null) return slots;
        }
      } catch (e) {
        if (e instanceof LimitReached) return null;
        throw e;
      }
      return null;
    }
    let current: Thread[] = [];
    const seen = new Array<number>(this.program.length).fill(-1);
    let best: (number | null)[] | null = null;
    for (let i = from; i <= chars.length; i++) {
      if (best === null) this.add(current, { pc: 0, slots: new Array(width).fill(null) }, i, chars, seen);
      const k = current.findIndex((t) => this.program[t.pc]!.op === "match");
      if (k >= 0) {
        best = current[k]!.slots.slice();
        current = current.slice(0, k);
      }
      if (i === chars.length || (current.length === 0 && best !== null)) break;
      current = this.step(current, i, chars, seen);
    }
    return best;
  }

  /** crates/model/src/wx_regex.rs `backtrack_from`: PCRE2's way, each choice undone when the rest fails; the end of the first path to `match` or
   *  `submatch`, with the slots it set. Throws `LimitReached` past the match limit. */
  private backtrackFrom(pc0: number, pos0: number, slots: (number | null)[], loops: number[], chars: readonly number[], budget: { left: number }): number | null {
    type Job = { t: "try"; pc: number; pos: number } | { t: "slot"; n: number; v: number | null } | { t: "loop"; k: number; v: number };
    const stack: Job[] = [{ t: "try", pc: pc0, pos: pos0 }];
    const take = (inner: (number | null)[]) => {
      for (let n = 0; n < inner.length; n++)
        if (slots[n] !== inner[n]) {
          stack.push({ t: "slot", n, v: slots[n]! ?? null });
          slots[n] = inner[n]!;
        }
    };
    while (stack.length > 0) {
      const job = stack.pop()!;
      if (job.t === "slot") {
        slots[job.n] = job.v;
        continue;
      }
      if (job.t === "loop") {
        loops[job.k] = job.v;
        continue;
      }
      let { pc, pos } = job;
      for (;;) {
        if (--budget.left < 0) throw new LimitReached();
        const ins = this.program[pc]!;
        let ok = true;
        switch (ins.op) {
          case "char":
          case "any":
          case "class":
            if (pos < chars.length && this.reads(ins, chars[pos]!)) {
              pc++;
              pos++;
            } else ok = false;
            break;
          case "bol":
          case "eol":
          case "endonly":
          case "wordb":
          case "wordedge":
            if (this.holds(ins, pos, chars)) pc++;
            else ok = false;
            break;
          case "split":
            stack.push({ t: "try", pc: ins.b, pos });
            pc = ins.a;
            break;
          case "jmp":
            pc = ins.to;
            break;
          case "save":
            stack.push({ t: "slot", n: ins.n, v: slots[ins.n] ?? null });
            slots[ins.n] = pos;
            pc++;
            break;
          case "loopmark":
            stack.push({ t: "loop", k: ins.k, v: loops[ins.k]! });
            loops[ins.k] = pos;
            pc++;
            break;
          case "loopcheck":
            pc = loops[ins.k] === pos ? ins.exit : pc + 1;
            break;
          case "match":
          case "submatch":
            return pos;
          case "backref": {
            const g = ins.groups.find((x) => slots[2 * x] != null && slots[2 * x + 1] != null);
            const a = g === undefined ? null : slots[2 * g]!;
            const b = g === undefined ? null : slots[2 * g + 1]!;
            if (a !== null && b !== null && b >= a && pos + (b - a) <= chars.length) {
              let same = true;
              for (let i = 0; i < b - a && same; i++) same = sameChar(chars[a + i]!, chars[pos + i]!, ins.icase);
              if (same) {
                pos += b - a;
                pc++;
              } else ok = false;
            } else ok = false;
            break;
          }
          case "lookahead": {
            const inner = slots.slice();
            const hit = this.backtrackFrom(pc + 1, pos, inner, loops.slice(), chars, budget) !== null;
            if (hit === ins.negate) ok = false;
            else {
              if (hit) take(inner);
              pc = ins.end;
            }
            break;
          }
          case "lookbehind": {
            let found: (number | null)[] | null = null;
            for (const b of ins.branches) {
              if (b.len > pos) continue;
              const inner = slots.slice();
              if (this.backtrackFrom(b.start, pos - b.len, inner, loops.slice(), chars, budget) === pos) {
                found = inner;
                break;
              }
            }
            if ((found !== null) === ins.negate) ok = false;
            else {
              if (found) take(found);
              pc = ins.end;
            }
            break;
          }
          case "atomic": {
            const inner = slots.slice();
            const e = this.backtrackFrom(pc + 1, pos, inner, loops.slice(), chars, budget);
            if (e === null) ok = false;
            else {
              take(inner);
              pos = e;
              pc = ins.end;
            }
            break;
          }
        }
        if (!ok) break;
      }
    }
    return null;
  }

  /** Where the first match starts in `text` (in characters, as `wxRegEx::GetMatch`), or null. */
  findStart(text: string): number | null {
    const m = this.findAt([...text].map((c) => c.codePointAt(0)!), 0);
    return m ? m[0]! : null;
  }

  /** `wxRegEx::Matches( text )`. */
  isMatch(text: string): boolean {
    return this.findStart(text) !== null;
  }

  private step(current: readonly Thread[], i: number, chars: readonly number[], seen: number[]): Thread[] {
    const c = chars[i]!;
    const next: Thread[] = [];
    for (const t of current) if (this.reads(this.program[t.pc]!, c)) this.add(next, { pc: t.pc + 1, slots: t.slots.slice() }, i + 1, chars, seen);
    return next;
  }

  private add(list: Thread[], t: Thread, pos: number, chars: readonly number[], seen: number[]): void {
    if (seen[t.pc] === pos) return;
    seen[t.pc] = pos;
    const ins = this.program[t.pc]!;
    switch (ins.op) {
      case "jmp":
        return this.add(list, { pc: ins.to, slots: t.slots }, pos, chars, seen);
      case "split":
        this.add(list, { pc: ins.a, slots: t.slots.slice() }, pos, chars, seen);
        return this.add(list, { pc: ins.b, slots: t.slots }, pos, chars, seen);
      case "bol":
      case "eol":
      case "endonly":
      case "wordb":
      case "wordedge":
        if (this.holds(ins, pos, chars)) this.add(list, { pc: t.pc + 1, slots: t.slots }, pos, chars, seen);
        return;
      case "save": {
        const slots = t.slots.slice();
        slots[ins.n] = pos;
        return this.add(list, { pc: t.pc + 1, slots }, pos, chars, seen);
      }
      default:
        list.push(t);
    }
  }
}

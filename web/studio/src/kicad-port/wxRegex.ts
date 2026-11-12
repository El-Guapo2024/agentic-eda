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
// leftmost-first (PCRE's), by simulation of the compiled machine (no backtracking blow-up). A pattern with anything else -- a back reference, a look-around,
// a named group, an option this port does not read -- does not compile (`null`), as the server's does not: KiCad's PCRE2 takes some of those.

const MAX_PROGRAM = 4000;
const MAX_DEPTH = 64;

type ClassItem =
  | { k: "range"; lo: number; hi: number }
  | { k: "digit" | "word" | "space" | "hspace" | "vspace"; yes: boolean }
  | { k: "posix"; name: PosixName };

type PosixName = "alpha" | "digit" | "alnum" | "upper" | "lower" | "space" | "punct" | "xdigit" | "blank" | "cntrl" | "print" | "graph" | "word" | "ascii";
const POSIX: readonly PosixName[] = ["alpha", "digit", "alnum", "upper", "lower", "space", "punct", "xdigit", "blank", "cntrl", "print", "graph", "word", "ascii"];

type Node =
  | { t: "char"; c: number }
  | { t: "any" }
  | { t: "class"; items: ClassItem[]; negate: boolean }
  | { t: "bol" }
  | { t: "eol" }
  | { t: "endonly" }
  | { t: "wordb"; yes: boolean }
  | { t: "wordedge"; start: boolean }
  | { t: "group"; inner: Node; g: number }
  | { t: "seq"; items: Node[] }
  | { t: "alt"; branches: Node[] }
  | { t: "repeat"; node: Node; min: number; max: number | null; greedy: boolean };

type Inst =
  | { op: "char"; c: number }
  | { op: "any" }
  | { op: "class"; k: number }
  | { op: "split"; a: number; b: number }
  | { op: "jmp"; to: number }
  | { op: "bol" }
  | { op: "eol" }
  | { op: "endonly" }
  | { op: "wordb"; yes: boolean }
  | { op: "wordedge"; start: boolean }
  | { op: "save"; n: number }
  | { op: "match" };

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
  }
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

class Parser {
  pos = 0;
  depth = 0;
  groups = 0;
  constructor(readonly chars: number[]) {}

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
    for (let c = this.peek(); c !== undefined; c = this.peek()) {
      if (c === 124 || c === 41) break; // | )
      const r = this.repeat();
      if (!r) return null;
      items.push(r);
    }
    return { t: "seq", items };
  }

  repeat(): Node | null {
    const atom = this.atom();
    if (!atom) return null;
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
    const greedy = !this.eat("?");
    const n = this.peek();
    if (n === 42 || n === 43 || n === 123) return null; // a second quantifier on the same atom
    return { t: "repeat", node: atom, min, max, greedy };
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
    switch (c) {
      case 40: {
        // (
        if (++this.depth > MAX_DEPTH) return null;
        let capture: number | null = null;
        if (this.eat("?")) {
          if (!this.eat(":")) return null; // look-arounds, named groups and inline options are not read
        } else capture = ++this.groups;
        const inner = this.alt();
        if (!inner || !this.eat(")")) return null;
        this.depth--;
        return capture === null ? inner : { t: "group", inner, g: capture };
      }
      case 41:
      case 42:
      case 43:
      case 63:
      case 123:
        return null;
      case 46:
        return { t: "any" };
      case 94:
        return { t: "bol" };
      case 36:
        return { t: "eol" };
      case 91:
        return this.klass();
      case 92: {
        const e = this.peek();
        if (e === undefined) return null;
        this.pos++;
        return this.escape(e);
      }
    }
    return { t: "char", c };
  }

  /** `n` hex digits at the parse position as one character (`PCRE2_ALT_BSUX`'s `\xhh`, `\uhhhh`); null, the position kept, when they are not there. */
  hexChar(n: number): number | null {
    const digits = this.chars.slice(this.pos, this.pos + n);
    if (digits.length !== n || !digits.every(isAsciiHex)) return null;
    this.pos += n;
    return parseInt(String.fromCodePoint(...digits), 16);
  }

  escape(e: number): Node | null {
    const ch = String.fromCodePoint(e);
    const klass = (item: ClassItem): Node => ({ t: "class", items: [item], negate: false });
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
      case "N":
        return { t: "class", items: [{ k: "range", lo: 10, hi: 10 }], negate: true };
      case "b":
        return { t: "wordb", yes: true };
      case "B":
        return { t: "wordb", yes: false };
      case "A":
        return { t: "bol" };
      case "Z":
        return { t: "eol" };
      case "z":
        return { t: "endonly" };
      case "x":
        return { t: "char", c: this.hexChar(2) ?? 120 };
      case "u":
        return { t: "char", c: this.hexChar(4) ?? 117 };
      case "U":
        return { t: "char", c: 85 };
    }
    const s = simpleEscape(e);
    return s === null ? null : { t: "char", c: s };
  }

  klass(): Node | null {
    const negate = this.eat("^");
    const items: ClassItem[] = [];
    let first = true;
    for (;;) {
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
        if (ch === "b") lo = 8; // in a class PCRE2's `\b` is a backspace
        else if (ch === "x") lo = this.hexChar(2) ?? 120;
        else if (ch === "u") lo = this.hexChar(4) ?? 117;
        else if (ch === "U") lo = 85;
        else {
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
    return { t: "class", items, negate };
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

/** A compiled pattern: `WxRegex.compile( pattern )` is `wxRegEx::Compile( pattern, wxRE_ADVANCED )`, null when it does not compile. */
export class WxRegex {
  private program: Inst[] = [];
  private classes: { items: ClassItem[]; negate: boolean }[] = [];
  private counted = 0;
  private constructor(
    readonly groups: number,
    private readonly icase: boolean
  ) {}

  static compile(pattern: string, icase = false): WxRegex | null {
    const conv = advancedMetasyntax(pattern, icase);
    if (!conv) return null;
    const p = new Parser([...conv.pattern].map((c) => c.codePointAt(0)!));
    const ast = p.alt();
    if (!ast || p.pos !== p.chars.length) return null;
    const r = new WxRegex(p.groups, conv.icase);
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

  private emit(n: Node): boolean {
    switch (n.t) {
      case "char":
        return this.push({ op: "char", c: n.c }) !== null;
      case "any":
        return this.push({ op: "any" }) !== null;
      case "class":
        this.classes.push({ items: n.items, negate: n.negate });
        return this.push({ op: "class", k: this.classes.length - 1 }) !== null;
      case "bol":
      case "eol":
      case "endonly":
        return this.push({ op: n.t }) !== null;
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
          if (at === null || !this.emit(n.node) || this.push({ op: "jmp", to: at }) === null) return false;
          this.program[at] = split(at + 1, this.program.length);
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
    }
  }

  /** The leftmost-first match at or after character `from`: the start and end of each group (group 0 the whole match), in characters; null for none. */
  findAt(chars: readonly number[], from = 0): (number | null)[] | null {
    const width = 2 * (this.groups + 1);
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
    for (const t of current) {
      const ins = this.program[t.pc]!;
      const advance =
        ins.op === "char" ? sameChar(ins.c, c, this.icase) : ins.op === "any" ? true : ins.op === "class" ? classMatches(this.classes[ins.k]!.items, this.classes[ins.k]!.negate, c, this.icase) : false;
      if (advance) this.add(next, { pc: t.pc + 1, slots: t.slots.slice() }, i + 1, chars, seen);
    }
    return next;
  }

  private add(list: Thread[], t: Thread, pos: number, chars: readonly number[], seen: number[]): void {
    if (seen[t.pc] === pos) return;
    seen[t.pc] = pos;
    const ins = this.program[t.pc]!;
    const on = () => this.add(list, { pc: t.pc + 1, slots: t.slots }, pos, chars, seen);
    const before = pos > 0 && wordChar(chars[pos - 1]!);
    const after = pos < chars.length && wordChar(chars[pos]!);
    switch (ins.op) {
      case "jmp":
        return this.add(list, { pc: ins.to, slots: t.slots }, pos, chars, seen);
      case "split":
        this.add(list, { pc: ins.a, slots: t.slots.slice() }, pos, chars, seen);
        return this.add(list, { pc: ins.b, slots: t.slots }, pos, chars, seen);
      case "bol":
        if (pos === 0) on();
        return;
      case "eol":
        // PCRE2's `$`: the end, or before a line feed that ends the text
        if (pos === chars.length || (pos + 1 === chars.length && chars[pos] === 10)) on();
        return;
      case "endonly":
        if (pos === chars.length) on();
        return;
      case "wordb":
        if ((before !== after) === ins.yes) on();
        return;
      case "wordedge":
        if ((after && !before && ins.start) || (before && !after && !ins.start)) on();
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

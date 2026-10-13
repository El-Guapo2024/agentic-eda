// The patterns that assign nets to net classes (Board Setup > Design Rules > Net Classes, "Netclass Assignments"): `EDA_COMBINED_MATCHER( pattern, CTX_NETCLASS )`.
//
// A net belongs to a pattern when *either* reading of the pattern matches the whole net name: as a regular expression (`EDA_PATTERN_MATCH_REGEX_ANCHORED`: `^` and `$` are added when the
// pattern lacks them) or as a wildcard pattern (`EDA_PATTERN_MATCH_WILDCARD_ANCHORED`: `*` is any run of characters, `?` any one character, everything else literal). So `USB_D.*`,
// `USB_D?` and `USB_D[PM]` all name the nets of a USB pair, and a pattern that is no regular expression (`USB_(`) still works as a wildcard pattern.
//
// This is the same matcher as the backend's (crates/model/src/net_pattern.rs) and the two are held to the same cases in their tests. The regular expressions are wxWidgets' advanced ones;
// it reads the part of them net names are matched with: literals, `.`, `[..]` classes (ranges, `^` negation, `[:alpha:]` and the like, the `\d \w \s` shorthands), `( .. )` and
// `(?: .. )` groups, `|`, and `* + ? {m} {m,} {m,n}` with an optional lazy `?`. Matching is by simulation of the machine the pattern compiles to, so no pattern can take longer than its
// length times the net name's (a browser's own RegExp could be made to hang by a pattern in a design file). A pattern with anything else in it (a back reference, a look-ahead, a word
// boundary) is no regular expression here, and matches as a wildcard pattern only -- as KiCad treats one wxRegEx cannot compile.


/**
 * What a net class pattern matches: `*` is any run of characters (empty too), `?` any one character, everything else itself, and the
 * pattern must cover the whole net name (`EDA_PATTERN_MATCH_WILDCARD_ANCHORED`; `eda_model::glob_match` is the backend's twin).
 */
export function globMatch(pattern: string, name: string): boolean {
  const p = Array.from(pattern);
  const s = Array.from(name);
  let pi = 0;
  let si = 0;
  let star = -1; // where the last `*` is in the pattern
  let mark = 0; //  and how much of the name it has swallowed so far
  while (si < s.length) {
    if (pi < p.length && p[pi] === "*") {
      star = pi++;
      mark = si;
    } else if (pi < p.length && (p[pi] === "?" || p[pi] === s[si])) {
      pi++;
      si++;
    } else if (star >= 0) {
      pi = star + 1;
      si = ++mark;
    } else {
      return false;
    }
  }
  while (pi < p.length && p[pi] === "*") pi++;
  return pi === p.length;
}

const MAX_PROGRAM = 4_000;
const MAX_DEPTH = 64;

type ClassItem = { kind: "range"; lo: string; hi: string } | { kind: "digit" | "word" | "space"; yes: boolean } | { kind: "posix"; name: string };

type Node =
  | { t: "char"; c: string }
  | { t: "any" }
  | { t: "class"; items: ClassItem[]; negate: boolean }
  | { t: "bol" }
  | { t: "eol" }
  | { t: "seq"; items: Node[] }
  | { t: "alt"; branches: Node[] }
  | { t: "repeat"; node: Node; min: number; max: number | null };

const isAlpha = (c: string) => /\p{L}/u.test(c);
const isAlnum = (c: string) => /[\p{L}\p{N}]/u.test(c);
const isDigit = (c: string) => c >= "0" && c <= "9";
const isSpace = (c: string) => /\s/u.test(c);

function classMatches(items: readonly ClassItem[], negate: boolean, c: string): boolean {
  const hit = items.some((i) => {
    switch (i.kind) {
      case "range":
        return i.lo <= c && c <= i.hi;
      case "digit":
        return isDigit(c) === i.yes;
      case "word":
        return (isAlnum(c) || c === "_") === i.yes;
      case "space":
        return isSpace(c) === i.yes;
      case "posix":
        switch (i.name) {
          case "alpha":
            return isAlpha(c);
          case "digit":
            return isDigit(c);
          case "alnum":
            return isAlnum(c);
          case "upper":
            return /\p{Lu}/u.test(c);
          case "lower":
            return /\p{Ll}/u.test(c);
          case "space":
            return isSpace(c);
          case "punct":
            return /[!-/:-@[-`{-~]/.test(c);
          case "xdigit":
            return /[0-9a-fA-F]/.test(c);
        }
        return false;
    }
  });
  return hit !== negate;
}

const shorthand = (e: string): ClassItem => (e === "d" ? { kind: "digit", yes: true } : e === "D" ? { kind: "digit", yes: false } : e === "w" ? { kind: "word", yes: true } : e === "W" ? { kind: "word", yes: false } : e === "s" ? { kind: "space", yes: true } : { kind: "space", yes: false });
const POSIX = new Set(["alpha", "digit", "alnum", "upper", "lower", "space", "punct", "xdigit"]);

class Parser {
  chars: string[];
  pos = 0;
  depth = 0;
  constructor(pattern: string) {
    this.chars = Array.from(pattern);
  }

  peek(): string | undefined {
    return this.chars[this.pos];
  }

  eat(c: string): boolean {
    if (this.peek() === c) {
      this.pos++;
      return true;
    }
    return false;
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
      if (c === "|" || c === ")") break;
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
    const q = this.peek();
    if (q === "*") [min, max] = [0, null];
    else if (q === "+") [min, max] = [1, null];
    else if (q === "?") [min, max] = [0, 1];
    else if (q === "{") {
      let close = -1;
      for (let i = this.pos; i < this.chars.length; i++) if (this.chars[i] === "}") { close = i; break; }
      if (close < 0) return null;
      const body = this.chars.slice(this.pos + 1, close).join("");
      const num = (t: string) => (/^\d+$/.test(t) ? Number(t) : NaN);
      const comma = body.indexOf(",");
      let lo: number;
      let hi: number | null;
      if (comma < 0) [lo, hi] = [num(body), num(body)];
      else if (comma === body.length - 1) [lo, hi] = [num(body.slice(0, comma)), null];
      else [lo, hi] = [num(body.slice(0, comma)), num(body.slice(comma + 1))];
      if (Number.isNaN(lo) || (hi !== null && (Number.isNaN(hi) || hi < lo)) || lo > 255 || (hi !== null && hi > 255)) return null;
      this.pos = close;
      [min, max] = [lo, hi];
    } else return atom;
    this.pos++;
    this.eat("?"); // a lazy quantifier matches the same names as a greedy one when the whole name must match
    const next = this.peek();
    if (next === "*" || next === "+" || next === "{") return null; // a second quantifier on the same atom
    return { t: "repeat", node: atom, min, max };
  }

  atom(): Node | null {
    const c = this.peek();
    if (c === undefined) return null;
    this.pos++;
    switch (c) {
      case "(": {
        this.depth++;
        if (this.depth > MAX_DEPTH) return null;
        if (this.eat("?") && !this.eat(":")) return null; // look-ahead, named groups and flags are not read
        const inner = this.alt();
        if (!inner || !this.eat(")")) return null;
        this.depth--;
        return inner;
      }
      case ")":
      case "*":
      case "+":
      case "?":
      case "{":
        return null;
      case ".":
        return { t: "any" };
      case "^":
        return { t: "bol" };
      case "$":
        return { t: "eol" };
      case "[":
        return this.cls();
      case "\\": {
        const e = this.peek();
        if (e === undefined) return null;
        this.pos++;
        if ("dDwWsS".includes(e)) return { t: "class", items: [shorthand(e)], negate: false };
        if (e === "n") return { t: "char", c: "\n" };
        if (e === "t") return { t: "char", c: "\t" };
        if (/[0-9A-Za-z<>]/.test(e)) return null; // back references, word boundaries
        return { t: "char", c: e };
      }
      default:
        return { t: "char", c };
    }
  }

  cls(): Node | null {
    const negate = this.eat("^");
    const items: ClassItem[] = [];
    let first = true;
    for (;;) {
      const c = this.peek();
      if (c === undefined) return null;
      this.pos++;
      if (c === "]" && !first) break;
      first = false;
      let lo: string;
      if (c === "[" && this.peek() === ":") {
        let close = -1;
        for (let i = this.pos + 1; i + 1 < this.chars.length; i++) if (this.chars[i] === ":" && this.chars[i + 1] === "]") { close = i; break; }
        if (close < 0) return null;
        const name = this.chars.slice(this.pos + 1, close).join("");
        if (!POSIX.has(name)) return null;
        this.pos = close + 2;
        items.push({ kind: "posix", name });
        continue;
      } else if (c === "\\") {
        const e = this.peek();
        if (e === undefined) return null;
        this.pos++;
        if ("dDwWsS".includes(e)) {
          items.push(shorthand(e));
          continue;
        }
        if (e === "n") lo = "\n";
        else if (e === "t") lo = "\t";
        else if (/[0-9A-Za-z]/.test(e)) return null;
        else lo = e;
      } else lo = c;
      // a range `a-z` (a `-` that ends the class or starts it is a literal)
      const dash = this.peek() === "-";
      const after = this.chars[this.pos + 1];
      if (dash && after !== undefined && after !== "]") {
        this.pos++;
        let hi = this.peek();
        if (hi === undefined) return null;
        this.pos++;
        if (hi === "\\") {
          hi = this.peek();
          if (hi === undefined) return null;
          this.pos++;
        }
        if (hi < lo) return null;
        items.push({ kind: "range", lo, hi });
      } else items.push({ kind: "range", lo, hi: lo });
    }
    return { t: "class", items, negate };
  }
}

type Inst =
  | { op: "char"; c: string }
  | { op: "any" }
  | { op: "class"; k: number }
  | { op: "split"; a: number; b: number }
  | { op: "jmp"; to: number }
  | { op: "bol" }
  | { op: "eol" }
  | { op: "match" };

/** A compiled pattern: matches a whole name. */
export class NetRegex {
  private program: Inst[] = [];
  private classes: Array<{ items: ClassItem[]; negate: boolean }> = [];

  private constructor() {}

  /** Compile `pattern` as KiCad's anchored regular expression; null when it is not one this reads. */
  static compile(pattern: string): NetRegex | null {
    const p = new Parser(pattern);
    const ast = p.alt();
    if (!ast || p.pos !== p.chars.length) return null; // an unmatched `)`
    const r = new NetRegex();
    if (!r.emit(ast)) return null;
    r.program.push({ op: "match" });
    return r;
  }

  private push(i: Inst): number {
    if (this.program.length >= MAX_PROGRAM) return -1;
    this.program.push(i);
    return this.program.length - 1;
  }

  private emit(n: Node): boolean {
    switch (n.t) {
      case "char":
        return this.push({ op: "char", c: n.c }) >= 0;
      case "any":
        return this.push({ op: "any" }) >= 0;
      case "class":
        this.classes.push({ items: n.items, negate: n.negate });
        return this.push({ op: "class", k: this.classes.length - 1 }) >= 0;
      case "bol":
        return this.push({ op: "bol" }) >= 0;
      case "eol":
        return this.push({ op: "eol" }) >= 0;
      case "seq":
        return n.items.every((i) => this.emit(i));
      case "alt": {
        const jumps: number[] = [];
        for (let k = 0; k < n.branches.length; k++) {
          if (k + 1 < n.branches.length) {
            const split = this.push({ op: "split", a: 0, b: 0 });
            if (split < 0 || !this.emit(n.branches[k]!)) return false;
            const jmp = this.push({ op: "jmp", to: 0 });
            if (jmp < 0) return false;
            jumps.push(jmp);
            this.program[split] = { op: "split", a: split + 1, b: this.program.length };
          } else if (!this.emit(n.branches[k]!)) return false;
        }
        for (const j of jumps) this.program[j] = { op: "jmp", to: this.program.length };
        return true;
      }
      case "repeat": {
        for (let i = 0; i < n.min; i++) if (!this.emit(n.node)) return false;
        if (n.max === null) {
          const split = this.push({ op: "split", a: 0, b: 0 });
          if (split < 0 || !this.emit(n.node) || this.push({ op: "jmp", to: split }) < 0) return false;
          this.program[split] = { op: "split", a: split + 1, b: this.program.length };
        } else {
          const splits: number[] = [];
          for (let i = n.min; i < n.max; i++) {
            const s = this.push({ op: "split", a: 0, b: 0 });
            if (s < 0 || !this.emit(n.node)) return false;
            splits.push(s);
          }
          for (const s of splits) this.program[s] = { op: "split", a: s + 1, b: this.program.length };
        }
        return true;
      }
    }
  }

  /** Whether the whole of `text` matches. */
  isMatch(text: string): boolean {
    const chars = Array.from(text);
    const n = this.program.length;
    const seen = new Array<number>(n).fill(-1);
    let current: number[] = [];
    this.add(current, 0, 0, chars.length, seen, 0);
    for (let i = 0; i < chars.length; i++) {
      const c = chars[i]!;
      const step = i + 1;
      const next: number[] = [];
      for (const pc of current) {
        const inst = this.program[pc]!;
        let advance = false;
        if (inst.op === "char") advance = inst.c === c;
        else if (inst.op === "any") advance = true;
        else if (inst.op === "class") advance = classMatches(this.classes[inst.k]!.items, this.classes[inst.k]!.negate, c);
        if (advance) this.add(next, pc + 1, step, chars.length, seen, step);
      }
      current = next;
      if (current.length === 0) return false;
    }
    return current.some((pc) => this.program[pc]!.op === "match");
  }

  private add(list: number[], pc: number, pos: number, len: number, seen: number[], mark: number): void {
    if (seen[pc] === mark) return;
    seen[pc] = mark;
    const inst = this.program[pc]!;
    switch (inst.op) {
      case "jmp":
        this.add(list, inst.to, pos, len, seen, mark);
        break;
      case "split":
        this.add(list, inst.a, pos, len, seen, mark);
        this.add(list, inst.b, pos, len, seen, mark);
        break;
      case "bol":
        if (pos === 0) this.add(list, pc + 1, pos, len, seen, mark);
        break;
      case "eol":
        if (pos === len) this.add(list, pc + 1, pos, len, seen, mark);
        break;
      default:
        list.push(pc);
    }
  }
}

const compiled = new Map<string, NetRegex | null>();

function regexOf(pattern: string): NetRegex | null {
  let hit = compiled.get(pattern);
  if (hit === undefined) {
    hit = NetRegex.compile(pattern);
    if (compiled.size > 2_000) compiled.clear();
    compiled.set(pattern, hit);
  }
  return hit;
}

/** Whether `pattern` compiles as a regular expression (otherwise it only works as a wildcard pattern). */
export function isRegex(pattern: string): boolean {
  return regexOf(pattern) !== null;
}

/** Whether the whole of `net` matches `pattern` read as an anchored regular expression. False when the pattern is no regular expression this reads. */
export function regexMatches(pattern: string, net: string): boolean {
  return regexOf(pattern)?.isMatch(net) ?? false;
}

/** `EDA_COMBINED_MATCHER::StartsWith( net )` in the net class context: the whole of `net` matches `pattern` read as a wildcard pattern or as a regular expression. */
export function patternMatches(pattern: string, net: string): boolean {
  return globMatch(pattern, net) || regexMatches(pattern, net);
}

/** Whether `pattern` has a character a regular expression gives a meaning to besides `*` and `?` (which are wildcards too): a pattern without one reads the same either way. */
export function hasRegexSyntax(pattern: string): boolean {
  return /[.+()[\]{}|^$\\]/.test(pattern);
}

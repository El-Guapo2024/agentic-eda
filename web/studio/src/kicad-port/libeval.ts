// INACTIVE (2026-10-28): replaced by the backend's expression language (`POST /api/pcb/query`, `crates/drc/src/query.rs` over `eda_drc::pcbexpr`
// and `eda_drc::libeval`, the workspace's one copy); kept for a future build that evaluates in the browser. Nothing in the studio imports it: only its
// own tests and the equivalence test (`pcbQuery.test.ts`, which holds this copy and the Rust one to the same golden answers) do.
//
// KiCad's expression language -- `libeval_compiler` (common/libeval_compiler/libeval_compiler.cpp, include/libeval_compiler/libeval_compiler.h, grammar.lemon at
// 8303b2ad): the lexer, the grammar with its precedences, the code generator that turns the tree into a list of operations (UCODE), and the machine that runs it
// on a stack of VALUEs. It is what `DRC_RULE_CONDITION` ("A.NetClass == 'HV'") and Find by Properties' Query tab compile. What the language talks about -- the items
// `A` and `B`, their properties, `A.hasNetclass('x')` -- is the host's (`UcodeHost`): pcbExpr.ts has the board's.
//
// Ported as it is, quirks included:
//   - only the space is white space: a tab or a new line is "Unrecognized character";
//   - a unit (`mil` `mm` `in` `deg` `fs` `ps`) is looked for before an identifier, at every token start, and it is case sensitive;
//   - a `,` is a decimal separator inside a number (`1,5` is 1.5) and a separator of arguments between two;
//   - `||` binds tighter than `&&` (lemon gives a later `%left` line the higher precedence), so `a && b || c` is `a && (b || c)`;
//   - the comparisons and `.` do not chain (`%nonassoc`: `a < b < c` is a syntax error);
//   - `Compile` answers true when the text parses: an unknown item, property or function is reported to the callback but is not a failure, as in `COMPILER::Compile`;
//   - the arguments of a call are pushed last first (`generateUCode` pushes them on its work stack in order and takes the top first).
//
// Pure: no React, no DOM, no board (compiled by `npm run test:unit`).

// ---------------------------------------------------------------------------------------------------------------------------------------- values

/** `EDA_UNITS` as far as the expression language uses it. */
export type Units = "unscaled" | "mil" | "mm" | "in" | "deg" | "fs" | "ps";

/** `VAR_TYPE_T`: what a property reference will produce (decided when the code is generated). */
export type VarType = "string" | "numeric" | "numeric_double" | "undefined" | "parse_error" | "null";

type ValueKind = "undefined" | "null" | "numeric" | "string";

/** `WildCompareString( pattern, string, false )` (common/string_utils.cpp): `*` any run, `?` any one character, both sides upper-cased. */
export function wildCompare(pattern: string, text: string, caseSensitive = false): boolean {
  const wild = [...(caseSensitive ? pattern : pattern.toUpperCase())];
  const str = [...(caseSensitive ? text : text.toUpperCase())];
  let w = 0;
  let s = 0;
  let mp = -1;
  let cp = -1;
  while (s < str.length && wild[w] !== "*") {
    if (wild[w] !== str[s] && wild[w] !== "?") return false;
    w++;
    s++;
  }
  while (s < str.length) {
    if (wild[w] === "*") {
      w++;
      if (w >= wild.length) return true;
      mp = w;
      cp = s + 1;
    } else if (wild[w] === str[s] || wild[w] === "?") {
      w++;
      s++;
    } else {
      w = mp;
      s = cp++;
    }
  }
  while (wild[w] === "*") w++;
  return w >= wild.length;
}

/** `LIBEVAL::VALUE`: a number, a string or nothing. A subclass overrides the comparison (a layer, a net class ...). */
export class Value {
  protected kind: ValueKind = "undefined";
  protected dbl = 0;
  protected str = "";
  /** A string literal holding `?` or `*`: as the right side of `==` it is a pattern. */
  wildcard = false;
  units: Units = "unscaled";
  private lazyDbl: (() => number) | null = null;
  private lazyStr: (() => string) | null = null;

  static undefinedValue(): Value {
    return new Value();
  }

  /** `VALUE::MakeNullValue`: the `null` of the language. */
  static nullValue(): Value {
    const v = new Value();
    v.kind = "null";
    return v;
  }

  static number(n: number, units: Units = "unscaled"): Value {
    const v = new Value();
    v.kind = "numeric";
    v.dbl = n;
    v.units = units;
    return v;
  }

  static string(s: string, wildcard = false): Value {
    const v = new Value();
    v.kind = "string";
    v.str = s;
    v.wildcard = wildcard;
    return v;
  }

  /** `VALUE::GetType`: VT_NUMERIC, VT_STRING, VT_UNDEFINED or VT_NULL. */
  get type(): ValueKind {
    return this.kind;
  }

  asDouble(): number {
    if (this.lazyDbl) {
      this.dbl = this.lazyDbl();
      this.lazyDbl = null;
    }
    return this.dbl;
  }

  asString(): string {
    if (this.lazyStr) {
      this.str = this.lazyStr();
      this.lazyStr = null;
    }
    return this.str;
  }

  set(n: number): void {
    this.kind = "numeric";
    this.dbl = n;
  }

  setText(s: string): void {
    this.kind = "string";
    this.str = s;
  }

  setUnits(u: Units): void {
    this.units = u;
  }

  /** `SetDeferredEval`: the number is worked out when it is first read (a function that may never be asked). */
  setDeferredNumber(fn: () => number): void {
    this.kind = "numeric";
    this.lazyDbl = fn;
  }

  setDeferredString(fn: () => string): void {
    this.kind = "string";
    this.lazyStr = fn;
  }

  /** `VALUE::Set( const VALUE& )`. */
  copyFrom(v: Value): void {
    this.kind = v.kind;
    this.dbl = v.asDouble();
    this.wildcard = v.wildcard;
    this.units = v.units;
    if (v.kind === "string") this.str = v.asString();
  }

  /** `VALUE::EqualTo`. */
  equalTo(ctx: Context, b: Value): boolean {
    if (this.kind === "undefined" || b.kind === "undefined") return false;
    if (this.kind === "null" && b.kind === "null") return true;
    if (this.kind === "numeric" && b.kind === "numeric") return this.asDouble() === b.asDouble();
    if (this.kind === "string" && b.kind === "string") {
      if (b.wildcard) return wildCompare(b.asString(), this.asString(), false);
      return this.asString().toUpperCase() === b.asString().toUpperCase();
    }
    void ctx;
    return false;
  }

  /** `VALUE::NotEqualTo`: not the inverse of `equalTo` -- both say false for an undefined value. */
  notEqualTo(ctx: Context, b: Value): boolean {
    if (this.kind === "undefined" || b.kind === "undefined") return false;
    return !this.equalTo(ctx, b);
  }
}

/** `LIBEVAL::VAR_REF`: a reference to a variable or a property, resolved to a value each time the code runs. */
export interface VarRef {
  getType(): VarType;
  getValue(ctx: Context): Value;
}

/** `FUNC_CALL_REF`: a function takes its arguments off the stack and leaves its result on it. */
export type FuncCall = (ctx: Context, self: VarRef | null) => void;

/** `LIBEVAL::CONTEXT`: the stack the code runs on, and the callback errors of a run are told to. */
export class Context {
  private stack: Value[] = [];
  errorCallback: ((message: string, offset: number) => void) | null = null;

  allocValue(): Value {
    return new Value();
  }

  storeValue(v: Value): Value {
    return v;
  }

  push(v: Value): void {
    this.stack.push(v);
  }

  /** `CONTEXT::Pop`: an empty stack is "Malformed expression" and an undefined value. */
  pop(): Value {
    const v = this.stack.pop();
    if (v) return v;
    this.reportError("Malformed expression");
    return this.allocValue();
  }

  get sp(): number {
    return this.stack.length;
  }

  hasErrorCallback(): boolean {
    return this.errorCallback !== null;
  }

  reportError(message: string): void {
    if (this.errorCallback) this.errorCallback(message, -1);
  }
}

// ----------------------------------------------------------------------------------------------------------------------------------------- code

type BinaryOp = "mul" | "div" | "add" | "sub" | "less" | "greater" | "less_equal" | "greater_equal" | "equal" | "not_equal" | "and" | "or";
type NodeOp = BinaryOp | "not" | "number" | "identifier" | "string" | "struct_ref" | "func_call" | "unit" | "arg_list" | "null";

/** `UOP`: one operation of the code. */
type Uop = { kind: "push_var"; ref: VarRef | null } | { kind: "push_value"; value: Value } | { kind: "method_call"; func: FuncCall | null; ref: VarRef | null } | { kind: "op"; op: NodeOp };

/** `LIBEVAL::UCODE`: the compiled expression. */
export class Ucode {
  readonly ops: Uop[] = [];

  /** `UCODE::Run`: a run that fails runs no rule (0), and so does one that leaves the stack other than with one value. */
  run(ctx: Context): Value {
    try {
      for (const op of this.ops) exec(op, ctx);
    } catch {
      return Value.number(0);
    }
    if (ctx.sp === 1) return ctx.pop();
    return Value.number(0);
  }
}

/** The host of a compilation: what `A.Width` and `A.hasNetclass('x')` mean (`UCODE::CreateVarRef`, `CreateFuncCall`). */
export interface UcodeHost {
  createVarRef(variable: string, field: string): VarRef | null;
  createFuncCall(name: string): FuncCall | null;
}

function resultUnits(a: Value | null, b: Value | null): Units {
  // `getOpResultUnits`: a unit from one side when the other has none, else the right side's.
  if (!a || !b) return "unscaled";
  if (a.units === "unscaled" && b.units !== "unscaled") return b.units;
  if (a.units !== "unscaled" && b.units === "unscaled") return a.units;
  return b.units;
}

/** `UOP::Exec`. */
function exec(u: Uop, ctx: Context): void {
  if (u.kind === "push_var") {
    ctx.push(u.ref ? ctx.storeValue(u.ref.getValue(ctx)) : ctx.allocValue());
    return;
  }
  if (u.kind === "push_value") {
    // A string literal is copied for each run, as the C++ does for the threads of a DRC.
    if (u.value.type === "string") {
      const copy = ctx.allocValue();
      copy.copyFrom(u.value);
      ctx.push(copy);
    } else {
      ctx.push(u.value);
    }
    return;
  }
  if (u.kind === "method_call") {
    if (u.func) u.func(ctx, u.ref);
    return;
  }
  const op = u.op;
  const asDouble = (v: Value | null): number => (v ? v.asDouble() : 0);
  if (op === "not") {
    const a = ctx.pop();
    const r = ctx.allocValue();
    r.set(a.asDouble() !== 0 ? 0 : 1);
    r.setUnits(a.units);
    ctx.push(r);
    return;
  }
  if (op === "number" || op === "identifier" || op === "string" || op === "struct_ref" || op === "func_call" || op === "unit" || op === "arg_list" || op === "null") return;
  // The binary operators.
  const b = ctx.pop();
  const a = ctx.pop();
  if (ctx.hasErrorCallback()) {
    if (a.type === "string" && b.type === "numeric") ctx.reportError(`Type mismatch between '${a.asString()}' and ${b.asDouble().toFixed(6)}`);
    else if (a.type === "numeric" && b.type === "string") ctx.reportError(`Type mismatch between ${a.asDouble().toFixed(6)} and '${b.asString()}'`);
  }
  let result: number;
  let units: Units = "unscaled";
  switch (op) {
    case "add":
      result = asDouble(a) + asDouble(b);
      units = resultUnits(a, b);
      break;
    case "sub":
      result = asDouble(a) - asDouble(b);
      units = resultUnits(a, b);
      break;
    case "mul":
      result = asDouble(a) * asDouble(b);
      units = resultUnits(a, b);
      break;
    case "div":
      result = asDouble(a) / asDouble(b);
      units = resultUnits(a, b);
      break;
    case "less_equal":
      result = asDouble(a) <= asDouble(b) ? 1 : 0;
      break;
    case "greater_equal":
      result = asDouble(a) >= asDouble(b) ? 1 : 0;
      break;
    case "less":
      result = asDouble(a) < asDouble(b) ? 1 : 0;
      break;
    case "greater":
      result = asDouble(a) > asDouble(b) ? 1 : 0;
      break;
    case "equal":
      // `if( arg2->GetType() == VT_UNDEFINED ) arg2->EqualTo( arg1 )`: an undefined value is false either way round.
      result = b.type === "undefined" ? (b.equalTo(ctx, a) ? 1 : 0) : a.equalTo(ctx, b) ? 1 : 0;
      break;
    case "not_equal":
      result = b.type === "undefined" ? (b.notEqualTo(ctx, a) ? 1 : 0) : a.notEqualTo(ctx, b) ? 1 : 0;
      break;
    case "and":
      result = asDouble(a) !== 0 && asDouble(b) !== 0 ? 1 : 0;
      break;
    case "or":
      result = asDouble(a) !== 0 || asDouble(b) !== 0 ? 1 : 0;
      break;
    default:
      result = 0;
  }
  const r = ctx.allocValue();
  r.set(result);
  r.setUnits(units);
  ctx.push(r);
}

// ------------------------------------------------------------------------------------------------------------------------------------- the lexer

/** `UNIT_RESOLVER`: the units a number may carry, in the order they are looked for, and what each means. */
export interface UnitResolver {
  names: readonly string[];
  types: readonly Units[];
  /** `GetSupportedUnitsMessage`: said of a number that has none ("must be mm, in, mil, deg, fs, or ps"); empty when units are not asked for. */
  message: string;
  convert(text: string, unitId: number): number;
}

type TokenKind = "value" | "unit" | "string" | "identifier" | "comma" | "lt" | "gt" | "le" | "ge" | "eq" | "ne" | "and" | "or" | "not" | "plus" | "minus" | "mult" | "divide" | "dot" | "lparen" | "rparen" | "semicolon" | "ends";

interface Token {
  kind: TokenKind;
  text: string;
  idx: number;
  /** `COMPILER::m_sourcePos` when the token was asked for: where the lexer stood before it skipped the blanks. */
  before: number;
}

const isDigit = (c: string): boolean => c >= "0" && c <= "9";
const isAlpha = (c: string): boolean => (c >= "a" && c <= "z") || (c >= "A" && c <= "Z");
const isAlnum = (c: string): boolean => isDigit(c) || isAlpha(c);

/** The lexer: `COMPILER::getToken` over `TOKENIZER`. `errors` are told as the C++ tells `reportError`. */
class Lexer {
  pos = 0;
  private inString = false;

  constructor(
    private readonly text: string,
    private readonly units: UnitResolver,
    private readonly fail: (message: string, pos: number) => void
  ) {}

  private char(): string {
    return this.pos >= this.text.length ? "\0" : (this.text[this.pos] as string);
  }

  /** `TOKENIZER::MatchAhead`. */
  private matchAhead(match: string, stop: (c: string) => boolean): boolean {
    const remaining = this.text.length - this.pos;
    if (remaining < match.length) return false;
    if (this.text.substr(this.pos, match.length) === match) return remaining === match.length || stop(this.text[this.pos + match.length] as string);
    return false;
  }

  /** `TOKENIZER::GetString`: up to the next quote that is not escaped, the closing one skipped. */
  private getString(): string {
    let out = "";
    while (this.pos < this.text.length && this.text[this.pos] !== "'") {
      if (this.text[this.pos] === "\\" && this.pos + 1 < this.text.length && this.text[this.pos + 1] === "'") this.pos++;
      out += this.text[this.pos++];
    }
    this.pos++;
    return out;
  }

  private resolveUnits(): number {
    for (let id = 0; id < this.units.names.length; id++) {
      const name = this.units.names[id] as string;
      if (this.matchAhead(name, (c) => !isAlnum(c))) {
        this.pos += name.length;
        return id;
      }
    }
    return -1;
  }

  next(): Token {
    const before = this.pos;
    for (;;) {
      if (this.inString) {
        this.inString = false;
        return { kind: "string", text: this.getString(), idx: 0, before };
      }
      const t = this.lexDefault(before);
      if (t) return t;
    }
  }

  /** `COMPILER::lexDefault`; null when the string state was entered and the lexer is to be asked again. */
  private lexDefault(before: number): Token | null {
    const tok = (kind: TokenKind, text = "", idx = -1): Token => ({ kind, text, idx, before });
    if (this.pos >= this.text.length) return tok("ends");
    let ch = this.char();
    while (ch === " ") {
      this.pos++;
      ch = this.char();
    }
    if (ch === "\0") return tok("ends");
    const isDecimal = (c: string): boolean => c === "." || c === ",";
    if (isDigit(ch)) {
      let current = "";
      let haveSeparator = false;
      do {
        if (isDecimal(ch) && haveSeparator) break;
        current += ch;
        if (isDecimal(ch)) haveSeparator = true;
        this.pos++;
        ch = this.char();
      } while (isDigit(ch) || isDecimal(ch));
      return tok("value", current.replace(/,/g, "."));
    }
    const unit = this.resolveUnits();
    if (unit >= 0) return tok("unit", "", unit);
    if (ch === "'") {
      this.inString = true;
      this.pos++;
      return null;
    }
    if (isAlpha(ch) || ch === "_") {
      let end = this.pos;
      while (end < this.text.length && (isAlnum(this.text[end] as string) || this.text[end] === "_")) end++;
      const name = this.text.slice(this.pos, end);
      this.pos = end;
      return tok("identifier", name);
    }
    const two: Array<[string, TokenKind, string]> = [
      ["==", "eq", "="],
      ["!=", "ne", "="],
      ["<=", "le", "="],
      [">=", "ge", "="],
      ["&&", "and", "&"],
      ["||", "or", "|"],
    ];
    for (const [text, kind, notFollowedBy] of two) {
      if (this.matchAhead(text, (c) => c !== notFollowedBy)) {
        this.pos += 2;
        return tok(kind);
      }
    }
    const single: Record<string, TokenKind> = { "+": "plus", "!": "not", "-": "minus", "*": "mult", "/": "divide", "<": "lt", ">": "gt", "(": "lparen", ")": "rparen", ";": "semicolon", ".": "dot", ",": "comma" };
    const kind = single[ch];
    if (kind) {
      this.pos++;
      return tok(kind);
    }
    // Anything else: the error, then the token that ends the text (`retval.token` stays `G_ENDS`).
    if (this.matchAhead("${", (c) => c !== "{")) this.fail("Unresolved text variable reference", before + 2);
    else this.fail(`Unrecognized character '${ch}'`, before);
    this.pos++;
    return tok("ends");
  }
}

// ------------------------------------------------------------------------------------------------------------------------------------ the parser

interface TreeNode {
  op: NodeOp;
  text: string | null;
  idx: number;
  leaf: [TreeNode | null, TreeNode | null];
  srcPos: number;
}

class SyntaxFailure extends Error {}

/** The binary operators of the grammar by token: the precedence (the order of the `%left`/`%nonassoc` lines of grammar.lemon, later is tighter) and whether they chain. */
const BINARY: Partial<Record<TokenKind, { prec: number; op: NodeOp; nonassoc: boolean }>> = {
  comma: { prec: 2, op: "arg_list", nonassoc: false },
  and: { prec: 3, op: "and", nonassoc: false },
  or: { prec: 4, op: "or", nonassoc: false },
  lt: { prec: 6, op: "less", nonassoc: true },
  gt: { prec: 6, op: "greater", nonassoc: true },
  le: { prec: 6, op: "less_equal", nonassoc: true },
  ge: { prec: 6, op: "greater_equal", nonassoc: true },
  eq: { prec: 6, op: "equal", nonassoc: true },
  ne: { prec: 6, op: "not_equal", nonassoc: true },
  plus: { prec: 8, op: "add", nonassoc: false },
  minus: { prec: 8, op: "sub", nonassoc: false },
  divide: { prec: 9, op: "div", nonassoc: false },
  mult: { prec: 9, op: "mul", nonassoc: false },
  dot: { prec: 10, op: "struct_ref", nonassoc: true },
};

/** The rules of grammar.lemon as a precedence-climbing parser; it accepts what the LALR(1) parser accepts and builds the tree its actions build. */
class Parser {
  private look: Token;

  constructor(
    private readonly lexer: Lexer,
    first: Token
  ) {
    this.look = first;
  }

  private node(op: NodeOp, text: string | null = null, idx = 0): TreeNode {
    // `newNode` is called when the parser reduces, i.e. with the next token already read.
    return { op, text, idx, leaf: [null, null], srcPos: this.look.before };
  }

  private advance(): Token {
    const t = this.look;
    this.look = this.lexer.next();
    return t;
  }

  /** The text is a statement: an expression and the end, or the end alone (nothing to compile). */
  parseStatement(): TreeNode | null {
    if (this.peekKind() === "ends") return null;
    const root = this.expr(0);
    if (this.peekKind() !== "ends") throw new SyntaxFailure();
    return root;
  }

  /** The token the parser failed on (its position is the syntax error's). */
  get lookahead(): Token {
    return this.look;
  }

  /** The kind of the next token, not narrowed by the checks made before it was read. */
  private peekKind(): TokenKind {
    return this.look.kind;
  }

  private expr(minPrec: number): TreeNode {
    let left = this.unary();
    for (;;) {
      const info = BINARY[this.look.kind];
      if (!info || info.prec < minPrec) break;
      this.advance();
      // A right operand of `.` is a primary (nothing binds tighter), of the others whatever binds tighter than the operator.
      const right = this.expr(info.prec + 1);
      const n = this.node(info.op);
      n.leaf = [left, right];
      left = n;
      if (info.nonassoc) {
        const again = BINARY[this.look.kind];
        if (again && again.prec === info.prec) throw new SyntaxFailure();
      }
    }
    return left;
  }

  private unary(): TreeNode {
    switch (this.look.kind) {
      case "not": {
        this.advance();
        // `G_BOOL_NOT nt` is %right at the precedence of NOT (7): what binds tighter belongs to its operand.
        const operand = this.expr(8);
        const n = this.node("not");
        n.leaf[0] = operand;
        return n;
      }
      case "plus": {
        this.advance();
        return this.expr(9);
      }
      case "minus": {
        this.advance();
        // `G_MINUS nt` has the precedence of MINUS (8, left): only * and / and . go into its operand. It is 0 - operand.
        const operand = this.expr(9);
        const zero = this.node("number");
        const n = this.node("sub");
        n.leaf = [zero, operand];
        return n;
      }
      default:
        return this.primary();
    }
  }

  private primary(): TreeNode {
    const t = this.look;
    switch (t.kind) {
      case "value": {
        this.advance();
        const n = this.node("number", t.text);
        if (this.peekKind() === "unit") {
          const u = this.advance();
          n.leaf[0] = this.node("unit", null, u.idx);
        }
        return n;
      }
      case "string": {
        this.advance();
        return this.node("string", t.text);
      }
      case "identifier": {
        this.advance();
        if (this.peekKind() === "lparen") {
          this.advance();
          const call = this.node("func_call");
          const name = this.node("identifier", t.text);
          let args: TreeNode;
          if (this.peekKind() === "rparen") {
            args = this.node("null");
          } else {
            args = this.expr(0);
            if (this.peekKind() !== "rparen") throw new SyntaxFailure();
          }
          this.advance();
          call.leaf = [name, args];
          return call;
        }
        return this.node("identifier", t.text);
      }
      case "lparen": {
        this.advance();
        const inner = this.expr(0);
        if (this.peekKind() !== "rparen") throw new SyntaxFailure();
        this.advance();
        return inner;
      }
      default:
        throw new SyntaxFailure();
    }
  }
}

// -------------------------------------------------------------------------------------------------------------------------------- the compiler

export type CompilationStage = "parse" | "codegen";

export interface CompileOutcome {
  /** `COMPILER::Compile`: false for a text that does not lex or parse; true otherwise, whatever `errors` say. */
  ok: boolean;
  ucode: Ucode;
  /** What the error callback was told, in order (the messages; the C++ also gives the offset). */
  errors: Array<{ message: string; stage: CompilationStage; pos: number }>;
}

/** `formatNode`: the text a node carries. */
const textOf = (n: TreeNode | null): string => (n && n.text !== null ? n.text : "");

/** Parameters of a call, in the order the text has them (`squashParamList`). */
function squashParams(root: TreeNode | null): TreeNode[] {
  const args: TreeNode[] = [];
  if (!root) return args;
  if (root.op !== "arg_list" && root.op !== "null") {
    args.push(root);
  } else {
    let n: TreeNode | null = root;
    do {
      if (n.leaf[1]) args.push(n.leaf[1]);
      n = n.leaf[0];
    } while (n && n.op === "arg_list");
    if (n) args.push(n);
  }
  return args.reverse();
}

/**
 * `COMPILER::Compile` for `text`: the code for `host`, with `units` for the numbers, and `preflight` (a context with no items) for the calls to try once. `onError` is the
 * error callback (`SetErrorCallback`).
 */
export function compile(text: string, host: UcodeHost, units: UnitResolver, preflight: Context, onError?: (message: string, pos: number) => void): CompileOutcome {
  const ucode = new Ucode();
  const errors: CompileOutcome["errors"] = [];
  let pending = false;
  const report = (stage: CompilationStage, message: string, pos: number): void => {
    pending = true;
    errors.push({ message, stage, pos });
    onError?.(message, pos);
  };

  if (text === "") {
    ucode.ops.push({ kind: "push_value", value: Value.number(1) });
    return { ok: true, ucode, errors };
  }

  // Lex and parse. A lexical error is reported and then the parser is given the end of the text, which may be a syntax error of its own.
  const lexer = new Lexer(text, units, (m, p) => report("parse", m, p));
  const parser = new Parser(lexer, lexer.next());
  let root: TreeNode | null = null;
  try {
    root = parser.parseStatement();
  } catch (e) {
    if (!(e instanceof SyntaxFailure)) throw e;
    report("parse", "Syntax error", parser.lookahead.before);
  }
  if (pending) return { ok: false, ucode, errors };

  generate(root, ucode, host, units, preflight, report);
  return { ok: true, ucode, errors };
}

/** `COMPILER::generateUCode`. */
function generate(root: TreeNode | null, ucode: Ucode, host: UcodeHost, units: UnitResolver, preflight: Context, report: (stage: CompilationStage, message: string, pos: number) => void): void {
  // Empty expression returns true.
  if (!root) {
    ucode.ops.push({ kind: "push_value", value: Value.number(1) });
    return;
  }
  let numericValueCount = 0;
  let missingUnitsMessage = "";
  let missingUnitsPos = 0;
  const zero = (): Uop => ({ kind: "push_value", value: Value.number(0) });

  const emit = (node: TreeNode): void => {
    switch (node.op) {
      case "func_call":
        // A call with no item in front of it: "we don't know who to apply it to".
        report("codegen", "Unknown parent of function parameters", node.srcPos);
        ucode.ops.push(zero());
        return;
      case "struct_ref": {
        const item = node.leaf[0] as TreeNode;
        const member = node.leaf[1] as TreeNode;
        if (item.op !== "identifier") {
          report("codegen", "Unknown parent of property", item.srcPos - (item.text !== null ? item.text.length : 0));
          ucode.ops.push(zero());
          return;
        }
        if (member.op === "identifier") {
          const itemName = textOf(item);
          const propName = textOf(member);
          const vref = host.createVarRef(itemName, propName);
          if (!vref) report("codegen", `Unrecognized item '${itemName}'`, item.srcPos - itemName.length);
          else if (vref.getType() === "parse_error") report("codegen", `Unrecognized property '${propName}'`, member.srcPos - propName.length);
          ucode.ops.push({ kind: "push_var", ref: vref });
          return;
        }
        if (member.op === "func_call") {
          const itemName = textOf(item);
          const vref = host.createVarRef(itemName, "");
          if (!vref) report("codegen", `Unrecognized item '${itemName}'`, item.srcPos - itemName.length);
          const functionName = textOf(member.leaf[0]);
          const func = host.createFuncCall(functionName);
          const params = squashParams(member.leaf[1]);
          if (!func) {
            report("codegen", `Unrecognized function '${functionName}'`, item.srcPos + 1);
          } else {
            // Preflight the call: the arguments as strings, on a context with no items.
            for (const p of params) {
              const v = preflight.allocValue();
              v.setText(textOf(p));
              preflight.push(v);
            }
            const before = preflight.errorCallback;
            const argsPos = member.leaf[1] ? member.leaf[1].srcPos : 0;
            preflight.errorCallback = (m) => report("codegen", m, argsPos - 1);
            try {
              func(preflight, vref);
              preflight.pop();
            } catch {
              // a call that fails its preflight is told nothing about
            }
            preflight.errorCallback = before;
          }
          // The arguments are taken off the work stack last first, so they are pushed that way round.
          for (let i = params.length - 1; i >= 0; i--) emit(params[i] as TreeNode);
          ucode.ops.push({ kind: "method_call", func, ref: vref });
          return;
        }
        // The member is neither a name nor a call: malformed.
        const itemName = textOf(item);
        const vref = host.createVarRef(itemName, textOf(member));
        if (!vref) report("codegen", `Unrecognized item '${itemName}'`, item.srcPos - itemName.length);
        report("codegen", `Unrecognized property '${textOf(member)}'`, item.srcPos + 1);
        ucode.ops.push(zero());
        return;
      }
      case "number": {
        const unit = node.leaf[0];
        let value: number;
        let unitsType: Units = "unscaled";
        if (node.text === null) {
          value = 0;
        } else if (unit && unit.op === "unit") {
          if (units.names.length === 0) report("codegen", `Unexpected units for '${node.text}'`, node.srcPos);
          unitsType = units.types[unit.idx] ?? "unscaled";
          value = units.convert(node.text, unit.idx);
        } else {
          if (units.message !== "") {
            missingUnitsMessage = `Missing units for '${node.text}'| (${units.message})`;
            missingUnitsPos = node.srcPos;
          }
          value = doubleValueFromString(node.text);
        }
        ucode.ops.push({ kind: "push_value", value: Value.number(value, unitsType) });
        numericValueCount++;
        return;
      }
      case "string": {
        const s = textOf(node);
        ucode.ops.push({ kind: "push_value", value: Value.string(s, s.includes("?") || s.includes("*")) });
        return;
      }
      case "identifier": {
        const name = textOf(node);
        const vref = host.createVarRef(name, "");
        if (!vref) report("codegen", `Unrecognized item '${name}'`, node.srcPos - name.length);
        ucode.ops.push({ kind: "push_var", ref: vref });
        return;
      }
      default:
        if (node.leaf[0]) emit(node.leaf[0]);
        if (node.leaf[1]) emit(node.leaf[1]);
        ucode.ops.push({ kind: "op", op: node.op });
    }
  };

  emit(root);

  // The common mistake of a single number with no unit (it is a number of nanometres, which rarely leads to anything useful).
  if (missingUnitsMessage !== "" && numericValueCount === 1) report("codegen", missingUnitsMessage, missingUnitsPos);
}

/** `EDA_UNIT_UTILS::UI::DoubleValueFromString( text )`: the number the text starts with (0 when there is none). */
export function doubleValueFromString(text: string): number {
  const m = /^[0-9.,+-]*/.exec(text.trim());
  const part = (m ? m[0] : "").replace(/,/g, ".");
  const n = Number.parseFloat(part);
  return Number.isFinite(n) ? n : 0;
}

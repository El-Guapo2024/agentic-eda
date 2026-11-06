#!/usr/bin/env node
// Extracts KiCad's simulation model tables -- the device and type registry and every built-in model's parameters -- from KiCad's own source, for
// the Simulation Model dialog (web/studio/src/kicad-port/sim/simModelBuiltin.ts):
//
//   KICAD_SRC_DIR=~/src/kicad-src-8303b2ad node tools/extract-sim-models.js
//
// What it reads, and how (nothing is retyped: the C++ is tokenized and the functions that build the tables are run by a small interpreter):
// - eeschema/sim/sim_model.h: the `DEFINE_ENUM_CLASS_WITH_ITERATOR( DEVICE_T, ... )` and `( TYPE, ... )` orders (`DEVICE_T_ITERATOR`,
//   `TYPE_ITERATOR`, which the dialog's choices follow), and `SIM_MODEL::PARAM::INFO`'s constructor (its arguments' order and defaults, and the
//   member each one sets: `legacy_unused_value` sets none).
// - eeschema/sim/sim_model.cpp: `SIM_MODEL::DeviceInfo`, `TypeInfo` and `SpiceInfo` (their `case X: return { ... };` rows, the missing members
//   taking `SPICE_INFO`'s defaults) and `SIM_MODEL::Create( TYPE )` (which class each type is).
// - the model classes' constructors and `make*ParamInfo*` functions in sim_model_ideal.cpp, sim_model_behavioral.cpp, sim_model_source.cpp,
//   sim_model_switch.cpp, sim_model_tline.cpp, sim_model_r_pot.cpp, sim_model_l_mutual.cpp, sim_model_raw_spice.cpp, sim_model_ibis.cpp and
//   sim_model_xspice.cpp, run statement by statement: one `PARAM::INFO paramInfo` mutated field by field and copied at each `push_back` (so a
//   field a block does not set keeps the value of the block before, as in C++), the `static` tables, `switch`, `if( !strcmp( .. ) )`, range-for
//   and `AddParam`.
// - eeschema/sim/sim_model_ngspice_data_*.cpp: each `modelInfos[MODEL_TYPE::X] = { name, variant1, variant2, { pins }, description, {}, {} }`
//   and every `modelParams` / `instanceParams` `emplace_back( ... )`, read as `PARAM::INFO`'s constructor reads them (positional, so a row with
//   fewer arguments puts its text where the constructor's next argument is, as C++ does); and sim_model_ngspice.cpp's `getModelType` (the TYPE ->
//   MODEL_TYPE map). Which instance parameters an ngspice model shows is code (`SIM_MODEL_NGSPICE::SIM_MODEL_NGSPICE`), ported by hand in
//   simModelBuiltin.ts.
//
// It writes src/kicad/sim_models.json. `--check` prints nothing and exits 1 if the file would change (for the unit test,
// tools/lib/simModels.test.js).

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readFileSync, writeFileSync } from "node:fs";
import { readKicadFile, resolveCommit } from "./lib/kicadSource.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT_JSON = join(__dirname, "..", "src", "kicad", "sim_models.json");

const SIM = "eeschema/sim/";
const MODEL_FILES = [
  "sim_model_ideal.cpp",
  "sim_model_behavioral.cpp",
  "sim_model_source.cpp",
  "sim_model_switch.cpp",
  "sim_model_tline.cpp",
  "sim_model_r_pot.cpp",
  "sim_model_l_mutual.cpp",
  "sim_model_raw_spice.cpp",
  "sim_model_ibis.cpp",
  "sim_model_xspice.cpp",
  "sim_model_subckt.cpp",
];
const HEADER_FILES = ["sim_model.h", "sim_model_raw_spice.h"];
const NGSPICE_DATA = ["bjt", "bsim1", "bsim2", "bsim3", "bsim4", "b3soi", "b4soi", "diode", "hfet", "hicum2", "hsim", "jfet", "mes", "mos", "mos6", "mos9", "vbic"].map(
  (n) => `sim_model_ngspice_data_${n}.cpp`
);

// ---------------------------------------------------------------------------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------------------------------------------------------------------------

const PUNCT = ["::", "->", "==", "!=", "&&", "||", "<=", ">=", "+=", "++", "--", "(", ")", "{", "}", "[", "]", ";", ",", "=", ".", "+", "-", "*", "/", "!", "&", "<", ">", ":", "?", "|", "%", "^", "~"];

/** C++ source -> tokens: `{ t: "id" | "str" | "num" | "chr" | "p", v }`. Comments and preprocessor lines go; adjacent string literals are one. */
export function tokenize(src) {
  const out = [];
  let i = 0;
  let lineStart = true;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    if (c === "\n") {
      lineStart = true;
      i++;
      continue;
    }
    if (c === " " || c === "\t" || c === "\r" || c === "\f" || c === "\v") {
      i++;
      continue;
    }
    if (lineStart && c === "#") {
      // a preprocessor line, with its `\` continuations
      while (i < n && src[i] !== "\n") i += src[i] === "\\" && src[i + 1] === "\n" ? 2 : 1;
      continue;
    }
    lineStart = false;
    if (c === "/" && src[i + 1] === "/") {
      while (i < n && src[i] !== "\n") i++;
      continue;
    }
    if (c === "/" && src[i + 1] === "*") {
      const end = src.indexOf("*/", i + 2);
      i = end < 0 ? n : end + 2;
      continue;
    }
    if (c === '"') {
      let s = "";
      i++;
      while (i < n && src[i] !== '"') {
        if (src[i] === "\\") {
          const e = src[i + 1];
          s += e === "n" ? "\n" : e === "t" ? "\t" : e === "0" ? "\0" : e;
          i += 2;
        } else s += src[i++];
      }
      i++;
      const last = out[out.length - 1];
      if (last && last.t === "str") last.v += s;
      else out.push({ t: "str", v: s });
      continue;
    }
    if (c === "'") {
      let s = "";
      i++;
      while (i < n && src[i] !== "'") {
        if (src[i] === "\\") {
          s += src[i + 1];
          i += 2;
        } else s += src[i++];
      }
      i++;
      out.push({ t: "chr", v: s });
      continue;
    }
    if (/[A-Za-z_]/.test(c)) {
      let j = i + 1;
      while (j < n && /[A-Za-z0-9_]/.test(src[j])) j++;
      out.push({ t: "id", v: src.slice(i, j) });
      i = j;
      continue;
    }
    if (/[0-9]/.test(c) || (c === "." && /[0-9]/.test(src[i + 1] ?? ""))) {
      let j = i + 1;
      // digits, a point, suffixes, and an exponent's sign
      while (j < n && (/[0-9A-Za-z_.]/.test(src[j]) || ((src[j] === "-" || src[j] === "+") && /[eE]/.test(src[j - 1]) && !/^0[xX]/.test(src.slice(i, j))))) j++;
      out.push({ t: "num", v: src.slice(i, j) });
      i = j;
      continue;
    }
    const p = PUNCT.find((q) => src.startsWith(q, i));
    if (!p) throw new Error(`unexpected character '${c}' at ${i}`);
    out.push({ t: "p", v: p });
    i += p.length;
  }
  return out;
}

const isP = (tok, v) => tok && tok.t === "p" && tok.v === v;
const isId = (tok, v) => tok && tok.t === "id" && (v === undefined || tok.v === v);

/** The index of the bracket that closes the one at `i`. */
function closing(toks, i) {
  const open = toks[i].v;
  const close = { "(": ")", "{": "}", "[": "]", "<": ">" }[open];
  let depth = 0;
  for (let j = i; j < toks.length; j++) {
    if (isP(toks[j], open)) depth++;
    else if (isP(toks[j], close)) {
      depth--;
      if (depth === 0) return j;
    }
  }
  throw new Error(`no closing '${close}'`);
}

/** Split `toks[a..b)` on the commas at bracket depth 0. */
function splitTop(toks, a, b) {
  const parts = [];
  let depth = 0;
  let start = a;
  for (let j = a; j < b; j++) {
    const t = toks[j];
    if (t.t === "p" && "({[".includes(t.v)) depth++;
    else if (t.t === "p" && ")}]".includes(t.v)) depth--;
    else if (depth === 0 && isP(t, ",")) {
      parts.push([start, j]);
      start = j + 1;
    }
  }
  if (start < b) parts.push([start, b]);
  return parts;
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------------------------------------------------------------------------

/** Every function defined at file scope (a namespace's too): name -> [{ params: [{ name, def }], body: [start, end) }] over `toks`. */
export function findFunctions(toks) {
  const fns = new Map();
  let i = 0;
  while (i < toks.length) {
    const t = toks[i];
    if (isId(t, "namespace")) {
      // `namespace [name] {`: its contents are at file scope for us
      let j = i + 1;
      while (j < toks.length && !isP(toks[j], "{") && !isP(toks[j], ";")) j++;
      i = j + 1;
      continue;
    }
    if (isId(t) && isP(toks[i + 1], "(")) {
      const close = closing(toks, i + 1);
      let j = close + 1;
      while (isId(toks[j], "const") || isId(toks[j], "override")) j++;
      if (isP(toks[j], ":")) {
        // a constructor's initializer list
        j++;
        while (j < toks.length && !isP(toks[j], "{")) j = isP(toks[j], "(") ? closing(toks, j) + 1 : j + 1;
      }
      if (isP(toks[j], "{")) {
        const end = closing(toks, j);
        const params = splitTop(toks, i + 2, close).map(([a, b]) => {
          const eq = toks.slice(a, b).findIndex((x) => isP(x, "="));
          const declEnd = eq < 0 ? b : a + eq;
          let name = null;
          for (let k = declEnd - 1; k >= a; k--) if (isId(toks[k])) {
            name = toks[k].v;
            break;
          }
          return { name, def: eq < 0 ? null : [a + eq + 1, b] };
        });
        if (!(params.length === 1 && params[0].name === "void")) {
          // `CLASS::name(`: kept under both names, as a call from a member of CLASS finds CLASS's first
          const cls = isP(toks[i - 1], "::") && isId(toks[i - 2]) ? toks[i - 2].v : null;
          const def = { name: cls ? `${cls}::${t.v}` : t.v, cls, params, body: [j + 1, end], toks };
          for (const key of cls ? [def.name, t.v] : [t.v]) {
            if (!fns.has(key)) fns.set(key, []);
            fns.get(key).push(def);
          }
        }
        i = end + 1;
        continue;
      }
    }
    i++;
  }
  return fns;
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// The interpreter
// ---------------------------------------------------------------------------------------------------------------------------------------------

/** An enum constant (`TYPE::V`, `SIM_VALUE::TYPE_FLOAT`), by its last name. */
class Enum {
  constructor(name) {
    this.name = name;
  }
}

const BREAK = Symbol("break");
class Return {
  constructor(value) {
    this.value = value;
  }
}

/** Functions a statement may call that build nothing (asserts, the IBIS pins, the raw SPICE model's code). */
const IGNORED_CALLS = new Set(["wxFAIL_MSG", "wxFAIL", "wxASSERT", "wxASSERT_MSG", "SwitchSingleEndedDiff", "SetParamValue"]);

export class Interpreter {
  /** `fns`: name -> definitions (several files' `findFunctions` merged); `enums`: enum name -> its constants; `infoCtor`: `PARAM::INFO`'s
   * constructor (`readInfoCtor`). */
  constructor(fns, enums, infoCtor) {
    this.fns = fns;
    this.enums = enums;
    this.infoCtor = infoCtor;
    this.statics = new Map();
  }

  defaultInfo() {
    return this.infoFromArgs([]);
  }

  /** `PARAM::INFO( args... )`: each argument to the member its constructor parameter initializes, the rest their defaults. */
  infoFromArgs(args) {
    const info = {};
    this.infoCtor.params.forEach((p, k) => {
      const v = k < args.length ? args[k] : p.def;
      if (p.member) info[p.member] = clone(v);
    });
    return info;
  }

  call(name, args, { arity, from } = {}) {
    const defs = (from && this.fns.get(`${from}::${name}`)) || this.fns.get(name);
    if (!defs) throw new Error(`no function ${name}`);
    const def = defs.find((d) => d.params.length === (arity ?? args.length)) ?? defs.find((d) => d.params.filter((p) => !p.def).length <= args.length && args.length <= d.params.length);
    if (!def) throw new Error(`no ${name} taking ${args.length} arguments`);
    const env = { vars: new Map(), def, params: [] };
    def.params.forEach((p, k) => env.vars.set(p.name, k < args.length ? args[k] : p.def ? this.evalRange(def.toks, p.def[0], p.def[1], env) : undefined));
    const r = this.block(def.toks, def.body[0], def.body[1], env);
    if (r instanceof Return) return r.value;
    return env.params; // a constructor: the parameters it added (`AddParam`)
  }

  /** Run the statements of `toks[a..b)`; a `Return` or `BREAK` stops them. */
  block(toks, a, b, env) {
    let i = a;
    while (i < b) {
      const { end, result } = this.statement(toks, i, b, env);
      if (result === BREAK || result instanceof Return) return result;
      i = end;
    }
    return undefined;
  }

  /** The end of the simple statement starting at `i` (the `;` at depth 0, past it). */
  stmtEnd(toks, i, b) {
    let j = i;
    while (j < b && !isP(toks[j], ";")) j = isP(toks[j], "(") || isP(toks[j], "{") || isP(toks[j], "[") ? closing(toks, j) + 1 : j + 1;
    return j + 1;
  }

  statement(toks, i, b, env) {
    const t = toks[i];
    if (isP(t, ";")) return { end: i + 1 };
    if (isP(t, "{")) {
      const end = closing(toks, i);
      return { end: end + 1, result: this.block(toks, i + 1, end, env) };
    }
    if (isId(t, "break")) return { end: this.stmtEnd(toks, i, b), result: BREAK };
    if (isId(t, "return")) {
      const end = this.stmtEnd(toks, i, b);
      return { end, result: new Return(end - 1 > i + 1 ? this.evalRange(toks, i + 1, end - 1, env) : undefined) };
    }
    if (isId(t, "if")) {
      const close = closing(toks, i + 1);
      const cond = this.evalRange(toks, i + 2, close, env);
      const thenEnd = this.statementEnd(toks, close + 1, b);
      let end = thenEnd;
      let elseAt = null;
      if (isId(toks[thenEnd], "else")) {
        elseAt = thenEnd + 1;
        end = this.statementEnd(toks, elseAt, b);
      }
      if (truthy(cond)) return { end, result: this.statement(toks, close + 1, b, env).result };
      if (elseAt !== null) return { end, result: this.statement(toks, elseAt, b, env).result };
      return { end };
    }
    if (isId(t, "switch")) {
      const close = closing(toks, i + 1);
      const value = this.evalRange(toks, i + 2, close, env);
      const bodyOpen = close + 1;
      const bodyClose = closing(toks, bodyOpen);
      return { end: bodyClose + 1, result: this.runSwitch(toks, bodyOpen + 1, bodyClose, value, env) };
    }
    if (isId(t, "for")) {
      const close = closing(toks, i + 1);
      const colon = toks.slice(i + 2, close).findIndex((x) => isP(x, ":"));
      if (colon < 0) throw new Error("only range-for is read");
      let name = null;
      for (let k = i + 2 + colon - 1; k > i + 1; k--) if (isId(toks[k])) {
        name = toks[k].v;
        break;
      }
      const list = this.evalRange(toks, i + 2 + colon + 1, close, env);
      const end = this.statementEnd(toks, close + 1, b);
      for (const item of list) {
        env.vars.set(name, item);
        const r = this.statement(toks, close + 1, b, env).result;
        if (r === BREAK) break;
        if (r instanceof Return) return { end, result: r };
      }
      return { end };
    }
    const end = this.stmtEnd(toks, i, b);
    this.simple(toks, i, end - 1, env);
    return { end };
  }

  /** The end of the statement at `i` without running it (for `if` / `for` bodies). */
  statementEnd(toks, i, b) {
    const t = toks[i];
    if (isP(t, "{")) return closing(toks, i) + 1;
    if (isId(t, "if")) {
      const close = closing(toks, i + 1);
      let end = this.statementEnd(toks, close + 1, b);
      if (isId(toks[end], "else")) end = this.statementEnd(toks, end + 1, b);
      return end;
    }
    if (isId(t, "for") || isId(t, "switch")) {
      const close = closing(toks, i + 1);
      return this.statementEnd(toks, close + 1, b);
    }
    return this.stmtEnd(toks, i, b);
  }

  runSwitch(toks, a, b, value, env) {
    // the case labels and where each one's statements start
    const labels = [];
    let i = a;
    while (i < b) {
      if (isId(toks[i], "case")) {
        let j = i + 1;
        while (!isP(toks[j], ":") || isP(toks[j - 1], ":")) j++;
        labels.push({ value: this.evalRange(toks, i + 1, j, env), at: j + 1 });
        i = j + 1;
        continue;
      }
      if (isId(toks[i], "default") && isP(toks[i + 1], ":")) {
        labels.push({ value: "default", at: i + 2 });
        i += 2;
        continue;
      }
      i = this.statementEnd(toks, i, b);
    }
    const match = labels.find((l) => l.value !== "default" && same(l.value, value)) ?? labels.find((l) => l.value === "default");
    if (!match) return undefined;
    // from the label, through the next labels (fallthrough), up to a `break` or `return`
    i = match.at;
    while (i < b) {
      if (isId(toks[i], "case")) {
        while (!isP(toks[i], ":") || isP(toks[i - 1], ":")) i++;
        i++;
        continue;
      }
      if (isId(toks[i], "default") && isP(toks[i + 1], ":")) {
        i += 2;
        continue;
      }
      const { end, result } = this.statement(toks, i, b, env);
      if (result === BREAK) return undefined;
      if (result instanceof Return) return result;
      i = end;
    }
    return undefined;
  }

  /** A declaration, an assignment or a call, `toks[a..b)` (no `;`). */
  simple(toks, a, b, env) {
    // a declaration: [static|const] Type [<...>] [*|&] name [= expr | ( args ) | ;]
    let i = a;
    let isStatic = false;
    while (isId(toks[i], "static") || isId(toks[i], "const")) {
      isStatic = isStatic || toks[i].v === "static";
      i++;
    }
    const typeStart = i;
    if (isId(toks[i]) && !["return"].includes(toks[i].v)) {
      let j = i;
      while (isId(toks[j]) && isP(toks[j + 1], "::")) j += 2;
      if (isId(toks[j])) {
        j++;
        if (isP(toks[j], "<")) j = closing(toks, j) + 1;
        while (isP(toks[j], "*") || isP(toks[j], "&")) j++;
        if (isId(toks[j]) && (j + 1 === b || isP(toks[j + 1], "=") || isP(toks[j + 1], "("))) {
          const type = toks.slice(typeStart, j).map((x) => x.v).join("");
          const name = toks[j].v;
          const key = `${env.def.name}:${name}`;
          if (isStatic && this.statics.has(key)) {
            env.vars.set(name, this.statics.get(key));
            return;
          }
          let value;
          if (j + 1 === b) value = this.defaultOf(type);
          else if (isP(toks[j + 1], "=")) {
            value = isP(toks[j + 2], "{") && closing(toks, j + 2) === b - 1 && /INFO$/.test(type) ? this.infoFromArgs(this.args(toks, j + 2, b - 1, env)) : this.evalRange(toks, j + 2, b, env);
          } else {
            const close = closing(toks, j + 1);
            const args = this.args(toks, j + 1, close, env);
            value = /INFO$/.test(type) ? this.infoFromArgs(args) : args[0];
          }
          if (isStatic) this.statics.set(key, value);
          env.vars.set(name, value);
          return;
        }
      }
    }
    // an assignment: lvalue = expr
    const eq = toks.slice(a, b).findIndex((x, k) => isP(x, "=") && !isP(toks[a + k - 1], "=") && !isP(toks[a + k + 1], "="));
    if (eq > 0) {
      const at = a + eq;
      const value = this.evalRange(toks, at + 1, b, env);
      if (at - a === 1 && isId(toks[a])) {
        env.vars.set(toks[a].v, value);
        return;
      }
      if (at - a === 3 && isId(toks[a]) && isP(toks[a + 1], ".") && isId(toks[a + 2])) {
        const obj = env.vars.get(toks[a].v);
        if (!obj || typeof obj !== "object") throw new Error(`${toks[a].v} is not an object`);
        obj[toks[a + 2].v] = clone(value);
        return;
      }
      throw new Error(`assignment not read: ${toks.slice(a, b).map((x) => x.v).join(" ")}`);
    }
    // a method call: name . push_back|emplace_back ( args )
    if (isId(toks[a]) && isP(toks[a + 1], ".") && isId(toks[a + 2]) && isP(toks[a + 3], "(")) {
      const list = env.vars.get(toks[a].v);
      const close = closing(toks, a + 3);
      const args = this.args(toks, a + 3, close, env);
      if (toks[a + 2].v === "push_back") list.push(clone(args[0]));
      else if (toks[a + 2].v === "emplace_back") list.push(this.infoFromArgs(args));
      else throw new Error(`method ${toks[a + 2].v} not read`);
      return;
    }
    // a call: AddParam( info ), a helper that appends, or one that does nothing here
    if (isId(toks[a]) && isP(toks[a + 1], "(")) {
      const name = toks[a].v;
      const close = closing(toks, a + 1);
      if (IGNORED_CALLS.has(name)) return;
      const args = this.args(toks, a + 1, close, env);
      if (name === "AddParam") {
        env.params.push(clone(args[0]));
        return;
      }
      this.call(name, args, { from: env.def.cls });
      return;
    }
    if (isId(toks[a]) && a + 1 === b && IGNORED_CALLS.has(toks[a].v)) return; // `wxFAIL;`
    throw new Error(`statement not read: ${toks.slice(a, b).map((x) => x.v).join(" ")}`);
  }

  defaultOf(type) {
    if (/\*$/.test(type)) return null;
    if (/vector/.test(type)) return [];
    if (/INFO$/.test(type)) return this.defaultInfo();
    if (/string/.test(type)) return "";
    return undefined;
  }

  args(toks, open, close, env) {
    return splitTop(toks, open + 1, close).map(([a, b]) => this.evalRange(toks, a, b, env));
  }

  /** Evaluate the expression `toks[a..b)`. */
  evalRange(toks, a, b, env) {
    const p = { toks, i: a, b, env };
    const v = this.condExpr(p);
    if (p.i !== b) throw new Error(`expression not read: ${toks.slice(a, b).map((x) => x.v).join(" ")}`);
    return v;
  }

  /** `c ? x : y`. */
  condExpr(p) {
    const c = this.orExpr(p);
    if (!(p.i < p.b && isP(p.toks[p.i], "?"))) return c;
    p.i++;
    const x = this.condExpr(p);
    if (!isP(p.toks[p.i], ":")) throw new Error("a '?' without its ':'");
    p.i++;
    const y = this.condExpr(p);
    return truthy(c) ? x : y;
  }

  orExpr(p) {
    let v = this.andExpr(p);
    while (isP(p.toks[p.i], "||") && p.i < p.b) {
      p.i++;
      const w = this.andExpr(p);
      v = truthy(v) || truthy(w);
    }
    return v;
  }

  andExpr(p) {
    let v = this.eqExpr(p);
    while (isP(p.toks[p.i], "&&") && p.i < p.b) {
      p.i++;
      const w = this.eqExpr(p);
      v = truthy(v) && truthy(w);
    }
    return v;
  }

  eqExpr(p) {
    let v = this.addExpr(p);
    while (p.i < p.b && (isP(p.toks[p.i], "==") || isP(p.toks[p.i], "!="))) {
      const op = p.toks[p.i++].v;
      const w = this.addExpr(p);
      v = op === "==" ? same(v, w) : !same(v, w);
    }
    return v;
  }

  addExpr(p) {
    let v = this.unary(p);
    while (p.i < p.b && isP(p.toks[p.i], "+")) {
      p.i++;
      const w = this.unary(p);
      v = typeof v === "number" && typeof w === "number" ? v + w : String(v) + String(w);
    }
    return v;
  }

  unary(p) {
    const t = p.toks[p.i];
    if (isP(t, "!")) {
      p.i++;
      return !truthy(this.unary(p));
    }
    if (isP(t, "&") || isP(t, "*")) {
      p.i++;
      return this.unary(p);
    }
    if (isP(t, "-")) {
      p.i++;
      return -this.unary(p);
    }
    return this.postfix(p);
  }

  postfix(p) {
    let v = this.primary(p);
    while (p.i < p.b && isP(p.toks[p.i], ".") && isId(p.toks[p.i + 1])) {
      const m = p.toks[p.i + 1].v;
      p.i += 2;
      if (isP(p.toks[p.i], "(")) {
        const close = closing(p.toks, p.i);
        if (m !== "c_str" || close !== p.i + 1) throw new Error(`method ${m} not read`);
        p.i = close + 1;
      } else v = v[m];
    }
    return v;
  }

  primary(p) {
    const { toks, env } = p;
    const t = toks[p.i];
    if (!t) throw new Error("expression ends early");
    if (t.t === "str" || t.t === "chr") {
      p.i++;
      return t.v;
    }
    if (t.t === "num") {
      p.i++;
      return Number(t.v.replace(/[uUlLfF]+$/, ""));
    }
    if (isP(t, "(")) {
      const close = closing(toks, p.i);
      const v = this.evalRange(toks, p.i + 1, close, env);
      p.i = close + 1;
      return v;
    }
    if (isP(t, "{")) {
      const close = closing(toks, p.i);
      const v = this.args(toks, p.i, close, env);
      p.i = close + 1;
      return v;
    }
    if (isId(t)) {
      // a qualified name
      const parts = [t.v];
      p.i++;
      while (isP(toks[p.i], "::") && isId(toks[p.i + 1])) {
        parts.push(toks[p.i + 1].v);
        p.i += 2;
      }
      const name = parts[parts.length - 1];
      if (isP(toks[p.i], "(")) {
        const close = closing(toks, p.i);
        const args = this.args(toks, p.i, close, env);
        p.i = close + 1;
        if (["_", "wxT", "wxS", "_HKI"].includes(name)) return args[0];
        if (name === "strcmp") return args[0] === args[1] ? 0 : 1;
        const iter = /^(\w+)_ITERATOR$/.exec(name);
        if (iter) {
          const values = this.enums.get(iter[1]);
          if (!values) throw new Error(`no enum ${iter[1]}`);
          return values.map((v) => new Enum(v));
        }
        return this.call(parts.length > 1 ? parts.join("::") : name, args, { from: env.def.cls });
      }
      if (parts.length === 1) {
        if (name === "true") return true;
        if (name === "false") return false;
        if (name === "nullptr") return null;
        if (env.vars.has(name)) return env.vars.get(name);
      }
      return new Enum(name);
    }
    throw new Error(`expression not read at '${t.v}'`);
  }
}

function truthy(v) {
  return !(v === false || v === 0 || v === null || v === undefined || v === "");
}

function same(a, b) {
  if (a instanceof Enum && b instanceof Enum) return a.name === b.name;
  return a === b;
}

function clone(v) {
  if (Array.isArray(v)) return v.map(clone);
  if (v instanceof Enum) return v;
  if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, clone(x)]));
  return v;
}

/** JSON of a value the interpreter made: an enum is its name. */
function plain(v) {
  if (Array.isArray(v)) return v.map(plain);
  if (v instanceof Enum) return v.name;
  if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, plain(x)]));
  return v;
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// The tables
// ---------------------------------------------------------------------------------------------------------------------------------------------

/** Every `DEFINE_ENUM_CLASS_WITH_ITERATOR( NAME, a, b, ... )` of `toks`: NAME -> [a, b, ...]. */
export function readEnums(toks) {
  const enums = new Map();
  for (let i = 0; i < toks.length; i++) {
    if (isId(toks[i], "DEFINE_ENUM_CLASS_WITH_ITERATOR") && isP(toks[i + 1], "(")) {
      const close = closing(toks, i + 1);
      const names = splitTop(toks, i + 2, close).map(([a]) => toks[a].v);
      enums.set(names[0], names.slice(1));
      i = close;
    }
  }
  return enums;
}

/** `struct INFO { INFO( params = defaults ) : member( param ), ... }` inside `struct PARAM`: each parameter's name, default and member. */
export function readInfoCtor(toks) {
  for (let i = 0; i < toks.length; i++) {
    if (isId(toks[i], "INFO") && isP(toks[i + 1], "(") && isId(toks[i - 1], "struct") === false && isP(toks[i - 1], "{")) {
      const close = closing(toks, i + 1);
      const interp = new Interpreter(new Map(), new Map(), { params: [] });
      const env = { vars: new Map(), def: { name: "INFO" }, params: [] };
      const params = splitTop(toks, i + 2, close).map(([a, b]) => {
        const eq = toks.slice(a, b).findIndex((x) => isP(x, "="));
        let name = null;
        for (let k = (eq < 0 ? b : a + eq) - 1; k >= a; k--) if (isId(toks[k])) {
          name = toks[k].v;
          break;
        }
        return { name, def: eq < 0 ? undefined : interp.evalRange(toks, a + eq + 1, b, env) };
      });
      if (!isP(toks[close + 1], ":")) throw new Error("PARAM::INFO's constructor has no initializer list");
      // member( aParam ) ...  {
      const members = new Map();
      let j = close + 2;
      while (!isP(toks[j], "{")) {
        if (isId(toks[j]) && isP(toks[j + 1], "(")) {
          const c = closing(toks, j + 1);
          const inner = toks.slice(j + 2, c).filter((x) => isId(x) && x.v !== "std" && x.v !== "move");
          if (inner.length) members.set(inner[inner.length - 1].v, toks[j].v);
          j = c + 1;
        } else j++;
      }
      return { params: params.map((p) => ({ ...p, member: members.get(p.name) ?? null })) };
    }
  }
  throw new Error("PARAM::INFO's constructor not found");
}

/** `switch( x ) { case A::B: return { ... }; ... }` of the function `name`: the case's constant -> the braced list's items (evaluated). */
function readReturnRows(interp, fns, name) {
  const def = fns.get(name)?.[0];
  if (!def) throw new Error(`no ${name}`);
  const { toks } = def;
  const rows = new Map();
  const env = { vars: new Map(), def, params: [] };
  for (let i = def.body[0]; i < def.body[1]; i++) {
    if (!isId(toks[i], "case")) continue;
    // `case A: case B: return { .. };`
    const labels = [];
    let j = i;
    while (isId(toks[j], "case")) {
      let k = j + 1;
      while (!isP(toks[k], ":") || isP(toks[k - 1], ":")) k++;
      labels.push(toks[k - 1].v);
      j = k + 1;
    }
    if (isId(toks[j], "return")) {
      const end = interp.stmtEnd(toks, j, def.body[1]);
      const v = end - 1 > j + 1 ? interp.evalRange(toks, j + 1, end - 1, env) : [];
      for (const l of labels) rows.set(l, plain(v));
      i = end - 1;
    } else i = j - 1;
  }
  return rows;
}

/** `SIM_MODEL::Create( TYPE aType )`: type -> the class `std::make_unique<CLASS>( .. )` builds, `default` -> the rest. */
function readCreate(fns) {
  const def = (fns.get("Create") ?? []).find((d) => d.params.length === 1 && d.params[0].name === "aType");
  if (!def) throw new Error("SIM_MODEL::Create( TYPE ) not found");
  const { toks } = def;
  const out = new Map();
  let pending = [];
  for (let i = def.body[0]; i < def.body[1]; i++) {
    if (isId(toks[i], "case")) {
      let k = i + 1;
      while (!isP(toks[k], ":") || isP(toks[k - 1], ":")) k++;
      pending.push(toks[k - 1].v);
      i = k;
    } else if (isId(toks[i], "default") && isP(toks[i + 1], ":")) {
      pending.push("default");
      i++;
    } else if (isId(toks[i], "make_unique") && isP(toks[i + 1], "<")) {
      const cls = toks[i + 2].v;
      for (const l of pending) out.set(l, cls);
      pending = [];
    }
  }
  return out;
}

/** `SIM_MODEL_NGSPICE::getModelType`: TYPE -> MODEL_TYPE. */
function readNgspiceModelTypes(fns) {
  const def = fns.get("getModelType")?.[0];
  const { toks } = def;
  const out = new Map();
  let pending = [];
  for (let i = def.body[0]; i < def.body[1]; i++) {
    if (isId(toks[i], "case")) {
      let k = i + 1;
      while (!isP(toks[k], ":") || isP(toks[k - 1], ":")) k++;
      pending.push(toks[k - 1].v);
      i = k;
    } else if (isId(toks[i], "return") && isId(toks[i + 1], "MODEL_TYPE")) {
      for (const l of pending) out.set(l, toks[i + 3].v);
      pending = [];
    } else if (isId(toks[i], "default")) pending = [];
  }
  return out;
}

/** The ngspice tables: MODEL_TYPE -> { name, variant1, variant2, pinNames, description, modelParams, instanceParams }. */
export function readNgspiceData(texts, interp) {
  const infos = new Map();
  const env = { vars: new Map(), def: { name: "ngspice" }, params: [] };
  for (const text of texts) {
    const toks = tokenize(text);
    for (let i = 0; i < toks.length; i++) {
      if (!(isId(toks[i], "modelInfos") && isP(toks[i + 1], "["))) continue;
      const close = closing(toks, i + 1);
      const type = toks[close - 1].v;
      if (isP(toks[close + 1], "=")) {
        // `modelInfos[MODEL_TYPE::X] = { name, variant1, variant2, { pins }, description, {}, {} };`
        const end = interp.stmtEnd(toks, close + 1, toks.length);
        const [name, variant1, variant2, pinNames, description] = interp.evalRange(toks, close + 2, end - 1, env);
        infos.set(type, { name, variant1, variant2, pinNames, description, modelParams: [], instanceParams: [] });
        i = end - 1;
      } else if (isP(toks[close + 1], ".") && isP(toks[close + 3], ".") && isId(toks[close + 4], "emplace_back")) {
        const list = toks[close + 2].v; // modelParams | instanceParams
        const open = close + 5;
        const end = closing(toks, open);
        infos.get(type)[list].push(plain(interp.infoFromArgs(interp.args(toks, open, end, env))));
        i = end;
      }
    }
  }
  return infos;
}

/** Compact a parameter: [name, id, dir, type, unit, category, defaultValue, description, isSpiceInstanceParam, isInstanceParam, spiceModelName,
 * spiceInstanceName, enumValues] with the enums by name (`DIR_INOUT`, `TYPE_FLOAT`, `PRINCIPAL`), the trailing ones that are `PARAM::INFO`'s own
 * defaults left out. */
export const PARAM_FIELDS = ["name", "id", "dir", "type", "unit", "category", "defaultValue", "description", "isSpiceInstanceParam", "isInstanceParam", "spiceModelName", "spiceInstanceName", "enumValues"];
/** `PARAM::INFO()`'s own values of the last five fields: a row leaves out the ones at its end that have them. */
const TRAILING_DEFAULTS = [false, false, "", "", []];
const row = (info) => {
  const r = PARAM_FIELDS.map((f) => info[f]);
  let n = r.length;
  while (n > PARAM_FIELDS.length - TRAILING_DEFAULTS.length && JSON.stringify(r[n - 1]) === JSON.stringify(TRAILING_DEFAULTS[n - 1 - (PARAM_FIELDS.length - TRAILING_DEFAULTS.length)])) n--;
  return r.slice(0, n);
};

/** Everything, from the source texts: `read(path)` gives a file of eeschema/sim/. */
export function build(read) {
  const headerToks = HEADER_FILES.map((f) => tokenize(read(f)));
  const enums = new Map();
  for (const toks of headerToks) for (const [k, v] of readEnums(toks)) enums.set(k, v);
  const infoCtor = readInfoCtor(headerToks[0]);

  const fns = new Map();
  for (const f of ["sim_model.cpp", "sim_model_ngspice.cpp", ...MODEL_FILES]) {
    for (const [name, defs] of findFunctions(tokenize(read(f)))) fns.set(name, [...(fns.get(name) ?? []), ...defs]);
  }
  const interp = new Interpreter(fns, enums, infoCtor);

  const deviceRows = readReturnRows(interp, fns, "DeviceInfo");
  const typeRows = readReturnRows(interp, fns, "TypeInfo");
  const spiceRows = readReturnRows(interp, fns, "SpiceInfo");
  const create = readCreate(fns);
  const ngspiceTypes = readNgspiceModelTypes(fns);

  const devices = enums.get("DEVICE_T").map((d) => {
    const [fieldValue = "", description = "", showInMenu = false] = deviceRows.get(d) ?? [];
    return { name: d, fieldValue, description, showInMenu };
  });
  const SPICE_INFO = ["itemType", "modelType", "functionName", "level", "isDefaultLevel", "hasExpression", "version"];
  const SPICE_DEFAULTS = ["", "", "", "", false, false, ""];
  const types = enums.get("TYPE").map((t) => {
    const [deviceType = "NONE", fieldValue = "", description = ""] = typeRows.get(t) ?? [];
    const spice = Object.fromEntries(SPICE_INFO.map((k, n) => [k, (spiceRows.get(t) ?? [])[n] ?? SPICE_DEFAULTS[n]]));
    const cls = create.get(t) ?? create.get("default");
    const out = { name: t, deviceType, fieldValue, description, spice, class: cls };
    if (cls === "SIM_MODEL_NGSPICE") out.ngspiceModel = ngspiceTypes.get(t) ?? "NONE";
    else {
      // the class's constructor, run for this type: the parameters its `AddParam`s add
      const ctor = fns.get(`${cls}::${cls}`);
      if (!ctor) throw new Error(`no constructor ${cls}`);
      const def = ctor.find((d) => d.params.length <= 1);
      const args = def.params.length === 1 && def.params[0].name === "aType" ? [new Enum(t)] : [];
      out.params = interp.call(`${cls}::${cls}`, args, { arity: def.params.length }).map((i) => row(plain(i)));
    }
    return out;
  });

  const ngspice = {};
  for (const [k, v] of readNgspiceData(NGSPICE_DATA.map((f) => read(f)), interp)) {
    ngspice[k] = { ...v, modelParams: v.modelParams.map(row), instanceParams: v.instanceParams.map(row) };
  }
  return { paramFields: PARAM_FIELDS, devices, types, ngspice, ngspiceModelTypes: enumsOrder(enums, "MODEL_TYPE") };
}

function enumsOrder(enums, name) {
  return enums.get(name) ?? [];
}

/** The JSON file: the tables with one line per parameter, so a diff reads. */
export function jsonText(commit, built) {
  const lines = [];
  lines.push("{");
  lines.push(`  "commit": ${JSON.stringify(commit)},`);
  lines.push(`  "source": ${JSON.stringify(["eeschema/sim/sim_model.h", "eeschema/sim/sim_model.cpp", ...MODEL_FILES.map((f) => SIM + f), ...NGSPICE_DATA.map((f) => SIM + f)])},`);
  lines.push(`  "paramFields": ${JSON.stringify(built.paramFields)},`);
  lines.push(`  "devices": [`);
  lines.push(built.devices.map((d) => `    ${JSON.stringify(d)}`).join(",\n"));
  lines.push("  ],");
  lines.push(`  "types": [`);
  lines.push(
    built.types
      .map((t) => {
        const { params, ...rest } = t;
        if (!params) return `    ${JSON.stringify(rest)}`;
        const head = JSON.stringify(rest).slice(0, -1);
        return `    ${head}, "params": [${params.length ? "\n" + params.map((p) => `      ${JSON.stringify(p)}`).join(",\n") + "\n    " : ""}]}`;
      })
      .join(",\n")
  );
  lines.push("  ],");
  lines.push(`  "ngspice": {`);
  lines.push(
    Object.entries(built.ngspice)
      .map(([k, v]) => {
        const list = (ps) => (ps.length ? "\n" + ps.map((p) => `        ${JSON.stringify(p)}`).join(",\n") + "\n      " : "");
        return `    ${JSON.stringify(k)}: {\n      "name": ${JSON.stringify(v.name)}, "variant1": ${JSON.stringify(v.variant1)}, "variant2": ${JSON.stringify(v.variant2)}, "pinNames": ${JSON.stringify(v.pinNames)}, "description": ${JSON.stringify(v.description)},\n      "modelParams": [${list(v.modelParams)}],\n      "instanceParams": [${list(v.instanceParams)}]\n    }`;
      })
      .join(",\n")
  );
  lines.push("  }");
  lines.push("}");
  return lines.join("\n") + "\n";
}

function main() {
  const commit = resolveCommit();
  const built = build((f) => readKicadFile(SIM + f));
  const text = jsonText(commit.sha, built);
  if (process.argv.includes("--check")) {
    let old = "";
    try {
      old = readFileSync(OUT_JSON, "utf8");
    } catch {
      // missing
    }
    process.exit(old === text ? 0 : 1);
  }
  writeFileSync(OUT_JSON, text);
  const nParams = Object.values(built.ngspice).reduce((n, m) => n + m.modelParams.length + m.instanceParams.length, 0);
  console.log(`${built.devices.length} devices, ${built.types.length} types, ${Object.keys(built.ngspice).length} ngspice models (${nParams} parameters) -> ${OUT_JSON}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();

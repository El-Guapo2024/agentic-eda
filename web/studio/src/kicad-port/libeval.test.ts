import { test } from "node:test";
import assert from "node:assert/strict";
import { compile, Context, doubleValueFromString, Value, wildCompare, type FuncCall, type UcodeHost, type UnitResolver, type VarRef } from "./libeval";

// A host with the variables a test names: `A.x` is whatever the test puts in `vars`, `f(a)` returns its argument's length, `g(a, b)` says which came off the stack first.
interface Fixture {
  vars: Record<string, Value>;
}

function host(fx: Fixture): UcodeHost {
  const funcs: Record<string, FuncCall> = {
    f: (ctx) => {
      const arg = ctx.pop();
      const r = ctx.allocValue();
      r.set(arg.asString().length);
      ctx.push(r);
    },
    g: (ctx) => {
      const first = ctx.pop();
      const second = ctx.pop();
      const r = ctx.allocValue();
      r.setText(`${first.asString()},${second.asString()}`);
      ctx.push(r);
    },
  };
  return {
    createVarRef(variable: string, field: string): VarRef | null {
      if (variable !== "A" && variable !== "B") return null;
      const key = `${variable}.${field}`;
      return {
        getType: () => (field === "" || key in fx.vars ? "numeric" : "parse_error"),
        getValue: () => fx.vars[key] ?? Value.undefinedValue(),
      };
    },
    createFuncCall: (name) => funcs[name] ?? null,
  };
}

const UNITS: UnitResolver = {
  names: ["mil", "mm", "in", "deg", "fs", "ps"],
  types: ["mil", "mm", "in", "deg", "fs", "ps"],
  message: "must be mm, in, mil, deg, fs, or ps",
  convert: (text, id) => doubleValueFromString(text) * [25_400, 1_000_000, 25_400_000, 1, 1_000, 1_000_000][id]!,
};

const NO_UNITS: UnitResolver = { names: [], types: [], message: "", convert: (text) => doubleValueFromString(text) };

function run(text: string, vars: Record<string, Value> = {}, units: UnitResolver = UNITS): { ok: boolean; value: number; errors: string[]; strings: string } {
  const out = compile(text, host({ vars }), units, new Context());
  const ctx = new Context();
  const v = out.ucode.run(ctx);
  return { ok: out.ok, value: v.asDouble(), errors: out.errors.map((e) => e.message), strings: v.asString() };
}

const truthy = (text: string, vars: Record<string, Value> = {}, units: UnitResolver = UNITS): boolean => run(text, vars, units).value !== 0;

// ----------------------------------------------------------------------------------------------------------------------------------------- lexing

test("numbers, the units of PCBEXPR and the decimal comma", () => {
  assert.equal(run("1mm == 1000000").value, 1, "a length is in nanometres");
  assert.equal(run("1 mm == 1000000").value, 1, "a unit may follow a blank");
  assert.equal(run("1mil == 25400").value, 1);
  assert.equal(run("1in == 25400000").value, 1);
  assert.equal(run("90deg == 90").value, 1);
  assert.equal(run("2ps == 2000000").value, 1);
  assert.equal(run("3fs == 3000").value, 1);
  assert.equal(run("1,5mm == 1500000").value, 1, "a comma is a decimal separator inside a number");
  assert.equal(run("0.5mm == 500000").value, 1);
  assert.equal(run("1MM == 1").ok, false, "a unit is case sensitive: MM is no unit, and no identifier either");
});

test("only the space is white space; an unrecognized character names itself", () => {
  const tab = run("1\t== 1");
  assert.equal(tab.ok, false);
  assert.deepEqual(tab.errors[0], "Unrecognized character '\t'");
  assert.deepEqual(run("1 === 1").errors[0], "Unrecognized character '='");
  assert.deepEqual(run("1 & 1").errors[0], "Unrecognized character '&'");
  assert.deepEqual(run("1 == ${X}").errors[0], "Unresolved text variable reference");
  // the parser is then given the end of the text: an incomplete expression is a second error
  assert.deepEqual(run("A.x == %").errors, ["Unrecognized character '%'", "Syntax error"]);
  assert.deepEqual(run("A.x == 1 %").errors, ["Unrecognized character '%'"]);
});

test("strings: escaped quotes, a missing closing quote takes the rest", () => {
  assert.equal(run("'it\\'s'").strings, "it's");
  assert.equal(run("'abc").strings, "abc");
  assert.equal(run("'a' == 'A'").value, 1, "a comparison of strings ignores case");
});

test("an empty text, or one of blanks, is true", () => {
  assert.deepEqual([run("").ok, run("").value], [true, 1]);
  assert.deepEqual([run("   ").ok, run("   ").value], [true, 1]);
});

// ---------------------------------------------------------------------------------------------------------------------------------------- parsing

test("|| binds tighter than &&, as the order of the %left lines in grammar.lemon makes it", () => {
  // C would say (0 && 0) || 1 = 1; here it is 0 && (0 || 1) = 0
  assert.equal(run("0 && 0 || 1").value, 0);
  assert.equal(run("1 || 0 && 0").value, 0, "(1 || 0) && 0");
  assert.equal(run("1 && 1 || 0").value, 1);
  assert.equal(run("0 || 1 && 1").value, 1);
});

test("comparisons and . do not chain", () => {
  assert.equal(run("1 < 2 < 3").ok, false);
  assert.deepEqual(run("1 < 2 < 3").errors, ["Syntax error"]);
  assert.equal(run("1 == 1 != 0").ok, false);
  assert.equal(run("A.x.y", { "A.x": Value.number(1) }).ok, false);
  assert.equal(run("1 < 2 && 2 < 3").ok, true);
  assert.equal(run("(1 < 2) < 3").ok, true);
});

test("! binds looser than + and tighter than ==; unary minus is 0 - x", () => {
  assert.equal(run("!1 == 0").value, 1, "(!1) == 0");
  assert.equal(run("!0 + 1").value, 0, "!(0 + 1)");
  assert.equal(run("-2 + 5 == 3").value, 1);
  assert.equal(run("-2 * 3 == -6").value, 1);
  assert.equal(run("+2 == 2").value, 1);
  assert.equal(run("2 - -3 == 5").value, 1);
  assert.equal(run("10 / 4 == 2.5").value, 1);
  assert.equal(run("2 + 3 * 4 == 14").value, 1);
  assert.equal(run("(2 + 3) * 4 == 20").value, 1);
});

test("syntax errors are told once, with the text of the grammar's own message", () => {
  for (const text of ["(", ")", "1 +", "== 1", "1 1", "f(", "A.x ==", "1 mm mm", "mm"]) {
    const r = run(text);
    assert.equal(r.ok, false, text);
    assert.deepEqual(r.errors, ["Syntax error"], text);
  }
  assert.equal(run("()").ok, false);
  assert.equal(run("f()").ok, true, "a call may have no argument");
});

// ----------------------------------------------------------------------------------------------------------------------------------- code generation

test("an unknown item, property or function is told, and the text still compiles", () => {
  const c = compile("X.y == 1 && A.nope == 2 && A.f2('a') && f(1)", host({ vars: {} }), UNITS, new Context());
  assert.equal(c.ok, true, "COMPILER::Compile answers true when the text parses");
  assert.deepEqual(
    c.errors.map((e) => e.message),
    ["Unrecognized item 'X'", "Unrecognized property 'nope'", "Unrecognized function 'f2'", "Unknown parent of function parameters"],
  );
});

test("a single number with no unit is told, with the message the DRC splits at the bar", () => {
  const c = compile("A.x > 5", host({ vars: { "A.x": Value.number(1) } }), UNITS, new Context());
  assert.deepEqual(c.errors.map((e) => e.message), ["Missing units for '5'| (must be mm, in, mil, deg, fs, or ps)"]);
  // two numbers, or a resolver that asks for no units, say nothing
  const known = { vars: { "A.x": Value.number(1) } };
  assert.deepEqual(compile("A.x > 5 && A.x < 9", host(known), UNITS, new Context()).errors, []);
  assert.deepEqual(compile("A.x > 5", host(known), NO_UNITS, new Context()).errors, []);
  assert.equal(compile("5mm", host({ vars: {} }), NO_UNITS, new Context()).ok, false, "with no units a unit is just an identifier after a number");
});

test("a call is made with its arguments, last pushed first", () => {
  assert.equal(run("A.f('abcd')", {}).ok, true);
  const c = compile("A.g('first', 'second')", host({ vars: {} }), UNITS, new Context());
  const v = c.ucode.run(new Context());
  // the first argument of the text is on top of the stack when the function pops first
  assert.equal(v.asString(), "first,second");
});

// ------------------------------------------------------------------------------------------------------------------------------------- running

test("values: strings compare without case, a pattern is the right side only, undefined is neither equal nor different", () => {
  const vars = { "A.s": Value.string("Pad"), "A.n": Value.number(3), "B.u": Value.undefinedValue() };
  assert.equal(truthy("A.s == 'pad'", vars), true);
  assert.equal(truthy("A.s == 'P*'", vars), true);
  assert.equal(truthy("'P*' == A.s", vars), false, "the wildcard is on the left: an exact comparison");
  assert.equal(truthy("A.s != 'P*'", vars), false);
  assert.equal(truthy("A.s == 3", vars), false, "a string is never a number");
  assert.equal(truthy("A.s != 3", vars), true);
  assert.equal(truthy("B.u == 1", vars), false);
  assert.equal(truthy("B.u != 1", vars), false, "NotEqualTo is not the inverse of EqualTo: both say no for an undefined value");
  assert.equal(truthy("1 == B.u", vars), false);
  assert.equal(truthy("A.n == 3 && A.n >= 3 && A.n <= 3 && A.n > 2 && A.n < 4", vars), true);
  assert.equal(truthy("null == null", { }), false, "null is no item here: an undefined value");
});

test("a run that leaves the stack other than with one value is false", () => {
  assert.equal(run("1, 2").value, 0);
  assert.equal(run("A.f(1, 2)").value, 0, "a call is not given what it did not ask for");
});

test("a result carries the unit of the numbers that made it", () => {
  const out = compile("2mm * 3", host({ vars: {} }), UNITS, new Context());
  const v = out.ucode.run(new Context());
  assert.equal(v.asDouble(), 6_000_000);
  assert.equal(v.units, "mm");
  const mixed = compile("2 * 3", host({ vars: {} }), UNITS, new Context()).ucode.run(new Context());
  assert.equal(mixed.units, "unscaled");
});

test("type mismatches are told to a run that listens", () => {
  const out = compile("A.s == 3", host({ vars: { "A.s": Value.string("x") } }), UNITS, new Context());
  const told: string[] = [];
  const ctx = new Context();
  ctx.errorCallback = (m) => told.push(m);
  out.ucode.run(ctx);
  assert.deepEqual(told, ["Type mismatch between 'x' and 3.000000"]);
});

// ------------------------------------------------------------------------------------------------------------------------------------ helpers

test("WildCompareString: * any run, ? any one character, case ignored unless asked", () => {
  assert.equal(wildCompare("U*", "u12"), true);
  assert.equal(wildCompare("U*", "u12", true), false);
  assert.equal(wildCompare("R?", "R12"), false);
  assert.equal(wildCompare("R??", "R12"), true);
  assert.equal(wildCompare("*GND*", "AGND_1"), true);
  assert.equal(wildCompare("*", ""), true);
  assert.equal(wildCompare("", ""), true);
  assert.equal(wildCompare("a*b*c", "aXXbYYc"), true);
  assert.equal(wildCompare("a*b*c", "aXXbYY"), false);
});

test("DoubleValueFromString takes the number a text starts with", () => {
  assert.equal(doubleValueFromString("12.5mm"), 12.5);
  assert.equal(doubleValueFromString("1,5"), 1.5);
  assert.equal(doubleValueFromString("abc"), 0);
  assert.equal(doubleValueFromString("-3"), -3);
});

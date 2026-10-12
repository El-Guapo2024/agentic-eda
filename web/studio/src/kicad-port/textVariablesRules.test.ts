import { test } from "node:test";
import assert from "node:assert/strict";
import {
  BUILTIN_PROJECT_VARIABLES,
  MAX_TEXT_VARIABLES,
  MAX_TEXT_VARIABLE_NAME,
  checkTextVariables,
  sameTextVariables,
  textVariableNameProblem,
  textVariableRows,
  textVariablesToSend,
} from "./textVariablesRules";

test("the grid lists the project's variables in the order of a std::map", () => {
  assert.deepEqual(textVariableRows({ b: "2", A: "1", a: "3" }), [
    { name: "A", value: "1" },
    { name: "a", value: "3" },
    { name: "b", value: "2" },
  ]);
  assert.deepEqual(textVariableRows(undefined), []);
});

// The same cases as crates/model/src/rules.rs `text_variable_names_follow_the_panels_validator`: the page checks before Apply what the backend refuses after.
test("a name follows the panel's validator: not empty, none of the characters KiCad filters out", () => {
  for (const ok of ["REV", "Designer name", "PART_NO", "ü", "a-b", "1st"]) assert.equal(textVariableNameProblem(ok), null, ok);
  assert.equal(textVariableNameProblem(""), "Variable name cannot be empty.");
  for (const bad of ["A.B", "A{B", "a$b", "net/name", "tab\\there", "x:y", "(1)", "a,b", "a;b", "q?", "a&b", "a|b", "a~b", "a%b", "a<b", "a=b", "a'b", 'a"b', "a`b", "a[b", "a]b", "a}b"]) {
    assert.match(textVariableNameProblem(bad) ?? "", /cannot hold/, bad);
  }
  assert.match(textVariableNameProblem("new\nline") ?? "", /control character/);
  assert.match(textVariableNameProblem("KIPRJMOD") ?? "", /reserved/);
  assert.match(textVariableNameProblem("x".repeat(MAX_TEXT_VARIABLE_NAME + 1)) ?? "", /at most 256 bytes/);
  assert.equal(textVariableNameProblem("x".repeat(MAX_TEXT_VARIABLE_NAME)), null);
});

test("Apply is stopped by an empty or bad name, and warned by a repeated or built-in one", () => {
  const rows = [
    { name: "REV", value: "A" },
    { name: "", value: "x" },
    { name: "REV", value: "B" },
    { name: "CURRENT_DATE", value: "never" },
    { name: "Bad.Name", value: "x" },
  ];
  const c = checkTextVariables(rows);
  assert.deepEqual(Object.keys(c.errors).map(Number), [1, 4]);
  assert.deepEqual(c.errors[1], { field: "name", message: "Variable name cannot be empty." });
  assert.match(c.notes[0] ?? "", /that one is kept/, "the first of two rows of one name is dropped, KiCad's `variables[ name ] = value`");
  assert.equal(c.notes[2], undefined, "the last row is the one kept");
  assert.match(c.notes[3] ?? "", /answers CURRENT_DATE itself/);
  assert.equal(c.page, null);
  assert.deepEqual(textVariablesToSend(rows.filter((r) => r.name && !r.name.includes("."))), { REV: "B", CURRENT_DATE: "never" });
  assert.ok(BUILTIN_PROJECT_VARIABLES.includes("PROJECTNAME") && BUILTIN_PROJECT_VARIABLES.includes("VCSHASH"));
});

test("a page with too many variables or a value that cannot be stored is refused", () => {
  const many = Array.from({ length: MAX_TEXT_VARIABLES + 1 }, (_, i) => ({ name: `V${i}`, value: "" }));
  assert.match(checkTextVariables(many).page ?? "", /at most 1000/);
  assert.match(checkTextVariables([{ name: "A", value: "x".repeat(16 * 1024 + 1) }]).errors[0]?.message ?? "", /at most 16384 bytes/);
  assert.equal(checkTextVariables([{ name: "A", value: "a\u0000b" }]).errors[0]?.field, "value");
  assert.deepEqual(checkTextVariables([]), { errors: {}, notes: {}, page: null });
});

test("rows that say what the project has are nothing to apply", () => {
  assert.equal(sameTextVariables(textVariableRows({ A: "1", B: "2" }), { B: "2", A: "1" }), true);
  assert.equal(sameTextVariables([{ name: "A", value: "1" }, { name: "B", value: "2" }], { A: "1" }), false);
  assert.equal(sameTextVariables([{ name: "A", value: "2" }], { A: "1" }), false);
  assert.equal(sameTextVariables([], undefined), true);
});

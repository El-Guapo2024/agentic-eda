#!/usr/bin/env node
// Extracts the words the custom rules editors offer after `A.` in a condition -- `PANEL_SETUP_RULES::onScintillaCharAdded` (and its copies in
// the DRC Rule Editor) list, there, the name of every property `PROPERTY_MANAGER` knows that is not `IsHiddenFromRulesEditor()`, spaces
// made underscores, sorted and without repeats, then every `PCBEXPR_BUILTIN_FUNCTIONS` signature that is not marked DEPRECATED:
//
//   KICAD_SRC_DIR=~/src/kicad-src-8303b2ad node tools/extract-rule-completions.js
//
// The properties: pcbnew links the property descriptors of pcbnew/ and common/ (the `static struct X_DESC { X_DESC() { ... } } _X_DESC;`
// constructors that run at start-up), so every .cpp under pcbnew/ and common/ is scanned for the registrations -- `new PROPERTY<...>( name, ... )`
// and `new PROPERTY_ENUM<...>( name, ... )` (include/properties/property.h), the name an `_HKI( "..." )`, `wxS( "..." )` or `wxT( "..." )` -- and
// whether the statement that adds it (`propMgr.AddProperty( ... )` or `ReplaceProperty`, or the variable it was kept in) goes on to
// `.SetIsHiddenFromRulesEditor()` (`m_hideFromRulesEditor`, false unless set). A property kept in a variable that is never added is not
// registered and is left out. `GetAllClasses()` / `GetProperties()` give a derived class its bases' properties too, but the editor only wants
// the names, so the union of the registered ones is the same list. A name hidden in one registration and not in another is offered (KiCad's
// loop skips the hidden property object, not the name).
//
// The functions: `PCBEXPR_BUILTIN_FUNCTIONS::RegisterAllFunctions` (pcbnew/pcbexpr_functions.cpp), each `RegisterFunc( wxT( "sig" ), ... )` in
// order (`GetSignatures()`), DEPRECATED ones kept -- the editor drops them.
//
// Writes src/kicad/rule_completions.json. `--check` prints nothing and exits 1 if the file would change (for the unit test).

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readFileSync, writeFileSync } from "node:fs";
import { readKicadFile, resolveCommit, listKicadDir } from "./lib/kicadSource.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "rule_completions.json");
const SCANNED = ["pcbnew", "common"];
const FUNCTIONS_FILE = "pcbnew/pcbexpr_functions.cpp";

/** C++ without its comments (`//` and block), string and character literals kept as they are. */
export function stripComments(src) {
  let out = "";
  for (let i = 0; i < src.length; ) {
    const c = src[i];
    if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < src.length && src[j] !== c) j += src[j] === "\\" ? 2 : 1;
      out += src.slice(i, j + 1);
      i = j + 1;
    } else if (c === "/" && src[i + 1] === "/") {
      while (i < src.length && src[i] !== "\n") i++;
    } else if (c === "/" && src[i + 1] === "*") {
      const end = src.indexOf("*/", i + 2);
      const block = src.slice(i, end < 0 ? src.length : end + 2);
      out += block.replace(/[^\n]/g, " ");
      i = end < 0 ? src.length : end + 2;
    } else {
      out += c;
      i++;
    }
  }
  return out;
}

/** Where the `(`..`)` or `<`..`>` that opens at `open` closes (strings skipped). */
function closing(src, open, a, b) {
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    const c = src[i];
    if (c === '"') {
      i++;
      while (i < src.length && src[i] !== '"') i += src[i] === "\\" ? 2 : 1;
    } else if (c === a) depth++;
    else if (c === b && --depth === 0) return i;
  }
  return -1;
}

/** Where the statement around `at` starts (after the `;`, `{` or `}` before it) and ends (its `;` outside brackets). */
function statementAround(src, at) {
  let start = at;
  while (start > 0 && !";{}".includes(src[start - 1])) start--;
  let depth = 0;
  for (let i = at; i < src.length; i++) {
    const c = src[i];
    if (c === '"') {
      i++;
      while (i < src.length && src[i] !== '"') i += src[i] === "\\" ? 2 : 1;
    } else if (c === "(" || c === "[" || c === "{") depth++;
    else if (c === ")" || c === "]" || c === "}") depth--;
    else if (c === ";" && depth <= 0) return { start, end: i + 1 };
  }
  return { start, end: src.length };
}

/** `.SetIsHiddenFromRulesEditor()` / `->SetIsHiddenFromRulesEditor( true )` in a piece of code (`( false )` does not hide). */
const hides = (code) => /SetIsHiddenFromRulesEditor\s*\(\s*(?:true\s*)?\)/.test(code);

/** Every property one file registers: `{ name, hidden, file }`. */
export function scanProperties(text, file) {
  const src = stripComments(text);
  const found = [];
  for (const m of src.matchAll(/\bnew\s+PROPERTY(?:_ENUM)?\s*</g)) {
    const lt = m.index + m[0].length - 1;
    const gt = closing(src, lt, "<", ">");
    const paren = src.indexOf("(", gt);
    if (gt < 0 || paren < 0 || src.slice(gt + 1, paren).trim() !== "") continue;
    const args = src.slice(paren + 1, closing(src, paren, "(", ")"));
    const nm = /^\s*(?:_HKI|wxS|wxT|_)?\s*\(?\s*"((?:[^"\\]|\\.)*)"/.exec(args);
    if (!nm) throw new Error(`${file}: a property whose name is not a string literal: ${args.slice(0, 80)}`);
    const name = nm[1];
    const st = statementAround(src, m.index);
    const statement = src.slice(st.start, st.end);
    const variable = /^\s*(?:auto|PROPERTY\w*\s*<[^;]*>)\s*\*?\s*(\w+)\s*=\s*new\b/.exec(statement)?.[1] ?? null;
    let hidden = hides(statement);
    if (variable) {
      // The property lives in a variable: it is registered where the variable is added, and hidden wherever it (or that statement) says so.
      const rest = src.slice(st.end);
      const uses = [...rest.matchAll(new RegExp(`[^;{}]*\\b${variable}\\b[^;]*;`, "g"))].map((u) => u[0]);
      const added = uses.some((u) => /\b(?:AddProperty|ReplaceProperty)\s*\(/.test(u));
      if (!added) continue;
      hidden = hidden || uses.some((u) => hides(u) && (new RegExp(`\\b${variable}\\s*->\\s*SetIsHiddenFromRulesEditor`).test(u) || /\b(?:AddProperty|ReplaceProperty)\s*\(/.test(u)));
    } else if (!/\b(?:AddProperty|ReplaceProperty)\s*\(/.test(statement)) {
      continue;
    }
    found.push({ name, hidden, file });
  }
  return found;
}

/** `RegisterFunc( wxT( "sig" ), ... )` in `RegisterAllFunctions`, in order. */
export function scanFunctions(text) {
  const src = stripComments(text);
  const at = src.indexOf("PCBEXPR_BUILTIN_FUNCTIONS::RegisterAllFunctions()");
  if (at < 0) throw new Error("RegisterAllFunctions not found");
  const open = src.indexOf("{", at);
  const body = src.slice(open, closing(src, open, "{", "}"));
  return [...body.matchAll(/RegisterFunc\(\s*(?:wxT|wxS)?\s*\(?\s*"((?:[^"\\]|\\.)*)"/g)].map((m) => m[1]);
}

/** The .cpp files under a directory of KiCad's source, recursively. */
function cppFiles(dir) {
  const out = [];
  for (const name of listKicadDir(dir)) {
    const path = `${dir}/${name}`;
    if (name.endsWith(".cpp")) out.push(path);
    else if (!name.includes(".")) {
      try {
        out.push(...cppFiles(path));
      } catch {
        /* a file without an extension */
      }
    }
  }
  return out;
}

/** wxString's `operator<` (a `std::set<wxString>`): by code point. */
const byCodePoint = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

export function build(files, read) {
  const registrations = files.flatMap((f) => scanProperties(read(f), f));
  const shown = new Set();
  const hidden = new Set();
  for (const r of registrations) (r.hidden ? hidden : shown).add(r.name.replaceAll(" ", "_"));
  return {
    properties: [...shown].sort(byCodePoint),
    hiddenFromRulesEditor: [...hidden].filter((n) => !shown.has(n)).sort(byCodePoint),
    registrationFiles: [...new Set(registrations.map((r) => r.file))].sort(),
  };
}

function main() {
  const commit = resolveCommit();
  const files = SCANNED.flatMap((d) => cppFiles(d)).sort();
  const props = build(files, readKicadFile);
  const functions = scanFunctions(readKicadFile(FUNCTIONS_FILE));
  const json =
    JSON.stringify(
      {
        commit: commit.sha,
        note:
          "GENERATED by web/studio/tools/extract-rule-completions.js. properties: the names (spaces as underscores, sorted, unique) of the properties " +
          "registered with new PROPERTY<..>( name ) / new PROPERTY_ENUM<..>( name ) in the .cpp files under pcbnew/ and common/ that are not " +
          ".SetIsHiddenFromRulesEditor() (hiddenFromRulesEditor: the names left out for it); functions: PCBEXPR_BUILTIN_FUNCTIONS::RegisterAllFunctions " +
          "(pcbnew/pcbexpr_functions.cpp), in order, DEPRECATED ones included. Do not edit by hand.",
        scanned: SCANNED.map((d) => `${d}/**/*.cpp`).concat(FUNCTIONS_FILE),
        registrationFiles: props.registrationFiles,
        properties: props.properties,
        hiddenFromRulesEditor: props.hiddenFromRulesEditor,
        functions,
      },
      null,
      2
    ) + "\n";
  if (process.argv.includes("--check")) {
    let same = false;
    try {
      same = readFileSync(OUT, "utf8") === json;
    } catch {
      same = false;
    }
    process.exit(same ? 0 : 1);
  }
  writeFileSync(OUT, json);
  console.log(`${props.properties.length} properties (${props.hiddenFromRulesEditor.length} hidden) from ${props.registrationFiles.length} files, ${functions.length} functions -> ${OUT}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();

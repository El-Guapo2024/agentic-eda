#!/usr/bin/env node
// Extracts each TOOL_ACTION's scope (`TOOL_ACTION_ARGS().Scope( AS_GLOBAL )`, AS_CONTEXT when the builder sets none: `TOOL_ACTION::TOOL_ACTION( const
// TOOL_ACTION_ARGS& )`'s `m_scope( aArgs.m_scope.value_or( AS_CONTEXT ) )`, common/tool/tool_action.cpp) into src/kicad/action_scopes.json. The hotkey editor
// needs it: `HOTKEY_STORE::CheckKeyConflicts` lets two actions share a key when both are AS_CONTEXT and of different tools.
//
//   KICAD_SRC_DIR=$HOME/src/kicad-src-8303b2ad KICAD10_SRC_DIR=$HOME/src/kicad-src-25ec2335 node tools/extract-action-scopes.js
//
// The files are the ones extract-actions.js reads (`collectAllActions`); the KiCad 10 actions src/kicad/actions.json takes from 25ec2335 (its
// `meta.kicad10Additions`) are read from that checkout's pcbnew/tools/pcb_actions.cpp when KICAD10_SRC_DIR is set. The action regex is
// tools/lib/actionsParser.js' `parseActionsFromFile`'s (that parser does not return the builder's body).
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readFileSync, existsSync } from "node:fs";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";
import { PRIMARY_ACTION_FILES, discoverMoreActionFiles } from "./extract-actions.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "action_scopes.json");
const ACTIONS = join(__dirname, "..", "src", "kicad", "actions.json");

/** Every action of `text` with its scope. */
export function scopesOf(text) {
  const out = [];
  const re = /(?:\bstatic\s+const\s+TOOL_ACTION\s+(\w+)|TOOL_ACTION\s+(\w+)::(\w+))\s*\(\s*TOOL_ACTION_ARGS\(\)([\s\S]*?)\)\s*;/g;
  let m;
  while ((m = re.exec(text))) {
    const body = m[4];
    const name = /\.Name\(\s*"((?:[^"\\]|\\.)*)"/.exec(body)?.[1];
    if (!name) continue;
    const scope = /\.Scope\(\s*(AS_CONTEXT|AS_ACTIVE|AS_GLOBAL)\s*\)/.exec(body)?.[1] ?? "AS_CONTEXT";
    out.push({ name, scope });
  }
  return out;
}

function main() {
  const files = [...PRIMARY_ACTION_FILES, ...discoverMoreActionFiles()];
  const scopes = {};
  for (const f of files) for (const { name, scope } of scopesOf(readKicadFile(f))) if (!(name in scopes)) scopes[name] = scope;
  const k10 = process.env.KICAD10_SRC_DIR;
  const wanted = JSON.parse(readFileSync(ACTIONS, "utf8")).actions.map((a) => a.name);
  if (k10 && existsSync(join(k10, "pcbnew/tools/pcb_actions.cpp"))) {
    for (const { name, scope } of scopesOf(readFileSync(join(k10, "pcbnew/tools/pcb_actions.cpp"), "utf8"))) if (!(name in scopes)) scopes[name] = scope;
  }
  const actions = {};
  for (const n of [...wanted].sort()) if (scopes[n]) actions[n] = scopes[n];
  const missing = wanted.filter((n) => !scopes[n]);
  writeJson(OUT, {
    meta: meta(files, k10 ? "the KiCad 10 additions of actions.json read from KiCad 25ec2335's pcbnew/tools/pcb_actions.cpp" : undefined),
    actions,
  });
  console.log(`${Object.keys(actions).length} scope(s); ${missing.length} action(s) of actions.json without one: ${missing.join(", ")}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}

#!/usr/bin/env node
// Extracts the 3D viewer's actions (`EDA_3D_ACTIONS`, 3d-viewer/3d_viewer/tools/eda_3d_actions.cpp) into src/kicad/actions3d.json: each with its friendly name ("" when
// the builder gives none: `HOTKEY_STORE::Init` leaves such an action out), tooltip, default hotkeys and scope. The PCB editor's Preferences > Hotkeys lists them in a
// "3D Viewer" section (pcbnew's kiface links the 3D viewer, so its static `TOOL_ACTION`s are in `ACTION_MANAGER::GetActionList()`, which `KIWAY::GetActions` hands the
// page), and the studio's 3D view reads its keys from them (kicad-port/actions3d.ts). Not in src/kicad/actions.json: the global dispatcher must not bind them.
//
//   KICAD_SRC_DIR=$HOME/src/kicad-src-8303b2ad node tools/extract-actions-3d.js
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";
import { parseActionsFromFile } from "./lib/actionsParser.js";
import { scopesOf } from "./extract-action-scopes.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "actions3d.json");
const FILE = "3d-viewer/3d_viewer/tools/eda_3d_actions.cpp";

function main() {
  // `TOOL_ACTION EDA_3D_ACTIONS:: reloadBoard(` and `.FriendlyName ( _( "Home View" ) )` have spaces the parsers' patterns do not allow
  const text = readKicadFile(FILE)
    .replace(/::\s+(\w+)\s*\(\s*TOOL_ACTION_ARGS/g, "::$1( TOOL_ACTION_ARGS")
    .replace(/\.(\w+)\s+\(/g, ".$1(");
  const scopes = new Map(scopesOf(text).map((s) => [s.name, s.scope]));
  // the parser falls back to the name for an action of no `.FriendlyName(`: KiCad's is empty
  const friendly = new Set();
  for (const m of text.matchAll(/TOOL_ACTION_ARGS\(\)([\s\S]*?)\)\s*;/g)) {
    const name = /\.Name\(\s*"([^"]*)"/.exec(m[1])?.[1];
    if (name && /\.FriendlyName\s*\(/.test(m[1])) friendly.add(name);
  }
  const actions = parseActionsFromFile(text)
    .map(({ symbol: _symbol, func: _func, ...a }) => ({ ...a, label: friendly.has(a.name) ? a.label : "", scope: scopes.get(a.name) ?? "AS_CONTEXT" }))
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  writeJson(OUT, { meta: meta([FILE]), actions });
  console.log(`${actions.length} action(s), ${actions.filter((a) => a.label).length} with a friendly name`);
}

main();

#!/usr/bin/env node
// Extracts every TOOL_ACTION the PCB editor's toolbars/menus can reference
// into src/kicad/actions.json. See tools/lib/kicadSource.js for why this
// cannot run inside the agent session that wrote it, and must be run
// from a plain terminal with git access to ~/ws/kicad-mirror:
//
//   node tools/extract-actions.js
//
// NOT YET VALIDATED against real KiCad source. The parsing (in
// tools/lib/actionsParser.js) follows the TOOL_ACTION_ARGS() builder
// pattern recent KiCad uses -- stable and well-known in shape, but the
// exact builder method KiCad 10.99 uses for a *second*, alternate hotkey
// (if any -- alternates may instead live in user-facing hotkey config,
// not the TOOL_ACTION definition itself) has not been confirmed against
// the pinned commit. Run this, skim the output against a `git show` of
// one real action, and adjust tools/lib/actionsParser.js before trusting
// it for the UI.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson, discoverFilesContaining } from "./lib/kicadSource.js";
import { parseActionsFromFile } from "./lib/actionsParser.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "actions.json");

export const PRIMARY_ACTION_FILES = ["pcbnew/tools/pcb_actions.cpp", "common/tool/actions.cpp"];

/** Any other file under pcbnew/, common/, or eeschema/ that defines TOOL_ACTIONs -- found by content, not guessed by filename. */
export function discoverMoreActionFiles() {
  return discoverFilesContaining("TOOL_ACTION_ARGS()", ["pcbnew", "common", "eeschema"]).filter((f) => !PRIMARY_ACTION_FILES.includes(f));
}

/**
 * Every action from every file, `symbol`/`func` included -- what
 * extract-toolbars.js and extract-menus.js need to resolve the C++
 * references they find. `files` defaults to the same discovery
 * extract-actions.js itself uses.
 */
export function collectAllActions(files = [...PRIMARY_ACTION_FILES, ...discoverMoreActionFiles()]) {
  const actions = [];
  const seen = new Set();
  for (const f of files) {
    const text = readKicadFile(f);
    for (const a of parseActionsFromFile(text)) {
      if (seen.has(a.name)) continue; // guard against a file being listed twice
      seen.add(a.name);
      actions.push(a);
    }
  }
  return { files, actions };
}

function main() {
  const { files, actions: withSymbols } = collectAllActions();
  // Drop the C++-internal `symbol`/`func` fields for the public file --
  // toolbars.json/menus.json resolve those themselves at extraction
  // time; the app only ever needs the dotted `name`.
  const actions = withSymbols.map(({ symbol: _symbol, func: _func, ...publicFields }) => publicFields).sort((a, b) => a.name.localeCompare(b.name));
  writeJson(OUT, { meta: meta(files), actions });
  console.log(`${actions.length} action(s) from ${files.length} file(s): ${files.join(", ")}`);
}

// Only run when invoked directly (`node extract-actions.js`), not when
// extract-toolbars.js/extract-menus.js import the helpers above.
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}

#!/usr/bin/env node
// Extracts the two library editors' menu bars:
//   - the Footprint Editor's (`pcbnew/menubar_footprint_editor.cpp`, `FOOTPRINT_EDIT_FRAME::doReCreateMenuBar`) into src/kicad/fp_menus.json
//   - the Symbol Editor's (`eeschema/symbol_editor/menubar_symbol_editor.cpp`, `SYMBOL_EDIT_FRAME::doReCreateMenuBar`) into src/kicad/sym_menus.json
// Same output shape as menus.json / sch_menus.json (MenusFile in src/kicad/types.ts). Run with KICAD_SRC_DIR set to a plain checkout:
//
//   KICAD_SRC_DIR=/path/to/kicad node tools/extract-lib-editor-menus.js
//
// The grammar is the one extract-menus.js and extract-sch-menus.js already handle (see tools/lib/menuExtract.js). Qualifier-first resolution
// (`PCB_ACTIONS::x`, `SCH_ACTIONS::x`, `ACTIONS::x`) matters here too: `SCH_ACTIONS::drawRectangle` and `PCB_ACTIONS::drawRectangle` are different
// actions with the same function name.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";
import { buildSymbolIndex } from "./lib/actionsParser.js";
import { collectAllActions } from "./extract-actions.js";
import { extractMenuBar } from "./lib/menuExtract.js";

const __dirname = dirname(fileURLToPath(import.meta.url));

const TARGETS = [
  { source: "pcbnew/menubar_footprint_editor.cpp", out: "fp_menus.json", qualifiers: ["PCB_ACTIONS", "ACTIONS"] },
  { source: "eeschema/symbol_editor/menubar_symbol_editor.cpp", out: "sym_menus.json", qualifiers: ["SCH_ACTIONS", "ACTIONS"] },
];

function main() {
  const { files: actionFiles, actions } = collectAllActions();
  const index = buildSymbolIndex(actions);
  for (const { source, out, qualifiers } of TARGETS) {
    const text = readKicadFile(source);
    const { menus, notes } = extractMenuBar(text, qualifiers, (sym) => index.resolve(sym));
    notes.push("textual scan does not evaluate C++ conditionals; items added by id (not a TOOL_ACTION), AddClose, the language list and the Help menu are not captured");
    writeJson(join(__dirname, "..", "src", "kicad", out), { meta: meta([source, ...actionFiles], notes.join("; ")), menus });
    for (const m of menus) console.log(`${out} ${m.label}: ${m.items.length} item(s)`);
    for (const n of notes.slice(0, -1)) console.warn(`${out}: ${n}`);
  }
}

main();

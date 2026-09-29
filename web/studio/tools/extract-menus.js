#!/usr/bin/env node
// Extracts the PCB editor's menu bar structure into src/kicad/menus.json.
// Run from a terminal with git access to ~/ws/kicad-mirror:
//
//   node tools/extract-menus.js
//
// LOW CONFIDENCE, same caveats as extract-toolbars.js (read that file's
// header comment first) -- this has not been run against real source.
//
// Approach: menu construction in wx-based KiCad code builds a local
// menu object with sequential `->Add(ACTION)` / `->AppendSeparator()` /
// `->AppendSubMenu(sub, _("Label"))` calls, then hands it to the menu
// bar with something like `menuBar->Append(fileMenu, _("&File"))`. This
// script finds every `*enu*ar->Append(<var>, _("Label"))` call (flexible
// on the menu bar's actual variable name) and treats the text *before*
// each one, back to the previous such call, as that top-level menu's
// construction code -- then walks that span for action refs, separators
// and submenu starts, same token-scan heuristic as extract-toolbars.js.
//
// Known limitation: submenu *contents* are NOT populated (each submenu
// comes out as `{ type: "submenu", label, items: [] }`) -- properly
// nesting items inside vs. after a submenu needs to track brace/variable
// scope, which this flat token scan does not do. Fill submenus in by
// hand after reading the real file, or extend this script to track
// scope once the real structure is visible.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";
import { buildSymbolIndex } from "./lib/actionsParser.js";
import { collectAllActions } from "./extract-actions.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "menus.json");
const SOURCE_FILE = "pcbnew/menubar_pcb_editor.cpp";

const TOP_LEVEL_APPEND = /\w*[Mm]enu[Bb]ar\w*\s*->\s*Append\s*\(\s*\w+\s*,\s*_\(\s*"((?:[^"\\]|\\.)*)"/g;

function splitTopLevelMenus(text) {
  const boundaries = [];
  let m;
  while ((m = TOP_LEVEL_APPEND.exec(text))) boundaries.push({ at: m.index, label: m[1].replace(/&/g, "").replace(/\\"/g, '"') });
  const menus = [];
  let start = 0;
  for (const b of boundaries) {
    menus.push({ label: b.label, span: text.slice(start, b.at) });
    start = b.at + 1;
  }
  return menus;
}

function walkSpan(span, resolve, unresolved) {
  const items = [];
  const tokenRe = /\b(?:PCB_ACTIONS|ACTIONS)::(\w+)\b|\b(\w*[Ss]eparator\w*)\s*\(|AppendSubMenu\s*\([^,]+,\s*_\(\s*"((?:[^"\\]|\\.)*)"/g;
  let m;
  while ((m = tokenRe.exec(span))) {
    if (m[1]) {
      const dotted = resolve(m[1]);
      if (dotted) items.push({ type: "item", action: dotted });
      else unresolved.add(m[1]);
    } else if (m[2]) {
      items.push({ type: "separator" });
    } else if (m[3] !== undefined) {
      items.push({ type: "submenu", label: m[3].replace(/&/g, "").replace(/\\"/g, '"'), items: [] });
    }
  }
  return items;
}

function main() {
  const { files: actionFiles, actions } = collectAllActions();
  const index = buildSymbolIndex(actions);

  const text = readKicadFile(SOURCE_FILE);
  const topLevel = splitTopLevelMenus(text);
  const unresolved = new Set();
  const menus = topLevel.map(({ label, span }) => ({ label, items: walkSpan(span, (sym) => index.resolve(sym), unresolved) }));

  const totalItems = menus.reduce((n, m) => n + m.items.length, 0);
  const notes = ["heuristic extraction, submenu contents not populated -- see script header comment"];
  if (menus.length === 0) notes.push("matched 0 top-level menus -- TOP_LEVEL_APPEND doesn't match this file's real menu-bar Append() calls");
  if (totalItems === 0 && menus.length > 0) notes.push("top-level menus found but 0 items inside them");
  if (unresolved.size > 0) notes.push(`${unresolved.size} referenced action symbol(s) did not resolve: ${[...unresolved].slice(0, 20).join(", ")}`);

  writeJson(OUT, { meta: meta([SOURCE_FILE, ...actionFiles], notes.join("; ")), menus });
  for (const m of menus) console.log(`${m.label}: ${m.items.length} item(s)`);
  if (unresolved.size > 0) console.warn(`${unresolved.size} unresolved action symbol(s), see menus.json meta.note`);
}

main();

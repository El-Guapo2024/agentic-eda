#!/usr/bin/env node
// Extracts the PCB editor's default toolbars (top main, left options,
// right drawing, auxiliary) into src/kicad/toolbars.json, in order, with
// separators. Run from a terminal with git access to ~/ws/kicad-mirror:
//
//   node tools/extract-toolbars.js
//
// LOWEST CONFIDENCE of the extraction scripts -- read this whole comment
// before trusting its output.
//
// pcbnew/toolbars_pcb_editor.cpp is named as a *dedicated* per-editor
// toolbar file, which strongly suggests recent KiCad moved toolbar setup
// out of the old imperative ReCreateHToolbar()-style methods on
// PCB_EDIT_FRAME into a declarative config built in one place -- but the
// exact builder API (method names like `.AppendAction()` used below are
// an informed guess, not a confirmed fact) has not been seen for the
// pinned commit. Rather than bet everything on one guessed grammar, this
// script ignores *how* actions are added and just walks the file
// top-to-bottom looking for two things that are very likely to appear
// as literal text no matter which wrapper API is used:
//   1. `PCB_ACTIONS::foo` / `ACTIONS::foo` tokens (action references)
//   2. anything with "separator" in its name (separator calls)
// and buckets them into one of the four toolbars by which region marker
// (TOP_MAIN/LEFT/RIGHT/AUX-ish keywords -- see REGION_MARKERS) it last
// saw. Toolbar *groups* (dropdown buttons) and *controls* (combo boxes)
// are NOT reliably detected by this heuristic and are left out; expect
// to add those by hand after reading the real file once.
//
// If `toolbars` comes back with everything dumped into one bucket, or
// empty, REGION_MARKERS below doesn't match this file's actual section
// markers -- open the file, find what really delimits the four toolbars
// (probably a switch over an enum, or four separate functions), and fix
// the regexes.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";
import { buildSymbolIndex } from "./lib/actionsParser.js";
import { collectAllActions } from "./extract-actions.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "toolbars.json");
const SOURCE_FILE = "pcbnew/toolbars_pcb_editor.cpp";

// Order matters: first match from this point on wins until the next one.
// Each is tried in the order listed against every position in the file;
// whichever occurs earliest becomes the next region switch.
const REGION_MARKERS = [
  { id: "main", pattern: /TOP_MAIN|MAIN_TOOLBAR|m_mainToolBar|MainToolbar/g },
  { id: "auxiliary", pattern: /TOP_AUX|AUX_TOOLBAR|m_auxiliaryToolBar|AuxiliaryToolbar/g },
  { id: "options", pattern: /\bLEFT\b|OPT_TOOLBAR|m_optionsToolBar|OptionsToolbar/g },
  { id: "drawing", pattern: /\bRIGHT\b|DRAW_TOOLBAR|m_drawToolBar|DrawingToolbar/g },
];

const ORIENTATION = { main: "horizontal", auxiliary: "horizontal", options: "vertical", drawing: "vertical" };

function findRegionSwitches(text) {
  const hits = [];
  for (const { id, pattern } of REGION_MARKERS) {
    pattern.lastIndex = 0;
    let m;
    while ((m = pattern.exec(text))) hits.push({ at: m.index, id });
  }
  hits.sort((a, b) => a.at - b.at);
  return hits;
}

function regionAt(switches, pos) {
  let current = null;
  for (const s of switches) {
    if (s.at > pos) break;
    current = s.id;
  }
  return current;
}

function extractToolbarItems(text, resolve) {
  const switches = findRegionSwitches(text);
  const byRegion = { main: [], options: [], drawing: [], auxiliary: [] };
  const unresolved = new Set();

  // One combined pass, in source order, over separators and action refs.
  const tokenRe = /\b(?:PCB_ACTIONS|ACTIONS)::(\w+)\b|\b(\w*[Ss]eparator\w*)\s*\(/g;
  let m;
  while ((m = tokenRe.exec(text))) {
    const region = regionAt(switches, m.index);
    if (!region) continue; // before any recognized region marker
    if (m[1]) {
      const dotted = resolve(m[1]);
      if (dotted) byRegion[region].push({ type: "action", action: dotted });
      else unresolved.add(m[1]);
    } else if (m[2]) {
      byRegion[region].push({ type: "separator" });
    }
  }
  return { byRegion, unresolved };
}

function main() {
  const { files: actionFiles, actions } = collectAllActions();
  const index = buildSymbolIndex(actions);

  const text = readKicadFile(SOURCE_FILE);
  const { byRegion, unresolved } = extractToolbarItems(text, (sym) => index.resolve(sym));

  const toolbars = Object.entries(byRegion).map(([id, items]) => ({ id, orientation: ORIENTATION[id], items }));
  const totalItems = toolbars.reduce((n, t) => n + t.items.length, 0);

  const notes = [
    "heuristic extraction -- action/separator tokens only, no groups or controls; see script header comment",
  ];
  if (totalItems === 0) notes.push("matched 0 items -- REGION_MARKERS likely doesn't match this file's real structure");
  if (unresolved.size > 0) notes.push(`${unresolved.size} referenced action symbol(s) did not resolve against actions.json's source files: ${[...unresolved].slice(0, 20).join(", ")}`);

  writeJson(OUT, { meta: meta([SOURCE_FILE, ...actionFiles], notes.join("; ")), toolbars });
  for (const t of toolbars) console.log(`${t.id}: ${t.items.length} item(s)`);
  if (unresolved.size > 0) console.warn(`${unresolved.size} unresolved action symbol(s), see toolbars.json meta.note`);
}

main();

#!/usr/bin/env node
// Extracts the PCB editor's default toolbars (top main, left options,
// right drawing, auxiliary) into src/kicad/toolbars.json, in order, with
// separators and dropdown groups. Run from a terminal with git access to
// ~/ws/kicad-mirror, or with KICAD_SRC_DIR set to a plain checkout:
//
//   node tools/extract-toolbars.js
//
// pcbnew/toolbars_pcb_editor.cpp defines each toolbar as a
// `TOOLBAR_CONFIGURATION` built with a fluent builder, one `switch` case
// per `TOOLBAR_LOC` (confirmed by reading the real file for the pinned
// commit -- this is no longer a guess):
//
//   case TOOLBAR_LOC::TOP_MAIN: config.AppendAction( ACTIONS::save ); ...
//   case TOOLBAR_LOC::LEFT:     config.AppendAction( ACTIONS::toggleGrid )
//                                     .AppendGroup( TOOLBAR_GROUP_CONFIG( _( "Units" ) )
//                                                   .AddAction( ACTIONS::unitsMM )
//                                                   .AddAction( ACTIONS::unitsInches ) ); ...
//   case TOOLBAR_LOC::RIGHT: ...
//   case TOOLBAR_LOC::TOP_AUX: ...
//
// Top-level items use `.AppendAction`/`.AppendSeparator`/`.AppendGroup`;
// a group's *members* use `.AddAction` (a different method, which is
// exactly what lets this script tell a group's own items apart from the
// `menu->Add(...)` calls inside an unrelated `.AddContextMenu(lambda)`/
// `.WithContextMenu(lambda)` attached to the same or a neighboring
// button -- those lambdas build a right-click menu, not toolbar buttons,
// and would otherwise leak into a group's item list since they often sit
// inside the same parenthesized span).
//
// Confidence: region markers and the group grammar are now confirmed
// against real source, not guessed -- higher confidence than earlier
// revisions of this script. Still unconfirmed: a group's *icon* (no
// `.Icon(...)` call was seen on `TOOLBAR_GROUP_CONFIG`; this script uses
// its first resolved member action's icon as a reasonable stand-in, not
// a source-verified fact) and `.AppendControl(...)`-style combo boxes on
// the auxiliary toolbar, which this script does not attempt to parse.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";
import { buildSymbolIndex } from "./lib/actionsParser.js";
import { collectAllActions } from "./extract-actions.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "toolbars.json");
const SOURCE_FILE = "pcbnew/toolbars_pcb_editor.cpp";

// First match from each position wins; tried against every position in
// the file, whichever occurs earliest becomes the next region switch.
// The precise `TOOLBAR_LOC::X` form is tried by putting it first in each
// alternation, but the looser fallback patterns stay in case a future
// KiCad version renames the enum without changing the general shape.
const REGION_MARKERS = [
  { id: "main", pattern: /TOOLBAR_LOC::TOP_MAIN|MAIN_TOOLBAR|m_mainToolBar/g },
  { id: "auxiliary", pattern: /TOOLBAR_LOC::TOP_AUX|AUX_TOOLBAR|m_auxiliaryToolBar/g },
  { id: "options", pattern: /TOOLBAR_LOC::LEFT|OPT_TOOLBAR|m_optionsToolBar/g },
  { id: "drawing", pattern: /TOOLBAR_LOC::RIGHT|DRAW_TOOLBAR|m_drawToolBar/g },
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

/** The balanced-paren end of a call whose `(` is the first one at or after `fromIndex`. Returns the index of the matching `)`, or -1. */
function findMatchingParen(text, fromIndex) {
  const start = text.indexOf("(", fromIndex);
  if (start === -1) return -1;
  let depth = 0;
  for (let i = start; i < text.length; i++) {
    if (text[i] === "(") depth++;
    else if (text[i] === ")") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

function extractToolbarItems(text, resolve, iconOf) {
  const switches = findRegionSwitches(text);
  const byRegion = { main: [], options: [], drawing: [], auxiliary: [] };
  const unresolved = new Set();

  const groupStartRe = /\.AppendGroup\(\s*TOOLBAR_GROUP_CONFIG\(\s*_\(\s*"((?:[^"\\]|\\.)*)"/g;
  const groupSpans = []; // {start, end, label}
  let gm;
  while ((gm = groupStartRe.exec(text))) {
    const appendGroupCallStart = text.lastIndexOf(".AppendGroup", gm.index);
    const end = findMatchingParen(text, appendGroupCallStart);
    if (end === -1) continue;
    groupSpans.push({ start: gm.index, end, label: gm[1].replace(/\\"/g, '"') });
  }

  function groupAt(pos) {
    return groupSpans.find((g) => pos >= g.start && pos <= g.end);
  }

  // One combined pass over separators, top-level actions, and group
  // starts, in source order; skip past a group's whole span once its
  // start is handled, so its internal `.AddAction`s and any nested
  // context-menu lambda aren't also scanned as top-level items.
  const tokenRe = /\.AppendSeparator\s*\(|\.AppendAction\(\s*(?:PCB_ACTIONS|ACTIONS)::(\w+)|\.AppendGroup\(\s*TOOLBAR_GROUP_CONFIG\(\s*_\(\s*"(?:[^"\\]|\\.)*"/g;
  let m;
  while ((m = tokenRe.exec(text))) {
    const region = regionAt(switches, m.index);
    if (!region) continue;
    const group = groupAt(m.index);
    if (group) {
      if (m.index !== group.start) continue; // inside a group's span but not its own start token -- skip (handled below)
      const body = text.slice(group.start, group.end);
      const memberRe = /\.AddAction\(\s*(?:PCB_ACTIONS|ACTIONS)::(\w+)/g;
      const items = [];
      let mm;
      while ((mm = memberRe.exec(body))) {
        const dotted = resolve(mm[1]);
        if (dotted) items.push(dotted);
        else unresolved.add(mm[1]);
      }
      const icon = items.map(iconOf).find((i) => i) ?? null;
      byRegion[region].push({ type: "group", label: group.label, icon, items });
      tokenRe.lastIndex = group.end; // skip the rest of this group's span
      continue;
    }
    if (m[0].startsWith(".AppendSeparator")) {
      byRegion[region].push({ type: "separator" });
    } else if (m[1]) {
      const dotted = resolve(m[1]);
      if (dotted) byRegion[region].push({ type: "action", action: dotted });
      else unresolved.add(m[1]);
    }
  }
  return { byRegion, unresolved };
}

function main() {
  const { files: actionFiles, actions } = collectAllActions();
  const index = buildSymbolIndex(actions);
  const iconByName = new Map(actions.map((a) => [a.name, a.icon]));

  const text = readKicadFile(SOURCE_FILE);
  const { byRegion, unresolved } = extractToolbarItems(
    text,
    (sym) => index.resolve(sym),
    (dottedName) => iconByName.get(dottedName) ?? null
  );

  const toolbars = Object.entries(byRegion).map(([id, items]) => ({ id, orientation: ORIENTATION[id], items }));
  const totalItems = toolbars.reduce((n, t) => n + t.items.length, 0);
  const totalGroups = toolbars.reduce((n, t) => n + t.items.filter((i) => i.type === "group").length, 0);

  const notes = ["group icons are a stand-in (first member's icon) -- TOOLBAR_GROUP_CONFIG has no .Icon() call this script found; auxiliary toolbar combo-box controls are not parsed"];
  if (totalItems === 0) notes.push("matched 0 items -- REGION_MARKERS likely doesn't match this file's real structure");
  if (unresolved.size > 0) notes.push(`${unresolved.size} referenced action symbol(s) did not resolve: ${[...unresolved].slice(0, 20).join(", ")}`);

  writeJson(OUT, { meta: meta([SOURCE_FILE, ...actionFiles], notes.join("; ")), toolbars });
  for (const t of toolbars) console.log(`${t.id}: ${t.items.length} item(s), ${t.items.filter((i) => i.type === "group").length} group(s)`);
  console.log(`${totalGroups} group(s) total`);
  if (unresolved.size > 0) console.warn(`${unresolved.size} unresolved action symbol(s), see toolbars.json meta.note`);
}

main();

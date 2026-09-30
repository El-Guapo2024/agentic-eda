#!/usr/bin/env node
// Extracts the Schematic editor's default toolbars (main, options/left,
// drawing/right) into src/kicad/sch_toolbars.json, in order, with
// separators and dropdown groups. This is eeschema's equivalent of
// extract-toolbars.js (the PCB editor's version) -- see that script's
// header for the general grammar this is built on. Run from a terminal
// with git access to ~/ws/kicad-mirror, or with KICAD_SRC_DIR set to a
// plain checkout:
//
//   node tools/extract-sch-toolbars.js
//
// Confirmed by reading eeschema/toolbars_sch_editor.cpp in full (491
// lines, pinned commit 8303b2ada05226fa603e5ac0420d240fd65ce52b) -- not
// assumed to match pcbnew's file just because the two editors are
// siblings:
//
//   - It uses the exact same builder grammar as pcbnew's file: a
//     `TOOLBAR_CONFIGURATION` built fluently inside
//     `SCH_EDIT_TOOLBAR_SETTINGS::DefaultToolbarConfig`, one `switch`
//     case per `TOOLBAR_LOC`, using `.AppendAction`/`.AppendSeparator`/
//     `.AppendGroup( TOOLBAR_GROUP_CONFIG(...).AddAction(...) )`/
//     `.AppendControl(...)`. No grammar differences from pcbnew were
//     found.
//
//   - Action references use `SCH_ACTIONS::name` (eeschema-specific,
//     defined in eeschema/tools/sch_actions.cpp -- found by content via
//     extract-actions.js's discoverMoreActionFiles(), which now also
//     searches eeschema/: 228 `TOOL_ACTION_ARGS()` entries in that file,
//     every one qualified `TOOL_ACTION SCH_ACTIONS::`, confirmed with
//     `grep -oE "TOOL_ACTION [A-Z_]+::" eeschema/tools/sch_actions.cpp`)
//     and plain `ACTIONS::name` (shared, common/tool/actions.cpp,
//     already covered by the existing PRIMARY_ACTION_FILES). There is
//     NO `EE_ACTIONS::` symbol anywhere under eeschema/ for this commit
//     -- checked with `grep -rn EE_ACTIONS eeschema/` => 0 hits.
//     eeschema's own action namespace is `SCH_ACTIONS`, not
//     `EE_ACTIONS`; adjust the regex below if a different KiCad version
//     ever renames it back.
//
//   - Only 3 of the 4 `TOOLBAR_LOC` cases produce a toolbar. `TOP_AUX`
//     is, in full:
//         case TOOLBAR_LOC::TOP_AUX:
//             return std::nullopt;
//     i.e. the schematic editor has no auxiliary toolbar at all -- not
//     merely one this script failed to parse. That is a real layout
//     difference from pcbnew (whose TOP_AUX is populated with
//     trackWidth/viaDiameter/etc.), confirmed by reading, so this
//     script defines only 3 regions: TOP_MAIN -> "main", LEFT ->
//     "options", RIGHT -> "drawing".
//
//   - `.AppendControl(...)` targets two enums: eeschema-only
//     `SCH_ACTION_TOOLBAR_CONTROLS::currentVariant` (declared at the
//     top of this same file) and shared `ACTION_TOOLBAR_CONTROLS::
//     ipcScripting` / `::overrideLocks` (declared in
//     common/tool/action_toolbar.cpp, the same enum pcbnew's toolbar
//     also draws trackWidth/viaDiameter/etc. from). Unlike
//     extract-toolbars.js's own note that AppendControl() names only an
//     enumerator with no label in source, for this commit
//     `ACTION_TOOLBAR_CONTROL`'s constructor *does* take a real
//     translated friendly-name string as its 2nd argument, e.g. (this
//     file, right above DefaultToolbarConfig):
//       ACTION_TOOLBAR_CONTROL SCH_ACTION_TOOLBAR_CONTROLS::currentVariant(
//               "control.currentVariant", _( "Current variant" ),
//               _( "Selects the current schematic variant" ), { FRAME_SCH } );
//     and (common/tool/action_toolbar.cpp):
//       ACTION_TOOLBAR_CONTROL ACTION_TOOLBAR_CONTROLS::ipcScripting( "control.IPCPlugin",
//               _( "IPC/Scripting plugins" ), ... );
//       ACTION_TOOLBAR_CONTROL ACTION_TOOLBAR_CONTROLS::overrideLocks( "control.OverrideLocks",
//               _( "Override locks" ), ... );
//     buildControlLabels() below extracts that 2nd-argument string
//     directly instead of hand-maintaining a guess table.
//
//   - One item, `.AppendAction( ACTIONS::toggleGrid )`, has a
//     `.WithContextMenu( []( TOOL_MANAGER* ) { ... menu->Add(
//     ACTIONS::gridProperties ); ... } )` chained onto it in the LEFT
//     region: the lambda's `menu->Add(...)` builds a right-click
//     context menu, not a toolbar button, so `ACTIONS::gridProperties`
//     must NOT appear as a toolbar item. It doesn't, because the token
//     regex below only matches literal `.AppendAction(`/`.AppendGroup(`/
//     `.AppendControl(`/`.AppendSeparator(`, never `menu->Add(`.
//
//   - Exactly one `return config;` in the whole file (the end of
//     `DefaultToolbarConfig`, right after the switch) -- confirmed with
//     a plain substring search -- so the same end-of-function
//     truncation trick extract-toolbars.js uses (to keep
//     `configureToolbars()`'s unrelated custom-control-factory
//     registrations, a few lines below, from leaking in) applies here
//     unchanged.
//
// Not filtered (same textual-scan limitation as extract-toolbars.js --
// neither script evaluates C++ conditionals): two items are gated
// behind runtime conditionals in source and still appear as ordinary
// items here, in their normal source position --
// `ACTIONS::doNew`/`ACTIONS::open` behind `if( Kiface().IsSingle() )` in
// TOP_MAIN, and `ACTIONS::toggleBoundingBoxes` behind
// `if( ADVANCED_CFG::GetCfg().m_DrawBoundingBoxes )` in LEFT (a debug
// build flag).
//
// Everything above was confirmed by reading the pinned commit's source;
// nothing here was guessed.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";
import { buildSymbolIndex } from "./lib/actionsParser.js";
import { collectAllActions } from "./extract-actions.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "sch_toolbars.json");
const SOURCE_FILE = "eeschema/toolbars_sch_editor.cpp";
// Where AppendControl()'s targets are actually constructed (see header) --
// read in addition to SOURCE_FILE so control labels can come from real
// source strings instead of a guess table.
const CONTROL_LABEL_SOURCE_FILES = [SOURCE_FILE, "common/tool/action_toolbar.cpp"];

// Only 3 confirmed marker patterns -- TOOLBAR_LOC::TOP_AUX's case returns
// std::nullopt immediately (see header comment), so it never builds a
// TOOLBAR_CONFIGURATION and gets no marker/region of its own. No looser
// fallback patterns (pcbnew's REGION_MARKERS also try e.g. `m_mainToolBar`)
// are added here since this file never uses any such legacy names --
// confirmed by reading the whole 491 lines.
const REGION_MARKERS = [
  { id: "main", pattern: /TOOLBAR_LOC::TOP_MAIN/g },
  { id: "options", pattern: /TOOLBAR_LOC::LEFT/g },
  { id: "drawing", pattern: /TOOLBAR_LOC::RIGHT/g },
];

const ORIENTATION = { main: "horizontal", options: "vertical", drawing: "vertical" };

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

/**
 * Friendly-name label for every ACTION_TOOLBAR_CONTROL constructed in
 * `files`, keyed by its enumerator name (e.g. "ipcScripting" ->
 * "IPC/Scripting plugins"), read straight from the constructor's 2nd
 * argument -- see this script's header comment for the confirmed source
 * shape and why (unlike pcbnew's CONTROL_LABELS) this isn't a guess.
 */
function buildControlLabels(files) {
  const labels = new Map();
  const re = /ACTION_TOOLBAR_CONTROL\s+(?:SCH_ACTION_TOOLBAR_CONTROLS|ACTION_TOOLBAR_CONTROLS)::(\w+)\(\s*"[^"]*"\s*,\s*_\(\s*"((?:[^"\\]|\\.)*)"/g;
  for (const f of files) {
    const text = readKicadFile(f);
    re.lastIndex = 0;
    let m;
    while ((m = re.exec(text))) labels.set(m[1], m[2].replace(/\\"/g, '"'));
  }
  return labels;
}

function extractToolbarItems(text, resolve, iconOf, controlLabels) {
  const switches = findRegionSwitches(text);
  const byRegion = { main: [], options: [], drawing: [] };
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

  // One combined pass over separators, top-level actions, controls, and
  // group starts, in source order; skip past a group's whole span once
  // its start is handled, so its internal `.AddAction`s and any nested
  // context-menu lambda aren't also scanned as top-level items.
  //
  // The qualifier (SCH_ACTIONS vs ACTIONS) is captured, not discarded,
  // and resolved as the full "QUALIFIER::func" symbol first -- NOT just
  // the bare func name. This matters here in a way it may not for
  // pcbnew: 46 bare func names (e.g. `mirrorV`, `mirrorH`, `drawArc`,
  // `generateBOM`, `lineModeFree`, `lineMode45`, ...) are defined
  // *independently* under both `PCB_ACTIONS::` and `SCH_ACTIONS::` with
  // different `.Name()` strings (confirmed: e.g. `PCB_ACTIONS::mirrorV`
  // -> "pcbnew.InteractiveEdit.mirrorVertically" vs
  // `SCH_ACTIONS::mirrorV` -> "eeschema.InteractiveEdit.mirrorV" -- two
  // distinct actions). buildSymbolIndex()'s byFunc fallback is
  // first-wins across every file collectAllActions() reads, and pcbnew's
  // action file is read before eeschema's is discovered, so resolving
  // by bare func name alone would silently mis-resolve this toolbar's
  // own `SCH_ACTIONS::mirrorV` reference to pcbnew's unrelated action.
  // Resolving the qualified symbol first avoids that; the bare-func
  // call is kept only as a last-resort fallback (should not be needed
  // in practice -- every SCH_ACTIONS::/ACTIONS:: symbol this file
  // references was confirmed present in the files collectAllActions()
  // reads).
  const tokenRe =
    /\.AppendSeparator\s*\(|\.AppendAction\(\s*(SCH_ACTIONS|ACTIONS)::(\w+)|\.AppendControl\(\s*(SCH_ACTION_TOOLBAR_CONTROLS|ACTION_TOOLBAR_CONTROLS)::(\w+)|\.AppendGroup\(\s*TOOLBAR_GROUP_CONFIG\(\s*_\(\s*"(?:[^"\\]|\\.)*"/g;
  let m;
  while ((m = tokenRe.exec(text))) {
    const region = regionAt(switches, m.index);
    if (!region) continue;
    const group = groupAt(m.index);
    if (group) {
      if (m.index !== group.start) continue; // inside a group's span but not its own start token -- skip (handled below)
      const body = text.slice(group.start, group.end);
      const memberRe = /\.AddAction\(\s*(SCH_ACTIONS|ACTIONS)::(\w+)/g;
      const items = [];
      let mm;
      while ((mm = memberRe.exec(body))) {
        const dotted = resolve(`${mm[1]}::${mm[2]}`) ?? resolve(mm[2]);
        if (dotted) items.push(dotted);
        else unresolved.add(mm[2]);
      }
      const icon = items.map(iconOf).find((i) => i) ?? null;
      byRegion[region].push({ type: "group", label: group.label, icon, items });
      tokenRe.lastIndex = group.end; // skip the rest of this group's span
      continue;
    }
    if (m[0].startsWith(".AppendSeparator")) {
      byRegion[region].push({ type: "separator" });
    } else if (m[2]) {
      const dotted = resolve(`${m[1]}::${m[2]}`) ?? resolve(m[2]);
      if (dotted) byRegion[region].push({ type: "action", action: dotted });
      else unresolved.add(m[2]);
    } else if (m[4]) {
      byRegion[region].push({ type: "control", control: m[4], label: controlLabels.get(m[4]) ?? m[4] });
    }
  }
  return { byRegion, unresolved };
}

function main() {
  const { files: actionFiles, actions } = collectAllActions();
  const index = buildSymbolIndex(actions);
  const iconByName = new Map(actions.map((a) => [a.name, a.icon]));
  const controlLabels = buildControlLabels(CONTROL_LABEL_SOURCE_FILES);

  const fullText = readKicadFile(SOURCE_FILE);
  // See header comment: DefaultToolbarConfig() ends at its own single
  // `return config;`; configureToolbars() right after it registers
  // custom control factories and must not be scanned.
  const endOfConfigFn = fullText.indexOf("return config;");
  const text = endOfConfigFn === -1 ? fullText : fullText.slice(0, endOfConfigFn);
  const { byRegion, unresolved } = extractToolbarItems(
    text,
    (sym) => index.resolve(sym),
    (dottedName) => iconByName.get(dottedName) ?? null,
    controlLabels
  );

  const toolbars = Object.entries(byRegion).map(([id, items]) => ({ id, orientation: ORIENTATION[id], items }));
  const totalItems = toolbars.reduce((n, t) => n + t.items.length, 0);
  const totalGroups = toolbars.reduce((n, t) => n + t.items.filter((i) => i.type === "group").length, 0);

  const notes = [
    "group icons are a stand-in (first member's icon) -- TOOLBAR_GROUP_CONFIG has no .Icon() call this script found",
    "TOOLBAR_LOC::TOP_AUX returns std::nullopt in source -- eeschema has no auxiliary toolbar at all (unlike pcbnew), so only main/options/drawing are emitted here",
    "control labels here ARE real source strings (ACTION_TOOLBAR_CONTROL's 2nd constructor argument, read from toolbars_sch_editor.cpp and common/tool/action_toolbar.cpp) -- not a hand-guessed table",
    "two items are behind source-level conditionals not evaluated by this script: ACTIONS.doNew/open (Kiface().IsSingle()) in main, ACTIONS.toggleBoundingBoxes (ADVANCED_CFG debug flag) in options",
  ];
  if (totalItems === 0) notes.push("matched 0 items -- REGION_MARKERS likely doesn't match this file's real structure");
  if (unresolved.size > 0) notes.push(`${unresolved.size} referenced action symbol(s) did not resolve: ${[...unresolved].slice(0, 20).join(", ")}`);

  writeJson(OUT, { meta: meta([SOURCE_FILE, "common/tool/action_toolbar.cpp", ...actionFiles], notes.join("; ")), toolbars });
  for (const t of toolbars) console.log(`${t.id}: ${t.items.length} item(s), ${t.items.filter((i) => i.type === "group").length} group(s)`);
  console.log(`${totalGroups} group(s) total`);
  if (unresolved.size > 0) console.warn(`${unresolved.size} unresolved action symbol(s), see sch_toolbars.json meta.note`);
}

main();

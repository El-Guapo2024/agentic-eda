#!/usr/bin/env node
// Extracts the default toolbars of the three editors that have no extraction of their own yet -- the Footprint Editor, the Symbol Editor and the 3D Viewer --
// into src/kicad/fp_toolbars.json, sym_toolbars.json and viewer3d_toolbars.json, in the shape extract-toolbars.js / extract-sch-toolbars.js write for the
// board and schematic editors (`ToolbarsFile`, src/kicad/types.ts). Run with KICAD_SRC_DIR set to a plain checkout of the pinned commit, or from a terminal
// with git access to ~/ws/kicad-mirror:
//
//   KICAD_SRC_DIR=~/ws/kicad-src-8303b2ad node tools/extract-editor-toolbars.js
//
// The sources (read in full at the pinned commit, not guessed):
//   - pcbnew/toolbars_footprint_editor.cpp -- FOOTPRINT_EDIT_TOOLBAR_SETTINGS::DefaultToolbarConfig. TOP_AUX returns std::nullopt (no auxiliary toolbar);
//     TOP_MAIN -> "main", LEFT -> "options", RIGHT -> "drawing". Actions are PCB_ACTIONS:: / ACTIONS::.
//   - eeschema/symbol_editor/toolbars_symbol_editor.cpp -- SYMBOL_EDIT_TOOLBAR_SETTINGS::DefaultToolbarConfig, same three regions, SCH_ACTIONS:: / ACTIONS::.
//   - 3d-viewer/3d_viewer/toolbars_3d.cpp -- EDA_3D_VIEWER_TOOLBAR_SETTINGS::DefaultToolbarConfig: only TOP_MAIN exists (LEFT, RIGHT and TOP_AUX return
//     std::nullopt). Its EDA_3D_ACTIONS:: are defined in 3d-viewer/3d_viewer/tools/eda_3d_actions.cpp, which tools/extract-actions.js does not read (it
//     covers pcbnew/, common/ and eeschema/), so the 3D toolbar file carries the 3D actions' own label/tooltip/hotkey/icon next to the toolbar.
//
// The grammar is the one extract-sch-toolbars.js documents: a `TOOLBAR_CONFIGURATION` built fluently, one `switch` case per `TOOLBAR_LOC`, with
// `.AppendAction` / `.AppendSeparator` / `.AppendGroup( TOOLBAR_GROUP_CONFIG(...).AddAction(...) )` / `.AppendControl(...)`. A `.WithContextMenu( lambda )`
// builds a right-click menu (its `menu->Add(...)` calls are never toolbar buttons, and this scan only matches the `.Append*` calls). Two items are behind
// source-level conditionals this textual scan does not evaluate: `ACTIONS::toggleBoundingBoxes` (ADVANCED_CFG debug flag) in the LEFT toolbars, which is
// kept out here because the app has no such debug flag.
//
// Action symbols are resolved by their QUALIFIED name first (`PCB_ACTIONS::rotateCcw`, `SCH_ACTIONS::rotateCCW`): 46 bare function names are defined
// under both PCB_ACTIONS and SCH_ACTIONS with different dotted names (see extract-sch-toolbars.js), so a bare-name lookup would silently pick the other
// editor's action.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";
import { buildSymbolIndex, parseActionsFromFile } from "./lib/actionsParser.js";
import { collectAllActions } from "./extract-actions.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT_DIR = join(__dirname, "..", "src", "kicad");
const ACTIONS_3D_FILE = "3d-viewer/3d_viewer/tools/eda_3d_actions.cpp";

const ORIENTATION = { main: "horizontal", options: "vertical", drawing: "vertical" };

const TARGETS = [
  {
    out: "fp_toolbars.json",
    file: "pcbnew/toolbars_footprint_editor.cpp",
    qualifiers: ["PCB_ACTIONS", "ACTIONS"],
    controlQualifiers: ["PCB_ACTION_TOOLBAR_CONTROLS", "ACTION_TOOLBAR_CONTROLS"],
    regions: [
      { id: "main", pattern: /TOOLBAR_LOC::TOP_MAIN/g },
      { id: "options", pattern: /TOOLBAR_LOC::LEFT/g },
      { id: "drawing", pattern: /TOOLBAR_LOC::RIGHT/g },
    ],
    skip: new Set(["ACTIONS::toggleBoundingBoxes"]),
  },
  {
    out: "sym_toolbars.json",
    file: "eeschema/symbol_editor/toolbars_symbol_editor.cpp",
    qualifiers: ["SCH_ACTIONS", "ACTIONS"],
    controlQualifiers: ["SCH_ACTION_TOOLBAR_CONTROLS", "ACTION_TOOLBAR_CONTROLS"],
    regions: [
      { id: "main", pattern: /TOOLBAR_LOC::TOP_MAIN/g },
      { id: "options", pattern: /TOOLBAR_LOC::LEFT/g },
      { id: "drawing", pattern: /TOOLBAR_LOC::RIGHT/g },
    ],
    skip: new Set(["ACTIONS::toggleBoundingBoxes"]),
  },
  {
    out: "viewer3d_toolbars.json",
    file: "3d-viewer/3d_viewer/toolbars_3d.cpp",
    qualifiers: ["EDA_3D_ACTIONS", "ACTIONS"],
    controlQualifiers: ["ACTION_TOOLBAR_CONTROLS"],
    // Only TOP_MAIN builds a configuration here; LEFT/RIGHT/TOP_AUX share one `return std::nullopt;` case, so they get no region of their own.
    regions: [{ id: "main", pattern: /TOOLBAR_LOC::TOP_MAIN/g }],
    skip: new Set(),
  },
];

function findRegionSwitches(text, regions) {
  const hits = [];
  for (const { id, pattern } of regions) {
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

/** Friendly name of every ACTION_TOOLBAR_CONTROL in common/tool/action_toolbar.cpp (and the editor's own file), keyed by enumerator. */
function buildControlLabels(files) {
  const labels = new Map();
  const re = /ACTION_TOOLBAR_CONTROL\s+(\w+)::(\w+)\(\s*"[^"]*"\s*,\s*_\(\s*"((?:[^"\\]|\\.)*)"/g;
  for (const f of files) {
    let text;
    try {
      text = readKicadFile(f);
    } catch {
      continue;
    }
    re.lastIndex = 0;
    let m;
    while ((m = re.exec(text))) labels.set(m[2], m[3].replace(/\\"/g, '"'));
  }
  return labels;
}

function extractToolbarItems(text, target, resolveSymbol, iconOf, controlLabels) {
  // `PCB_ACTIONS` derives from `ACTIONS`, so a toolbar may name a shared action through the derived class (`PCB_ACTIONS::togglePolarCoords` is
  // `ACTIONS::togglePolarCoords`): the qualified symbol first, then the same function under each qualifier this editor may use.
  const resolve = (symbol) => {
    const direct = resolveSymbol(symbol);
    if (direct) return direct;
    const func = symbol.split("::")[1];
    for (const qualifier of target.qualifiers) {
      const viaBase = resolveSymbol(`${qualifier}::${func}`);
      if (viaBase) return viaBase;
    }
    return null;
  };
  const switches = findRegionSwitches(text, target.regions);
  const byRegion = Object.fromEntries(target.regions.map((r) => [r.id, []]));
  const unresolved = new Set();
  const q = target.qualifiers.join("|");
  const cq = target.controlQualifiers.join("|");

  const groupStartRe = /\.AppendGroup\(\s*TOOLBAR_GROUP_CONFIG\(\s*_\(\s*"((?:[^"\\]|\\.)*)"/g;
  const groupSpans = [];
  let gm;
  while ((gm = groupStartRe.exec(text))) {
    const appendGroupCallStart = text.lastIndexOf(".AppendGroup", gm.index);
    const end = findMatchingParen(text, appendGroupCallStart);
    if (end === -1) continue;
    groupSpans.push({ start: gm.index, end, label: gm[1].replace(/\\"/g, '"') });
  }
  const groupAt = (pos) => groupSpans.find((g) => pos >= g.start && pos <= g.end);

  const tokenRe = new RegExp(
    `\\.AppendSeparator\\s*\\(|\\.AppendAction\\(\\s*(${q})::(\\w+)|\\.AppendControl\\(\\s*(?:${cq})::(\\w+)|\\.AppendGroup\\(\\s*TOOLBAR_GROUP_CONFIG\\(\\s*_\\(\\s*"(?:[^"\\\\]|\\\\.)*"`,
    "g"
  );
  let m;
  while ((m = tokenRe.exec(text))) {
    const region = regionAt(switches, m.index);
    if (!region) continue;
    const group = groupAt(m.index);
    if (group) {
      if (m.index !== group.start) continue;
      const body = text.slice(group.start, group.end);
      const memberRe = new RegExp(`\\.AddAction\\(\\s*(${q})::(\\w+)`, "g");
      const items = [];
      let mm;
      while ((mm = memberRe.exec(body))) {
        const dotted = resolve(`${mm[1]}::${mm[2]}`);
        if (dotted) items.push(dotted);
        else unresolved.add(`${mm[1]}::${mm[2]}`);
      }
      const icon = items.map(iconOf).find((i) => i) ?? null;
      byRegion[region].push({ type: "group", label: group.label, icon, items });
      tokenRe.lastIndex = group.end;
      continue;
    }
    if (m[0].startsWith(".AppendSeparator")) {
      byRegion[region].push({ type: "separator" });
    } else if (m[2]) {
      const symbol = `${m[1]}::${m[2]}`;
      if (target.skip.has(symbol)) continue;
      const dotted = resolve(symbol);
      if (dotted) byRegion[region].push({ type: "action", action: dotted });
      else unresolved.add(symbol);
    } else if (m[3]) {
      byRegion[region].push({ type: "control", control: m[3], label: controlLabels.get(m[3]) ?? m[3] });
    }
  }
  return { byRegion, unresolved };
}

/** The 3D viewer's own actions: eda_3d_actions.cpp's `TOOL_ACTION EDA_3D_ACTIONS:: name(...)` has a stray space after `::` for one of them, which the shared parser's regex does not allow. */
function parse3dActions() {
  const text = readKicadFile(ACTIONS_3D_FILE).replace(/::\s+(\w)/g, "::$1");
  return parseActionsFromFile(text).map((a) => {
    // `.ToolbarState( TOOLBAR_STATE::TOGGLE )`: a toggle button (shown pressed while on) -- the shared parser keeps only `.Flags(...)`.
    const bodyRe = new RegExp(`\\.Name\\(\\s*"${a.name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"[\\s\\S]*?\\)\\s*;`);
    const body = bodyRe.exec(text)?.[0] ?? "";
    return { ...a, toggle: /TOOLBAR_STATE::TOGGLE/.test(body) };
  });
}

function main() {
  const { files: actionFiles, actions } = collectAllActions();
  const actions3d = parse3dActions();
  const all = [...actions, ...actions3d];
  const index = buildSymbolIndex(all);
  const iconByName = new Map(all.map((a) => [a.name, a.icon]));

  for (const target of TARGETS) {
    const controlLabels = buildControlLabels(["common/tool/action_toolbar.cpp", target.file]);
    const fullText = readKicadFile(target.file);
    // DefaultToolbarConfig ends at its own `return config;`; the code after it registers custom control factories and must not be scanned.
    const end = fullText.indexOf("return config;");
    // Comments are blanked (same length, so offsets hold): the symbol editor's source has `//    .AppendAction( SCH_ACTIONS::togglePinAltIcons );` commented out.
    const blank = (m) => m.replace(/[^\n]/g, " ");
    const text = (end === -1 ? fullText : fullText.slice(0, end)).replace(/\/\*[\s\S]*?\*\//g, blank).replace(/\/\/[^\n]*/g, blank);
    const { byRegion, unresolved } = extractToolbarItems(text, target, (sym) => index.resolve(sym), (name) => iconByName.get(name) ?? null, controlLabels);

    const toolbars = Object.entries(byRegion).map(([id, items]) => ({ id, orientation: ORIENTATION[id], items }));
    const totalItems = toolbars.reduce((n, t) => n + t.items.length, 0);
    const notes = [
      "group icons are a stand-in (first member's icon) -- TOOLBAR_GROUP_CONFIG has no .Icon() call",
      "TOOLBAR_LOC::TOP_AUX returns std::nullopt in source: this editor has no auxiliary toolbar",
      "control labels are the real source strings (ACTION_TOOLBAR_CONTROL's 2nd constructor argument) where the constructor is in common/tool/action_toolbar.cpp or the editor's own file, else the enumerator name",
    ];
    if (target.out === "viewer3d_toolbars.json") notes.push("LEFT, RIGHT and TOP_AUX return std::nullopt in source: the 3D viewer has only the top toolbar");
    if (totalItems === 0) notes.push("matched 0 items -- the region markers probably do not match this file's structure");
    if (unresolved.size > 0) notes.push(`${unresolved.size} referenced action symbol(s) did not resolve: ${[...unresolved].join(", ")}`);

    const file = { meta: meta([target.file, "common/tool/action_toolbar.cpp", ...actionFiles, ...(target.out === "viewer3d_toolbars.json" ? [ACTIONS_3D_FILE] : [])], notes.join("; ")), toolbars };
    if (target.out === "viewer3d_toolbars.json") {
      // The actions the 3D toolbar names, with what a button needs: label, tooltip, default hotkey, icon, toggle state.
      const used = new Set(toolbars.flatMap((t) => t.items.flatMap((i) => (i.type === "action" ? [i.action] : i.type === "group" ? i.items : []))));
      file.actions = actions3d
        .filter((a) => used.has(a.name))
        .map((a) => ({ name: a.name, label: a.label, tooltip: a.tooltip, hotkey: a.hotkey, icon: a.icon, toggle: a.toggle }));
      // The 3D actions of eda_3d_actions.cpp that carry an icon but are not on the toolbar (the View menu's face views, the model-attribute toggles): the
      // Appearance panel and the view presets use these icons.
      file.otherActions = actions3d
        .filter((a) => !used.has(a.name) && a.icon)
        .map((a) => ({ name: a.name, label: a.label, tooltip: a.tooltip, hotkey: a.hotkey, icon: a.icon, toggle: a.toggle }));
    }
    writeJson(join(OUT_DIR, target.out), file);
    for (const t of toolbars) console.log(`${target.out} ${t.id}: ${t.items.length} item(s), ${t.items.filter((i) => i.type === "group").length} group(s)`);
    if (unresolved.size > 0) console.warn(`${target.out}: ${unresolved.size} unresolved action symbol(s): ${[...unresolved].join(", ")}`);
  }
}

main();

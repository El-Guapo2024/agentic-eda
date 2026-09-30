#!/usr/bin/env node
// Extracts the PCB editor's menu bar structure into src/kicad/menus.json.
// Run from a terminal with git access to ~/ws/kicad-mirror, or with
// KICAD_SRC_DIR set to a plain checkout:
//
//   node tools/extract-menus.js
//
// menubar_pcb_editor.cpp builds each top-level menu (and each submenu)
// as its own local variable -- `ACTION_MENU* routeMenu = new ACTION_MENU(
// false, selTool );` -- then adds items with `routeMenu->Add(...)`/
// `routeMenu->AppendSeparator()`/`routeMenu->AppendSubMenu(subVar, _(
// "Label"))`. Two different attachment patterns matter here:
//   - a SUBMENU is consumed close to where it's built: right after its
//     own ->Add() calls, its parent does `->AppendSubMenu(thisVar, ...)`.
//   - every TOP-LEVEL menu is instead attached in one batch at the very
//     end of the function: `menuBar->Append(fileMenu, _("&File"));
//     menuBar->Append(editMenu, _("&Edit")); ...` one after another,
//     with nothing of substance between them.
// A first version of this script bounded every menu's span as
// "declaration to wherever it's consumed", which is right for submenus
// but wrong for top-level ones -- their consumption site sits at the far
// end of the file, so each top-level span swallowed everything in
// between (every other menu's construction) as if it were its own
// items. Fixed: a top-level menu's span is bounded by the *next
// top-level declaration* instead (both lists come from the same
// declaration order, confirmed to match the menuBar->Append order for
// this file); only submenus use the "bounded by consumption" rule.
//
// Submenus are attached two different ways in this file: some via
// `parent->AppendSubMenu( subVar, _("Label") )`, but *most* via
// `subVar->SetTitle( _("Label") ); ... parent->Add( subVar );` -- a bare
// variable, no `PCB_ACTIONS::`/`ACTIONS::` prefix, picked out by
// checking the name against the set of variables this script already
// found an ACTION_MENU declaration for. Both patterns are handled.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";
import { buildSymbolIndex } from "./lib/actionsParser.js";
import { collectAllActions } from "./extract-actions.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "menus.json");
const SOURCE_FILE = "pcbnew/menubar_pcb_editor.cpp";

const TOP_LEVEL_APPEND = /\w*[Mm]enu[Bb]ar\w*\s*->\s*Append\s*\(\s*(\w+)\s*,\s*_\(\s*"((?:[^"\\]|\\.)*)"/g;
const DECL_RE = /ACTION_MENU\*\s+(\w+)\s*=\s*new ACTION_MENU/g;

function cleanLabel(raw) {
  return raw.replace(/&/g, "").replace(/\\"/g, '"');
}

/** Every `ACTION_MENU* name = new ACTION_MENU(...)` declaration, in file order, with its own [start, end) span -- a top-level menu (in `topLevelNames`) is bounded by the next *top-level* declaration; a submenu is bounded by wherever it's actually consumed (`->AppendSubMenu(name,`), which sits close by. */
function findVarSpans(text, topLevelNames) {
  const decls = [];
  let d;
  DECL_RE.lastIndex = 0;
  while ((d = DECL_RE.exec(text))) decls.push({ name: d[1], at: d.index });

  const spans = new Map();
  for (let i = 0; i < decls.length; i++) {
    const { name, at } = decls[i];
    let end;
    if (topLevelNames.has(name)) {
      const nextTopLevel = decls.slice(i + 1).find((o) => topLevelNames.has(o.name));
      end = nextTopLevel ? nextTopLevel.at : text.length;
    } else {
      // End *before* the consumption call itself (not after it): this
      // span is walked recursively to get the submenu's own items, which
      // never includes its own attachment call anyway, and the parent's
      // scan needs to still see that call as a token once it skips past
      // this range -- ending after it would skip the attachment call
      // too, and the parent would never learn this submenu exists.
      const consumeRe = new RegExp(`->AppendSubMenu\\(\\s*${name}\\s*,|->\\s*Add\\(\\s*${name}\\s*\\)`);
      const m = consumeRe.exec(text.slice(at));
      const nextDecl = decls[i + 1];
      end = m ? at + m.index : (nextDecl?.at ?? text.length);
    }
    spans.set(name, { start: at, end });
  }
  return spans;
}

/** `varName->SetTitle( _("Label") )` calls, anywhere in the file -- the label for a submenu attached via the bare `parent->Add(subVar)` pattern (see header comment). */
function findTitles(text) {
  const titles = new Map();
  const re = /(\w+)->SetTitle\(\s*_\(\s*"((?:[^"\\]|\\.)*)"/g;
  let m;
  while ((m = re.exec(text))) titles.set(m[1], cleanLabel(m[2]));
  return titles;
}

/** Items directly inside `span`, recursing into any nested submenu span it contains (skipping that inner range for this level's own scan, then picking the submenu up again at its attachment call -- `->AppendSubMenu(var, "Label")` or bare `->Add(var)` -- just past it). */
function walkSpan(span, text, spans, topLevelNames, titles, resolve, unresolved) {
  const nested = [...spans.entries()]
    .filter(([name, s]) => !topLevelNames.has(name) && s.start > span.start && s.start < span.end)
    .map(([name, s]) => ({ name, start: s.start, end: s.end }))
    .sort((a, b) => a.start - b.start);

  const itemRe = /->\s*Add\(\s*(?:PCB_ACTIONS|ACTIONS)::(\w+)|->\s*AppendSeparator\s*\(|->\s*AppendSubMenu\(\s*(\w+)\s*,\s*_\(\s*"((?:[^"\\]|\\.)*)"|->\s*Add\(\s*(\w+)\s*\)/g;
  const body = text.slice(span.start, span.end);
  const items = [];
  let m;
  while ((m = itemRe.exec(body))) {
    const absPos = span.start + m.index;
    const inNested = nested.find((n) => absPos >= n.start && absPos < n.end);
    if (inNested) {
      itemRe.lastIndex = inNested.end - span.start; // this level's own scan skips the nested submenu's inner content entirely
      continue;
    }
    if (m[1]) {
      const dotted = resolve(m[1]);
      if (dotted) items.push({ type: "item", action: dotted });
      else unresolved.add(m[1]);
    } else if (m[2] !== undefined) {
      const subSpan = spans.get(m[2]);
      const subItems = subSpan ? walkSpan(subSpan, text, spans, topLevelNames, titles, resolve, unresolved) : [];
      items.push({ type: "submenu", label: cleanLabel(m[3]), items: subItems });
    } else if (m[4] !== undefined) {
      if (!spans.has(m[4])) continue; // a bare ->Add(x) of something that isn't a known ACTION_MENU var -- not a submenu, ignore
      const subSpan = spans.get(m[4]);
      const subItems = walkSpan(subSpan, text, spans, topLevelNames, titles, resolve, unresolved);
      items.push({ type: "submenu", label: titles.get(m[4]) ?? m[4], items: subItems });
    } else {
      items.push({ type: "separator" });
    }
  }
  return items;
}

function main() {
  const { files: actionFiles, actions } = collectAllActions();
  const index = buildSymbolIndex(actions);

  const text = readKicadFile(SOURCE_FILE);

  const topLevel = [];
  let tm;
  TOP_LEVEL_APPEND.lastIndex = 0;
  while ((tm = TOP_LEVEL_APPEND.exec(text))) topLevel.push({ varName: tm[1], label: cleanLabel(tm[2]) });
  const topLevelNames = new Set(topLevel.map((t) => t.varName));

  const spans = findVarSpans(text, topLevelNames);
  const titles = findTitles(text);
  const unresolved = new Set();
  const missingSpan = [];
  const menus = topLevel.map(({ varName, label }) => {
    const span = spans.get(varName);
    if (!span) {
      missingSpan.push(varName);
      return { label, items: [] };
    }
    return { label, items: walkSpan(span, text, spans, topLevelNames, titles, (sym) => index.resolve(sym), unresolved) };
  });

  const totalItems = menus.reduce((n, m) => n + m.items.length, 0);
  const notes = [];
  if (menus.length === 0) notes.push("matched 0 top-level menus -- TOP_LEVEL_APPEND doesn't match this file's real menu-bar Append() calls");
  if (missingSpan.length > 0) notes.push(`${missingSpan.length} menu variable(s) had no declaration found: ${missingSpan.join(", ")}`);
  if (totalItems === 0 && menus.length > 0) notes.push("top-level menus found but 0 items inside them -- check walkSpan()'s item regex against the real ->Add()/->AppendSeparator() call syntax");
  if (unresolved.size > 0) notes.push(`${unresolved.size} referenced action symbol(s) did not resolve: ${[...unresolved].slice(0, 20).join(", ")}`);

  writeJson(OUT, { meta: meta([SOURCE_FILE, ...actionFiles], notes.join("; ") || undefined), menus });
  for (const m of menus) console.log(`${m.label}: ${m.items.length} item(s)`);
  if (unresolved.size > 0) console.warn(`${unresolved.size} unresolved action symbol(s), see menus.json meta.note`);
}

main();

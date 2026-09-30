#!/usr/bin/env node
// Extracts the Schematic editor's menu bar structure into
// src/kicad/sch_menus.json. This is eeschema's equivalent of
// extract-menus.js (the PCB editor's version) -- see that script's
// header for the general span-bounding approach this is built on. Run
// from a terminal with git access to ~/ws/kicad-mirror, or with
// KICAD_SRC_DIR set to a plain checkout:
//
//   node tools/extract-sch-menus.js
//
// Confirmed by reading eeschema/menubar.cpp in full (363 lines, pinned
// commit 8303b2ada05226fa603e5ac0420d240fd65ce52b) -- not assumed to
// match pcbnew's file just because the two editors are siblings. The
// real file is eeschema/menubar.cpp; "menubar_sch_editor.cpp" (the name
// you'd guess from menubar_pcb_editor.cpp) does not exist.
//
//   - Same builder grammar as pcbnew's file: each top-level menu (and
//     each submenu) is its own local `ACTION_MENU* x = new ACTION_MENU(
//     false, selTool );`, filled with `x->Add(...)` / `x->AppendSeparator()`,
//     with every top-level menu attached in one batch at the end via
//     `menuBar->Append( fileMenu, _("&File") ); ...`. Same
//     top-level-vs-submenu span-bounding distinction from
//     extract-menus.js applies unchanged: a top-level span runs to the
//     *next top-level* declaration; a submenu span runs to wherever it's
//     actually consumed.
//
//   - Every submenu here (submenuImport, submenuExport, submenuAttributes,
//     showHidePanels, submenuVariants -- 5 total) uses ONLY the
//     `sub->SetTitle(_("Label")); ... parent->Add(sub);` bare-variable
//     pattern. Confirmed with `grep -n AppendSubMenu eeschema/menubar.cpp`
//     => 0 hits: unlike pcbnew's file (which uses both patterns), this
//     file never calls `->AppendSubMenu(var, _("Label"))`. The
//     AppendSubMenu alternative is kept in ITEM_RE below anyway, purely
//     to stay a close adaptation of extract-menus.js -- it will just
//     never match against this file.
//
//   - Real top-level menus (the `menuBar->Append(var, _("Label"))` calls,
//     in order): File, Edit, View, Place, Inspect, Tools, Preferences --
//     7 total. CONFIRMED DIFFERENT from pcbnew's 8 (File, Edit, View,
//     Place, Route, Inspect, Tools, Preferences): eeschema has no Route
//     menu (`grep -n routeMenu eeschema/menubar.cpp` => 0 hits; routing
//     is a PCB-only concept). Both files call `AddStandardHelpMenu(
//     menuBar )` directly as a plain function call right after their
//     explicit `menuBar->Append(...)` lines (eeschema: line 359, right
//     after `menuBar->Append( prefsMenu, _("P&references") );` on line
//     358; pcbnew: same pattern, line 476) -- never through the
//     `->Append(var, _("Label"))` shape TOP_LEVEL_APPEND matches, so Help
//     is excluded from both extractions identically. That is not a bug;
//     it mirrors menus.json's own existing 8-menu (no Help) output.
//
//   - Action references use both `ACTIONS::x` (shared, common/tool/
//     actions.cpp) and `SCH_ACTIONS::x` (eeschema-specific, eeschema/
//     tools/sch_actions.cpp -- same namespace extract-sch-toolbars.js
//     already confirmed). Zero `PCB_ACTIONS::` or `EE_ACTIONS::`
//     references in this file (`grep -n "PCB_ACTIONS\|EE_ACTIONS"
//     eeschema/menubar.cpp` => 0 hits).
//
//   - Same namespace-collision pitfall extract-sch-toolbars.js already
//     hit and fixed applies here, and this file actually exercises it:
//     `SCH_ACTIONS::drawRectangle` (line 261) and `SCH_ACTIONS::
//     generateBOM` (line 318) both name bare functions that ALSO exist,
//     with different `.Name()` strings, under `PCB_ACTIONS::`. Verified
//     directly against this repo's own buildSymbolIndex()/collectAllActions():
//       SCH_ACTIONS::drawRectangle -> eeschema.InteractiveDrawing.drawRectangle
//       PCB_ACTIONS::drawRectangle -> pcbnew.InteractiveDrawing.rectangle
//       SCH_ACTIONS::generateBOM   -> eeschema.EditorControl.generateBOM
//       PCB_ACTIONS::generateBOM  -> pcbnew.EditorControl.generateBOM
//     Resolving by bare func name alone (buildSymbolIndex()'s byFunc
//     fallback, first-wins, and pcbnew's own action file is read before
//     eeschema's is discovered) would silently mis-resolve both of these
//     to pcbnew's unrelated action. ITEM_RE below captures the qualifier
//     (SCH_ACTIONS|ACTIONS) alongside the func name, and resolution
//     always tries the full "QUALIFIER::func" symbol first, falling back
//     to the bare func name only as a last resort -- same fix
//     extract-sch-toolbars.js already applies, confirmed here to
//     resolve every symbol this file references correctly.
//
//   - Textual scan, same as extract-menus.js/extract-sch-toolbars.js:
//     neither this script nor those evaluate C++ conditionals, so items
//     gated behind one still appear as ordinary items in their normal
//     source position. Concrete cases in this file:
//       - File menu's `if( Kiface().IsSingle() ) fileMenu->Add( ACTIONS::
//         saveAs ); else fileMenu->Add( SCH_ACTIONS::saveCurrSheetCopyAs
//         );` (lines 90-93) -- BOTH branches appear as separate
//         sequential items, not just the one that would actually render.
//       - `#ifdef __APPLE__` gates a trailing separator in View (lines
//         230-232); `#ifdef KICAD_IPC_API` gates a trailing separator +
//         `ACTIONS::pluginsReload` in Tools (lines 332-335) -- both
//         included unconditionally.
//       - `if( ADVANCED_CFG::GetCfg().m_IncrementalConnectivity )` gates
//         `SCH_ACTIONS::showNetNavigator` inside the Panels submenu
//         (lines 183-184) -- included unconditionally.
//       - Several `if( !Kiface().IsSingle() )` / `->Enable(!Kiface()
//         .IsSingle())` guards in File/Tools (e.g. lines 295-296, 322) --
//         the item is captured regardless; only its later Enable() state
//         is conditional, which this schema doesn't track anyway.
//
//   - Three call shapes are NOT captured as menu items, matching this
//     script's (and extract-menus.js's) "unrecognized calls are silently
//     skipped" behavior -- MenuNode has no slot for any of these anyway:
//       - `fileMenu->AddQuitOrClose( &Kiface(), _("Schematic Editor") );`
//         (line 129) -- OS-specific Quit/Close item, a distinct helper
//         call, not `->Add(...)`.
//       - `AddMenuLanguageList( prefsMenu, selTool );` (line 347) -- a
//         dynamically populated language-list submenu; the call doesn't
//         match `parent->Add(...)` at all (parent is an argument here,
//         not the receiver).
//       - `fileMenu->Add( openRecentMenu->Clone() );` (line 79) -- the
//         dynamic "Open Recent" file-history submenu. `openRecentMenu`
//         is declared as `static ACTION_MENU* openRecentMenu;` (line 55)
//         *separately* from its `openRecentMenu = new ACTION_MENU(...)`
//         assignment (line 65), so it never matches DECL_RE's `ACTION_MENU*
//         name = new ACTION_MENU` shape and gets no span; and the call
//         itself is `->Add(x->Clone())`, not bare `->Add(x)`, so it
//         doesn't match the bare-submenu-add alternative either. No item
//         is emitted for it, which is correct -- its contents are
//         runtime file-history entries, not static actions.
//
// Output shape matches menus.json exactly (MenuNode/MenusFile in
// src/kicad/types.ts, unchanged): { meta, menus: [ { label, items } ] }.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";
import { buildSymbolIndex } from "./lib/actionsParser.js";
import { collectAllActions } from "./extract-actions.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "sch_menus.json");
const SOURCE_FILE = "eeschema/menubar.cpp";

const TOP_LEVEL_APPEND = /\w*[Mm]enu[Bb]ar\w*\s*->\s*Append\s*\(\s*(\w+)\s*,\s*_\(\s*"((?:[^"\\]|\\.)*)"/g;
const DECL_RE = /ACTION_MENU\*\s+(\w+)\s*=\s*new ACTION_MENU/g;

function cleanLabel(raw) {
  return raw.replace(/&/g, "").replace(/\\"/g, '"');
}

/** Every `ACTION_MENU* name = new ACTION_MENU(...)` declaration, in file order, with its own [start, end) span -- a top-level menu (in `topLevelNames`) is bounded by the next *top-level* declaration; a submenu is bounded by wherever it's actually consumed (`->AppendSubMenu(name,` or bare `->Add(name)`), which sits close by. Identical logic to extract-menus.js's findVarSpans(). */
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

/** `varName->SetTitle( _("Label") )` calls, anywhere in the file -- the label for a submenu attached via the bare `parent->Add(subVar)` pattern (see header comment). Every submenu in this file uses this pattern. */
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

  // Group 1: qualifier (SCH_ACTIONS|ACTIONS) -- captured, not discarded,
  // and resolved as the full "QUALIFIER::func" symbol FIRST (see header
  // comment's namespace-collision pitfall). Group 2: func name. Group 3/4:
  // ->AppendSubMenu(var, "Label") -- unused by this file (see header),
  // kept for structural parity with extract-menus.js. Group 5: bare
  // ->Add(var) submenu attachment.
  const itemRe =
    /->\s*Add\(\s*(SCH_ACTIONS|ACTIONS)::(\w+)|->\s*AppendSeparator\s*\(|->\s*AppendSubMenu\(\s*(\w+)\s*,\s*_\(\s*"((?:[^"\\]|\\.)*)"|->\s*Add\(\s*(\w+)\s*\)/g;
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
    if (m[2]) {
      const dotted = resolve(`${m[1]}::${m[2]}`) ?? resolve(m[2]);
      if (dotted) items.push({ type: "item", action: dotted });
      else unresolved.add(m[2]);
    } else if (m[3] !== undefined) {
      const subSpan = spans.get(m[3]);
      const subItems = subSpan ? walkSpan(subSpan, text, spans, topLevelNames, titles, resolve, unresolved) : [];
      items.push({ type: "submenu", label: cleanLabel(m[4]), items: subItems });
    } else if (m[5] !== undefined) {
      if (!spans.has(m[5])) continue; // a bare ->Add(x) of something that isn't a known ACTION_MENU var -- not a submenu, ignore
      const subSpan = spans.get(m[5]);
      const subItems = walkSpan(subSpan, text, spans, topLevelNames, titles, resolve, unresolved);
      items.push({ type: "submenu", label: titles.get(m[5]) ?? m[5], items: subItems });
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
  notes.push(
    "textual scan does not evaluate C++ conditionals (#ifdef/#if/runtime if-else) -- e.g. File menu includes both ACTIONS.saveAs and SCH_ACTIONS.saveCurrSheetCopyAs from an if/else, Tools includes ACTIONS.pluginsReload from #ifdef KICAD_IPC_API; see this script's header comment for the full list"
  );

  writeJson(OUT, { meta: meta([SOURCE_FILE, ...actionFiles], notes.join("; ") || undefined), menus });
  for (const m of menus) console.log(`${m.label}: ${m.items.length} item(s)`);
  if (unresolved.size > 0) console.warn(`${unresolved.size} unresolved action symbol(s), see sch_menus.json meta.note`);
}

main();

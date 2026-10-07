// The menu-bar builder grammar KiCad's `menubar*.cpp` files share, as one reusable extractor -- extract-menus.js (pcbnew) and
// extract-sch-menus.js (eeschema) each carry their own copy of this logic; the library editors' menus (extract-lib-editor-menus.js) use this one.
//
// Grammar (confirmed in pcbnew/menubar_footprint_editor.cpp and eeschema/symbol_editor/menubar_symbol_editor.cpp, same as the two files above):
//   - every top-level menu and every submenu is a local `ACTION_MENU* x = new ACTION_MENU( false, selTool );`, filled with `x->Add( QUALIFIER::action ...)`
//     and `x->AppendSeparator()`;
//   - a submenu is attached to its parent by the bare `parent->Add( sub )` (its label comes from `sub->SetTitle( _( "Label" ) )`) or by
//     `parent->AppendSubMenu( sub, _( "Label" ) )`;
//   - every top-level menu is attached in one batch at the end: `menuBar->Append( fileMenu, _( "&File" ) ); ...`, so a top-level menu's span runs to the
//     next top-level declaration, a submenu's to its attachment call.
// Not captured, as in the other two extractors (a `MenuNode` has no slot for them): `AddClose`, `AddMenuLanguageList`, `AddStandardHelpMenu`, an item added
// by id and label (`Add( _( "View as &PNG..." ), ..., ID_FPEDIT_SAVE_PNG, ...)`), and C++ conditionals around an `Add` (the textual scan takes both branches).

const TOP_LEVEL_APPEND = /\w*[Mm]enu[Bb]ar\w*\s*->\s*Append\s*\(\s*(\w+)\s*,\s*_\(\s*"((?:[^"\\]|\\.)*)"/g;
const DECL_RE = /ACTION_MENU\*\s+(\w+)\s*=\s*new ACTION_MENU/g;

export function cleanLabel(raw) {
  return raw.replace(/&/g, "").replace(/\\"/g, '"');
}

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
      const consumeRe = new RegExp(`->AppendSubMenu\\(\\s*${name}\\s*,|->\\s*Add\\(\\s*${name}\\s*\\)`);
      const m = consumeRe.exec(text.slice(at));
      const nextDecl = decls[i + 1];
      end = m ? at + m.index : (nextDecl?.at ?? text.length);
    }
    spans.set(name, { start: at, end });
  }
  return spans;
}

function findTitles(text) {
  const titles = new Map();
  const re = /(\w+)->SetTitle\(\s*_\(\s*"((?:[^"\\]|\\.)*)"/g;
  let m;
  while ((m = re.exec(text))) titles.set(m[1], cleanLabel(m[2]));
  return titles;
}

/** `qualifiers` are the action namespaces the file uses, e.g. ["PCB_ACTIONS", "ACTIONS"]. */
function walkSpan(span, text, spans, topLevelNames, titles, resolve, unresolved, itemRe) {
  const nested = [...spans.entries()]
    .filter(([name, s]) => !topLevelNames.has(name) && s.start > span.start && s.start < span.end)
    .map(([name, s]) => ({ name, start: s.start, end: s.end }))
    .sort((a, b) => a.start - b.start);

  const re = new RegExp(itemRe.source, "g");
  const body = text.slice(span.start, span.end);
  const items = [];
  let m;
  while ((m = re.exec(body))) {
    const absPos = span.start + m.index;
    const inNested = nested.find((n) => absPos >= n.start && absPos < n.end);
    if (inNested) {
      re.lastIndex = inNested.end - span.start;
      continue;
    }
    if (m[2]) {
      const dotted = resolve(`${m[1]}::${m[2]}`) ?? resolve(m[2]);
      if (dotted) items.push({ type: "item", action: dotted });
      else unresolved.add(`${m[1]}::${m[2]}`);
    } else if (m[3] !== undefined) {
      const subSpan = spans.get(m[3]);
      const subItems = subSpan ? walkSpan(subSpan, text, spans, topLevelNames, titles, resolve, unresolved, itemRe) : [];
      items.push({ type: "submenu", label: cleanLabel(m[4]), items: subItems });
    } else if (m[5] !== undefined) {
      if (!spans.has(m[5])) continue; // a bare ->Add(x) of something that is not a known ACTION_MENU variable
      const subItems = walkSpan(spans.get(m[5]), text, spans, topLevelNames, titles, resolve, unresolved, itemRe);
      items.push({ type: "submenu", label: titles.get(m[5]) ?? m[5], items: subItems });
    } else {
      items.push({ type: "separator" });
    }
  }
  return items;
}

/**
 * The menu bar a `menubar*.cpp` builds, as `{ menus: [{ label, items }], notes: [string] }` (the shape of `src/kicad/menus.json`).
 * `resolve("QUALIFIER::action")` -> dotted action name or null.
 */
export function extractMenuBar(text, qualifiers, resolve) {
  const topLevel = [];
  let tm;
  TOP_LEVEL_APPEND.lastIndex = 0;
  while ((tm = TOP_LEVEL_APPEND.exec(text))) topLevel.push({ varName: tm[1], label: cleanLabel(tm[2]) });
  const topLevelNames = new Set(topLevel.map((t) => t.varName));
  const spans = findVarSpans(text, topLevelNames);
  const titles = findTitles(text);
  const unresolved = new Set();
  const missingSpan = [];
  // Group 1/2: qualifier and action; 3/4: ->AppendSubMenu( var, "Label" ); 5: bare ->Add( var ); no group: ->AppendSeparator().
  const itemRe = new RegExp(`->\\s*Add\\(\\s*(${qualifiers.join("|")})::(\\w+)|->\\s*AppendSeparator\\s*\\(|->\\s*AppendSubMenu\\(\\s*(\\w+)\\s*,\\s*_\\(\\s*"((?:[^"\\\\]|\\\\.)*)"|->\\s*Add\\(\\s*(\\w+)\\s*\\)`, "g");
  const menus = topLevel.map(({ varName, label }) => {
    const span = spans.get(varName);
    if (!span) {
      missingSpan.push(varName);
      return { label, items: [] };
    }
    return { label, items: walkSpan(span, text, spans, topLevelNames, titles, resolve, unresolved, itemRe) };
  });
  const notes = [];
  if (menus.length === 0) notes.push("matched 0 top-level menus -- TOP_LEVEL_APPEND does not match this file's menu-bar Append() calls");
  if (missingSpan.length > 0) notes.push(`${missingSpan.length} menu variable(s) had no declaration found: ${missingSpan.join(", ")}`);
  if (unresolved.size > 0) notes.push(`${unresolved.size} referenced action symbol(s) did not resolve: ${[...unresolved].slice(0, 20).join(", ")}`);
  return { menus, notes };
}

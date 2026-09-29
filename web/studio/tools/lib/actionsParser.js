// Shared TOOL_ACTION_ARGS() parser, used by extract-actions.js directly
// and by extract-toolbars.js/extract-menus.js to resolve the C++ symbols
// those files reference (e.g. `PCB_ACTIONS::routeSingleTrack`) back to
// an action's dotted `name` ("pcbnew.InteractiveRouter.SingleTrack").
//
// See extract-actions.js's header comment for how confident this is.

/** First quoted string after `.Method(`, allowing an optional wrapping `_( ... )` translation macro. */
function extractString(body, methodName) {
  const re = new RegExp(`\\.${methodName}\\(\\s*_?\\(?\\s*"((?:[^"\\\\]|\\\\.)*)"`, "s");
  const m = re.exec(body);
  return m ? m[1].replace(/\\"/g, '"') : null;
}

/** Raw text of a `.Method( ... )` argument list, unparsed. */
function extractRaw(body, methodName) {
  const re = new RegExp(`\\.${methodName}\\(([^;]*?)\\)\\s*(?:\\.|$)`, "s");
  const m = re.exec(body);
  return m ? m[1].trim() : null;
}

const NAMED_KEYS = {
  WXK_DELETE: "Del",
  WXK_BACK: "Backspace",
  WXK_ESCAPE: "Esc",
  WXK_RETURN: "Enter",
  WXK_TAB: "Tab",
  WXK_SPACE: "Space",
  WXK_UP: "Up",
  WXK_DOWN: "Down",
  WXK_LEFT: "Left",
  WXK_RIGHT: "Right",
  WXK_HOME: "Home",
  WXK_END: "End",
  WXK_PLUS: "+",
  WXK_MINUS: "-",
  WXK_TILDE: "`",
};
for (let i = 1; i <= 12; i++) NAMED_KEYS[`WXK_F${i}`] = `F${i}`;

/** Best-effort "MD_CTRL + 'X'" / "WXK_DELETE" -> "Ctrl+X" / "Del". */
export function normalizeHotkey(raw) {
  if (!raw) return null;
  const s = raw.trim();
  if (/^0\s*$/.test(s) || /WXK_NONE/.test(s)) return null;
  const parts = [];
  if (/MD_CTRL/.test(s)) parts.push("Ctrl");
  if (/MD_ALT/.test(s)) parts.push("Alt");
  if (/MD_SHIFT/.test(s)) parts.push("Shift");
  let key = null;
  for (const [k, v] of Object.entries(NAMED_KEYS)) {
    if (s.includes(k)) {
      key = v;
      break;
    }
  }
  if (!key) {
    const charMatch = /'(.)'/.exec(s);
    if (charMatch) key = charMatch[1].toUpperCase();
  }
  if (!key) return null;
  parts.push(key);
  return parts.join("+");
}

/**
 * Every `TOOL_ACTION <Qualifier>::<func>( TOOL_ACTION_ARGS() ... );` in
 * `fileText`. `symbol` is "<Qualifier>::<func>" as toolbar/menu source
 * would reference it; `func` is the bare name, for files that reference
 * actions unqualified.
 */
export function parseActionsFromFile(fileText) {
  const actions = [];
  const re = /TOOL_ACTION\s+(\w+)::(\w+)\s*\(\s*TOOL_ACTION_ARGS\(\)([\s\S]*?)\)\s*;/g;
  let m;
  while ((m = re.exec(fileText))) {
    const [, qualifier, func, body] = m;
    const name = extractString(body, "Name");
    if (!name) continue;
    const hotkeyRaw = extractRaw(body, "DefaultHotkey");
    const altHotkeyRaw = extractRaw(body, "DefaultHotkeyAlt") || extractRaw(body, "AlternateHotkey");
    const flags = [];
    const flagsRaw = extractRaw(body, "Flags");
    if (flagsRaw)
      flags.push(
        ...flagsRaw
          .split("|")
          .map((s) => s.trim())
          .filter(Boolean)
      );
    actions.push({
      symbol: `${qualifier}::${func}`,
      func,
      name,
      label: extractString(body, "FriendlyName") ?? extractString(body, "MenuText") ?? name,
      tooltip: extractString(body, "Tooltip") ?? "",
      hotkey: normalizeHotkey(hotkeyRaw),
      altHotkey: normalizeHotkey(altHotkeyRaw),
      hotkeyRaw,
      altHotkeyRaw,
      icon: (/\.Icon\(\s*BITMAPS::(\w+)/s.exec(body) || [])[1] ?? null,
      flags,
    });
  }
  return actions;
}

/** symbol ("PCB_ACTIONS::foo") and bare func name ("foo") -> dotted action name, from a list of parseActionsFromFile() results (possibly concatenated across files). */
export function buildSymbolIndex(actionEntries) {
  const bySymbol = new Map();
  const byFunc = new Map();
  for (const a of actionEntries) {
    bySymbol.set(a.symbol, a.name);
    if (!byFunc.has(a.func)) byFunc.set(a.func, a.name); // first wins on ambiguity
  }
  return {
    resolve(symbolOrFunc) {
      return bySymbol.get(symbolOrFunc) ?? byFunc.get(symbolOrFunc) ?? null;
    },
  };
}

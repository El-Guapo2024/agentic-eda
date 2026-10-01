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

/**
 * Some actions (common/tool/actions.cpp: redo, doDelete, zoomIn, zoomOut,
 * zoomRedraw, zoomFitScreen) give `methodName` two different values behind
 * a `#if defined( __WXMAC__ ) ... #else ... #endif` -- a real macOS-only
 * default, not just KiCad's usual "Cmd stands in for Ctrl" substitution
 * (e.g. zoomIn is bare WXK_F1 on Windows/Linux but Cmd+'+' on macOS; a
 * plain single-value extractRaw() can't tell the two branches apart).
 *
 * The naive `extractRaw` regex is particularly dangerous here: its lazy
 * `[^;]*?` has no reason to stop at the *first* `.Method(...)` it meets --
 * since wxWidgets preprocessor lines (`#else`, `#endif`) aren't `.` or
 * end-of-string, the "stop here" lookahead fails inside the #if branch and
 * the match silently expands across both branches, often splicing a
 * fragment of each into one bogus hotkey (this is exactly how zoomIn and
 * zoomOut previously came out as "Ctrl+F1"/"Ctrl+F2": the mac branch's
 * MD_CTRL plus the non-mac branch's WXK_F1/WXK_F2, two different
 * platforms' tokens glued into one that's wrong on both).
 *
 * Returns `{ primary, mac }` -- `primary` is the `#else` (non-mac) value
 * when an ifdef is present (matching every other action's convention,
 * where `hotkey` already means "the Windows/Linux default, with Cmd
 * standing in for Ctrl on macOS"), `mac` is the `#if __WXMAC__` value, or
 * `{ primary: extractRaw(...), mac: null }` when there's no such split.
 */
function extractPlatformRaw(body, methodName) {
  const ifdefRe = new RegExp(
    `#if\\s+defined\\(\\s*__WXMAC__\\s*\\)\\s*` +
      `\\.${methodName}\\(([^)]*)\\)\\s*` +
      `#else\\s*` +
      `\\.${methodName}\\(([^)]*)\\)\\s*` +
      `#endif`,
    "s"
  );
  const m = ifdefRe.exec(body);
  if (m) return { primary: m[2].trim(), mac: m[1].trim() };
  return { primary: extractRaw(body, methodName), mac: null };
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
    // A literal char constant, e.g. `.DefaultHotkey( ' ' )` for
    // common.Control.resetLocalCoords (status-bar relative origin) --
    // KiCad writes this one as a bare space char rather than WXK_SPACE,
    // so it isn't caught by the NAMED_KEYS lookup above. Canonicalize it
    // the same way DOM_KEY_TO_CANONICAL (actions/hotkeys.ts) names a
    // literal " " keydown, or a real keypress for this action could
    // never match the stored hotkey string.
    const charMatch = /'(.)'/.exec(s);
    if (charMatch) key = charMatch[1] === " " ? "Space" : charMatch[1].toUpperCase();
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
    const hotkeySplit = extractPlatformRaw(body, "DefaultHotkey");
    const altHotkeySplit = /DefaultHotkeyAlt/.test(body) ? extractPlatformRaw(body, "DefaultHotkeyAlt") : extractPlatformRaw(body, "AlternateHotkey");
    const hotkeyRaw = hotkeySplit.primary;
    const altHotkeyRaw = altHotkeySplit.primary;
    const macHotkeyRaw = hotkeySplit.mac;
    const macAltHotkeyRaw = altHotkeySplit.mac;
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
      /** The real macOS-only default when it genuinely differs from a plain Cmd-for-Ctrl swap of `hotkey` (see extractPlatformRaw) -- null means macOS uses `hotkey` with Cmd standing in for Ctrl, same as every other action. */
      macHotkey: normalizeHotkey(macHotkeyRaw),
      macAltHotkey: normalizeHotkey(macAltHotkeyRaw),
      macHotkeyRaw,
      macAltHotkeyRaw,
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

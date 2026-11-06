// Normalizes a browser KeyboardEvent to the same "Ctrl+Shift+X" style
// string tools/lib/actionsParser.js's normalizeHotkey() produces for
// src/kicad/actions.json, so a live keydown can be looked up against
// extracted hotkey data directly. Cmd on macOS stands in for Ctrl, per
// the task's explicit hotkey-mapping instruction.
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import type { KicadAction } from "../kicad/types";
import { isMac } from "../platform";

const DOM_KEY_TO_CANONICAL: Record<string, string> = {
  Delete: "Del",
  Backspace: "Backspace",
  Escape: "Esc",
  Enter: "Enter",
  Tab: "Tab",
  " ": "Space",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Home: "Home",
  End: "End",
  "`": "`",
  "+": "+",
  "-": "-",
  Insert: "Insert",
  // KiCad's names (common/hotkeys_basic.cpp `hotkeyNameList`): keys a person can assign in Preferences > Hotkeys
  PageUp: "PgUp",
  PageDown: "PgDn",
};

/** The number pad's keys under KiCad's names ("Num Pad 5"), by `KeyboardEvent.code`: wx reports them apart from the main keys (`WXK_NUMPAD0` ..). */
const NUMPAD_BY_CODE: Record<string, string> = {
  ...Object.fromEntries(Array.from({ length: 10 }, (_, i) => [`Numpad${i}`, `Num Pad ${i}`])),
  NumpadAdd: "Num Pad +",
  NumpadSubtract: "Num Pad -",
  NumpadMultiply: "Num Pad *",
  NumpadDivide: "Num Pad /",
  NumpadEnter: "Num Pad Enter",
};

/** The unshifted character of the physical key `KeyboardEvent.code` names -- for hotkeys KiCad writes as Shift + that key ("Ctrl+Shift+."), where the browser instead reports the shifted character (">") in `key`. */
const UNSHIFTED_CHAR_BY_CODE: Record<string, string> = {
  Period: ".",
  Comma: ",",
  Slash: "/",
  Minus: "-",
  Equal: "=",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Backquote: "`",
};

/**
 * Every hotkey string a keydown could be spelled as in actions.json:
 * `eventToHotkey`'s own reading first, then -- only when Shift is down and the
 * typed key is a single symbol character -- two alternates, because the
 * browser reports a *shifted* symbol and KiCad's table spells such hotkeys
 * both ways:
 *   - the typed character already carries the Shift, so the explicit Shift
 *     is dropped ("Ctrl++" for Ctrl+Shift+= typed as "+", "Ctrl+<", "?");
 *   - explicit Shift plus the physical key's unshifted character, from
 *     `e.code` ("Ctrl+Shift+." for Ctrl+Shift+> ).
 * Letters/digits/named keys never produce alternates.
 */
export function eventToHotkeyCandidates(e: KeyboardEvent | ReactKeyboardEvent, isMacPlatform: boolean = isMac()): string[] {
  const primary = eventToHotkey(e, isMacPlatform);
  if (!primary) return [];
  const out = [primary];
  const ctrlLike = isMacPlatform ? e.metaKey : e.ctrlKey;
  const prefix = (withShift: boolean) => `${ctrlLike ? "Ctrl+" : ""}${e.altKey ? "Alt+" : ""}${withShift ? "Shift+" : ""}`;
  // a number pad key also under its own name, the key the Set Hotkey prompt records for it (`MapKeypressToKeycode` of `WXK_NUMPAD*`, Shift kept: a code above 256)
  const pad = NUMPAD_BY_CODE[(e as KeyboardEvent).code];
  if (pad) out.push(`${prefix(e.shiftKey)}${pad}`);
  if (!e.shiftKey || e.key.length !== 1 || /[A-Za-z0-9]/.test(e.key)) return out;
  out.push(`${prefix(false)}${e.key}`);
  const unshifted = UNSHIFTED_CHAR_BY_CODE[(e as KeyboardEvent).code];
  if (unshifted) out.push(`${prefix(true)}${unshifted}`);
  return out;
}

/**
 * e.g. `{ctrlKey:true, shiftKey:true, key:"z"}` -> "Ctrl+Shift+Z" (or with
 * meta on macOS standing in for Ctrl). `isMacPlatform` defaults to the
 * real `isMac()`; overridable so tests can exercise both platforms
 * without a `navigator` global to mock.
 */
export function eventToHotkey(e: KeyboardEvent | ReactKeyboardEvent, isMacPlatform: boolean = isMac()): string | null {
  // A bare modifier keypress isn't a hotkey combo by itself.
  if (["Control", "Meta", "Alt", "Shift"].includes(e.key)) return null;

  const parts: string[] = [];
  const ctrlLike = isMacPlatform ? e.metaKey : e.ctrlKey;
  if (ctrlLike) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");

  let key = DOM_KEY_TO_CANONICAL[e.key];
  if (!key) {
    if (/^F\d{1,2}$/.test(e.key)) key = e.key;
    else if (e.key.length === 1) key = e.key.toUpperCase();
    else return null; // an unmapped multi-char key name (Shift, CapsLock, ...) -- not a hotkey we can express
  }
  parts.push(key);
  return parts.join("+");
}

/**
 * Default hotkeys the extracted table (src/kicad/actions.json) has wrong, by action name, read again from the C++ (KiCad 8303b2ad unless noted): the extractor
 * reads one arm of an `#ifdef __WXMAC__ .. #else .. #endif` around `.DefaultHotkey(..)` (or glues the two together), and the table predates KiCad 10's actions.
 *   - pcbnew.PointEditor.addCorner ("Create Corner", pcbnew/tools/pcb_actions.cpp) and eeschema.InteractiveEdit.repeatDrawItem ("Repeat Last Item",
 *     eeschema/tools/sch_actions.cpp): `#ifdef __WXMAC__ WXK_F1 #else WXK_INSERT` -- Insert, and F1 on macOS (off macOS F1 is Zoom In's).
 *   - pcbnew.InteractiveDrawing.zone ("Draw Filled Zones"): `#ifdef __WXOSX_MAC__ MD_ALT + 'Z' #else MD_CTRL + MD_SHIFT + 'Z'` (the table glued them into
 *     Ctrl+Alt+Shift+Z).
 *   - common.InteractiveDrawing.cycleArcDrawMode ("Cycle Arc Drawing Mode", KiCad 25ec2335's common/tool/actions.cpp): `MD_ALT + '/'` (src/kicad/kicad10Actions.ts).
 * KiCad 8303b2ad gives Group Items (common.Interactive.group) no key: Ctrl+G is Search's (`ACTIONS::showSearch`). The dispatcher used to bind Ctrl+G to Group as
 * well; it no longer does (a person can give Group a key in Preferences > Hotkeys).
 */
const DEFAULT_HOTKEY_FIXES: Readonly<Record<string, { hotkey: string; macHotkey: string }>> = {
  "pcbnew.PointEditor.addCorner": { hotkey: "Insert", macHotkey: "F1" },
  "eeschema.InteractiveEdit.repeatDrawItem": { hotkey: "Insert", macHotkey: "F1" },
  "pcbnew.InteractiveDrawing.zone": { hotkey: "Ctrl+Shift+Z", macHotkey: "Alt+Z" },
  "common.InteractiveDrawing.cycleArcDrawMode": { hotkey: "Alt+/", macHotkey: "Alt+/" },
};

type HotkeyFields = Pick<KicadAction, "hotkey" | "altHotkey" | "macHotkey" | "macAltHotkey"> & { name?: string };

/**
 * The default hotkey/altHotkey of `action` on the current platform (`TOOL_ACTION::GetDefaultHotKey` / `GetDefaultHotKeyAlt`). Almost always just `{hotkey,
 * altHotkey}` as extracted (macOS gets there via eventToHotkey/displayHotkey's own Cmd-for-Ctrl substitution) -- but a handful of actions (redo, delete, the
 * F1/F2/F5/Home zoom actions) have a macOS default in KiCad's source that isn't just that substitution, e.g. zoomIn is bare F1 on Windows/Linux but Cmd+'+' on
 * macOS (see tools/lib/actionsParser.js's extractPlatformRaw and src/kicad/types.ts's KicadAction.macHotkey doc), and a few the table has wrong
 * (`DEFAULT_HOTKEY_FIXES`).
 */
export function defaultHotkey(action: HotkeyFields, isMacPlatform: boolean = isMac()): { hotkey: string | null; altHotkey: string | null } {
  const fix = action.name ? DEFAULT_HOTKEY_FIXES[action.name] : undefined;
  if (fix) return { hotkey: isMacPlatform ? fix.macHotkey : fix.hotkey, altHotkey: isMacPlatform ? (action.macAltHotkey ?? action.altHotkey) : action.altHotkey };
  if (!isMacPlatform) return { hotkey: action.hotkey, altHotkey: action.altHotkey };
  return { hotkey: action.macHotkey ?? action.hotkey, altHotkey: action.macAltHotkey ?? action.altHotkey };
}

/**
 * The person's hotkeys (Preferences > Hotkeys, `user.hotkeys`; state/userHotkeys.ts keeps them and sets them here): by action name, the hotkey and the alternate
 * as studio strings (null: none). `ReadHotKeyConfigIntoActions` sets an action's hotkeys to the file's when the file names it.
 */
let userHotkeys: ReadonlyMap<string, { hotkey: string | null; altHotkey: string | null }> | null = null;

/** Sets the person's hotkeys (null: none, every action has its default). */
export function setUserHotkeyOverrides(map: ReadonlyMap<string, { hotkey: string | null; altHotkey: string | null }> | null): void {
  userHotkeys = map;
}

/**
 * The hotkey/altHotkey this app should actually look up and display for `action` on the current platform (`TOOL_ACTION::GetHotKey` / `GetHotKeyAlt`): the
 * person's own (`setUserHotkeyOverrides`) when they set the action's, else the default (`defaultHotkey`). Centralized here so every reader (the global hotkey
 * dispatcher, menu/toolbar/hotkeys-list display) resolves it the same way instead of reading `.hotkey` raw.
 *
 * `isMacPlatform` defaults to the real `isMac()` at call time; it's a parameter (rather than reading `isMac()` directly in the body) so unit tests can exercise
 * both platforms without a `navigator` global to mock.
 */
export function effectiveHotkey(action: HotkeyFields, isMacPlatform: boolean = isMac()): { hotkey: string | null; altHotkey: string | null } {
  const own = action.name ? userHotkeys?.get(action.name) : undefined;
  if (own) return { hotkey: own.hotkey, altHotkey: own.altHotkey };
  return defaultHotkey(action, isMacPlatform);
}

/** Display form for a menu/toolbar tooltip: Ctrl -> the platform symbol on macOS. `isMacPlatform` defaults to the real `isMac()`; overridable for tests. */
export function displayHotkey(hk: string | null, isMacPlatform: boolean = isMac()): string {
  if (!hk) return "";
  if (!isMacPlatform) return hk;
  // Modifiers are always a fixed-order prefix -- "Ctrl+", then "Alt+",
  // then "Shift+", each optional (eventToHotkey/normalizeHotkey only
  // ever `parts.push()` in that order) -- followed by exactly one key
  // token. Matching that prefix in one anchored regex and dropping its
  // own "+" separators, rather than three independent word replacements
  // plus a separate "strip stray +" pass, can't misfire when the key
  // token itself is "+" or "-" (e.g. zoomIn's real macOS default,
  // "Ctrl++", must display as "⌘+" -- the modifier's separator drops but
  // the literal plus-key survives; a blind "+" stripper either leaves a
  // stray "⌘⇧+Z"-style separator before an ordinary key, or swallows a
  // literal "+"/"-" key along with it).
  return hk.replace(/^(Ctrl\+)?(Alt\+)?(Shift\+)?/, (_m, ctrl, alt, shift) => `${ctrl ? "⌘" : ""}${alt ? "⌥" : ""}${shift ? "⇧" : ""}`);
}

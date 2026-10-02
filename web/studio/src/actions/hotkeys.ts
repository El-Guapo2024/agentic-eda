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
  if (!e.shiftKey || e.key.length !== 1 || /[A-Za-z0-9]/.test(e.key)) return out;
  const ctrlLike = isMacPlatform ? e.metaKey : e.ctrlKey;
  const prefix = (withShift: boolean) => `${ctrlLike ? "Ctrl+" : ""}${e.altKey ? "Alt+" : ""}${withShift ? "Shift+" : ""}`;
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
 * The hotkey/altHotkey this app should actually look up and display for
 * `action` on the current platform. Almost always just `{hotkey,
 * altHotkey}` as extracted (macOS gets there via eventToHotkey/
 * displayHotkey's own Cmd-for-Ctrl substitution) -- but a handful of
 * actions (redo, delete, the F1/F2/F5/Home zoom actions) have a macOS
 * default in KiCad's source that isn't just that substitution, e.g.
 * zoomIn is bare F1 on Windows/Linux but Cmd+'+' on macOS (see
 * tools/lib/actionsParser.js's extractPlatformRaw and
 * src/kicad/types.ts's KicadAction.macHotkey doc). Centralized here so
 * every reader (the global hotkey dispatcher, menu/toolbar/hotkeys-list
 * display) resolves it the same way instead of reading `.hotkey` raw.
 *
 * `isMacPlatform` defaults to the real `isMac()` at call time; it's a
 * parameter (rather than reading `isMac()` directly in the body) so unit
 * tests can exercise both platforms without a `navigator` global to mock.
 */
export function effectiveHotkey(
  action: Pick<KicadAction, "hotkey" | "altHotkey" | "macHotkey" | "macAltHotkey">,
  isMacPlatform: boolean = isMac()
): { hotkey: string | null; altHotkey: string | null } {
  if (!isMacPlatform) return { hotkey: action.hotkey, altHotkey: action.altHotkey };
  return { hotkey: action.macHotkey ?? action.hotkey, altHotkey: action.macAltHotkey ?? action.altHotkey };
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

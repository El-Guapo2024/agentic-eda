// Normalizes a browser KeyboardEvent to the same "Ctrl+Shift+X" style
// string tools/lib/actionsParser.js's normalizeHotkey() produces for
// src/kicad/actions.json, so a live keydown can be looked up against
// extracted hotkey data directly. Cmd on macOS stands in for Ctrl, per
// the task's explicit hotkey-mapping instruction.
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
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
};

/** e.g. `{ctrlKey:true, shiftKey:true, key:"z"}` -> "Ctrl+Shift+Z" (or with meta on macOS standing in for Ctrl). */
export function eventToHotkey(e: KeyboardEvent | ReactKeyboardEvent): string | null {
  // A bare modifier keypress isn't a hotkey combo by itself.
  if (["Control", "Meta", "Alt", "Shift"].includes(e.key)) return null;

  const parts: string[] = [];
  const ctrlLike = isMac() ? e.metaKey : e.ctrlKey;
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

/** Display form for a menu/toolbar tooltip: Ctrl -> the platform symbol on macOS. */
export function displayHotkey(hk: string | null): string {
  if (!hk) return "";
  if (!isMac()) return hk;
  const withSymbols = hk.replace(/\bCtrl\b/, "⌘").replace(/\bAlt\b/, "⌥").replace(/\bShift\b/, "⇧");
  // Once symbols replace the modifier words, the "+" separators between
  // them read better dropped (macOS convention: "⌘⇧Z", not "⌘+⇧+Z").
  return withSymbols.replace(/\+(?=[⌘⌥⇧])/g, "").replace(/\+$/, "");
}

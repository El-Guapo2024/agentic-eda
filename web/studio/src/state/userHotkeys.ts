// The person's hotkeys: KiCad's `user.hotkeys` (common/hotkeys_basic.cpp `WriteHotKeyConfig` writes it into the user's settings folder, `ReadHotKeyConfig`
// reads it back into every action), kept here as that file's text in the browser's local storage -- per person, as the colour themes are
// (state/colorThemeStore.ts), never in a project, no undo step. Preferences > Hotkeys (components/HotkeysPage.tsx) writes it on OK; actions/hotkeys.ts'
// `effectiveHotkey` reads the keys it names (`ReadHotKeyConfigIntoActions`), so the dispatcher, the menus, the toolbars and List Hotkeys use them.
import { useSyncExternalStore } from "react";
import { setUserHotkeyOverrides } from "../actions/hotkeys";
import { keyCodeToStudioHotkey, readHotKeyConfig, writeHotKeyConfig, type HotkeyConfig } from "../kicad-port/hotkeyStore";
import { isMac } from "../platform";

export const USER_HOTKEYS_STORAGE_KEY = "eda-studio.hotkeys.v1";

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

let text: string | null = (() => {
  try {
    return storage()?.getItem(USER_HOTKEYS_STORAGE_KEY) ?? null;
  } catch {
    return null;
  }
})();
let version = 0;
const listeners = new Set<() => void>();

/** `ReadHotKeyConfigIntoActions`: the file's keys by action name, as the studio's hotkey strings. */
function apply(): void {
  if (text === null) {
    setUserHotkeyOverrides(null);
    return;
  }
  const map = new Map<string, { hotkey: string | null; altHotkey: string | null }>();
  for (const [name, [k, alt]] of readHotKeyConfig(text, isMac())) map.set(name, { hotkey: keyCodeToStudioHotkey(k), altHotkey: keyCodeToStudioHotkey(alt) });
  setUserHotkeyOverrides(map);
}
apply();

/** The `user.hotkeys` text, or null while the person has none (every action has its default). */
export function getUserHotkeysText(): string | null {
  return text;
}

/** The file as `ReadHotKeyConfig` reads it (empty while there is none). */
export function getUserHotkeyConfig(): HotkeyConfig {
  return text === null ? new Map() : readHotKeyConfig(text, isMac());
}

/** `WriteHotKeyConfig( aActions )`: the file's hotkeys with these actions' laid over them, saved. */
export function writeUserHotkeys(actions: ReadonlyArray<{ name: string; key: number; keyAlt: number }>): void {
  setUserHotkeysText(writeHotKeyConfig(getUserHotkeyConfig(), actions, isMac()));
}

/** Replaces the file (null removes it). */
export function setUserHotkeysText(next: string | null): void {
  if (next === text) return;
  text = next;
  try {
    if (next === null) storage()?.removeItem(USER_HOTKEYS_STORAGE_KEY);
    else storage()?.setItem(USER_HOTKEYS_STORAGE_KEY, next);
  } catch {
    /* private window or quota: the hotkeys still apply for this session */
  }
  apply();
  version++;
  for (const l of [...listeners]) l();
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

const getVersion = () => version;

/** A number that changes whenever the person's hotkeys do (for the dispatcher's index and the menus' hotkey labels). */
export function useUserHotkeysVersion(): number {
  return useSyncExternalStore(subscribe, getVersion, getVersion);
}

/** For the tests and the scripted hook: forget the person's hotkeys. */
export function resetUserHotkeysForTest(): void {
  setUserHotkeysText(null);
}

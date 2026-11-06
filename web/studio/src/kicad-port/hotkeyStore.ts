// The hotkey editor's rules, ported from KiCad 8303b2ad:
//   * common/hotkeys_basic.cpp: the key names (`hotkeyNameList`, `KeyNameFromKeyCode`, `KeyCodeFromKeyName`) and the `user.hotkeys` file (`ReadHotKeyConfig`,
//     `WriteHotKeyConfig`): one line per action, "<action name>\t<hotkey>\t<alternate>", written in the order of a `std::map` (the names' byte order);
//   * common/hotkey_store.cpp: the list's sections (`HOTKEY_STORE::Init`, `GetAppName`, `GetSectionName`, the pseudo actions of the platform commands and the
//     gestures), `SaveAllHotkeys`, `ResetAllHotkeysToDefault` / `ResetAllHotkeysToOriginal` and `CheckKeyConflicts`;
//   * common/widgets/widget_hotkey_list.cpp: what a row shows (`updateFromClientData`), the filter (`HOTKEY_FILTER`), `changeHotkey`, `resetItem` (the context
//     menu's Undo Changes, Clear Assigned Hotkey, Clear Assigned Alternate, Restore Defaults), `resolveKeyConflicts`' question, the reserved keys and
//     `MapKeypressToKeycode` (what a key press in the Set Hotkey prompt is).
//
// Keys are KiCad's key codes: a character or a `wxKeyCode` (wxWidgets' include/wx/defs.h values) plus the `MD_*` modifier bits of include/tool/tool_event.h. The
// studio's own hotkey strings (actions/hotkeys.ts: "Ctrl+Shift+X", "Del", "Backspace", "Enter", "Insert") are KiCad's names off macOS with three keys spelled
// otherwise (Back, Return, Ins); `studioHotkeyToKeyCode` / `keyCodeToStudioHotkey` go between the two.
//
// Pure: no store, no DOM beyond the KeyboardEvent's fields. Unit-tested in hotkeyStore.test.ts.

/** `MD_SHIFT` .. `MD_ALTGR` (include/tool/tool_event.h). */
export const MD_SHIFT = 0x1000;
export const MD_CTRL = 0x2000;
export const MD_ALT = 0x4000;
export const MD_SUPER = 0x8000;
export const MD_META = 0x10000;
export const MD_ALTGR = 0x20000;
export const MD_MODIFIER_MASK = MD_SHIFT | MD_CTRL | MD_ALT | MD_SUPER | MD_META | MD_ALTGR;

/** `KEY_NON_FOUND`: what `KeyCodeFromKeyName` gives for a name it does not know. */
export const KEY_NON_FOUND = -1;

/** The `wxKeyCode`s the names stand for. */
export const WXK = {
  BACK: 8,
  TAB: 9,
  RETURN: 13,
  ESCAPE: 27,
  SPACE: 32,
  DELETE: 127,
  SHIFT: 306,
  ALT: 307,
  CONTROL: 308,
  CAPITAL: 311,
  END: 312,
  HOME: 313,
  LEFT: 314,
  UP: 315,
  RIGHT: 316,
  DOWN: 317,
  INSERT: 322,
  NUMPAD0: 324,
  F1: 340,
  NUMLOCK: 364,
  SCROLL: 365,
  PAGEUP: 366,
  PAGEDOWN: 367,
  NUMPAD_ENTER: 370,
  NUMPAD_F1: 371,
  NUMPAD_MULTIPLY: 387,
  NUMPAD_ADD: 388,
  NUMPAD_SEPARATOR: 389,
  NUMPAD_SUBTRACT: 390,
  NUMPAD_DIVIDE: 392,
  /** `PSEUDO_WXK_CLICK`, `PSEUDO_WXK_DBLCLICK`, `PSEUDO_WXK_WHEEL` (include/hotkeys_basic.h): the gestures' "keys". */
  PSEUDO_CLICK: 400,
  PSEUDO_DBLCLICK: 401,
  PSEUDO_WHEEL: 402,
} as const;

/** `hotkeyNameList`, in its order (the first entry of a code names it; the first of a name, compared without case, reads it). */
const HOTKEY_NAME_LIST: ReadonlyArray<readonly [string, number]> = [
  ...Array.from({ length: 24 }, (_, i) => [`F${i + 1}`, WXK.F1 + i] as const),
  ["Esc", WXK.ESCAPE],
  ["Del", WXK.DELETE],
  ["Tab", WXK.TAB],
  ["Back", WXK.BACK],
  ["Ins", WXK.INSERT],
  ["Home", WXK.HOME],
  ["End", WXK.END],
  ["PgUp", WXK.PAGEUP],
  ["PgDn", WXK.PAGEDOWN],
  ["Up", WXK.UP],
  ["Down", WXK.DOWN],
  ["Left", WXK.LEFT],
  ["Right", WXK.RIGHT],
  ["Return", WXK.RETURN],
  ["Space", WXK.SPACE],
  ...Array.from({ length: 10 }, (_, i) => [`Num Pad ${i}`, WXK.NUMPAD0 + i] as const),
  ["Num Pad +", WXK.NUMPAD_ADD],
  ["Num Pad -", WXK.NUMPAD_SUBTRACT],
  ["Num Pad *", WXK.NUMPAD_MULTIPLY],
  ["Num Pad /", WXK.NUMPAD_DIVIDE],
  ["Num Pad .", WXK.NUMPAD_SEPARATOR],
  ["Num Pad Enter", WXK.NUMPAD_ENTER],
  ["Num Pad F1", WXK.NUMPAD_F1],
  ["Num Pad F2", WXK.NUMPAD_F1 + 1],
  ["Num Pad F3", WXK.NUMPAD_F1 + 2],
  ["Num Pad F4", WXK.NUMPAD_F1 + 3],
  ["", 0],
  ["Click", WXK.PSEUDO_CLICK],
  ["DblClick", WXK.PSEUDO_DBLCLICK],
  ["Wheel", WXK.PSEUDO_WHEEL],
];

/** The modifier names (`MODIFIER_CTRL` .. `MODIFIER_ALTGR`): on macOS `USING_MAC_CMD` writes Ctrl as "Cmd+" and Alt as "Option+". */
function modifierNames(mac: boolean) {
  return { ctrl: mac ? "Cmd+" : "Ctrl+", alt: mac ? "Option+" : "Alt+" };
}

/**
 * `KeyNameFromKeyCode`: the modifiers (Ctrl, Alt, Shift, Meta, Win, AltGr, in that order), then the key -- a printable character as itself, a named key by
 * `hotkeyNameList`, else "<unknown>" (`found` false). 0 is "" (unassigned). The bare modifier keys are named alone.
 */
export function keyNameFromKeyCode(code: number, mac: boolean): { name: string; found: boolean } {
  const m = modifierNames(mac);
  if (code === WXK.CONTROL) return { name: m.ctrl.slice(0, -1), found: false };
  if (code === WXK.SHIFT) return { name: "Shift", found: false };
  if (code === WXK.ALT) return { name: m.alt.slice(0, -1), found: false };
  let modifier = "";
  if (code & MD_CTRL) modifier += m.ctrl;
  if (code & MD_ALT) modifier += m.alt;
  if (code & MD_SHIFT) modifier += "Shift+";
  if (code & MD_META) modifier += "Meta+";
  if (code & MD_SUPER) modifier += "Win+";
  if (code & MD_ALTGR) modifier += "AltGr+";
  const key = code & ~MD_MODIFIER_MASK;
  if (key > 0x20 && key < 0x7f) return { name: modifier + String.fromCharCode(key), found: true };
  const hit = HOTKEY_NAME_LIST.find(([, c]) => c === key);
  return hit ? { name: modifier + hit[0], found: true } : { name: modifier + "<unknown>", found: false };
}

/** `KeyNameFromKeyCode` without the found flag. */
export function keyName(code: number, mac: boolean): string {
  return keyNameFromKeyCode(code, mac).name;
}

/**
 * `KeyCodeFromKeyName`: the modifier prefixes ("Ctrl+" and "Cmd+" are Ctrl on every platform; Alt is the platform's "Alt+" or "Option+"; "Shift+", "Meta+",
 * "Win+" and "Super+", "AltGr+"; case-sensitive), then one printable character, or a `hotkeyNameList` name compared without case. `KEY_NON_FOUND` otherwise.
 */
export function keyCodeFromKeyName(name: string, mac: boolean): number {
  const alt = modifierNames(mac).alt;
  const prefixes: ReadonlyArray<readonly [string, number]> = [
    ["Ctrl+", MD_CTRL],
    ["Cmd+", MD_CTRL],
    [alt, MD_ALT],
    ["Shift+", MD_SHIFT],
    ["Meta+", MD_META],
    ["Win+", MD_SUPER],
    ["Super+", MD_SUPER],
    ["AltGr+", MD_ALTGR],
  ];
  let key = name;
  let modifier = 0;
  for (;;) {
    const p = prefixes.find(([text]) => key.startsWith(text));
    if (!p) break;
    modifier |= p[1];
    key = key.slice(p[0].length);
  }
  if (key.length === 1 && key.charCodeAt(0) > 0x20 && key.charCodeAt(0) < 0x7f) return key.charCodeAt(0) + modifier;
  const lower = key.toLowerCase();
  const hit = HOTKEY_NAME_LIST.find(([n]) => n.toLowerCase() === lower);
  return hit ? hit[1] + modifier : KEY_NON_FOUND;
}

/** The three keys the studio's hotkey strings name otherwise than KiCad does. */
const STUDIO_TO_KICAD: Readonly<Record<string, string>> = { Backspace: "Back", Insert: "Ins", Enter: "Return" };
const KICAD_TO_STUDIO: Readonly<Record<string, string>> = { Back: "Backspace", Ins: "Insert", Return: "Enter" };
const MODIFIER_PREFIX = /^(?:(?:Ctrl|Alt|Shift|Meta|Win|AltGr)\+)*/;

/** A studio hotkey string ("Ctrl+Shift+Z", "Del", "Ctrl++") as KiCad's key code; null and "" are 0 (unassigned). */
export function studioHotkeyToKeyCode(hotkey: string | null): number {
  if (!hotkey) return 0;
  // "Ctrl++": the prefix is "Ctrl+" and the key "+"
  const prefix = MODIFIER_PREFIX.exec(hotkey)![0];
  const key = hotkey.slice(prefix.length);
  return keyCodeFromKeyName(prefix + (STUDIO_TO_KICAD[key] ?? key), false);
}

/** KiCad's key code as the studio's hotkey string (KiCad's name off macOS, Back / Ins / Return spelled Backspace / Insert / Enter); null for 0 or an unknown key. */
export function keyCodeToStudioHotkey(code: number): string | null {
  if (code <= 0) return null;
  const { name, found } = keyNameFromKeyCode(code, false);
  if (!found || name === "") return null;
  const prefix = MODIFIER_PREFIX.exec(name)![0];
  const key = name.slice(prefix.length);
  return prefix + (KICAD_TO_STUDIO[key] ?? key);
}

// ---------------------------------------------------------------------------------------------- user.hotkeys

/** What `ReadHotKeyConfig` returns: by action name, the hotkey and the alternate (key codes; 0 unassigned, `KEY_NON_FOUND` a name it did not know). */
export type HotkeyConfig = Map<string, readonly [number, number]>;

/** `wxStringTokenizer( .., wxTOKEN_STRTOK )`: the pieces between the delimiters, empty ones dropped. */
function strtok(text: string, delimiter: string): string[] {
  return text.split(delimiter).filter((t) => t !== "");
}

/**
 * `ReadHotKeyConfig`: "\r\n" made "\n"; each line is "<name>\t<hotkey>\t<alternate>". The fields are read with `wxTOKEN_STRTOK` (the default for a whitespace
 * delimiter), which drops empty fields: a line with no hotkey and an alternate ("name\t\tCtrl+X", as `WriteHotKeyConfig` writes it) reads the alternate as the
 * hotkey and no alternate -- KiCad's own round trip, kept here. A later line of the same name wins (`aHotKeys[cmdName] = ...`).
 */
export function readHotKeyConfig(text: string, mac: boolean): HotkeyConfig {
  const out: HotkeyConfig = new Map();
  for (const line of strtok(text.replace(/\r\n/g, "\n"), "\n")) {
    const [cmdName = "", primary = "", secondary = ""] = strtok(line, "\t");
    if (cmdName !== "") out.set(cmdName, [keyCodeFromKeyName(primary, mac), keyCodeFromKeyName(secondary, mac)]);
  }
  return out;
}

/**
 * `WriteHotKeyConfig`: the existing file's hotkeys with `actions`' current ones laid over them, every entry written in the map's order (`std::map<std::string,
 * ..>`: the names' byte order) as "<name>\t<hotkey>\t<alternate>" with Unix line ends, an unassigned key as "".
 */
export function writeHotKeyConfig(existing: HotkeyConfig, actions: ReadonlyArray<{ name: string; key: number; keyAlt: number }>, mac: boolean): string {
  const all = new Map(existing);
  for (const a of actions) all.set(a.name, [a.key, a.keyAlt]);
  const names = [...all.keys()].sort(byteOrder);
  return names.map((n) => `${n}\t${keyName(all.get(n)![0], mac)}\t${keyName(all.get(n)![1], mac)}\n`).join("");
}

/** `std::string`'s `operator<`: the bytes' order (the names are ASCII). */
function byteOrder(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

// ---------------------------------------------------------------------------------------------- the store

export type ActionScope = "AS_CONTEXT" | "AS_ACTIVE" | "AS_GLOBAL";

/** The part of a `TOOL_ACTION` the hotkey list reads. */
export interface HotkeyAction {
  /** `GetName()`: "pcbnew.InteractiveRouter.SingleTrack"; "" for a pseudo action. */
  name: string;
  /** `GetFriendlyName()`. */
  label: string;
  /** `GetDescription()`: the tooltip. */
  description: string;
  scope: ActionScope;
  /** `GetDefaultHotKey()` / `GetDefaultHotKeyAlt()`. */
  defaultKey: number;
  defaultKeyAlt: number;
  /** `GetHotKey()` / `GetHotKeyAlt()`: the default, or the person's `user.hotkeys`. */
  key: number;
  keyAlt: number;
  /** A `PSEUDO_ACTION` (the platform commands and the gestures): listed, not editable here (see `isEditable`). */
  pseudo?: boolean;
}

/** `HOTKEY`: an action and its keys as edited. */
export interface Hotkey {
  action: HotkeyAction;
  editKey: number;
  editKeyAlt: number;
}

/** `HOTKEY_SECTION`. */
export interface HotkeySection {
  name: string;
  hotkeys: Hotkey[];
}

/** `HOTKEY_STORE::GetAppName`: the name up to its first dot. */
export function appName(action: Pick<HotkeyAction, "name">): string {
  const i = action.name.indexOf(".");
  return i < 0 ? action.name : action.name.slice(0, i);
}

const APP_NAMES: Readonly<Record<string, string>> = {
  common: "Common",
  kicad: "Project Manager",
  eeschema: "Schematic Editor",
  pcbnew: "PCB Editor",
  plEditor: "Drawing Sheet Editor",
  "3DViewer": "3D Viewer",
  gerbview: "Gerber Viewer",
};

/** `HOTKEY_STORE::GetSectionName`: the application's title, else its name. */
export function sectionName(action: Pick<HotkeyAction, "name">): string {
  const app = appName(action);
  return APP_NAMES[app] ?? app;
}

/** `TOOL_ACTION::GetToolName`: the name without its last part ("pcbnew.InteractiveRouter"); a name of one dot is itself, of none "". */
export function toolName(action: Pick<HotkeyAction, "name">): string {
  const dots = action.name.split(".").length - 1;
  if (dots === 1) return action.name;
  if (dots === 2) return action.name.slice(0, action.name.lastIndexOf("."));
  return "";
}

function pseudo(label: string, key: number, keyAlt = 0): HotkeyAction {
  return { name: "", label, description: "", scope: "AS_GLOBAL", defaultKey: key, defaultKeyAlt: keyAlt, key, keyAlt, pseudo: true };
}

/** `g_gesturePseudoActions`. */
export const GESTURE_PSEUDO_ACTIONS: readonly HotkeyAction[] = [
  pseudo("Accept Autocomplete", WXK.RETURN, WXK.NUMPAD_ENTER),
  pseudo("Cancel Autocomplete", WXK.ESCAPE),
  pseudo("Toggle Checkbox", WXK.SPACE),
  pseudo("Pan Left/Right", MD_CTRL + WXK.PSEUDO_WHEEL),
  pseudo("Pan Up/Down", MD_SHIFT + WXK.PSEUDO_WHEEL),
  pseudo("Finish Drawing", WXK.PSEUDO_DBLCLICK),
  pseudo("Add to Selection", MD_SHIFT + WXK.PSEUDO_CLICK),
  pseudo("Highlight Net", MD_CTRL + WXK.PSEUDO_CLICK),
  pseudo("Remove from Selection", MD_CTRL + MD_SHIFT + WXK.PSEUDO_CLICK),
  pseudo("Ignore Grid Snaps", MD_CTRL),
  pseudo("Ignore Other Snaps", MD_SHIFT),
];

/** `g_standardPlatformCommands`: Close (not on Windows) and Quit. */
export function standardPlatformCommands(windows: boolean): HotkeyAction[] {
  return [...(windows ? [] : [pseudo("Close", MD_CTRL + "W".charCodeAt(0))]), pseudo("Quit", MD_CTRL + "Q".charCodeAt(0))];
}

/**
 * `HOTKEY_STORE::Init( aActionsList, aIncludeReadOnlyCmds )`: the actions by name (a `std::map`, so the names' byte order), without the ones of no friendly name
 * ("Internal actions probably shouldn't be allowed hotkeys") and the two zone display modes of `m_ExtraZoneDisplayModes` (`excluded`); a section per application
 * as the names change ("3DViewer", "common", "eeschema", ..., "pcbnew" in that order), each hotkey starting at the action's current keys. With the read-only
 * commands, the Common section starts with the platform's Close / Quit, and a last "Gestures" section lists the mouse gestures.
 */
export function initHotkeySections(actions: readonly HotkeyAction[], includeReadOnly: boolean, opts: { windows?: boolean; excluded?: ReadonlySet<string> } = {}): HotkeySection[] {
  const byName = new Map<string, HotkeyAction>();
  for (const a of actions) {
    if (a.label === "") continue;
    if (opts.excluded?.has(a.name)) continue;
    // `masterMap[ name ]`'s first action keeps the keys (`if( !hotkey.m_EditKeycode )`); the studio has one action per name
    if (!byName.has(a.name)) byName.set(a.name, a);
  }
  const sections: HotkeySection[] = [];
  let currentApp: string | null = null;
  let current: HotkeySection | null = null;
  for (const name of [...byName.keys()].sort(byteOrder)) {
    const action = byName.get(name)!;
    const app = appName(action);
    if (!current || app !== currentApp) {
      current = { name: sectionName(action), hotkeys: [] };
      sections.push(current);
      currentApp = app;
      if (includeReadOnly && app === "common") for (const c of standardPlatformCommands(!!opts.windows)) current.hotkeys.push({ action: c, editKey: c.key, editKeyAlt: c.keyAlt });
    }
    current.hotkeys.push({ action, editKey: action.key, editKeyAlt: action.keyAlt });
  }
  if (includeReadOnly) sections.push({ name: "Gestures", hotkeys: GESTURE_PSEUDO_ACTIONS.map((g) => ({ action: g, editKey: g.key, editKeyAlt: g.keyAlt })) });
  return sections;
}

/** Every hotkey of the sections. */
function allHotkeys(sections: readonly HotkeySection[]): Hotkey[] {
  return sections.flatMap((s) => s.hotkeys);
}

/** The hotkey of an action (by name; a pseudo row by its label). */
export function findHotkey(sections: readonly HotkeySection[], action: HotkeyAction): Hotkey | undefined {
  return allHotkeys(sections).find((h) => h.action === action) ?? allHotkeys(sections).find((h) => !action.pseudo && h.action.name === action.name);
}

/**
 * Whether a row is edited here. KiCad's list lets a pseudo action's row be edited too, but its key is no action's and is never written (`WriteHotKeyConfig`
 * writes the actions only): a change to it is lost on OK. The studio leaves those rows read-only.
 */
export function isEditable(h: Hotkey): boolean {
  return !h.action.pseudo;
}

/**
 * `HOTKEY_STORE::CheckKeyConflicts`: the first other hotkey of the action's section or of Common whose hotkey or alternate is `key` -- unless both actions are
 * `AS_CONTEXT` and of different tools, which may share a key. KiCad's comment says a key in Common "can't be in any other section and vice versa", but the loop
 * reads only those two sections: an editor's action may not take a key of Common's, while a Common action may take a key an editor's action has.
 */
export function checkKeyConflicts(sections: readonly HotkeySection[], action: HotkeyAction, key: number): Hotkey | null {
  const section = sectionName(action);
  const common = sectionName({ name: "common.Control.Fake" });
  for (const s of sections) {
    if (!(s.name === section || s.name === common)) continue;
    for (const h of s.hotkeys) {
      if (h.action === action) continue;
      if (h.editKey === key || h.editKeyAlt === key) {
        if (h.action.scope === "AS_CONTEXT" && action.scope === "AS_CONTEXT" && toolName(h.action) !== toolName(action)) continue;
        return h;
      }
    }
  }
  return null;
}

/** `resolveKeyConflicts`' question (title "Confirm change", Yes / No with No the default). */
export function conflictMessage(key: number, conflict: Hotkey, mac: boolean): string {
  return `'${keyName(key, mac)}' is already assigned to '${conflict.action.label}' in section '${sectionName(conflict.action)}'. Are you sure you want to change its assignment?`;
}

export const CONFLICT_TITLE = "Confirm change";

/** `m_reservedHotkeys`: Ctrl+Tab and Ctrl+Shift+Tab, by code. */
export const RESERVED_HOTKEYS: ReadonlyMap<number, string> = new Map(["Ctrl+Tab", "Ctrl+Shift+Tab"].map((k) => [keyCodeFromKeyName(k, false), k] as const));

/** `editItem`'s message for a reserved key. */
export function reservedMessage(key: number): string | null {
  const name = RESERVED_HOTKEYS.get(key);
  return name === undefined ? null : `'${name}' is a reserved hotkey in KiCad and cannot be assigned.`;
}

/**
 * `WIDGET_HOTKEY_LIST::changeHotkey`'s plan for one change: nothing when the key has no name (`KeyNameFromKeyCode( aKey, &exists )`) or the hotkey already
 * has it -- compared with the *hotkey* even for a change of the alternate (`aHotkey.m_EditKeycode != aKey`), KiCad's test; a key of 0 needs no question; any
 * other first asks about a conflict (`resolveKeyConflicts`).
 */
export type HotkeyChangePlan = { kind: "none" } | { kind: "apply" } | { kind: "ask"; conflict: Hotkey };

export function planHotkeyChange(sections: readonly HotkeySection[], hotkey: Hotkey, key: number): HotkeyChangePlan {
  const { found } = keyNameFromKeyCode(key, false);
  if (!found || hotkey.editKey === key) return { kind: "none" };
  if (key === 0) return { kind: "apply" };
  const conflict = checkKeyConflicts(sections, hotkey.action, key);
  return conflict ? { kind: "ask", conflict } : { kind: "apply" };
}

/** The sections with one hotkey's keys changed (the sections are immutable here; KiCad's `HOTKEY` is edited in place). */
function withHotkey(sections: readonly HotkeySection[], target: Hotkey, patch: Partial<Pick<Hotkey, "editKey" | "editKeyAlt">>): HotkeySection[] {
  return sections.map((s) => (s.hotkeys.includes(target) ? { ...s, hotkeys: s.hotkeys.map((h) => (h === target ? { ...h, ...patch } : h)) } : s));
}

/**
 * The change made: the key becomes the hotkey or the alternate; with `clearConflict` (the question answered Yes) the other action loses its *hotkey*
 * (`conflictingHotKey->m_EditKeycode = 0`), even when the key was its alternate -- KiCad's behaviour.
 */
export function applyHotkeyChange(sections: readonly HotkeySection[], hotkey: Hotkey, key: number, alternate: boolean, clearConflict?: Hotkey): HotkeySection[] {
  let out = [...sections];
  if (clearConflict) out = withHotkey(out, clearConflict, { editKey: 0 });
  const target = allHotkeys(out).find((h) => h.action === hotkey.action)!;
  return withHotkey(out, target, alternate ? { editKeyAlt: key } : { editKey: key });
}

/** The context menu's entries (`onContextMenu`), in order. */
export type HotkeyMenuId = "edit" | "edit_alt" | "reset" | "clear" | "clear_alt" | "default";
export const HOTKEY_MENU: ReadonlyArray<{ id: HotkeyMenuId; label: string }> = [
  { id: "edit", label: "Edit..." },
  { id: "edit_alt", label: "Edit Alternate..." },
  { id: "reset", label: "Undo Changes" },
  { id: "clear", label: "Clear Assigned Hotkey" },
  { id: "clear_alt", label: "Clear Assigned Alternate" },
  { id: "default", label: "Restore Defaults" },
];

/**
 * `resetItem`: the `changeHotkey` calls of a menu entry, in order. Undo Changes sets the hotkey to the action's current one and then the alternate to the
 * current *hotkey* (`changeHotkey( changed_hk, GetHotKey(), true )`, as KiCad has it -- after the first call that second one does nothing, the two keys being
 * equal); Restore Defaults the default hotkey, then the default alternate; the Clear entries 0.
 */
export function resetSteps(hotkey: Hotkey, id: Exclude<HotkeyMenuId, "edit" | "edit_alt">): Array<{ key: number; alternate: boolean }> {
  const a = hotkey.action;
  if (id === "reset") return [{ key: a.key, alternate: false }, { key: a.key, alternate: true }];
  if (id === "clear") return [{ key: 0, alternate: false }];
  if (id === "clear_alt") return [{ key: 0, alternate: true }];
  return [{ key: a.defaultKey, alternate: false }, { key: a.defaultKeyAlt, alternate: true }];
}

/** `ResetAllHotkeysToDefault` (true: the dialog's reset button, `ResetPanel`) / `ResetAllHotkeysToOriginal` (false: Undo All Changes): every hotkey, shown or not. */
export function resetAllHotkeys(sections: readonly HotkeySection[], toDefault: boolean): HotkeySection[] {
  return sections.map((s) => ({
    ...s,
    hotkeys: s.hotkeys.map((h) => ({ ...h, editKey: toDefault ? h.action.defaultKey : h.action.key, editKeyAlt: toDefault ? h.action.defaultKeyAlt : h.action.keyAlt })),
  }));
}

/** `PANEL_HOTKEYS_EDITOR::ImportHotKeys`: each hotkey named in the file takes the file's two keys. */
export function importHotkeys(sections: readonly HotkeySection[], imported: HotkeyConfig): HotkeySection[] {
  return sections.map((s) => ({
    ...s,
    hotkeys: s.hotkeys.map((h) => {
      const k = h.action.pseudo ? undefined : imported.get(h.action.name);
      return k ? { ...h, editKey: k[0], editKeyAlt: k[1] } : h;
    }),
  }));
}

/** `SaveAllHotkeys`: the actions' keys as edited, what `WriteHotKeyConfig` then writes (the pseudo actions are no actions and are left out). */
export function savedKeys(sections: readonly HotkeySection[]): Array<{ name: string; key: number; keyAlt: number }> {
  return allHotkeys(sections)
    .filter((h) => !h.action.pseudo)
    .map((h) => ({ name: h.action.name, key: h.editKey, keyAlt: h.editKeyAlt }));
}

/** One row as `updateFromClientData` shows it: the label without "..." / "…", " *" while the hotkey is not the saved one, the keys' names, the description on one line. */
export function hotkeyRow(h: Hotkey, mac: boolean): { label: string; key: string; alt: string; description: string } {
  let label = h.action.label || h.action.name;
  label = label.replaceAll("...", "").replaceAll("…", "");
  if (h.editKey !== h.action.key) label += " *";
  return { label, key: keyName(h.editKey, mac), alt: keyName(h.editKeyAlt, mac), description: h.action.description.replace(/[\n\r]/g, " ") };
}

/** `HOTKEY_FILTER::FilterMatches`: no filter matches everything; else the label or the hotkey's name (not the alternate) holds the text, without case. */
export function filterMatches(h: Hotkey, filter: string, mac: boolean): boolean {
  const f = filter.toUpperCase();
  if (f.length === 0) return true;
  if (h.action.label.toUpperCase().includes(f)) return true;
  return keyName(h.editKey, mac).toUpperCase().includes(f);
}

/** The column titles (`WIDGET_HOTKEY_LIST::WIDGET_HOTKEY_LIST`). */
export function hotkeyColumns(readOnly: boolean): [string, string, string, string] {
  return [readOnly ? "Command" : "Command (double-click to edit)", "Hotkey", "Alternate", "Description"];
}

// ---------------------------------------------------------------------------------------------- the Set Hotkey prompt

/** The part of a key press the prompt reads. */
export interface KeyPress {
  key: string;
  code?: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/** The named keys of a browser key press, as `wxKeyCode`s. */
const DOM_NAMED: Readonly<Record<string, number>> = {
  Escape: WXK.ESCAPE,
  Enter: WXK.RETURN,
  Tab: WXK.TAB,
  Backspace: WXK.BACK,
  Delete: WXK.DELETE,
  Insert: WXK.INSERT,
  Home: WXK.HOME,
  End: WXK.END,
  PageUp: WXK.PAGEUP,
  PageDown: WXK.PAGEDOWN,
  ArrowUp: WXK.UP,
  ArrowDown: WXK.DOWN,
  ArrowLeft: WXK.LEFT,
  ArrowRight: WXK.RIGHT,
  " ": WXK.SPACE,
};

/** `HK_PROMPT_DIALOG::OnCharHook`'s skipped keys: the modifiers and the locks press nothing. */
const SKIPPED = new Set(["Shift", "Control", "Alt", "Meta", "CapsLock", "NumLock", "ScrollLock", "AltGraph", "OS", "Dead", "Unidentified", "Process"]);

/** The `wxKeyCode` (or character) of a browser key press, as wx reports it in the prompt's key events; null for a key that is skipped. */
export function wxKeyOf(e: KeyPress): number | null {
  if (SKIPPED.has(e.key)) return null;
  const code = e.code ?? "";
  // the number pad (wx gives its own codes for it)
  const pad = /^Numpad(\d)$/.exec(code);
  if (pad && e.key.length === 1 && /\d/.test(e.key)) return WXK.NUMPAD0 + Number(pad[1]);
  if (code === "NumpadAdd") return WXK.NUMPAD_ADD;
  if (code === "NumpadSubtract") return WXK.NUMPAD_SUBTRACT;
  if (code === "NumpadMultiply") return WXK.NUMPAD_MULTIPLY;
  if (code === "NumpadDivide") return WXK.NUMPAD_DIVIDE;
  if (code === "NumpadEnter") return WXK.NUMPAD_ENTER;
  const named = DOM_NAMED[e.key];
  if (named !== undefined) return named;
  const f = /^F(\d{1,2})$/.exec(e.key);
  if (f && Number(f[1]) >= 1 && Number(f[1]) <= 24) return WXK.F1 + Number(f[1]) - 1;
  if (e.key.length === 1) return e.key.charCodeAt(0);
  return null;
}

/**
 * `WIDGET_HOTKEY_LIST::MapKeypressToKeycode`: Esc is 0 (the prompt is cancelled); a letter is made upper case; Shift is kept only for a letter, a special key
 * (a code above 256), Tab and Space -- a symbol is the character the layout types (Shift+/ is "?", so Ctrl+5 and Ctrl+% stay apart); Ctrl (Cmd on macOS) and
 * Alt are added. The Windows / Super key and a Mac's Control key are not read (the studio's dispatcher does not read them either). Null for a skipped key.
 */
export function mapKeypressToKeycode(e: KeyPress, mac: boolean): number | null {
  let key = wxKeyOf(e);
  if (key === null) return null;
  if (key === WXK.ESCAPE) return 0;
  if (key >= 0x61 && key <= 0x7a) key -= 0x20;
  const isLetter = key >= 0x41 && key <= 0x5a;
  if (e.shiftKey && (isLetter || key > 256 || key === WXK.TAB || key === WXK.SPACE)) key |= MD_SHIFT;
  if (mac ? e.metaKey : e.ctrlKey) key |= MD_CTRL;
  if (e.altKey) key |= MD_ALT;
  return key;
}

/** The prompt's words (`HK_PROMPT_DIALOG`). */
export const PROMPT_TEXT = {
  title: "Set Hotkey",
  instruction: "Press a new hotkey, or press Esc to cancel...",
  command: "Command:",
  current: "Current key:",
  clear: "Clear assigned hotkey",
} as const;

// The actions the hotkeys are of: the extracted table (src/kicad/actions.json, KiCad 8303b2ad's pcbnew, eeschema and common actions) and the KiCad 10 actions
// newer than it (src/kicad/kicad10Actions.ts), each with its scope (src/kicad/action_scopes.json, tools/extract-action-scopes.js) -- the list
// `EDA_BASE_FRAME::ShowPreferences` hands the Hotkeys page (`hotkeysPanel->ActionsList()`: the KiCad manager's actions and each kiface's `GetActions`) and
// DIALOG_LIST_HOTKEYS its read-only panel. The global dispatcher (useGlobalHotkeys.ts) binds the same actions.
//
// The 3D viewer's actions (`EDA_3D_ACTIONS`, src/kicad/actions3d.json, tools/extract-actions-3d.js) are listed too, a "3D Viewer" section first (UI-LEFT-3 C3-4): pcbnew's
// kiface links the 3D viewer, so `KIWAY::GetActions` (`ACTION_MANAGER::GetActionList()`) hands the page its static actions. They are not in `HOTKEY_TABLE`: the global
// dispatcher does not bind them; the 3D view reads their keys itself (kicad-port/actions3d.ts `keyActions3D`).
import actionsData from "../kicad/actions.json";
import actions3dData from "../kicad/actions3d.json";
import scopesData from "../kicad/action_scopes.json";
import type { ActionsFile, KicadAction } from "../kicad/types";
import { KICAD10_ACTIONS } from "../kicad/kicad10Actions";
import { defaultHotkey, effectiveHotkey } from "./hotkeys";
import { studioHotkeyToKeyCode, type ActionScope, type HotkeyAction } from "../kicad-port/hotkeyStore";

const actionsFile = actionsData as ActionsFile;
const scopes = (scopesData as { actions: Record<string, ActionScope> }).actions;

/** Every action a hotkey can be given, as the dispatcher and the hotkey list see it (the table's, then the KiCad 10 ones it lacks). */
export const HOTKEY_TABLE: readonly KicadAction[] = [
  ...actionsFile.actions,
  ...KICAD10_ACTIONS.filter((k) => !actionsFile.actions.some((a) => a.name === k.name)).map(
    (k): KicadAction => ({ name: k.name, label: k.label, tooltip: k.tooltip, hotkey: null, altHotkey: null, hotkeyRaw: null, altHotkeyRaw: null, macHotkey: null, macAltHotkey: null, macHotkeyRaw: null, macAltHotkeyRaw: null, icon: k.icon, flags: [] })
  ),
];

/** `EDA_3D_ACTIONS` with their scopes (an action of no friendly name has label ""). */
export const HOTKEY_TABLE_3D: ReadonlyArray<KicadAction & { scope: ActionScope }> = (actions3dData as unknown as { actions: Array<KicadAction & { scope: ActionScope }> }).actions;

/** The actions as the hotkey store reads them: the defaults and the current keys (the person's, else the defaults) as KiCad key codes. */
export function hotkeyActions(isMacPlatform?: boolean): HotkeyAction[] {
  return [...HOTKEY_TABLE, ...HOTKEY_TABLE_3D].map((a) => {
    const d = defaultHotkey(a, isMacPlatform);
    const c = effectiveHotkey(a, isMacPlatform);
    return {
      name: a.name,
      label: a.label,
      description: a.tooltip,
      // the KiCad 10 actions are all `AS_GLOBAL` (KiCad 25ec2335's sch_actions.cpp and actions.cpp)
      scope: ("scope" in a ? (a.scope as ActionScope) : undefined) ?? scopes[a.name] ?? "AS_GLOBAL",
      defaultKey: studioHotkeyToKeyCode(d.hotkey),
      defaultKeyAlt: studioHotkeyToKeyCode(d.altHotkey),
      key: studioHotkeyToKeyCode(c.hotkey),
      keyAlt: studioHotkeyToKeyCode(c.altHotkey),
    };
  });
}

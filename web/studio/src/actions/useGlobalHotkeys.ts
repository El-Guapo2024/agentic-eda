// One window-level keydown listener, driven entirely by each action's
// hotkey/altHotkey as `effectiveHotkey` gives it -- the person's own
// (Preferences > Hotkeys, state/userHotkeys.ts) else the default of
// src/kicad/actions.json (actions/hotkeyActions.ts' table) -- matched
// against useActionRunner's registry: an action fires from its keyboard
// shortcut only once it's actually implemented, same rule as the menu/
// toolbar's disabled state. Call once, near the app's root (App.tsx).
import { useEffect, useMemo } from "react";
import { useActionRunner } from "./useActionRunner";
import { eventToHotkeyCandidates, effectiveHotkey } from "./hotkeys";
import { HOTKEY_TABLE } from "./hotkeyActions";
import { useUserHotkeysVersion } from "../state/userHotkeys";

const TEXT_INPUT_TAGS = new Set(["INPUT", "SELECT", "TEXTAREA"]);
const NAV_COMBO = /^(?:(?:Ctrl|Shift)\+)?(?:Up|Down|Left|Right)$|^(?:Enter|End)$/;

export function useGlobalHotkeys() {
  const { run, isEnabled } = useActionRunner();

  // Several extracted hotkeys collide across actions (e.g. "Ctrl+F1" is
  // shared by common.Control.zoomIn and common.SuiteControl.listHotKeys,
  // "Ctrl+Home" by zoomFitObjects and zoomFitScreen) -- real KiCad has
  // only one such action live in a given tool context at a time, but this
  // app has no tool-context model, so every candidate for a key is kept
  // here and the first one that's actually enabled wins at dispatch time.
  // A plain "first name wins" index would let an unimplemented action
  // permanently shadow an implemented one that happens to share its key.
  // `ACTION_MANAGER::UpdateHotKeys`: rebuilt when the person's hotkeys change (Preferences' OK, List Hotkeys' OK).
  const hotkeysVersion = useUserHotkeysVersion();
  const hotkeyIndex = useMemo(() => {
    const idx = new Map<string, string[]>();
    const add = (hk: string | null, name: string) => {
      if (!hk) return;
      const list = idx.get(hk);
      if (list) list.push(name);
      else idx.set(hk, [name]);
    };
    // Every key comes from `effectiveHotkey`, the hand-corrected defaults included (Create Corner's and Repeat Last Item's Insert, Draw Filled Zones' Ctrl+Shift+Z,
    // KiCad 10's Cycle Arc Drawing Mode on Alt+/: actions/hotkeys.ts `DEFAULT_HOTKEY_FIXES`), so a person can move them. Group Items has no key, as in KiCad
    // 8303b2ad (Ctrl+G is Search's); Ungroup neither -- both stay on the toolbar and in the context menu.
    for (const a of HOTKEY_TABLE) {
      const { hotkey, altHotkey } = effectiveHotkey(a);
      add(hotkey, a.name);
      add(altHotkey, a.name);
    }
    return idx;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hotkeysVersion]);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const target = e.target as HTMLElement | null;

      // DIALOG_SHIM / wxDialog: Escape cancels the open dialog, even from
      // one of its text fields -- before any editor action sees the key.
      if (e.key === "Escape" && !e.defaultPrevented && escapeTopDialog()) {
        e.preventDefault();
        e.stopPropagation();
        return;
      }
      if (target && TEXT_INPUT_TAGS.has(target.tagName)) return;

      // common.Interactive.cancel's extracted hotkey is null (KiCad
      // likely binds Escape-to-cancel at the tool-manager level, not as
      // a per-action default the way most hotkeys work) -- special-cased
      // here so it still goes through the same registry/enabled check
      // as everything else, rather than being hardcoded separately.
      if (e.key === "Escape" && isEnabled("common.Interactive.cancel")) {
        e.preventDefault();
        run("common.Interactive.cancel");
        return;
      }

      // `eventToHotkeyCandidates`: a Shift-typed symbol ("+", ">") is also
      // tried the way KiCad's table spells it ("Ctrl++", "Ctrl+Shift+.").
      // common.Control.cursor*/pan*/cursorClick/cursorDblClick/finish bind bare
      // navigation keys (arrows, Enter, End). In KiCad those only reach the
      // canvas tool framework -- a dialog, menu or focused button gets them
      // first. Only fire them from the canvas/body so a list in a dialog or
      // a focused toolbar button keeps its own arrow/Enter behavior.
      let actionName: string | undefined;
      for (const combo of eventToHotkeyCandidates(e)) {
        if (NAV_COMBO.test(combo) && target && target !== document.body && !target.closest(".pcb-canvas-container")) continue;
        actionName = hotkeyIndex.get(combo)?.find(isEnabled);
        if (actionName) break;
      }
      if (!actionName) return;
      e.preventDefault();
      run(actionName);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [hotkeyIndex, run, isEnabled]);
}

/**
 * Escape on an open dialog (`DIALOG_SHIM`'s wxID_CANCEL handling): press the
 * topmost dialog's Cancel (or Close) button, else click its backdrop, which
 * is how every dialog here closes without applying. Returns whether a dialog
 * was open.
 */
export function escapeTopDialog(doc: Document = document): boolean {
  const backdrops = doc.querySelectorAll<HTMLElement>(".dialog-backdrop");
  const top = backdrops[backdrops.length - 1];
  if (!top) return false;
  const buttons = [...top.querySelectorAll<HTMLButtonElement>(".dialog-footer button, .dialog-header button, button")];
  const cancel = buttons.find((b) => /^\s*cancel\s*$/i.test(b.textContent ?? "")) ?? buttons.find((b) => /^\s*close\s*$/i.test(b.textContent ?? ""));
  if (cancel) cancel.click();
  else top.click();
  return true;
}

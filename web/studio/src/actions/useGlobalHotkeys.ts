// One window-level keydown listener, driven entirely by src/kicad/
// actions.json's extracted hotkey/altHotkey strings matched against
// useActionRunner's registry -- an action fires from its keyboard
// shortcut only once it's actually implemented, same rule as the menu/
// toolbar's disabled state. Call once, near the app's root (App.tsx).
import { useEffect, useMemo } from "react";
import actionsData from "../kicad/actions.json";
import type { ActionsFile } from "../kicad/types";
import { useActionRunner } from "./useActionRunner";
import { eventToHotkeyCandidates, effectiveHotkey } from "./hotkeys";

const actionsFile = actionsData as ActionsFile;

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
  const hotkeyIndex = useMemo(() => {
    const idx = new Map<string, string[]>();
    const add = (hk: string | null, name: string) => {
      if (!hk) return;
      const list = idx.get(hk);
      if (list) list.push(name);
      else idx.set(hk, [name]);
    };
    for (const a of actionsFile.actions) {
      const { hotkey, altHotkey } = effectiveHotkey(a);
      add(hotkey, a.name);
      add(altHotkey, a.name);
    }
    // Task item 5: common.Interactive.group/ungroup's extracted hotkey is
    // null (same extraction gap group_tool.cpp's own default-hotkey
    // registration apparently hits -- every other common.Groups.*/
    // common.Interactive.*Group* action is null too), but the task brief
    // names Ctrl+G/Ctrl+Shift+G explicitly, matching real KiCad's actual
    // shipped defaults; added directly here rather than guessed at in the
    // extractor.
    // pcbnew.PointEditor.addCorner ("Create Corner"): source is
    // `#ifdef __WXMAC__ WXK_F1 #else WXK_INSERT`, and the extractor kept only the
    // macOS arm (F1, which on every other platform is zoomIn's). Insert is the
    // real non-Mac default.
    add("Insert", "pcbnew.PointEditor.addCorner");
    add("Ctrl+G", "common.Interactive.group");
    add("Ctrl+Shift+G", "common.Interactive.ungroup");
    return idx;
  }, []);

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

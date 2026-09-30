// One window-level keydown listener, driven entirely by src/kicad/
// actions.json's extracted hotkey/altHotkey strings matched against
// useActionRunner's registry -- an action fires from its keyboard
// shortcut only once it's actually implemented, same rule as the menu/
// toolbar's disabled state. Call once, near the app's root (App.tsx),
// not per-component -- component-local shortcuts that need component
// state (Canvas.tsx's Escape/M) stay directly on that component instead
// of going through this registry-driven path.
import { useEffect, useMemo } from "react";
import actionsData from "../kicad/actions.json";
import type { ActionsFile } from "../kicad/types";
import { useActionRunner } from "./useActionRunner";
import { eventToHotkey } from "./hotkeys";

const actionsFile = actionsData as ActionsFile;

const TEXT_INPUT_TAGS = new Set(["INPUT", "SELECT", "TEXTAREA"]);

export function useGlobalHotkeys() {
  const { run, isEnabled } = useActionRunner();

  const hotkeyIndex = useMemo(() => {
    const idx = new Map<string, string>();
    for (const a of actionsFile.actions) {
      if (a.hotkey && !idx.has(a.hotkey)) idx.set(a.hotkey, a.name);
      if (a.altHotkey && !idx.has(a.altHotkey)) idx.set(a.altHotkey, a.name);
    }
    return idx;
  }, []);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const target = e.target as HTMLElement | null;
      if (target && TEXT_INPUT_TAGS.has(target.tagName)) return;
      const combo = eventToHotkey(e);
      if (!combo) return;
      const actionName = hotkeyIndex.get(combo);
      if (!actionName || !isEnabled(actionName)) return;
      e.preventDefault();
      run(actionName);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [hotkeyIndex, run, isEnabled]);
}

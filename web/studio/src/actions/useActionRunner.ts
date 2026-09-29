// Bridges KiCad's dotted action names (src/kicad/actions.json, referenced
// by menus.json/toolbars.json) to this app's own implementations.
//
// The registry below is intentionally empty. This session could not read
// KiCad's source (see the report), so there is no verified action-name
// string to hang any of the already-implemented behavior (rotate,
// delete, route, ...) on -- and guessing a name like
// "pcbnew.InteractiveEdit.rotateCw" would be exactly the kind of
// unverified content the extraction scripts exist to avoid. Everything
// stays disabled with "(not ported yet)" in the menu bar and toolbars
// until real names come back from tools/extract-actions.js; add an entry
// here for each one this app already implements, e.g.:
//
//   registry.set("pcbnew.InteractiveEdit.rotateCw", () => api.rotateSelection(1));
//
// Core interactions that already work (pan/zoom/select/move/rotate/
// delete) run today via direct keyboard/pointer handlers in
// components/canvas/Canvas.tsx, independent of this registry -- so the
// app is usable now even though menu/toolbar wiring waits on real data.

import { useCallback, useMemo } from "react";
import { useStudioApi } from "../state/store";

export function useActionRunner() {
  const api = useStudioApi();

  const registry = useMemo(() => {
    const m = new Map<string, () => void>();
    void api; // referenced so this recomputes if `api` identity ever changes; see module doc for how to populate it
    return m;
  }, [api]);

  const isEnabled = useCallback((name: string) => registry.has(name), [registry]);
  const run = useCallback((name: string) => registry.get(name)?.(), [registry]);
  return { run, isEnabled };
}

// useGlobalHotkeys.ts's `hotkeyIndex.get(combo)?.find(isEnabled)`: when a
// physical key is double-booked by one `pcbnew.*` and one `eeschema.*`
// action with the *same* extracted hotkey (R, M, G, X, E, U, V, F all
// collide this way in the real table -- `hotkeyIndex.get` returns both),
// whichever name comes first in actions.json's own order used to win on
// *every* tab, because a registered-but-tab-irrelevant handler (every
// `pcbOnly`/`schematicOnly` wrapper is itself always registered -- only
// its function *body* checks the tab, and only once actually called) was
// indistinguishable from a truly-applicable one. `pcbnew.InteractiveEdit.
// rotateCcw` (R) and `pcbnew.InteractiveMove.move` (M) were already
// losing that race on the PCB tab before this module existed -- real,
// silent regressions from the schematic port's own earlier work, not a
// hypothetical. This is the fix, pulled out of useActionRunner.ts's own
// `isEnabled` so the actual decision is unit-testable without a React
// render: gate on the action name's own module prefix, the established
// "pcbnew.X"/"eeschema.X"/"common.X" naming convention every action in
// this table already follows.
export function isActionEnabledForTab(name: string, tab: string, registered: boolean): boolean {
  if (!registered) return false;
  if (name.startsWith("pcbnew.")) return tab === "pcb";
  if (name.startsWith("eeschema.")) return tab === "schematic";
  return true; // common.*, and anything else with no tab of its own
}

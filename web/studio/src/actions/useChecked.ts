// Whether a toolbar button or a menu entry is drawn checked: the registry's own answer first (`isChecked`: the schematic editor's View toggles, its Net
// Navigator and the attributes of the selection, actions/schControlActions.ts), then the toggles of the other editors and the panes
// (actions/useActionChecked.ts: grid, units, the library trees, the Properties / Hierarchy / Appearance panes, the editors' active tool). `undefined` =
// the action is not a toggle, so nothing is drawn.
import { useActionChecked } from "./useActionChecked";
import { useActionRunner } from "./useActionRunner";

export function useChecked(): (name: string) => boolean | undefined {
  const { isChecked } = useActionRunner();
  const mine = useActionChecked();
  return (name) => isChecked(name) ?? mine(name);
}

// How the schematic edit actions register into useActionRunner's handler map. Several of KiCad's `eeschema.*` actions are shared with the Symbol Editor
// (`drawRectangle`, `drawCircle`, `drawArc`, ...), which registers its own handler under the same name; a plain `m.set` would make whichever editor registers
// last take the action on every tab. `schematicActions` keeps what was registered before and runs it on any other tab, so the two compose in either order.
export type ActionMap = Map<string, () => void>;

/** A registry whose `set` installs `fn` for the schematic tab and leaves every other tab to the handler already registered under that name, if any. */
export function schematicActions(m: ActionMap, tab: string): { set: (name: string, fn: () => void) => void } {
  return {
    set: (name, fn) => {
      const other = m.get(name);
      m.set(name, () => (tab === "schematic" ? fn() : other?.()));
    },
  };
}

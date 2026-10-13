// Swap Pins, Swap Pin Labels and Swap Unit Labels -- `SCH_EDIT_TOOL::SwapPins`, `SwapPinLabels` and `SwapUnitLabels` (eeschema/tools/sch_edit_tool.cpp at 8303b2ad).
// The work is the `sch_edit` verbs `swap_pins`, `swap_pin_labels` and `swap_unit_labels` (crates/ops/src/sch_pins.rs), one undo step each; this file is the conditions the
// tools put on the selection before they run.
//
// A pin is an item of the sheet (kicad-port/schPins.ts): the selection holds `pin:<reference>:<number>` ids, in the order they were picked, which is the order the swaps run in.
import { schematicActions, type ActionMap } from "./schActionRegistry";
import type { SchEditContext } from "./schEditActions";

export function registerSchPinActions(registry: ActionMap, ctx: SchEditContext): void {
  const { state, api, dispatch } = ctx;
  const m = schematicActions(registry, state.tab);
  const sch = state.schematic;

  // Swap Pins: "if( !m_frame->eeconfig()->m_Input.allow_unconstrained_pin_swaps ) return 0;" -- the preference (Preferences > Editing Options) is checked first and nothing is said --
  // then `RequestSelection( { SCH_PIN_T } )`, and fewer than two pins does nothing. The other conditions (all pins on one symbol, a symbol not shared by several sheet
  // instances) are the verb's, which says why it refuses.
  m.set("eeschema.InteractiveEdit.swapPins", () => {
    if (!state.prefs.allowUnconstrainedPinSwaps) return;
    const pins = ctx.requestPins();
    if (pins.length < 2) return;
    void api.cmd({ op: "sch_edit", verb: "swap_pins", pins });
  });

  // Swap Pin Labels: two or more pins, each with exactly one net label on its connection and no other pin there (the verb checks that last part).
  m.set("eeschema.InteractiveEdit.swapPinLabels", () => {
    const pins = ctx.requestPins();
    if (pins.length < 2) return;
    void api.cmd({ op: "sch_edit", verb: "swap_pin_labels", pins });
  });

  // Swap Unit Labels: `GetSameSymbolMultiUnitSelection` -- the selected symbols are units of one reference that has several units placed; each pin of those units needs
  // exactly one net label (the verb checks it). A reference is one selectable item for all its placed units.
  m.set("eeschema.InteractiveEdit.swapUnitLabels", () => {
    if (!sch) return;
    const ids = [...new Set(ctx.requestSelection().filter((id) => sch.symbols.some((s) => s.id === id)))];
    if (ids.length === 0) return;
    if (ids.length > 1 || sch.symbols.filter((s) => s.id === ids[0]).length < 2) {
      dispatch({ type: "TOAST", message: ids.length > 1 ? "Select the units of one multi-unit symbol." : "Only one unit of this symbol is placed.", kind: "info" });
      return;
    }
    void api.cmd({ op: "sch_edit", verb: "swap_unit_labels", ids });
  });
}

// The variant choice of the top toolbars (a KiCad 10 feature, ported from KiCad 25ec2335):
//   * the schematic editor's (`SCH_EDIT_FRAME::configureToolbars`' `variantSelectionCtrlFactory`, `UpdateVariantSelectionCtrl`, `onVariantSelected`,
//     eeschema/toolbars_sch_editor.cpp): the names for the UI, "---" and "Add New Design Variant...", which opens the New Design Variant dialog and keeps
//     the previous selection; picking a name makes it the current variant, which the sheet and the panels then show;
//   * the board editor's (`PCB_EDIT_FRAME::UpdateVariantSelectionCtrl`, pcbnew/toolbars_pcb_editor.cpp): the names for the UI only, and the board's own
//     current variant (`BOARD::SetCurrentVariant`), which the Plot and placement dialogs start from.
import { useSchControlDispatch } from "../../state/schControlStore";
import { useStudioDispatch, useStudioState } from "../../state/store";
import { pickVariantEntry, selectorValue, variantNamesForUI, variantSelectorEntries, VARIANT_SELECTOR_TIP } from "../../kicad-port/designVariants";

export function SchVariantSelector() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const controlDispatch = useSchControlDispatch();
  const variants = state.schematic?.variants ?? [];
  const entries = variantSelectorEntries(variants);
  return (
    <div className="toolbar-control" title={VARIANT_SELECTOR_TIP}>
      <select
        aria-label="Current variant"
        data-testid="sch-variant-select"
        value={selectorValue(variants, state.schVariant)}
        onChange={(e) => {
          const pick = pickVariantEntry(entries, e.target.selectedIndex);
          // "Add New Design Variant...": the selection goes back to the current variant (a controlled choice never left it) and the dialog opens
          if (pick.kind === "add") controlDispatch({ type: "OPEN_DIALOG", dialog: { kind: "variant", action: "add" } });
          else if (pick.kind === "select") dispatch({ type: "SET_SCH_VARIANT", variant: pick.variant });
        }}
      >
        {entries.map((name, i) => (
          <option key={`${i}:${name}`} value={name}>
            {name}
          </option>
        ))}
      </select>
    </div>
  );
}

export function PcbVariantSelector() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const variants = state.board?.variants ?? state.schematic?.variants ?? [];
  const names = variantNamesForUI(variants);
  return (
    <div className="toolbar-control" title={VARIANT_SELECTOR_TIP}>
      <select aria-label="Current variant" data-testid="pcb-variant-select" value={selectorValue(variants, state.pcbVariant)} onChange={(e) => dispatch({ type: "SET_PCB_VARIANT", variant: e.target.selectedIndex === 0 ? "" : e.target.value })}>
        {names.map((name) => (
          <option key={name} value={name}>
            {name}
          </option>
        ))}
      </select>
    </div>
  );
}

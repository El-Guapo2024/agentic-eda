// The "Design variant:" choice of the output dialogs (KiCad 25ec2335 `m_variantChoiceCtrl` of DIALOG_PLOT, DIALOG_GEN_FOOTPRINT_POSITION and DIALOG_PLOT_SCHEMATIC):
// the variant names for the UI (`GetVariantNamesForUI`, the default placeholder first); `getSelectedVariant` is the chosen name, empty for entry 0, which the server
// passes to kicad-cli as `--variant` (and leaves out for the default).
import type { CSSProperties } from "react";
import type { DesignVariantInfo } from "../api/types";
import { DEFAULT_VARIANT_NAME, selectorValue, variantNamesForUI } from "../kicad-port/designVariants";

export function VariantChoice({ variants, value, onChange, testId, style }: { variants: readonly DesignVariantInfo[]; value: string; onChange: (variant: string) => void; testId?: string; style?: CSSProperties }) {
  const names = variantNamesForUI(variants);
  return (
    <label style={{ display: "flex", alignItems: "center", gap: 6, ...style }}>
      Design variant:
      <select value={selectorValue(variants, value)} onChange={(e) => onChange(e.target.selectedIndex === 0 || e.target.value === DEFAULT_VARIANT_NAME ? "" : e.target.value)} data-testid={testId}>
        {names.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </select>
    </label>
  );
}

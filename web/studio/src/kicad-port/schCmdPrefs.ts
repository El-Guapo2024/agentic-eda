// The person's schematic preferences a command reads on the server, where KiCad's tools read `EESCHEMA_SETTINGS` (eeschema.json) while they run:
//   * Symbol Field Automatic Placement (`m_AutoplaceFields`: "Automatically place symbol fields", "Allow field autoplace to change justification", "Always align
//     autoplaced fields to the 50 mil grid"), which `PlaceSymbol`, `Rotate`, `editFieldText`, `Properties`, `SelectUnit`, Change Symbols and `AUTOPLACER` read
//     (crates/engine/src/fields.rs `AutoplacePrefs`);
//   * the colours of a new sheet (`m_Drawing.default_sheet_border_color` / `default_sheet_background_color`, `DrawSheet`; crates/ops/src/sch_prefs.rs).
// The studio keeps them in the browser (kicad-port/preferences.ts); every `/api/cmd` and the placing tools' preview (`/api/sch/place_preview`) carry them.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { SchColor } from "../api/schEditTypes";
import type { Preferences } from "./preferences";

/** What a request carries: `autoplace` and `sheet_colors`, as the server reads them. */
export interface SchCmdPrefs {
  autoplace: { enable: boolean; allow_rejustify: boolean; align_to_grid: boolean };
  sheet_colors: { border: SchColor | null; background: SchColor | null };
}

export function schCmdPrefsOf(p: Pick<Preferences, "schAutoplaceFields" | "schAutoplaceJustify" | "schAutoplaceAlign" | "schSheetBorderColor" | "schSheetBackgroundColor">): SchCmdPrefs {
  return {
    autoplace: { enable: p.schAutoplaceFields, allow_rejustify: p.schAutoplaceJustify, align_to_grid: p.schAutoplaceAlign },
    sheet_colors: { border: p.schSheetBorderColor, background: p.schSheetBackgroundColor },
  };
}

// What the schematic editor's View menu toggles (`EESCHEMA_SETTINGS::m_Appearance`, eeschema/eeschema_settings.cpp at 8303b2ad): which of the optional
// things the canvas draws. Plain data, so the painter and the store share one definition; the store keeps the values and saves them in the browser.

export interface SchDisplayOptions {
  /** `show_hidden_pins` -- `eeschema.EditorControl.showHiddenPins`: pins marked hidden are drawn too, in the hidden-items colour. */
  showHiddenPins: boolean;
  /** `show_hidden_fields` -- `eeschema.EditorControl.showHiddenFields`: a field marked hidden is drawn too, in the hidden-items colour (it is no more selectable for that: `SCH_SELECTION_TOOL::Selectable`). */
  showHiddenFields: boolean;
  /** `show_directive_labels` -- `showDirectiveLabels`. */
  showDirectiveLabels: boolean;
  /** `show_erc_errors` / `show_erc_warnings` / `show_erc_exclusions` -- `showERCErrors` / `showERCWarnings` / `showERCExclusions`: which ERC markers are drawn. */
  showErcErrors: boolean;
  showErcWarnings: boolean;
  showErcExclusions: boolean;
  /** `mark_sim_exclusions` -- `markSimExclusions`: a symbol excluded from simulation is drawn with a frame and a mark. */
  markSimExclusions: boolean;
}

/** KiCad's defaults (`eeschema_settings.cpp`: hidden pins and fields off, directive labels on, ERC errors and warnings on, exclusions off, sim marks on). */
export const DEFAULT_SCH_DISPLAY: SchDisplayOptions = {
  showHiddenPins: false,
  showHiddenFields: false,
  showDirectiveLabels: true,
  showErcErrors: true,
  showErcWarnings: true,
  showErcExclusions: false,
  markSimExclusions: true,
};

/** Merge saved values over the defaults, ignoring anything that is not a known boolean. */
export function sanitizeDisplay(raw: unknown): SchDisplayOptions {
  const out: SchDisplayOptions = { ...DEFAULT_SCH_DISPLAY };
  if (raw && typeof raw === "object") {
    for (const key of Object.keys(DEFAULT_SCH_DISPLAY) as (keyof SchDisplayOptions)[]) {
      const v = (raw as Record<string, unknown>)[key];
      if (typeof v === "boolean") out[key] = v;
    }
  }
  return out;
}

/** Whether an ERC marker of this severity is drawn (`LAYER_ERC_ERR` / `LAYER_ERC_WARN` / `LAYER_ERC_EXCLUSION` visibility). */
export function ercSeverityShown(severity: string, d: Pick<SchDisplayOptions, "showErcErrors" | "showErcWarnings" | "showErcExclusions">): boolean {
  if (severity === "error") return d.showErcErrors;
  if (severity === "warning") return d.showErcWarnings;
  return d.showErcExclusions; // "excluded" (and anything else KiCad may add) is an exclusion marker
}

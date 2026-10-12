// Toggle Units (Ctrl+U): `COMMON_TOOLS::ToggleUnits` and `SwitchUnits` (common/tool/common_tools.cpp at 8303b2ad).
//
//     m_frame->ChangeUserUnits( EDA_UNIT_UTILS::IsImperialUnit( m_frame->GetUserUnits() ) ? m_metricUnit : m_imperialUnit );
//
// The tool remembers the last unit of each kind that was picked (`SwitchUnits`: `m_imperialUnit = newUnit`; `m_imperialUnit( EDA_UNITS::INCH )` to start), so Ctrl+U
// goes from inches or mils to millimetres and from millimetres to whichever of the two was last chosen -- back to mils after mils, not to inches.
// Pure: no store, no DOM.

export type ToggledUnit = "mm" | "mil" | "in";
export type ImperialUnit = "in" | "mil";

/** `m_imperialUnit( EDA_UNITS::INCH )`. */
export const DEFAULT_IMPERIAL_UNIT: ImperialUnit = "in";

/** The unit Toggle Units switches to. */
export function toggledUnits(current: ToggledUnit, lastImperial: ImperialUnit): ToggledUnit {
  return current === "mm" ? lastImperial : "mm";
}

/** The imperial unit to return to once `picked` is chosen: `SwitchUnits` records an imperial pick and leaves it alone for a metric one. */
export function rememberedImperial(lastImperial: ImperialUnit, picked: ToggledUnit): ImperialUnit {
  return picked === "in" || picked === "mil" ? picked : lastImperial;
}

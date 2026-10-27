// What `DIALOG_PIN_PROPERTIES` (eeschema/dialogs/dialog_pin_properties.cpp) decides beyond the fields: the off-grid question of `TransferDataFromWindow`, the two "Common to
// all ..." boxes and their tooltips, and the Synchronized Pins Mode info bar of `TransferDataToWindow`. Also the pin-name offset the New Symbol and Symbol Properties
// dialogs store for "Pin name inside" with an offset of 0. Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { LibrarySymbolPin, MmPoint } from "../api/types";

/** Schematic internal units per mm (`SCH_IU_PER_MM`). */
const IU_PER_MM = 1e4;
const iu = (mm: number) => Math.round(mm * IU_PER_MM);

/**
 * `DIALOG_PIN_PROPERTIES::TransferDataFromWindow`'s question, or null: asked "only ... if the position has been changed" and the new X or Y is not a multiple of
 * `standard_grid = 50`. Ported as the C++ is: `m_posX.GetValue() % standard_grid` tests the value in schematic internal units (100 nm), not in mils, so only a position
 * off a 0.005 mm step asks, though the message says 50 mils.
 */
export function pinOffGridMessage(original: MmPoint, next: MmPoint): string | null {
  const standardGrid = 50;
  if (iu(original.x) === iu(next.x) && iu(original.y) === iu(next.y)) return null;
  if (iu(next.x) % standardGrid === 0 && iu(next.y) % standardGrid === 0) return null;
  return `This pin is not on a ${standardGrid} mils grid which will make it difficult to connect to in the schematic.\nDo you wish to continue?`;
}

export interface CommonBox {
  enabled: boolean;
  checked: boolean;
  tooltip: string | null;
}

/**
 * `m_checkApplyToAllParts` as `TransferDataToWindow` sets it: enabled only for a symbol of several units, checked when it is and the pin is on unit 0, with the tooltip of the
 * mode ("This symbol only has one unit. This control has no effect." for a single unit).
 */
export function commonUnitsBox(pin: Pick<LibrarySymbolPin, "unit">, unitCount: number, syncMode: boolean): CommonBox {
  const multi = unitCount > 1;
  let tooltip = syncMode ? "Synchronized pins mode is enabled.\nSimilar pins will be edited regardless of this option." : "If checked, this pin will exist in all units.";
  if (!multi) tooltip = "This symbol only has one unit. This control has no effect.";
  return { enabled: multi, checked: multi && pin.unit === 0, tooltip };
}

/** `m_checkApplyToAllBodyStyles`: enabled only for a symbol with an alternate body style, checked when the pin is on body style 0. */
export function commonBodyStylesBox(pin: Pick<LibrarySymbolPin, "body_style">, hasAlternateBodyStyle: boolean): CommonBox {
  return { enabled: hasAlternateBodyStyle, checked: pin.body_style === 0, tooltip: null };
}

/** `TransferDataFromWindow`: `SetUnit( m_checkApplyToAllParts->GetValue() ? 0 : m_frame->GetUnit() )`, the same for the body style. */
export function pinUnitsOnOk(commonUnits: boolean, commonBodyStyles: boolean, activeUnit: number, activeBodyStyle: number): { unit: number; body_style: number } {
  return { unit: commonUnits ? 0 : activeUnit, body_style: commonBodyStyles ? 0 : activeBodyStyle };
}

/** `getSyncPinsMessage`: the info bar shown while `m_SyncPinEdit` is on. */
export function syncPinsMessage(commonUnitsChecked: boolean, isNew: boolean): string {
  if (commonUnitsChecked) return "Synchronized Pins Mode.";
  if (isNew) return "Synchronized Pins Mode.  New pin will be added to all units.";
  return "Synchronized Pins Mode.  Matching pins in other units will be updated.";
}

/**
 * The pin-name offset stored for the "Pin name inside" box, in mm: the dialog's offset, KiCad-rounded to internal units, and an offset of 0 (which would mean "outside")
 * made non-zero -- `LIB_SYMBOL_LIBRARY_MANAGER::CreateSymbol` (New Symbol) makes it 1 IU, `DIALOG_LIB_SYMBOL_PROPERTIES::TransferDataFromWindow` (Symbol Properties)
 * `offset == 0 ? 20 : offset`, both in schematic internal units (0.0001 mm and 0.002 mm). Unchecked, the offset is 0.
 */
export function storedPinNameOffsetMm(inside: boolean, offsetMm: number, from: "new_symbol" | "symbol_properties"): number {
  if (!inside) return 0;
  const units = Math.round(offsetMm * IU_PER_MM);
  return (units === 0 ? (from === "new_symbol" ? 1 : 20) : units) / IU_PER_MM;
}

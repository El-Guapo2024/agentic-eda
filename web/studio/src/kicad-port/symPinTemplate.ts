// `SYMBOL_EDITOR_PIN_TOOL`'s sticky defaults (`g_LastPinType`/`g_LastPinShape`/`g_LastPinOrient`/`g_LastPinLength`/
// `g_LastPinNameSize`/`g_LastPinNumSize`/`g_LastPinCommonUnit`/`g_LastPinCommonBodyStyle`/`g_LastPinVisible`, set at the end of
// `EditPinProperties` and read by `CreatePin`), and the pin one click of the Pin tool makes from them. Pure, so the unit tests can
// pin the "the next pin carries the last one's look" rule down: only the Pin Properties dialog's OK (`EditPinProperties`) updates
// the template -- merely selecting a pin does not -- and it leaves what it was told for the click after.
import type { LibrarySymbolPin, MmPoint, PinElectricalType, PinShape } from "../api/types";
import { orientationToAngleDeg } from "./pinOrientation";

/** What the next pin starts with. `unit`/`body_style` are the last pin's: 0 means "common to all units/body styles", which
 * `CreatePin` then keeps for the next pin (`g_LastPinCommonUnit ? 0 : GetUnit()`); any other value takes the unit/body style being
 * edited. The alternate functions and the pin's own id, number, place and name are never carried (`g_LastPin*` holds no name). */
export interface PinTemplate {
  electrical_type: PinElectricalType;
  shape: PinShape;
  angle_deg: number;
  length_mm: number;
  hidden: boolean;
  name_size_mm: number | null;
  number_size_mm: number | null;
  unit: number;
  body_style: number;
}

/** `SYMBOL_EDITOR_SETTINGS::m_Defaults`: input/line/right (180deg, see pinOrientation.ts), 100 mil (2.54mm) length, 50 mil (1.27mm) name/number text. */
export const DEFAULT_PIN_TEMPLATE: PinTemplate = {
  electrical_type: "input",
  shape: "line",
  angle_deg: orientationToAngleDeg("right"),
  length_mm: 2.54,
  hidden: false,
  name_size_mm: 1.27,
  number_size_mm: 1.27,
  unit: 1,
  body_style: 1,
};

/** `SYMBOL_EDITOR_PIN_TOOL::CreatePin`: the pin a click makes -- the sticky defaults at the clicked place (mm, +y up), on the unit
 * and body style being edited unless the last pin was common to all of them. It has no name and no number: `CreatePin` sets neither
 * (`SCH_PIN::SCH_PIN( LIB_SYMBOL* )` starts empty), so Pin Properties opens with both empty. Only Repeat Pin (Insert) increments a number
 * (`RepeatPin`'s `IncrementString`, kicad-port/symEditItems.ts). */
export function newPinFromClick(template: PinTemplate, activeUnit: number, activeBodyStyle: number, at: MmPoint): LibrarySymbolPin {
  return {
    ...template,
    name: "",
    number: "",
    at,
    unit: template.unit === 0 ? 0 : activeUnit,
    body_style: template.body_style === 0 ? 0 : activeBodyStyle,
  };
}

/** The template a pin leaves for the next one (the fields `EditPinProperties` saves). */
export function pinTemplateOf(pin: LibrarySymbolPin): PinTemplate {
  return {
    electrical_type: pin.electrical_type,
    shape: pin.shape,
    angle_deg: pin.angle_deg,
    length_mm: pin.length_mm,
    hidden: pin.hidden,
    name_size_mm: pin.name_size_mm,
    number_size_mm: pin.number_size_mm,
    unit: pin.unit,
    body_style: pin.body_style,
  };
}

/** Whether two templates are the same look -- the document poll re-reads the selected pin every ~700ms, and the store must not
 * re-render for a template it already has. */
export function samePinTemplate(a: PinTemplate, b: PinTemplate): boolean {
  return (
    a.electrical_type === b.electrical_type &&
    a.shape === b.shape &&
    a.angle_deg === b.angle_deg &&
    a.length_mm === b.length_mm &&
    a.hidden === b.hidden &&
    a.name_size_mm === b.name_size_mm &&
    a.number_size_mm === b.number_size_mm &&
    a.unit === b.unit &&
    a.body_style === b.body_style
  );
}

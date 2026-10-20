// The alternate functions of a library symbol's pin as `DIALOG_PIN_PROPERTIES` edits them (eeschema/dialogs/dialog_pin_properties.cpp): the grid of rows
// "Alternate Pin Name / Electrical Type / Graphic Style", the row a new button press adds, the check made on OK, and when the pane is available at all.
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { PinAlternate, PinElectricalType, PinShape } from "../api/types";

/** `ALT_PIN_DATA_MODEL::GetColLabelValue`. */
export const ALTERNATE_COLUMNS: readonly string[] = ["Alternate Pin Name", "Electrical Type", "Graphic Style"];

/** The turndown's title (`m_alternatesTurndown`). */
export const ALTERNATES_TITLE = "Alternate pin function definitions";

/** The tooltip of the disabled pane (`DIALOG_PIN_PROPERTIES` constructor, `IsMultiBodyStyle()`). */
export const ALTERNATES_NOT_AVAILABLE = "Alternate pin assignments are not available for symbols with multiple body styles.";

/** What `TransferDataFromWindow` says of a row without a name. */
export const ALTERNATE_NAME_REQUIRED = "Alternate pin definitions must have a name.";

/** The pane is open to begin with when the pin has alternates, or when it was open the last time (`s_alternatesTurndownOpen`). */
export function alternatesPaneOpen(pinAlternates: number, openBefore: boolean): boolean {
  return pinAlternates > 0 || openBefore;
}

/** `OnAddAlternate`: a new row has no name and the pin's own electrical type and graphic style (the pin as stored, not as the dialog's combos now say). */
export function newAlternate(pin: { electrical_type: PinElectricalType; shape: PinShape }): PinAlternate {
  return { name: "", electrical_type: pin.electrical_type, shape: pin.shape };
}

/** `TransferDataFromWindow`: the message of the first row without a name, or null. */
export function alternatesError(rows: readonly PinAlternate[]): string | null {
  return rows.some((r) => r.name === "") ? ALTERNATE_NAME_REQUIRED : null;
}

/** `alternates[ alt.m_Name ] = alt`: a name once, the last definition standing, in the order of the names (`std::map`). What the server keeps; the grid shows the rows as typed. */
export function alternatesByName(rows: readonly PinAlternate[]): PinAlternate[] {
  const map = new Map<string, PinAlternate>();
  for (const r of rows) map.set(r.name, r);
  return [...map.values()].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

/** `pin_type.cpp::InitTables`' own combo order (`PinTypeNames()`): the Electrical Type column's choices, and the dialog's Electrical type combo. */
export const PIN_TYPE_OPTIONS: readonly { value: PinElectricalType; label: string }[] = [
  { value: "input", label: "Input" },
  { value: "output", label: "Output" },
  { value: "bidirectional", label: "Bidirectional" },
  { value: "tri_state", label: "Tri-state" },
  { value: "passive", label: "Passive" },
  { value: "free", label: "Free" },
  { value: "unspecified", label: "Unspecified" },
  { value: "power_in", label: "Power input" },
  { value: "power_out", label: "Power output" },
  { value: "open_collector", label: "Open collector" },
  { value: "open_emitter", label: "Open emitter" },
  { value: "no_connect", label: "Unconnected" },
];

/** Same source, `PinShapeNames()`: the Graphic Style column's choices, and the dialog's Graphic style combo. */
export const PIN_SHAPE_OPTIONS: readonly { value: PinShape; label: string }[] = [
  { value: "line", label: "Line" },
  { value: "inverted", label: "Inverted" },
  { value: "clock", label: "Clock" },
  { value: "inverted_clock", label: "Inverted clock" },
  { value: "input_low", label: "Input low" },
  { value: "clock_low", label: "Clock low" },
  { value: "output_low", label: "Output low" },
  { value: "edge_clock_high", label: "Falling edge clock" },
  { value: "non_logic", label: "NonLogic" },
];

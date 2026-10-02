// `EDA_UNIT_UTILS::UI::MessageTextFromValue` (common/eda_units.cpp) with
// pcbnew's IU scale -- the "lower-precision (for readability)" formatter
// every pcbnew info dialog uses (Board Statistics, the message panel...),
// plus `EDA_UNIT_UTILS::GetText`'s unit suffix. Same function the backend
// ports for the Board Statistics text report
// (crates/cli/src/board_stats.rs `message_text_from_value`), so the dialog
// and the saved report print identically. Dependency-free on purpose (see
// tsconfig.test.json).

export type MessageUnits = "mm" | "mil" | "in";

/** `EDA_DATA_TYPE` (only the two pcbnew dialogs here need). */
export type MessageDataType = "distance" | "area";

/** pcbnew IU (nm) per user unit -- `ToUserUnit`'s divisor. */
const IU_PER_UNIT: Record<MessageUnits, number> = { mm: 1_000_000, mil: 25_400, in: 25_400_000 };

const LABEL: Record<MessageUnits, string> = { mm: " mm", mil: " mils", in: " in" };

/** printf `%.Nf`. */
function fixed(v: number, decimals: number): string {
  return v.toFixed(decimals);
}

/** printf `%.3e` (two-digit, signed exponent). */
function cExp(v: number): string {
  const [m, e] = v.toExponential(3).split("e");
  const sign = e!.startsWith("-") ? "-" : "+";
  const digits = e!.replace(/^[+-]/, "").padStart(2, "0");
  return `${m}e${sign}${digits}`;
}

/**
 * `valueUm` is this app's µm (µm² for an area); converted to pcbnew IU
 * (nm / nm²) first, then `ToUserUnit` once per dimension, exactly as the
 * source's fall-through switch does.
 */
export function messageTextFromValue(valueUm: number, units: MessageUnits, addUnitLabel = true, type: MessageDataType = "distance"): string {
  const iu = type === "area" ? valueUm * 1e6 : valueUm * 1e3;
  let value = iu / IU_PER_UNIT[units];
  if (type === "area") value /= IU_PER_UNIT[units];

  const shortForm = type === "area";
  let decimals: number;
  switch (units) {
    case "mil":
      decimals = shortForm ? 0 : 2;
      break;
    case "in":
      decimals = shortForm ? 3 : 4;
      break;
    default:
      decimals = shortForm ? 3 : 4;
  }

  let text = fixed(value, decimals);

  // Check if the formatted value shows only zeros but the actual value is non-zero.
  if (value !== 0 && !/[1-9]/.test(text)) text = cExp(value);

  // Trim to 2-1/2 digits after the decimal place for short-form mm.
  if (shortForm && units === "mm") {
    const n = text.length;
    if (n > 4 && text[n - 4] === "." && text[n - 1] === "0") text = text.slice(0, n - 1);
  }

  if (addUnitLabel) {
    text += LABEL[units];
    if (type === "area") text += "²";
  }
  return text;
}

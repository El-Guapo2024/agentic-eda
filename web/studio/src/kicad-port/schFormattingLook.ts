// Schematic Setup > Formatting as the schematic's canvas lays text and pins out by it: the render settings KiCad's schematic frame copies from the
// project (`SCH_EDIT_FRAME::LoadProjectSettings` / `eeschema_config.cpp`: `GetRenderSettings()->m_LabelSizeRatio = Schematic().Settings().m_LabelSizeRatio`,
// `m_TextOffsetRatio`, `m_PinSymbolSize`, and `SCH_ITEM::GetFontMetrics`' `m_FontMetrics.m_OverbarHeight` from the overbar offset ratio), which kicad-cli's
// plots use too (`EESCHEMA_JOBS_HANDLER::InitRenderSettings`).
//
//   * Label offset ratio (`m_TextOffsetRatio`): `SCH_TEXT::GetTextOffset` = `KiROUND( ratio * GetTextSize().y )` -- how far a local label's text stands off
//     its wire (with the pen), a hierarchical label's and a sheet pin's text off its flag, the margin of a sheet pin's box; and a pin's texts off its line
//     (`PIN_LAYOUT_CACHE::getPinTextOffset`: `MilsToIU( KiROUND( 24 * ratio ) )`).
//   * Global label margin ratio (`m_LabelSizeRatio`): `SCH_LABEL_BASE::GetLabelBoxExpansion` = `KiROUND( ratio * GetTextSize().y )` -- the room around a global
//     label's text inside its flag (`SCH_GLOBALLABEL::CreateGraphicShape`, `GetSchematicTextOffset`).
//   * Pin symbol size (`m_PinSymbolSize`): the pin decorations' radius (`SCH_PAINTER::externalPinDecoSize` / `internalPinDecoSize`), or, at 0, half the pin's
//     text size.
//   * Overbar offset ratio: the height of a `~{..}` overbar over the baseline, in text sizes (`KIFONT::METRICS::m_OverbarHeight`).
//
// The look is the sheet in view's: the schematic painter sets it from the schematic it paints (`setSchFormattingLook`) and the hit tests, which run on that
// sheet, read the same; the Symbol Editor's canvas and the choosers' previews draw with KiCad's defaults (`withDefaultSchLook`), as the library editor's
// render settings keep them.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { SchematicSetupLook } from "../api/types";

/** eeschema/default_values.h: `DEFAULT_TEXT_OFFSET_RATIO`, `DEFAULT_LABEL_SIZE_RATIO`; font_metrics.h's `m_OverbarHeight` (1.23); `DEFAULT_TEXT_SIZE / 2` mils. */
export const DEFAULT_TEXT_OFFSET_RATIO = 0.15;
export const DEFAULT_LABEL_SIZE_RATIO = 0.375;
export const DEFAULT_OVERBAR_RATIO = 1.23;
export const DEFAULT_PIN_SYMBOL_SIZE_NM = 635_000;

const NM_PER_MIL = 25_400;

export interface SchFormattingLook {
  textOffsetRatio: number;
  labelSizeRatio: number;
  /** 0 is half the pin's text size. */
  pinSymbolSizeNm: number;
  overbarRatio: number;
}

export const DEFAULT_SCH_LOOK: SchFormattingLook = { textOffsetRatio: DEFAULT_TEXT_OFFSET_RATIO, labelSizeRatio: DEFAULT_LABEL_SIZE_RATIO, pinSymbolSizeNm: DEFAULT_PIN_SYMBOL_SIZE_NM, overbarRatio: DEFAULT_OVERBAR_RATIO };

let current: SchFormattingLook = DEFAULT_SCH_LOOK;

/** The look of Formatting's values as the schematic's `setup` sends them (an older backend's or none: KiCad's defaults). */
export function schLookOf(setup: Partial<SchematicSetupLook> | null | undefined): SchFormattingLook {
  const num = (v: unknown, d: number) => (typeof v === "number" && Number.isFinite(v) ? v : d);
  return {
    textOffsetRatio: num(setup?.text_offset_ratio, DEFAULT_TEXT_OFFSET_RATIO),
    labelSizeRatio: num(setup?.label_size_ratio, DEFAULT_LABEL_SIZE_RATIO),
    pinSymbolSizeNm: num(setup?.pin_symbol_size_nm, DEFAULT_PIN_SYMBOL_SIZE_NM),
    overbarRatio: num(setup?.overbar_offset_ratio, DEFAULT_OVERBAR_RATIO),
  };
}

/** The schematic painter's: the sheet it paints sets the look its text and pins are laid out by (and the hit tests on it read). */
export function setSchFormattingLook(setup: Partial<SchematicSetupLook> | null | undefined): void {
  current = schLookOf(setup);
}

export const schFormattingLook = (): SchFormattingLook => current;

/** Run `f` with KiCad's defaults (the Symbol Editor's and the choosers' canvases), then put the sheet's look back. */
export function withDefaultSchLook<T>(f: () => T): T {
  const was = current;
  current = DEFAULT_SCH_LOOK;
  try {
    return f();
  } finally {
    current = was;
  }
}

/** KiROUND: half away from zero. */
const kiRound = (x: number): number => (x >= 0 ? Math.round(x) : -Math.round(-x));

/** `SCH_TEXT::GetTextOffset`: `KiROUND( m_TextOffsetRatio * GetTextSize().y )`. */
export const textOffsetNm = (sizeNm: number, look: SchFormattingLook = current): number => kiRound(look.textOffsetRatio * sizeNm);

/** `SCH_LABEL_BASE::GetLabelBoxExpansion`: `KiROUND( m_LabelSizeRatio * GetTextSize().y )`. */
export const labelBoxExpansionNm = (sizeNm: number, look: SchFormattingLook = current): number => kiRound(look.labelSizeRatio * sizeNm);

/** `PIN_LAYOUT_CACHE::getPinTextOffset() + PIN_TEXT_MARGIN`: `MilsToIU( KiROUND( 24 * ratio ) )` and 4 mils -- the gap between a pin's line and its text. */
export const pinTextClearanceNm = (look: SchFormattingLook = current): number => (kiRound(24 * look.textOffsetRatio) + 4) * NM_PER_MIL;

/** `SCH_PAINTER::externalPinDecoSize` / `internalPinDecoSize`: `m_PinSymbolSize` when it is set, else half the pin's text size (50 mils here). */
export const pinDecoSizeNm = (pinTextSizeNm = 1_270_000, look: SchFormattingLook = current): number => (look.pinSymbolSizeNm > 0 ? look.pinSymbolSizeNm : Math.trunc(pinTextSizeNm / 2));

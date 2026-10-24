// Where a placed symbol's Reference / Value / footprint text is drawn.
//
// `/api/schematic` does not carry per-symbol field positions (real KiCad stores each field's own place), so the painter puts them by
// a rule. Text drawn across a wire is the commonest ugliness of a generated schematic, and a resistor or capacitor standing on its
// end has a wire out of both ends, straight through "above and below the symbol": so a symbol that is exactly two pins, both
// vertical, gets its text beside the body, where KiCad's own `Device:R` puts it; everything else keeps its text above (Reference)
// and below (Value, footprint), now centred on the symbol instead of starting at its middle.
//
// The same rule is modelled in crates/engine/src/hier/kit.rs (`Placed::field_rects`) so a layout can keep clear of its own text.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).

export const REF_FONT_NM = 1_600_000;
export const VALUE_FONT_NM = 1_400_000;
export const FIELD_FONT_NM = 1_000_000;

export interface FieldBox {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export interface FieldAnchors {
  /** Baseline anchors, um. */
  ref: [number, number];
  value: [number, number];
  footprint: [number, number];
  justify: "left" | "center";
}

/** A pin as the painter resolved it: `tip` the wire end, `root` the body end. */
export interface FieldPin {
  tip: [number, number];
  root: [number, number];
}

/** Exactly two pins, both standing up (the wire leaves the top and the bottom). */
export function isVerticalTwoPin(pins: readonly FieldPin[]): boolean {
  return pins.length === 2 && pins.every((p) => Math.abs(p.tip[1] - p.root[1]) > Math.abs(p.tip[0] - p.root[0]));
}

export function fieldAnchors(bbox: FieldBox, verticalTwoPin: boolean): FieldAnchors {
  if (verticalTwoPin) {
    const x = bbox.maxX + 800_000;
    const cy = (bbox.minY + bbox.maxY) / 2;
    return { ref: [x, cy - 300_000], value: [x, cy + 1_500_000], footprint: [x, cy + 1_500_000 + 1_150_000], justify: "left" };
  }
  const cx = (bbox.minX + bbox.maxX) / 2;
  return { ref: [cx, bbox.minY - 400_000], value: [cx, bbox.maxY + 1_800_000], footprint: [cx, bbox.maxY + 1_800_000 + 1_150_000], justify: "center" };
}

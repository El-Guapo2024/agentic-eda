// A footprint's own bounding box: `FOOTPRINT::GetBoundingBox( aIncludeText )` (pcbnew/footprint.cpp at 8303b2ad), as far as the studio knows the footprint.
//
// The box is KiCad's: it starts as the footprint's position grown by 0.25 mm ("Give a min size to the bbox") and joins
//   * the footprint's drawings -- here the courtyard (its lines are `F.CrtYd` shapes of the footprint) and the `F.Fab` body; the rest of the silkscreen and fab
//     graphics are not on the board yet (GAPS.md item 25), so they are not in the box;
//   * every pad (`PAD::GetBoundingBox`: a turned pad's box);
//   * with `aIncludeText`: the Reference, Value and other fields that are shown (`if( text->Type() == PCB_FIELD_T && !text->IsVisible() ) continue;`).
//
// Which one a tool asks for is source's: `EDA_ITEM::GetBoundingBox()` is the text-included one (`FOOTPRINT::GetBoundingBox() const { return GetBoundingBox( true ); }`),
// and it is what `SELECTION::GetCenter()` joins for Rotate and Flip's reference point, `PCB_SELECTION::GetBoundingBox` for Zoom to Selection and `PCB_GROUP`
// for a group's box; Align and Distribute ask for `GetBoundingBox( false )` (`getBoundingBox` of align_distribute_tool.cpp), as do Pack and Move and the
// selection's hit tests.
//
// Pure: no store, no DOM. Unit-tested in footprintBox.test.ts.
import type { FieldInfo, Part } from "../api/types";

/** `[minX, minY, maxX, maxY]`, board µm. */
export type FootprintBox = readonly [number, number, number, number];

/** What of a footprint the box reads (the snap scene's parts carry no more). */
export type FootprintBoxPart = Pick<Part, "placed"> & Partial<Pick<Part, "at" | "pads" | "courtyard" | "body" | "fields">>;

/** `bbox.Inflate( pcbIUScale.mmToIU( 0.25 ) )`: "Give a min size to the bbox". */
export const MIN_HALF_SIZE_UM = 250;

function merge(a: FootprintBox, b: FootprintBox): FootprintBox {
  return [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[2], b[2]), Math.max(a[3], b[3])];
}

/**
 * `FOOTPRINT::GetBoundingBox( includeText )` of the placed footprint `part`; null for one that is not placed. `textBox` is the box of a field's text on the
 * board (the caller knows the glyph metrics: kicad-port/pcbItems.ts); a field it gives none for adds nothing.
 */
export function footprintBox(part: FootprintBoxPart, includeText: boolean, textBox?: (field: FieldInfo) => FootprintBox | null): FootprintBox | null {
  if (!part.placed || !part.at) return null;
  const [x, y] = part.at;
  let box: FootprintBox = [x - MIN_HALF_SIZE_UM, y - MIN_HALF_SIZE_UM, x + MIN_HALF_SIZE_UM, y + MIN_HALF_SIZE_UM];
  if (part.courtyard) box = merge(box, [part.courtyard[0], part.courtyard[1], part.courtyard[2], part.courtyard[3]]);
  if (part.body) box = merge(box, [part.body[0], part.body[1], part.body[2], part.body[3]]);
  for (const pad of part.pads ?? []) box = merge(box, [pad.x - pad.w / 2, pad.y - pad.h / 2, pad.x + pad.w / 2, pad.y + pad.h / 2]);
  if (includeText && textBox) {
    for (const field of part.fields ?? []) {
      if (!field.visible) continue;
      const t = textBox(field);
      if (t) box = merge(box, t);
    }
  }
  return box;
}

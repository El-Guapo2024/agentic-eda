// The hatch fill of a schematic shape (r7 M5): KiCad 25ec2335 `SCH_SHAPE::GetHatchLineWidth` (half the shape's effective stroke width),
// `GetHatchLineSpacing` (40 hatch widths) and `EDA_SHAPE::UpdateHatching`'s slopes -- `HATCH` -1, `REVERSE_HATCH` +1, `CROSS_HATCH` both -- with the spacing
// widened so no more than 100 lines cross the major axis (`if( majorAxis / spacing > 100 ) spacing = majorAxis / 100`). `SCH_PAINTER::draw( SCH_SHAPE )` strokes
// the lines in the background layer's colour with the hatch width; here they cover the shape's box and the caller clips them to the shape.
// **Differs**: KiCad clips the lines to the shape's polygon with its knockouts (`getHatchingKnockouts`: the text of a text box); the canvas clip is the shape's path.

export type SchHatchFill = "hatch" | "reverse_hatch" | "cross_hatch";

export interface HatchLines {
  width: number;
  segments: [number, number, number, number][];
}

/** The hatch lines over the box `[x0, y0, x1, y1]` (y down) of a shape drawn `strokeWidth` wide. */
export function hatchLines(fill: SchHatchFill, box: [number, number, number, number], strokeWidth: number): HatchLines {
  const width = Math.round(strokeWidth / 2);
  let spacing = width * 40;
  const [x0, y0, x1, y1] = box;
  const major = Math.max(x1 - x0, y1 - y0);
  if (spacing <= 0) return { width, segments: [] };
  if (major / spacing > 100) spacing = major / 100;
  const slopes = fill === "cross_hatch" ? [1, -1] : fill === "hatch" ? [-1] : [1];
  const segments: [number, number, number, number][] = [];
  for (const slope of slopes) {
    // the lines y = slope * x + c, c on the spacing's grid along the y axis (offset by the slope's span over the box)
    const cs = [y0 - slope * x0, y0 - slope * x1, y1 - slope * x0, y1 - slope * x1];
    const cMin = Math.min(...cs);
    const cMax = Math.max(...cs);
    const step = spacing * Math.SQRT2;
    for (let c = Math.ceil(cMin / step) * step; c <= cMax; c += step) {
      segments.push([x0, slope * x0 + c, x1, slope * x1 + c]);
    }
  }
  return { width, segments };
}
